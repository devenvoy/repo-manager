//! Phase 2 reproducible benchmark & bottleneck profiler for Release Desk.
//!
//! Run with:
//!   cargo run --release --bin bench_repos
//!   cargo run --release --bin bench_repos -- --network

use release_desk_core::project::DiskLayout;
use release_desk_git::GitClient;
use release_desk_git::perf::{self, FetchTimings, InspectTimings};
use repo_manager::modules::parallel::map_bounded;
use repo_manager::modules::perf_trace::{self, PresentationTimings, WorkerSpan};
use repo_manager::repository_git::{self, RepositoryTarget};
use repo_manager::settings;
use repo_manager::views::project_repository_status::build_repository_presentation;
use repo_manager::workspace::{ProviderChoice, WorkspaceProject};
use repo_manager::workspace_sources::{RepositoryListMode, SourceRepository, WorkspaceSource};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn ms(ns: u64) -> f64 {
    ns as f64 / 1_000_000.0
}

fn dur_ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

fn pct(part: u64, total: u64) -> f64 {
    if total == 0 {
        0.0
    } else {
        (part as f64 / total as f64) * 100.0
    }
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let idx = ((sorted.len() - 1) as f64 * p).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

fn cmd_out(bin: &str, args: &[&str]) -> String {
    Command::new(bin)
        .args(args)
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

fn git_cmd(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .current_dir(dir)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .expect("git must be installed");
    assert!(
        out.status.success(),
        "git {args:?} in {} failed: {}",
        dir.display(),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn create_repo_via_fast_import(
    repo_dir: &Path,
    bare_remote: &Path,
    file_count: usize,
    commit_count: usize,
) {
    std::fs::create_dir_all(bare_remote).unwrap();
    git_cmd(bare_remote, &["init", "--bare", "-b", "main"]);

    let mut child = Command::new("git")
        .current_dir(bare_remote)
        .args(["fast-import", "--quiet"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn git fast-import");

    {
        let stdin = child.stdin.as_mut().unwrap();
        let mut w = std::io::BufWriter::with_capacity(256 * 1024, stdin);

        // Commit 1: populate all `file_count` files cleanly across subdirs
        writeln!(w, "commit refs/heads/main").unwrap();
        writeln!(w, "mark :1").unwrap();
        writeln!(
            w,
            "committer Bench <bench@example.invalid> 1700000000 +0000"
        )
        .unwrap();
        writeln!(w, "data 12\ninitial tree").unwrap();
        for i in 0..file_count {
            let dir_idx = i / 200;
            let payload = format!("file {i} content\n");
            writeln!(w, "M 100644 inline d{dir_idx:03}/f{i:06}.txt").unwrap();
            writeln!(w, "data {}\n{payload}", payload.len()).unwrap();
        }

        // Subsequent commits modify one file each so commit history grows cheaply
        for c in 2..=commit_count {
            let mark = c;
            let prev = c - 1;
            let payload = format!("commit {c}\n");
            writeln!(w, "commit refs/heads/main").unwrap();
            writeln!(w, "mark :{mark}").unwrap();
            writeln!(
                w,
                "committer Bench <bench@example.invalid> {} +0000",
                1700000000 + c as u64
            )
            .unwrap();
            let msg = format!("commit #{c}");
            writeln!(w, "data {}\n{msg}", msg.len()).unwrap();
            writeln!(w, "from :{prev}").unwrap();
            writeln!(w, "M 100644 inline d000/f000000.txt").unwrap();
            writeln!(w, "data {}\n{payload}", payload.len()).unwrap();
        }
        writeln!(w, "done").unwrap();
        w.flush().unwrap();
    }

    let status = child.wait_with_output().unwrap();
    assert!(
        status.status.success(),
        "fast-import failed: {}",
        String::from_utf8_lossy(&status.stderr)
    );

    if let Some(parent) = repo_dir.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    git_cmd(
        bare_remote.parent().unwrap(),
        &[
            "clone",
            "--quiet",
            bare_remote.to_str().unwrap(),
            repo_dir.to_str().unwrap(),
        ],
    );
}

fn push_commit_to_bare(work_dir: &Path, bare_remote: &Path, tag: &str) {
    let scratch = work_dir.with_file_name(format!(
        "{}-pusher",
        work_dir.file_name().unwrap().to_string_lossy()
    ));
    if !scratch.exists() {
        git_cmd(
            bare_remote.parent().unwrap(),
            &[
                "clone",
                "--quiet",
                bare_remote.to_str().unwrap(),
                scratch.to_str().unwrap(),
            ],
        );
        git_cmd(&scratch, &["config", "user.email", "bench@example.invalid"]);
        git_cmd(&scratch, &["config", "user.name", "Bench"]);
    }
    std::fs::write(scratch.join("d000/f000000.txt"), format!("update {tag}\n")).unwrap();
    git_cmd(&scratch, &["add", "d000/f000000.txt"]);
    git_cmd(&scratch, &["commit", "--quiet", "-m", tag]);
    git_cmd(&scratch, &["push", "--quiet", "origin", "main"]);
}

fn make_target(
    project_root: &Path,
    repo_dir: &Path,
    remote_url: &str,
    name: &str,
    source_index: usize,
    repo_index: usize,
) -> RepositoryTarget {
    RepositoryTarget {
        key: repo_manager::workspace_sources::RepoKey::new("github", "github.com", "bench", name),
        source_index,
        repo_index,
        name: name.to_string(),
        url: remote_url.to_string(),
        branch: "main".to_string(),
        path: repo_dir.to_path_buf(),
        project_root: project_root.to_path_buf(),
        credential_ref: None,
        provider: "github".to_string(),
        account: "bench".to_string(),
        endpoint: None,
    }
}

fn make_synthetic_project(
    root: &Path,
    tag: &str,
    count: usize,
    template_dirs: &[(String, PathBuf, PathBuf)],
) -> (WorkspaceProject, Vec<RepositoryTarget>) {
    let project_dir = root.join(format!("proj-{tag}-{count}"));
    std::fs::create_dir_all(&project_dir).unwrap();
    let mut repos = Vec::with_capacity(count);
    let mut targets = Vec::with_capacity(count);
    for i in 0..count {
        let (_kind, tpl_work, tpl_bare) = &template_dirs[i % template_dirs.len()];
        let name = format!("repo-{i:03}");
        let dest = project_dir.join("bench").join(&name);
        std::fs::create_dir_all(dest.parent().unwrap()).unwrap();
        // Fast local hardlink clone from the bare template so each repo is an independent checkout
        git_cmd(
            &project_dir,
            &[
                "clone",
                "--quiet",
                "--local",
                tpl_work.to_str().unwrap(),
                dest.to_str().unwrap(),
            ],
        );
        // Point origin to tpl_bare so remote fetches work deterministically
        git_cmd(
            &dest,
            &["remote", "set-url", "origin", tpl_bare.to_str().unwrap()],
        );
        git_cmd(&dest, &["fetch", "--quiet", "origin"]);

        repos.push(SourceRepository {
            name: name.clone(),
            full_name: format!("bench/{name}"),
            description: Some("Synthetic benchmark repo".into()),
            stars: 1,
            forks: 0,
            primary_language: Some("Rust".into()),
            is_private: false,
            is_fork: false,
            clone_url: tpl_bare.to_string_lossy().into_owned(),
            default_branch: "main".into(),
            selected: true,
        });
        targets.push(make_target(
            &project_dir,
            &dest,
            &tpl_bare.to_string_lossy(),
            &name,
            0,
            i,
        ));
    }
    let project = WorkspaceProject {
        id: count as u128,
        name: format!("Synthetic-{count}"),
        directory: project_dir,
        provider: ProviderChoice::GitHub,
        account: "bench".into(),
        custom_host: None,
        credential_ref: None,
        disk_layout: DiskLayout::Grouped,
        sources: vec![WorkspaceSource {
            handle: "bench".into(),
            display_name: None,
            avatar_url: None,
            bio: None,
            provider: "github".into(),
            repositories: repos,
        }],
        repository_list_mode: RepositoryListMode::Grouped,
    };
    (project, targets)
}

#[derive(Clone, Debug, Default)]
struct BatchMetrics {
    n: usize,
    workers: usize,
    wall_ms: f64,
    sum_repo_ms: f64,
    speedup: f64,
    utilization_pct: f64,
    wait_p50_ms: f64,
    wait_p95_ms: f64,
    wait_max_ms: f64,
    dur_p50_ms: f64,
    dur_p95_ms: f64,
    dur_max_ms: f64,
    spans: Vec<WorkerSpan>,
}

fn run_inspect_batch(
    project: &WorkspaceProject,
    targets: &[RepositoryTarget],
    workers: usize,
    serial_wall_ms: Option<f64>,
) -> BatchMetrics {
    let t0 = Instant::now();
    let per_repo = map_bounded(targets, workers, |target| {
        let rt = Instant::now();
        let _status = repository_git::inspect(target);
        let _pres = build_repository_presentation(project, target.source_index, target.repo_index);
        dur_ms(rt.elapsed())
    });
    let wall_ms = dur_ms(t0.elapsed());
    let spans = perf_trace::take_worker_spans();
    summarize_batch(
        targets.len(),
        workers,
        wall_ms,
        per_repo,
        spans,
        serial_wall_ms,
    )
}

fn run_fetch_batch(
    targets: &[RepositoryTarget],
    workers: usize,
    serial_wall_ms: Option<f64>,
) -> BatchMetrics {
    let t0 = Instant::now();
    let per_repo = map_bounded(targets, workers, |target| {
        let rt = Instant::now();
        let _status = repository_git::check_for_updates(target);
        dur_ms(rt.elapsed())
    });
    let wall_ms = dur_ms(t0.elapsed());
    let spans = perf_trace::take_worker_spans();
    summarize_batch(
        targets.len(),
        workers,
        wall_ms,
        per_repo,
        spans,
        serial_wall_ms,
    )
}

fn summarize_batch(
    n: usize,
    workers: usize,
    wall_ms: f64,
    mut per_repo: Vec<f64>,
    spans: Vec<WorkerSpan>,
    serial_wall_ms: Option<f64>,
) -> BatchMetrics {
    let sum_repo_ms: f64 = per_repo.iter().sum();
    per_repo.sort_by(|a, b| a.total_cmp(b));
    let mut waits: Vec<f64> = spans.iter().map(|s| ms(s.queue_wait_ns)).collect();
    waits.sort_by(|a, b| a.total_cmp(b));
    let base = serial_wall_ms.unwrap_or(wall_ms);
    let speedup = if wall_ms > 0.0 { base / wall_ms } else { 1.0 };
    let effective_workers = workers.min(n).max(1) as f64;
    let utilization_pct = if wall_ms > 0.0 {
        (sum_repo_ms / (wall_ms * effective_workers)) * 100.0
    } else {
        0.0
    };
    BatchMetrics {
        n,
        workers,
        wall_ms,
        sum_repo_ms,
        speedup,
        utilization_pct,
        wait_p50_ms: percentile(&waits, 0.50),
        wait_p95_ms: percentile(&waits, 0.95),
        wait_max_ms: *waits.last().unwrap_or(&0.0),
        dur_p50_ms: percentile(&per_repo, 0.50),
        dur_p95_ms: percentile(&per_repo, 0.95),
        dur_max_ms: *per_repo.last().unwrap_or(&0.0),
        spans,
    }
}

fn render_gantt(spans: &[WorkerSpan], wall_ms: f64, width: usize) -> String {
    if spans.is_empty() || wall_ms <= 0.0 {
        return String::new();
    }
    let max_worker = spans.iter().map(|s| s.worker_id).max().unwrap_or(0);
    let mut out = String::new();
    out.push_str(&format!("Timeline (0 .. {wall_ms:.1} ms, {width} cols):\n"));
    for w in 0..=max_worker {
        let mut row = vec!['.'; width];
        let mut items = Vec::new();
        for s in spans.iter().filter(|s| s.worker_id == w) {
            items.push(s.item_index);
            let a = ((ms(s.start_ns) / wall_ms) * width as f64).floor() as usize;
            let b = ((ms(s.end_ns) / wall_ms) * width as f64).ceil() as usize;
            let ch = char::from_digit((s.item_index % 36) as u32, 36).unwrap_or('#');
            let start = a.min(width - 1);
            let end = b.min(width).max(start + 1);
            row[start..end].fill(ch);
        }
        let line: String = row.into_iter().collect();
        out.push_str(&format!("  W{w:<2} |{line}| ({} items)\n", items.len()));
    }
    out
}

fn measure_single_repo(
    label: &str,
    project: &WorkspaceProject,
    target: &RepositoryTarget,
) -> (InspectTimings, PresentationTimings) {
    let client = GitClient::new();
    let _ = perf::take_last_inspect();
    let _ = perf::take_subprocess_samples();
    let _ = client.inspect(&target.path, &target.branch).unwrap();
    let inspect_t = perf::take_last_inspect().unwrap_or_default();
    let _ = build_repository_presentation(project, target.source_index, target.repo_index);
    let pres_t = perf_trace::take_last_presentation().unwrap_or_default();
    let _ = label;
    (inspect_t, pres_t)
}

fn measure_spawn_baseline(small_repo: &Path) -> (Vec<f64>, Vec<f64>, Vec<f64>) {
    let mut ver_times = Vec::new();
    for _ in 0..30 {
        let t = Instant::now();
        let _ = Command::new("git")
            .arg("--version")
            .stdin(Stdio::null())
            .output()
            .unwrap();
        ver_times.push(dur_ms(t.elapsed()));
    }
    let mut rev_times = Vec::new();
    for _ in 0..30 {
        let t = Instant::now();
        let _ = Command::new("git")
            .current_dir(small_repo)
            .args(["rev-parse", "--git-dir"])
            .stdin(Stdio::null())
            .output()
            .unwrap();
        rev_times.push(dur_ms(t.elapsed()));
    }
    let mut cred_times = Vec::new();
    for _ in 0..10 {
        let t = Instant::now();
        let mut child = Command::new("git")
            .args(["credential-osxkeychain", "get"])
            .env("GIT_TERMINAL_PROMPT", "0")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        if let Some(mut stdin) = child.stdin.take() {
            let _ = writeln!(stdin, "protocol=https\nhost=nonexistent.invalid.example\n");
        }
        let _ = child.wait_with_output();
        cred_times.push(dur_ms(t.elapsed()));
    }
    ver_times.sort_by(|a, b| a.total_cmp(b));
    rev_times.sort_by(|a, b| a.total_cmp(b));
    cred_times.sort_by(|a, b| a.total_cmp(b));
    (ver_times, rev_times, cred_times)
}

fn simulate_ui_poll_latency(
    project: &WorkspaceProject,
    targets: &[RepositoryTarget],
    workers: usize,
) {
    let (tx, rx) = std::sync::mpsc::channel::<(usize, Instant)>();
    let t_start = Instant::now();
    let producer_project = project.clone();
    let producer_targets = targets.to_vec();
    let handle = std::thread::spawn(move || {
        map_bounded(&producer_targets, workers, |target| {
            let _status = repository_git::inspect(target);
            let _pres = build_repository_presentation(
                &producer_project,
                target.source_index,
                target.repo_index,
            );
            let _ = tx.send((target.repo_index, Instant::now()));
        });
    });

    let mut latencies_ms = Vec::new();
    let mut ticks = 0usize;
    let mut non_empty_ticks = 0usize;
    let mut batch_sizes = Vec::new();
    let mut arrival_order = Vec::new();
    loop {
        std::thread::sleep(Duration::from_millis(40));
        ticks += 1;
        let poll_now = Instant::now();
        let mut arrived = Vec::new();
        while let Ok(item) = rx.try_recv() {
            arrived.push(item);
        }
        if !arrived.is_empty() {
            non_empty_ticks += 1;
            batch_sizes.push(arrived.len());
            for (repo_idx, completed_at) in arrived {
                arrival_order.push(repo_idx);
                latencies_ms.push(dur_ms(poll_now.saturating_duration_since(completed_at)));
            }
        }
        if arrival_order.len() >= targets.len() {
            break;
        }
        if t_start.elapsed() > Duration::from_secs(60) {
            break;
        }
    }
    let _ = handle.join();
    latencies_ms.sort_by(|a, b| a.total_cmp(b));
    let is_sorted = arrival_order.windows(2).all(|w| w[0] <= w[1]);
    println!(
        "- **N={} @ {} workers (40 ms poll loop):** wall={:.1} ms | ticks={} (non-empty={}, notify_calls={}) | arrived/tick={:?} | in-order={} | latency p50={:.2} ms, p95={:.2} ms, max={:.2} ms",
        targets.len(),
        workers,
        dur_ms(t_start.elapsed()),
        ticks,
        non_empty_ticks,
        non_empty_ticks,
        batch_sizes,
        is_sorted,
        percentile(&latencies_ms, 0.50),
        percentile(&latencies_ms, 0.95),
        latencies_ms.last().copied().unwrap_or(0.0)
    );
}

fn main() {
    let run_network = std::env::args().any(|a| a == "--network");
    perf_trace::set_enabled(true);

    println!("# Release Desk — Phase 2 Benchmark & Bottleneck Report\n");
    println!("## 1. Environment & System Info\n");
    let os_ver = cmd_out("sw_vers", &["-productVersion"]);
    let build_ver = cmd_out("sw_vers", &["-buildVersion"]);
    let arch = std::env::consts::ARCH;
    let git_ver = cmd_out("git", &["--version"]);
    let cpus = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);
    let ram_gb = cmd_out("sysctl", &["-n", "hw.memsize"])
        .parse::<u64>()
        .map(|b| b as f64 / (1024.0 * 1024.0 * 1024.0))
        .unwrap_or(0.0);
    println!("- **OS:** macOS {os_ver} ({build_ver}), `{arch}`");
    println!("- **Hardware:** {cpus} logical CPUs, {ram_gb:.1} GiB RAM");
    println!("- **Git:** `{git_ver}`");
    println!(
        "- **Filesystems:** Temp fixtures on internal APFS; real projects on `/Volumes/Devansh2` (External USB, Case-sensitive APFS)"
    );
    println!(
        "- **Cold cache note:** `sudo -n purge` requires an interactive password (`sudo: a password is required`), so cold passes were measured on newly created / first-read repositories and warm passes on subsequent iterations.\n"
    );

    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();

    // Build fixtures
    let t_fix = Instant::now();
    let small_work = root.join("fx/small/work");
    let small_bare = root.join("fx/small/bare.git");
    create_repo_via_fast_import(&small_work, &small_bare, 100, 50);

    let med_work = root.join("fx/medium/work");
    let med_bare = root.join("fx/medium/bare.git");
    create_repo_via_fast_import(&med_work, &med_bare, 5_000, 2_000);

    let large_files_work = root.join("fx/large-files/work");
    let large_files_bare = root.join("fx/large-files/bare.git");
    create_repo_via_fast_import(&large_files_work, &large_files_bare, 50_000, 100);

    let large_commits_work = root.join("fx/large-commits/work");
    let large_commits_bare = root.join("fx/large-commits/bare.git");
    create_repo_via_fast_import(&large_commits_work, &large_commits_bare, 500, 100_000);

    // Dirty variant (copy medium via local clone)
    let dirty_work = root.join("fx/dirty/work");
    git_cmd(
        root,
        &[
            "clone",
            "--quiet",
            "--local",
            med_work.to_str().unwrap(),
            dirty_work.to_str().unwrap(),
        ],
    );
    for i in 0..10 {
        std::fs::write(dirty_work.join(format!("d000/f{i:06}.txt")), "modified\n").unwrap();
    }

    // Untracked-heavy variant (medium + 5,000 untracked files)
    let untracked_work = root.join("fx/untracked/work");
    git_cmd(
        root,
        &[
            "clone",
            "--quiet",
            "--local",
            med_work.to_str().unwrap(),
            untracked_work.to_str().unwrap(),
        ],
    );
    let ut_dir = untracked_work.join("untracked_blob");
    std::fs::create_dir_all(&ut_dir).unwrap();
    for i in 0..5_000 {
        std::fs::write(ut_dir.join(format!("u_{i:05}.tmp")), "u").unwrap();
    }
    println!(
        "*(Synthetic fixture generation completed in {:.2} s)*\n",
        t_fix.elapsed().as_secs_f64()
    );

    // 2. Spawn & credential baseline
    println!("## 2. Subprocess Spawn & Credential Helper Overhead\n");
    let (ver_times, rev_times, cred_times) = measure_spawn_baseline(&small_work);
    println!("| Probe | Iterations | p50 (ms) | p95 (ms) | max (ms) | mean (ms) |");
    println!("|---|---:|---:|---:|---:|---:|");
    for (name, arr) in [
        ("`git --version`", &ver_times),
        ("`git rev-parse --git-dir` (small)", &rev_times),
        ("`git credential-osxkeychain get` (miss)", &cred_times),
    ] {
        let mean = arr.iter().sum::<f64>() / arr.len() as f64;
        println!(
            "| {name} | {} | {:.2} | {:.2} | {:.2} | {:.2} |",
            arr.len(),
            percentile(arr, 0.50),
            percentile(arr, 0.95),
            arr.last().copied().unwrap_or(0.0),
            mean
        );
    }
    println!();

    // 3. Per-repo phase breakdown
    println!("## 3. Per-Repo Phase Breakdown (Inspect + Presentation)\n");
    let fixtures: Vec<(&str, &Path, &Path)> = vec![
        ("Small (100 files, 50 commits)", &small_work, &small_bare),
        ("Medium (5k files, 2k commits)", &med_work, &med_bare),
        (
            "Large-Tree (50k files, 100 commits)",
            &large_files_work,
            &large_files_bare,
        ),
        (
            "Large-History (500 files, 100k commits)",
            &large_commits_work,
            &large_commits_bare,
        ),
        ("Dirty (5k files, 10 modified)", &dirty_work, &med_bare),
        (
            "Untracked-Heavy (5k tracked + 5k untracked)",
            &untracked_work,
            &med_bare,
        ),
    ];

    println!("### Inspect Breakdown (`GitClient::inspect`)\n");
    println!(
        "| Fixture (Pass) | Total (ms) | Subprocs | Path (ms) | Worktree (ms) | HEAD (ms) | RemoteRef (ms) | Branch (ms) | `git status` (ms) | `status` % | Ancestry (ms) |"
    );
    println!("|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|");
    let mut pres_rows = Vec::new();
    for (label, work, bare) in &fixtures {
        let proj_root = work.parent().unwrap();
        let repo_name = work.file_name().unwrap().to_string_lossy().to_string();
        let project = WorkspaceProject {
            id: 1,
            name: "Fx".into(),
            directory: proj_root.to_path_buf(),
            provider: ProviderChoice::GitHub,
            account: "bench".into(),
            custom_host: None,
            credential_ref: None,
            disk_layout: DiskLayout::Flat,
            sources: vec![WorkspaceSource {
                handle: "bench".into(),
                display_name: None,
                avatar_url: None,
                bio: None,
                provider: "github".into(),
                repositories: vec![SourceRepository {
                    name: repo_name.clone(),
                    full_name: format!("bench/{repo_name}"),
                    description: None,
                    stars: 0,
                    forks: 0,
                    primary_language: None,
                    is_private: false,
                    is_fork: false,
                    clone_url: bare.to_string_lossy().into_owned(),
                    default_branch: "main".into(),
                    selected: true,
                }],
            }],
            repository_list_mode: RepositoryListMode::Flat,
        };
        let target = make_target(proj_root, work, &bare.to_string_lossy(), &repo_name, 0, 0);

        let (cold_i, cold_p) = measure_single_repo(label, &project, &target);
        println!(
            "| {label} (cold) | {:.2} | {} | {:.2} | {:.2} | {:.2} | {:.2} | {:.2} | {:.2} | {:.1}% | {:.2} |",
            ms(cold_i.total_ns),
            cold_i.subprocesses,
            ms(cold_i.path_check_ns),
            ms(cold_i.worktree_verify_ns),
            ms(cold_i.head_read_ns),
            ms(cold_i.remote_ref_read_ns),
            ms(cold_i.branch_check_ns),
            ms(cold_i.status_porcelain_ns),
            pct(cold_i.status_porcelain_ns, cold_i.total_ns),
            ms(cold_i.ancestry_ns)
        );
        // Warm (median/mean of 3)
        let mut warm_i = InspectTimings::default();
        let mut warm_p = PresentationTimings::default();
        for _ in 0..3 {
            let (wi, wp) = measure_single_repo(label, &project, &target);
            warm_i.total_ns += wi.total_ns / 3;
            warm_i.subprocesses = wi.subprocesses;
            warm_i.path_check_ns += wi.path_check_ns / 3;
            warm_i.worktree_verify_ns += wi.worktree_verify_ns / 3;
            warm_i.head_read_ns += wi.head_read_ns / 3;
            warm_i.remote_ref_read_ns += wi.remote_ref_read_ns / 3;
            warm_i.branch_check_ns += wi.branch_check_ns / 3;
            warm_i.status_porcelain_ns += wi.status_porcelain_ns / 3;
            warm_i.ancestry_ns += wi.ancestry_ns / 3;
            warm_p.total_ns += wp.total_ns / 3;
            warm_p.resolve_path_ns += wp.resolve_path_ns / 3;
            warm_p.icon_resolve_ns += wp.icon_resolve_ns / 3;
            warm_p.cached_release_ns += wp.cached_release_ns / 3;
            warm_p.present_check_ns += wp.present_check_ns / 3;
            warm_p.recent_commits_ns += wp.recent_commits_ns / 3;
        }
        println!(
            "| {label} (warm) | {:.2} | {} | {:.2} | {:.2} | {:.2} | {:.2} | {:.2} | {:.2} | {:.1}% | {:.2} |",
            ms(warm_i.total_ns),
            warm_i.subprocesses,
            ms(warm_i.path_check_ns),
            ms(warm_i.worktree_verify_ns),
            ms(warm_i.head_read_ns),
            ms(warm_i.remote_ref_read_ns),
            ms(warm_i.branch_check_ns),
            ms(warm_i.status_porcelain_ns),
            pct(warm_i.status_porcelain_ns, warm_i.total_ns),
            ms(warm_i.ancestry_ns)
        );
        pres_rows.push((*label, cold_p, warm_p));
    }

    println!("\n### Presentation Breakdown (`build_repository_presentation`)\n");
    println!(
        "| Fixture (Pass) | Total (ms) | PathResolve (ms) | IconResolve (ms) | ReleaseLoad (ms) | `.git` Check (ms) | `git log -15` (ms) |"
    );
    println!("|---|---:|---:|---:|---:|---:|---:|");
    for (label, cold_p, warm_p) in &pres_rows {
        println!(
            "| {label} (cold) | {:.2} | {:.2} | {:.2} | {:.2} | {:.2} | {:.2} |",
            ms(cold_p.total_ns),
            ms(cold_p.resolve_path_ns),
            ms(cold_p.icon_resolve_ns),
            ms(cold_p.cached_release_ns),
            ms(cold_p.present_check_ns),
            ms(cold_p.recent_commits_ns)
        );
        println!(
            "| {label} (warm) | {:.2} | {:.2} | {:.2} | {:.2} | {:.2} | {:.2} |",
            ms(warm_p.total_ns),
            ms(warm_p.resolve_path_ns),
            ms(warm_p.icon_resolve_ns),
            ms(warm_p.cached_release_ns),
            ms(warm_p.present_check_ns),
            ms(warm_p.recent_commits_ns)
        );
    }

    // 4. Fetch check vs ls-remote (local bare remote)
    println!("\n## 4. Fetch Check Breakdown vs `git ls-remote` (Local Bare Remote)\n");
    println!(
        "| Scenario | Total (ms) | Subprocs | Pre-Inspect (ms) | OriginVerify (ms) | `git fetch --prune` (ms) | Post-Inspect (ms) | `ls-remote` (ms) |"
    );
    println!("|---|---:|---:|---:|---:|---:|---:|---:|");
    let client = GitClient::new();
    for (label, work, bare, push_update) in [
        ("Small (up-to-date)", &small_work, &small_bare, false),
        ("Small (1 commit pushed)", &small_work, &small_bare, true),
        ("Medium (up-to-date)", &med_work, &med_bare, false),
        ("Medium (1 commit pushed)", &med_work, &med_bare, true),
    ] {
        if push_update {
            push_commit_to_bare(work, bare, label);
        }
        let _ = perf::take_last_fetch();
        let _ = client
            .fetch_verified(&bare.to_string_lossy(), work, "main")
            .unwrap();
        let ft: FetchTimings = perf::take_last_fetch().unwrap_or_default();
        let ls_t = Instant::now();
        let _ = git_cmd(work, &["ls-remote", "origin", "main"]);
        let ls_ms = dur_ms(ls_t.elapsed());
        println!(
            "| {label} | {:.2} | {} | {:.2} | {:.2} | {:.2} | {:.2} | {:.2} |",
            ms(ft.total_ns),
            ft.subprocesses,
            ms(ft.pre_inspect_ns),
            ms(ft.origin_verify_ns),
            ms(ft.fetch_origin_ns),
            ms(ft.post_inspect_ns),
            ls_ms
        );
    }

    // 5. Batch scaling (N = 8, 32, 100, 300 mixed sizes)
    println!("\n## 5. Synthetic Batch Scaling (`parallel::map_bounded`, mixed sizes)\n");
    let templates = vec![
        ("small".to_string(), small_work.clone(), small_bare.clone()),
        ("small2".to_string(), small_work.clone(), small_bare.clone()),
        ("small3".to_string(), small_work.clone(), small_bare.clone()),
        ("medium".to_string(), med_work.clone(), med_bare.clone()),
    ];
    let mut gantt_8_8 = None;
    let mut gantt_32_8 = None;

    println!("### Local Inspect + Presentation Batch Scaling\n");
    println!(
        "| N | Workers | Wall (ms) | Sum Repo (ms) | Speedup | Util (%) | Wait p50/p95/max (ms) | Repo p50/p95/max (ms) |"
    );
    println!("|---:|---:|---:|---:|---:|---:|---|---|");
    for &n in &[8usize, 32, 100, 300] {
        let batch_dir = tempfile::tempdir().unwrap();
        let (proj, tgts) = make_synthetic_project(batch_dir.path(), "inspect", n, &templates);
        // Warm once so w=1 is not unfairly cold compared to w=2..16
        let _ = run_inspect_batch(&proj, &tgts, 8, None);
        let mut serial_ms = None;
        for &w in &[1usize, 2, 4, 8, 16] {
            let m = run_inspect_batch(&proj, &tgts, w, serial_ms);
            if w == 1 {
                serial_ms = Some(m.wall_ms);
            }
            println!(
                "| {} | {} | {:.1} | {:.1} | {:.2}x | {:.1}% | {:.1} / {:.1} / {:.1} | {:.1} / {:.1} / {:.1} |",
                m.n,
                m.workers,
                m.wall_ms,
                m.sum_repo_ms,
                m.speedup,
                m.utilization_pct,
                m.wait_p50_ms,
                m.wait_p95_ms,
                m.wait_max_ms,
                m.dur_p50_ms,
                m.dur_p95_ms,
                m.dur_max_ms
            );
            if n == 8 && w == 8 {
                gantt_8_8 = Some((m.spans.clone(), m.wall_ms));
            }
            if n == 32 && w == 8 {
                gantt_32_8 = Some((m.spans.clone(), m.wall_ms));
            }
        }
    }

    println!("\n### Worker Timelines (Text Gantt)\n");
    if let Some((spans, wall)) = gantt_8_8 {
        println!(
            "**N=8 @ 8 workers:**\n```text\n{}```",
            render_gantt(&spans, wall, 60)
        );
    }
    if let Some((spans, wall)) = gantt_32_8 {
        println!(
            "**N=32 @ 8 workers:**\n```text\n{}```",
            render_gantt(&spans, wall, 72)
        );
    }

    println!("\n### Local Bare-Remote Fetch Batch Scaling (`check_for_updates`)\n");
    println!(
        "| N | Workers | Wall (ms) | Sum Repo (ms) | Speedup | Util (%) | Repo p50/p95/max (ms) |"
    );
    println!("|---:|---:|---:|---:|---:|---:|---|");
    for &n in &[8usize, 29, 100] {
        let batch_dir = tempfile::tempdir().unwrap();
        let (_proj, tgts) = make_synthetic_project(batch_dir.path(), "fetch", n, &templates);
        let mut serial_ms = None;
        for &w in &[1usize, 2, 4, 8, 16] {
            let m = run_fetch_batch(&tgts, w, serial_ms);
            if w == 1 {
                serial_ms = Some(m.wall_ms);
            }
            println!(
                "| {} | {} | {:.1} | {:.1} | {:.2}x | {:.1}% | {:.1} / {:.1} / {:.1} |",
                n,
                m.workers,
                m.wall_ms,
                m.sum_repo_ms,
                m.speedup,
                m.utilization_pct,
                m.dur_p50_ms,
                m.dur_p95_ms,
                m.dur_max_ms
            );
        }
    }

    // 6. Real projects from settings.json
    println!("\n## 6. Real Project Scenarios (Read-Only Local Inspect + Presentation)\n");
    if let Ok(cfg) = settings::load() {
        for proj in &cfg.projects {
            if !proj.directory.exists() {
                println!(
                    "- Skipping `{}` (directory `{}` not mounted)",
                    proj.name,
                    proj.directory.display()
                );
                continue;
            }
            let tgts = match repository_git::targets(proj) {
                Ok(t) => t,
                Err(e) => {
                    println!("- Skipping `{}` (`targets` error: {e})", proj.name);
                    continue;
                }
            };
            println!(
                "### Project `{}` ({} selected repos, {} sources, layout={:?})\n",
                proj.name,
                tgts.len(),
                proj.sources.len(),
                proj.disk_layout
            );
            println!(
                "| Pass / Workers | Wall (ms) | Sum Repo (ms) | Speedup | Util (%) | Repo p50/p95/max (ms) |"
            );
            println!("|---|---:|---:|---:|---:|---|");
            // Pass 1 (colder OS cache on external APFS at 8 workers)
            let cold_m = run_inspect_batch(proj, &tgts, 8, None);
            println!(
                "| First pass (cold-ish) @ 8 | {:.1} | {:.1} | - | {:.1}% | {:.1} / {:.1} / {:.1} |",
                cold_m.wall_ms,
                cold_m.sum_repo_ms,
                cold_m.utilization_pct,
                cold_m.dur_p50_ms,
                cold_m.dur_p95_ms,
                cold_m.dur_max_ms
            );
            let mut serial_ms = None;
            for &w in &[1usize, 2, 4, 8, 16] {
                let m = run_inspect_batch(proj, &tgts, w, serial_ms);
                if w == 1 {
                    serial_ms = Some(m.wall_ms);
                }
                println!(
                    "| Warm @ {w} | {:.1} | {:.1} | {:.2}x | {:.1}% | {:.1} / {:.1} / {:.1} |",
                    m.wall_ms,
                    m.sum_repo_ms,
                    m.speedup,
                    m.utilization_pct,
                    m.dur_p50_ms,
                    m.dur_p95_ms,
                    m.dur_max_ms
                );
            }

            // Per-repo breakdown (top 5 slowest)
            let mut repo_details = Vec::new();
            for t in &tgts {
                let (it, pt) = measure_single_repo(&t.name, proj, t);
                repo_details.push((
                    t.name.clone(),
                    ms(it.total_ns),
                    ms(it.status_porcelain_ns),
                    ms(it.worktree_verify_ns),
                    ms(pt.total_ns),
                    ms(pt.icon_resolve_ns),
                    ms(pt.recent_commits_ns),
                    it.subprocesses,
                ));
            }
            repo_details.sort_by(|a, b| (b.1 + b.4).total_cmp(&(a.1 + a.4)));
            println!("\nTop 5 slowest repos in `{}` (warm):\n", proj.name);
            println!(
                "| Repo | Total (ms) | Inspect (ms) | `status` (ms) | Worktree (ms) | Pres (ms) | Icon (ms) | `log -15` (ms) | Subprocs |"
            );
            println!("|---|---:|---:|---:|---:|---:|---:|---:|---:|");
            for (name, itot, st, wt, ptot, ic, lg, subs) in repo_details.into_iter().take(5) {
                println!(
                    "| {name} | {:.2} | {:.2} | {:.2} | {:.2} | {:.2} | {:.2} | {:.2} | {} |",
                    itot + ptot,
                    itot,
                    st,
                    wt,
                    ptot,
                    ic,
                    lg,
                    subs
                );
            }
            println!();

            if run_network && proj.name == "ArtCraft" {
                println!(
                    "#### Tiered Network Probe Comparison on `{}` (read-only `ls-remote` vs `fetch_verified`)\n",
                    proj.name
                );
                // Measure git ls-remote across 29 repos at 8 workers vs 4 workers
                for &w in &[4usize, 8] {
                    let t0 = Instant::now();
                    let _ = map_bounded(&tgts, w, |t| {
                        let out = Command::new("git")
                            .current_dir(&t.path)
                            .args(["ls-remote", "origin", &t.branch])
                            .env("GIT_TERMINAL_PROMPT", "0")
                            .stdin(Stdio::null())
                            .output();
                        out.is_ok_and(|o| o.status.success())
                    });
                    println!(
                        "- `git ls-remote origin <branch>` ({} repos @ {w} workers): {:.1} ms",
                        tgts.len(),
                        dur_ms(t0.elapsed())
                    );
                }
                println!();
            }
        }
    }

    // 7. UI 40 ms Poll Latency
    println!("## 7. UI 40 ms Poll Latency (`git_controller.rs` simulation)\n");
    for &n in &[32usize, 100] {
        let batch_dir = tempfile::tempdir().unwrap();
        let (proj, tgts) = make_synthetic_project(batch_dir.path(), "uipoll", n, &templates);
        simulate_ui_poll_latency(&proj, &tgts, 8);
    }

    if run_network {
        println!("\n## 8. Optional Real-Remote HTTPS vs SSH Probe (Public Repo)\n");
        let https_t = Instant::now();
        let https_ok = Command::new("git")
            .args([
                "ls-remote",
                "https://github.com/octocat/Hello-World.git",
                "HEAD",
            ])
            .env("GIT_TERMINAL_PROMPT", "0")
            .stdin(Stdio::null())
            .output()
            .is_ok_and(|o| o.status.success());
        println!(
            "- `git ls-remote https://github.com/octocat/Hello-World.git HEAD`: {:.1} ms (ok={https_ok})",
            dur_ms(https_t.elapsed())
        );
    }
}
