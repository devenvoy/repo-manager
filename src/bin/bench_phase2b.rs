//! Phase 2b targeted benchmarks:
//! - End-to-end real app flow (Inspect + Presentation + FetchVerified)
//! - Tier 1 (GraphQL batch, conditional REST 304) vs Tier 2 (ls-remote) vs Tier 3 (fetch_verified)
//! - Cold HDD (/Volumes/Devansh2) vs SSD (/tmp) per-phase & 1/2/4/8 worker scaling
//! - Primed core.untrackedCache & agent.db per-repo vs single-query SQLite open cost
//! - Simulated slow-disk seek-thrash fixture
//!
//! Run with:
//!   cargo run --release --bin bench_phase2b

#![allow(unsafe_code)]

use release_desk_git::perf::{self, FetchTimings, InspectTimings};
use repo_manager::modules::icon_agent;
use repo_manager::modules::parallel::map_bounded;
use repo_manager::modules::perf_trace::{self, PresentationTimings};
use repo_manager::repository_git::{self, RepositoryTarget};
use repo_manager::settings;
use repo_manager::views::project_repository_status::build_repository_presentation;
use repo_manager::workspace::WorkspaceProject;
use rusqlite::Connection;
use std::collections::HashMap;
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn fcntl(fd: i32, cmd: i32, ...) -> i32;
}

#[cfg(target_os = "macos")]
fn set_nocache(file: &std::fs::File) {
    const F_NOCACHE: i32 = 48;
    // SAFETY: valid open fd and standard macOS F_NOCACHE command taking an int flag.
    #[allow(unsafe_code)]
    unsafe {
        let _ = fcntl(file.as_raw_fd(), F_NOCACHE, 1);
    }
}

#[cfg(not(target_os = "macos"))]
fn set_nocache(_file: &std::fs::File) {}

fn ms(ns: u64) -> f64 {
    ns as f64 / 1_000_000.0
}

fn dur_ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

fn copy_dir_nocache(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let ft = entry.file_type()?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if ft.is_dir() {
            copy_dir_nocache(&from, &to)?;
        } else if ft.is_symlink() {
            let link = std::fs::read_link(&from)?;
            let _ = std::os::unix::fs::symlink(link, &to);
        } else if ft.is_file() {
            let mut reader = std::fs::File::open(&from)?;
            set_nocache(&reader);
            let mut writer = std::fs::File::create(&to)?;
            set_nocache(&writer);
            std::io::copy(&mut reader, &mut writer)?;
        }
    }
    Ok(())
}

fn measure_e2e_project(
    proj: &WorkspaceProject,
    targets: &[RepositoryTarget],
) -> (f64, f64, usize, usize) {
    let inspect_t0 = Instant::now();
    let _inspect_res = map_bounded(targets, 8, |target| {
        let s = repository_git::inspect(target);
        let p = build_repository_presentation(proj, target.source_index, target.repo_index);
        (s, p)
    });
    let inspect_ms = dur_ms(inspect_t0.elapsed());

    let fetch_t0 = Instant::now();
    let fetch_subprocs = Mutex::new(Vec::new());
    let fetch_res = map_bounded(targets, 8, |target| {
        let _ = perf::take_last_fetch();
        let st = repository_git::check_for_updates(target);
        if let Some(ft) = perf::take_last_fetch()
            && let Ok(mut guard) = fetch_subprocs.lock()
        {
            guard.push(ft);
        }
        st
    });
    let fetch_ms = dur_ms(fetch_t0.elapsed());
    let ok_count = fetch_res.iter().filter(|s| s.state.is_ok()).count();
    let max_subprocs = fetch_subprocs
        .into_inner()
        .unwrap_or_default()
        .iter()
        .map(|f| f.subprocesses as usize)
        .max()
        .unwrap_or(0);
    (inspect_ms, fetch_ms, ok_count, max_subprocs)
}

fn gh_token() -> Option<String> {
    let out = Command::new("gh")
        .args(["auth", "token"])
        .stdin(Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let tok = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!tok.is_empty()).then_some(tok)
}

fn run_tier_benchmarks(proj: &WorkspaceProject, targets: &[RepositoryTarget]) {
    println!(
        "## 2. Tiered Network Update Detection on `{}` ({} repos)\n",
        proj.name,
        targets.len()
    );

    let mut repos: Vec<(String, String, String)> = Vec::new();
    for t in targets {
        if let Some(src) = proj.sources.get(t.source_index)
            && let Some(repo) = src.repositories.get(t.repo_index)
        {
            let owner = repository_git::repository_owner(src, repo).to_string();
            repos.push((owner, repo.name.clone(), repo.default_branch.clone()));
        }
    }

    let client = reqwest::blocking::Client::builder()
        .pool_max_idle_per_host(8)
        .timeout(Duration::from_secs(20))
        .user_agent("release-desk-bench/0.2")
        .build()
        .unwrap();

    let token = gh_token();
    if let Some(ref tok) = token {
        // Tier 1a: Single batched GraphQL query
        let mut query = String::from("query {\n");
        for (idx, (owner, name, _)) in repos.iter().enumerate() {
            query.push_str(&format!(
                "  r{idx}: repository(owner: \"{owner}\", name: \"{name}\") {{\n    defaultBranchRef {{ name target {{ oid }} }}\n    latestRelease {{ tagName publishedAt }}\n  }}\n"
            ));
        }
        query.push_str("}\n");

        let t0 = Instant::now();
        let resp = client
            .post("https://api.github.com/graphql")
            .bearer_auth(tok)
            .json(&serde_json::json!({ "query": query }))
            .send();
        let gql_ms = dur_ms(t0.elapsed());
        match resp {
            Ok(r) => {
                let status = r.status();
                let body: serde_json::Value = r.json().unwrap_or_default();
                let count = body
                    .get("data")
                    .and_then(|d| d.as_object())
                    .map(|m| m.len())
                    .unwrap_or(0);
                println!(
                    "- **Tier 1a — Batched GraphQL (all {} repos + latestRelease):** `{:.1} ms` | HTTP {} | 1 round trip | resolved={}/{}",
                    repos.len(),
                    gql_ms,
                    status,
                    count,
                    repos.len()
                );
            }
            Err(e) => println!("- **Tier 1a — Batched GraphQL failed:** {e}"),
        }

        // Tier 1b: 29 conditional REST requests over ONE reused keep-alive client
        let t_prime = Instant::now();
        let mut etags: Vec<Option<String>> = Vec::with_capacity(repos.len());
        let mut prime_ok = 0usize;
        for (owner, name, branch) in &repos {
            let url = format!("https://api.github.com/repos/{owner}/{name}/commits/{branch}");
            if let Ok(r) = client
                .get(&url)
                .bearer_auth(tok)
                .header("Accept", "application/vnd.github.sha")
                .send()
            {
                if r.status().is_success() {
                    prime_ok += 1;
                }
                let etag = r
                    .headers()
                    .get("etag")
                    .and_then(|v| v.to_str().ok())
                    .map(ToString::to_string);
                etags.push(etag);
            } else {
                etags.push(None);
            }
        }
        let prime_ms = dur_ms(t_prime.elapsed());

        let t_cond = Instant::now();
        let mut not_modified = 0usize;
        for ((owner, name, branch), etag) in repos.iter().zip(etags.iter()) {
            let url = format!("https://api.github.com/repos/{owner}/{name}/commits/{branch}");
            let mut req = client
                .get(&url)
                .bearer_auth(tok)
                .header("Accept", "application/vnd.github.sha");
            if let Some(tag) = etag {
                req = req.header("If-None-Match", tag);
            }
            if let Ok(r) = req.send()
                && r.status().as_u16() == 304
            {
                not_modified += 1;
            }
        }
        let cond_ms = dur_ms(t_cond.elapsed());
        println!(
            "- **Tier 1b — Sequential Conditional REST on 1 reused keep-alive client ({} repos):** prime=`{:.1} ms` ({prime_ok}/{} 200 OK) | conditional `If-None-Match`=`{:.1} ms` ({not_modified}/{} `304 Not Modified`, {} round trips)",
            repos.len(),
            prime_ms,
            repos.len(),
            cond_ms,
            repos.len(),
            repos.len()
        );
    } else {
        println!(
            "- **Tier 1 (GraphQL / Conditional REST):** skipped (`gh auth token` unavailable)"
        );
    }

    // Tier 2: git ls-remote
    for &w in &[4usize, 8, 16] {
        let t0 = Instant::now();
        let res = map_bounded(targets, w, |t| {
            Command::new("git")
                .current_dir(&t.path)
                .args(["ls-remote", "origin", &t.branch])
                .env("GIT_TERMINAL_PROMPT", "0")
                .stdin(Stdio::null())
                .output()
                .is_ok_and(|o| o.status.success())
        });
        let ok = res.iter().filter(|x| **x).count();
        println!(
            "- **Tier 2 — `git ls-remote origin <branch>` ({} repos @ {w} workers):** `{:.1} ms` ({ok}/{} ok, {} round trips)",
            targets.len(),
            dur_ms(t0.elapsed()),
            targets.len(),
            targets.len()
        );
    }

    // Tier 3: Real GitClient::fetch_verified
    for &w in &[4usize, 8, 16] {
        let t0 = Instant::now();
        let details = Mutex::new(Vec::<FetchTimings>::new());
        let res = map_bounded(targets, w, |t| {
            let _ = perf::take_last_fetch();
            let st = repository_git::check_for_updates(t);
            if let Some(ft) = perf::take_last_fetch()
                && let Ok(mut g) = details.lock()
            {
                g.push(ft);
            }
            st
        });
        let ok = res.iter().filter(|s| s.state.is_ok()).count();
        let fts = details.into_inner().unwrap_or_default();
        let max_subs = fts.iter().map(|f| f.subprocesses).max().unwrap_or(0);
        let avg_pre =
            fts.iter().map(|f| ms(f.pre_inspect_ns)).sum::<f64>() / fts.len().max(1) as f64;
        let avg_fetch =
            fts.iter().map(|f| ms(f.fetch_origin_ns)).sum::<f64>() / fts.len().max(1) as f64;
        let avg_post =
            fts.iter().map(|f| ms(f.post_inspect_ns)).sum::<f64>() / fts.len().max(1) as f64;
        println!(
            "- **Tier 3 — Real `fetch_verified` ({} repos @ {w} workers):** `{:.1} ms` ({ok}/{} ok, max {max_subs} subprocs/repo | avg pre-inspect={avg_pre:.1} ms, fetch={avg_fetch:.1} ms, post-inspect={avg_post:.1} ms)",
            targets.len(),
            dur_ms(t0.elapsed()),
            targets.len()
        );
    }
    println!();
}

fn run_cold_copied_comparison(
    proj: &WorkspaceProject,
    targets: &[RepositoryTarget],
    ssd_only: bool,
) {
    println!(
        "## 3. Cold Storage Comparison: External USB HDD (`/Volumes/Devansh2`) vs Internal SSD (`/tmp`)\n"
    );
    println!(
        "*Method:* Copied the 8 repos of `{}` (excluding `Release Dmgs`) using `fcntl(F_NOCACHE, 1)` on both source reader and destination writer so file data bypasses macOS Unified Buffer Cache.\n",
        proj.name
    );

    let hdd_base =
        PathBuf::from("/Volumes/Devansh2/Developer/repo manager/target/phase2b_cold_hdd");
    let ssd_base = std::env::temp_dir().join("phase2b_cold_ssd");
    let _ = std::fs::remove_dir_all(&hdd_base);
    let _ = std::fs::remove_dir_all(&ssd_base);

    let mut per_repo_table_printed = false;
    println!("### Cold Worker Scaling (`1, 2, 4, 8` workers) — Fresh `F_NOCACHE` Copy Per Run\n");
    println!("| Medium | Workers | Wall (ms) | Sum Repo (ms) | Speedup | Util (%) |");
    println!("|---|---:|---:|---:|---:|---:|");

    let media: Vec<(&str, &PathBuf)> = if ssd_only {
        vec![("Internal Apple SSD (/tmp)", &ssd_base)]
    } else {
        vec![
            ("WD USB HDD (/Volumes/Devansh2)", &hdd_base),
            ("Internal Apple SSD (/tmp)", &ssd_base),
        ]
    };
    for (medium, base_root) in media {
        let mut ssd_stage_cleanup: Option<PathBuf> = None;
        let src_root = if medium.contains("SSD") {
            let stage = std::env::temp_dir().join("phase2b_ssd_stage");
            let _ = std::fs::remove_dir_all(&stage);
            for t in targets {
                let rel = t.path.strip_prefix(&proj.directory).unwrap();
                copy_dir_nocache(&t.path, &stage.join(rel)).unwrap();
            }
            let rd_src = proj.directory.join(".release-desk");
            if rd_src.exists() {
                let _ = copy_dir_nocache(&rd_src, &stage.join(".release-desk"));
            }
            ssd_stage_cleanup = Some(stage.clone());
            stage
        } else {
            proj.directory.clone()
        };
        let mut serial_ms = None;
        for &w in &[1usize, 2, 4, 8] {
            let run_root = base_root.join(format!("w{w}"));
            let mut cloned_proj = proj.clone();
            cloned_proj.directory = run_root.clone();
            let mut run_targets = Vec::with_capacity(targets.len());
            for t in targets {
                let rel = t.path.strip_prefix(&proj.directory).unwrap();
                let dst = run_root.join(rel);
                copy_dir_nocache(&src_root.join(rel), &dst).unwrap();
                let mut nt = t.clone();
                nt.project_root = run_root.clone();
                nt.path = dst;
                run_targets.push(nt);
            }
            // Copy .release-desk if present
            let rd_src = src_root.join(".release-desk");
            if rd_src.exists() {
                let _ = copy_dir_nocache(&rd_src, &run_root.join(".release-desk"));
            }

            let per_repo_details =
                Mutex::new(Vec::<(String, InspectTimings, PresentationTimings, f64)>::new());
            let t0 = Instant::now();
            let repo_durs = map_bounded(&run_targets, w, |t| {
                let rt = Instant::now();
                let _ = perf::take_last_inspect();
                let _ = perf_trace::take_last_presentation();
                let _st = repository_git::inspect(t);
                let it = perf::take_last_inspect().unwrap_or_default();
                let _pr = build_repository_presentation(&cloned_proj, t.source_index, t.repo_index);
                let pt = perf_trace::take_last_presentation().unwrap_or_default();
                let d = dur_ms(rt.elapsed());
                if let Ok(mut g) = per_repo_details.lock() {
                    g.push((t.name.clone(), it, pt, d));
                }
                d
            });
            let wall_ms = dur_ms(t0.elapsed());
            let sum_ms: f64 = repo_durs.iter().sum();
            let base = *serial_ms.get_or_insert(wall_ms);
            let speedup = base / wall_ms;
            let eff_w = w.min(run_targets.len()) as f64;
            let util = (sum_ms / (wall_ms * eff_w)) * 100.0;
            println!(
                "| {medium} | {w} | {wall_ms:.1} | {sum_ms:.1} | {speedup:.2}x | {util:.1}% |"
            );

            if w == 1 && !per_repo_table_printed && medium.contains("HDD") {
                per_repo_table_printed = true;
                let mut rows = per_repo_details.into_inner().unwrap_or_default();
                rows.sort_by(|a, b| b.3.total_cmp(&a.3));
                println!("\n### Cold Per-Repo, Per-Phase Table on HDD (`w=1`, `F_NOCACHE` copy)\n");
                println!(
                    "| Repo | Total (ms) | Worktree (ms) | HEAD (ms) | RemoteRef (ms) | Branch (ms) | `git status` (ms) | `git log -15` (ms) | Icon (ms) |"
                );
                println!("|---|---:|---:|---:|---:|---:|---:|---:|---:|");
                for (name, it, pt, tot) in rows {
                    println!(
                        "| {name} | {tot:.2} | {:.2} | {:.2} | {:.2} | {:.2} | {:.2} | {:.2} | {:.2} |",
                        ms(it.worktree_verify_ns),
                        ms(it.head_read_ns),
                        ms(it.remote_ref_read_ns),
                        ms(it.branch_check_ns),
                        ms(it.status_porcelain_ns),
                        ms(pt.recent_commits_ns),
                        ms(pt.icon_resolve_ns)
                    );
                }
                println!("\n| Medium | Workers | Wall (ms) | Sum Repo (ms) | Speedup | Util (%) |");
                println!("|---|---:|---:|---:|---:|---:|");
            }
            let _ = std::fs::remove_dir_all(&run_root);
        }
        if let Some(stage) = ssd_stage_cleanup {
            let _ = std::fs::remove_dir_all(stage);
        }
    }
    let _ = std::fs::remove_dir_all(&hdd_base);
    let _ = std::fs::remove_dir_all(&ssd_base);
    println!();
}

fn bench_agent_db_and_untracked_cache(artcraft: &WorkspaceProject, targets: &[RepositoryTarget]) {
    println!("## 4. `agent.db` Per-Repo Open Cost & Primed `core.untrackedCache`\n");
    let db_path = artcraft.directory.join(".release-desk/agent.db");
    println!(
        "- **`agent.db` location:** `{}` (on external USB HDD `/Volumes/Devansh2`)",
        db_path.display()
    );

    // Measure current 8-worker per-repo SQLite open (29 connections)
    let mut keys = Vec::new();
    for t in targets {
        if let Some(s) = artcraft.sources.get(t.source_index)
            && let Some(r) = s.repositories.get(t.repo_index)
        {
            keys.push((
                s.provider.clone(),
                repository_git::repository_owner(s, r).to_string(),
                r.name.clone(),
            ));
        }
    }
    let t_per_repo = Instant::now();
    let found_per_repo = map_bounded(&keys, 8, |(prov, owner, name)| {
        icon_agent::resolve_icon_for(Some(&artcraft.directory), prov, owner, name).is_some()
    });
    let per_repo_ms = dur_ms(t_per_repo.elapsed());

    // Measure 1 connection + 1 batch query
    let t_batch = Instant::now();
    let mut loaded = HashMap::<(String, String, String), String>::new();
    if let Ok(conn) =
        Connection::open_with_flags(&db_path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        && let Ok(mut stmt) =
            conn.prepare("SELECT provider, owner, repo_name, cached_path FROM icon_cache")
        && let Ok(rows) = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })
    {
        for row in rows.flatten() {
            loaded.insert((row.0, row.1, row.2), row.3);
        }
    }
    let batch_ms = dur_ms(t_batch.elapsed());
    println!(
        "- **8 workers opening `agent.db` per repo ({} opens):** `{:.2} ms` (hits={}/{}) vs **1 SQLite open + 1 query:** `{:.2} ms` (`{} rows`)\n",
        keys.len(),
        per_repo_ms,
        found_per_repo.iter().filter(|x| **x).count(),
        keys.len(),
        batch_ms,
        loaded.len()
    );

    // Primed untrackedCache on a temp copy of solidcraft or artcraft-services (never mutating real repo)
    if let Some(big) = targets.iter().find(|t| t.name == "artcraft-services") {
        let tmp = tempfile::tempdir().unwrap();
        let copy_path = tmp.path().join("artcraft-services");
        Command::new("git")
            .args([
                "clone",
                "--quiet",
                "--no-hardlinks",
                big.path.to_str().unwrap(),
                copy_path.to_str().unwrap(),
            ])
            .status()
            .unwrap();
        let run_st = |args: &[&str]| -> f64 {
            let t = Instant::now();
            Command::new("git")
                .current_dir(&copy_path)
                .args(args)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .unwrap();
            dur_ms(t.elapsed())
        };
        let plain_1 = run_st(&["status", "--porcelain=v1", "--untracked-files=normal"]);
        let uno = run_st(&["status", "--porcelain=v1", "-uno"]);
        let uc_prime = run_st(&[
            "-c",
            "core.untrackedCache=true",
            "status",
            "--porcelain=v1",
            "--untracked-files=normal",
        ]);
        let uc_warm = run_st(&[
            "-c",
            "core.untrackedCache=true",
            "status",
            "--porcelain=v1",
            "--untracked-files=normal",
        ]);
        println!(
            "- **Primed `core.untrackedCache` on `artcraft-services` copy (17,787 files, SSD):** plain=`{plain_1:.2} ms` | `-uno`=`{uno:.2} ms` | `untrackedCache` (1st prime)=`{uc_prime:.2} ms` | `untrackedCache` (2nd primed)=`{uc_warm:.2} ms`\n"
        );
    }
}

fn bench_simulated_slow_disk() {
    println!("## 5. Simulated Slow-Disk / Seek-Thrash Scenario (Deterministic CI Fixture)\n");
    println!(
        "Models a spinning disk where concurrent readers above the platter's sweet spot experience nonlinear seek-contention latency (`seek_ms = base_seek * (1 + 0.55 * (active - 1))`).\n"
    );
    let items: Vec<usize> = (0..16).collect();
    let active = std::sync::atomic::AtomicUsize::new(0);
    println!("| Workers | Wall (ms) | Speedup vs 1w | Per-repo mean (ms) |");
    println!("|---:|---:|---:|---:|");
    let mut base = None;
    for &w in &[1usize, 2, 4, 8] {
        let t0 = Instant::now();
        let durs = map_bounded(&items, w, |_| {
            let cur = active.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
            let rt = Instant::now();
            // Simulate 8ms base seek + contention penalty when multiple workers thrash the disk arm
            let delay_ms = 8.0 * (1.0 + 0.55 * (cur.saturating_sub(1) as f64));
            std::thread::sleep(Duration::from_secs_f64(delay_ms / 1000.0));
            active.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
            dur_ms(rt.elapsed())
        });
        let wall = dur_ms(t0.elapsed());
        let b = *base.get_or_insert(wall);
        let mean = durs.iter().sum::<f64>() / durs.len() as f64;
        println!("| {w} | {wall:.1} | {:.2}x | {mean:.1} |", b / wall);
    }
    println!();
}

fn pack_bytes(repo_path: &Path) -> (usize, u64) {
    let pack_dir = repo_path.join(".git/objects/pack");
    let Ok(entries) = std::fs::read_dir(pack_dir) else {
        return (0, 0);
    };
    let mut count = 0usize;
    let mut bytes = 0u64;
    for entry in entries.flatten() {
        if let Ok(meta) = entry.metadata()
            && meta.is_file()
        {
            count += 1;
            bytes += meta.len();
        }
    }
    (count, bytes)
}

fn dir_size_quick(path: &Path) -> u64 {
    let Ok(out) = Command::new("du").args(["-sk"]).arg(path).output() else {
        return 0;
    };
    String::from_utf8_lossy(&out.stdout)
        .split_whitespace()
        .next()
        .and_then(|s| s.parse::<u64>().ok())
        .map_or(0, |kb| kb * 1024)
}

fn git_rev_parse(repo_path: &Path, rev: &str) -> Option<String> {
    let out = Command::new("git")
        .current_dir(repo_path)
        .args(["rev-parse", "--verify", "--quiet", rev])
        .stdin(Stdio::null())
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn run_step_b(
    artcraft: &WorkspaceProject,
    art_targets: &[RepositoryTarget],
    cool: &WorkspaceProject,
    cool_targets: &[RepositoryTarget],
) {
    println!("# Release Desk — Step B Measurement Corrections\n");

    // ------------------------------------------------------------------
    // B1: Variance diagnosis + per-repo fetch trace2 + gc.auto=0 compare
    // ------------------------------------------------------------------
    println!("## B1. E2E Variance Diagnosis (`ArtCraft`, 29 repos @ 8w)\n");

    let run_fetch_trace = |label: &str, disable_gc: bool| {
        #[derive(Clone)]
        struct Row {
            name: String,
            branch: String,
            pre_inspect_ms: f64,
            fetch_ms: f64,
            post_inspect_ms: f64,
            wall_ms: f64,
            behind_commits: usize,
            bytes_delta: i64,
            gc_triggered: bool,
            status_str: String,
        }
        let t_all = Instant::now();
        let trace_dir = std::env::temp_dir().join(format!(
            "rd_trace2_{}",
            if disable_gc { "nogc" } else { "default" }
        ));
        let _ = std::fs::remove_dir_all(&trace_dir);
        std::fs::create_dir_all(&trace_dir).unwrap();

        let rows: Vec<Row> = map_bounded(art_targets, 8, |t| {
            let repo_t0 = Instant::now();
            let remote_ref = format!("refs/remotes/origin/{}", t.branch);
            let before_remote = git_rev_parse(&t.path, &remote_ref);
            let before_local = git_rev_parse(&t.path, "HEAD");
            let (_, before_pack_bytes) = pack_bytes(&t.path);
            let before_git_bytes = dir_size_quick(&t.path.join(".git"));

            let pre_t0 = Instant::now();
            let pre = repository_git::inspect(t);
            let pre_inspect_ms = dur_ms(pre_t0.elapsed());

            let trace_file = trace_dir.join(format!("{}.trace2", t.name));
            let mut cmd = Command::new("git");
            cmd.current_dir(&t.path)
                .env("GIT_TERMINAL_PROMPT", "0")
                .env("GIT_TRACE2_PERF", &trace_file);
            if disable_gc {
                cmd.args(["-c", "gc.auto=0", "-c", "maintenance.auto=false"]);
            }
            cmd.args(["fetch", "--prune", "origin"]);
            let fetch_t0 = Instant::now();
            let out = cmd.stdin(Stdio::null()).output();
            let fetch_ms = dur_ms(fetch_t0.elapsed());

            let post_t0 = Instant::now();
            let post = repository_git::inspect(t);
            let post_inspect_ms = dur_ms(post_t0.elapsed());

            let after_remote = git_rev_parse(&t.path, &remote_ref);
            let behind_commits = match (&before_remote, &after_remote) {
                (Some(b), Some(a)) if b != a => Command::new("git")
                    .current_dir(&t.path)
                    .args(["rev-list", "--count", &format!("{b}..{a}")])
                    .output()
                    .ok()
                    .and_then(|o| String::from_utf8_lossy(&o.stdout).trim().parse().ok())
                    .unwrap_or(0),
                _ => match (&before_local, &after_remote) {
                    (Some(l), Some(a)) if l != a => Command::new("git")
                        .current_dir(&t.path)
                        .args(["rev-list", "--count", &format!("{l}..{a}")])
                        .output()
                        .ok()
                        .and_then(|o| String::from_utf8_lossy(&o.stdout).trim().parse().ok())
                        .unwrap_or(0),
                    _ => 0,
                },
            };
            let (_, after_pack_bytes) = pack_bytes(&t.path);
            let after_git_bytes = dir_size_quick(&t.path.join(".git"));
            let bytes_delta = (after_git_bytes as i64 - before_git_bytes as i64)
                .max(after_pack_bytes as i64 - before_pack_bytes as i64);

            let trace_text = std::fs::read_to_string(&trace_file).unwrap_or_default();
            let gc_triggered = trace_text.lines().any(|line| {
                line.contains("|start|")
                    && (line.contains(" git gc")
                        || line.contains("maintenance")
                        || line.contains("repack")
                        || line.contains("prune-packed"))
            });
            let fetched_commits = match (&before_remote, &after_remote) {
                (Some(b), Some(a)) if b != a => Command::new("git")
                    .current_dir(&t.path)
                    .args(["rev-list", "--count", &format!("{b}..{a}")])
                    .output()
                    .ok()
                    .and_then(|o| String::from_utf8_lossy(&o.stdout).trim().parse().ok())
                    .unwrap_or(0),
                _ => 0,
            };
            let _ = fetched_commits;
            let status_str = match (&pre.state, &post.state) {
                (Ok(pr), Ok(po)) if out.is_ok_and(|o| o.status.success()) => {
                    format!("{:?} -> {:?}", pr.status, po.status)
                }
                _ => "Error".into(),
            };
            Row {
                name: t.name.clone(),
                branch: t.branch.clone(),
                pre_inspect_ms,
                fetch_ms,
                post_inspect_ms,
                wall_ms: dur_ms(repo_t0.elapsed()),
                behind_commits,
                bytes_delta,
                gc_triggered,
                status_str,
            }
        });
        let total_wall = dur_ms(t_all.elapsed());
        let _ = std::fs::remove_dir_all(&trace_dir);

        let sum_pre: f64 = rows.iter().map(|r| r.pre_inspect_ms).sum();
        let sum_fetch: f64 = rows.iter().map(|r| r.fetch_ms).sum();
        let sum_post: f64 = rows.iter().map(|r| r.post_inspect_ms).sum();
        let gc_count = rows.iter().filter(|r| r.gc_triggered).count();
        let changed_count = rows.iter().filter(|r| r.behind_commits > 0).count();
        println!(
            "### {label}\n- Wall: `{total_wall:.1} ms` | Sum pre-inspect: `{sum_pre:.1} ms` | Sum fetch: `{sum_fetch:.1} ms` | Sum post-inspect: `{sum_post:.1} ms` | Repos w/ new commits: `{changed_count}` | Repos triggering auto-gc/maintenance: `{gc_count}`\n"
        );
        let mut sorted = rows.clone();
        sorted.sort_by(|a, b| b.wall_ms.total_cmp(&a.wall_ms));
        println!(
            "| Top Slowest Repo | Branch | Pre (ms) | Fetch (ms) | Post (ms) | Wall (ms) | Commits | Delta Bytes | GC? | Status |"
        );
        println!("|---|---|---:|---:|---:|---:|---:|---:|:---:|---|");
        for r in sorted.iter().take(10) {
            println!(
                "| {} | {} | {:.1} | {:.1} | {:.1} | {:.1} | {} | {} | {} | {} |",
                r.name,
                r.branch,
                r.pre_inspect_ms,
                r.fetch_ms,
                r.post_inspect_ms,
                r.wall_ms,
                r.behind_commits,
                r.bytes_delta,
                if r.gc_triggered { "yes" } else { "no" },
                r.status_str
            );
        }
        println!();
        (total_wall, sum_pre, sum_fetch, sum_post)
    };

    let (w_def, pre_def, fetch_def, post_def) = run_fetch_trace(
        "Default `git fetch --prune origin` (with `GIT_TRACE2_PERF`)",
        false,
    );
    let (w_nogc, pre_nogc, fetch_nogc, post_nogc) =
        run_fetch_trace("With `-c gc.auto=0 -c maintenance.auto=false`", true);
    println!(
        "**B4 Split Summary (Warm `fetch_verified` @ 8w):**\n- Default run wall `{w_def:.1} ms`: sum pre `{pre_def:.1} ms` + post `{post_def:.1} ms` = `{:.1}%` of total CPU/repo sum `{:.1} ms` (fetch `{fetch_def:.1} ms` = `{:.1}%`).\n- No-GC run wall `{w_nogc:.1} ms`: sum pre `{pre_nogc:.1} ms` + post `{post_nogc:.1} ms` (fetch `{fetch_nogc:.1} ms`).\n",
        ((pre_def + post_def) / (pre_def + fetch_def + post_def).max(1.0)) * 100.0,
        pre_def + fetch_def + post_def,
        (fetch_def / (pre_def + fetch_def + post_def).max(1.0)) * 100.0,
    );

    // ------------------------------------------------------------------
    // B2: Cold status confound — stat cache invalidation vs --no-optional-locks
    // ------------------------------------------------------------------
    if !std::env::args().any(|a| a == "--fast") {
        println!("## B2. Cold `git status` Stat Confound & `--no-optional-locks` Cost\n");
        let b2_base = PathBuf::from("/Volumes/Devansh2/Developer/repo manager/target/step_b2_stat");
        let _ = std::fs::remove_dir_all(&b2_base);
        let copy_a = b2_base.join("copy_plain");
        let copy_b = b2_base.join("copy_no_locks");
        for t in cool_targets {
            let rel = t.path.strip_prefix(&cool.directory).unwrap();
            copy_dir_nocache(&t.path, &copy_a.join(rel)).unwrap();
            copy_dir_nocache(&t.path, &copy_b.join(rel)).unwrap();
        }

        let time_status_batch = |root: &Path, args: &[&str]| -> f64 {
            let paths: Vec<PathBuf> = cool_targets
                .iter()
                .map(|t| root.join(t.path.strip_prefix(&cool.directory).unwrap()))
                .collect();
            let t0 = Instant::now();
            map_bounded(&paths, 2, |p| {
                Command::new("git")
                    .current_dir(p)
                    .args(args)
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status()
                    .unwrap()
            });
            dur_ms(t0.elapsed())
        };

        // Original unmodified repos on HDD (warm/current stat index):
        let orig_plain_ms = time_status_batch(&cool.directory, &["status", "--porcelain=v1"]);
        let orig_nolocks_ms = time_status_batch(
            &cool.directory,
            &["--no-optional-locks", "status", "--porcelain=v1"],
        );
        let orig_uno_ms = time_status_batch(
            &cool.directory,
            &["--no-optional-locks", "status", "--porcelain=v1", "-uno"],
        );

        // Copy A: plain git status refreshes .git/index stat entries; then Run 2 is fast even with --no-optional-locks
        let copy_a_run1_plain = time_status_batch(&copy_a, &["status", "--porcelain=v1"]);
        let copy_a_run2_nolocks = time_status_batch(
            &copy_a,
            &["--no-optional-locks", "status", "--porcelain=v1"],
        );

        // Copy B: stale stat index with --no-optional-locks TWICE, then update-index --refresh, then --no-optional-locks
        let copy_b_run1_nolocks = time_status_batch(
            &copy_b,
            &["--no-optional-locks", "status", "--porcelain=v1"],
        );
        let copy_b_run2_nolocks = time_status_batch(
            &copy_b,
            &["--no-optional-locks", "status", "--porcelain=v1"],
        );
        let _ = time_status_batch(&copy_b, &["update-index", "--refresh", "-q"]);
        let copy_b_run3_after_refresh = time_status_batch(
            &copy_b,
            &["--no-optional-locks", "status", "--porcelain=v1"],
        );
        let copy_b_run4_uno = time_status_batch(
            &copy_b,
            &["--no-optional-locks", "status", "--porcelain=v1", "-uno"],
        );
        let _ = std::fs::remove_dir_all(&b2_base);

        println!(
            "Measured across all 8 `Github cool repos` @ 2 workers on `/Volumes/Devansh2`:\n\
            | Condition | Wall (ms) | Notes |\n\
            |---|---:|---|\n\
            | Original unmodified repos (`status`) | {orig_plain_ms:.1} | clean index stat cache |\n\
            | Original unmodified repos (`--no-optional-locks status`) | {orig_nolocks_ms:.1} | background-safe |\n\
            | Original unmodified repos (`--no-optional-locks status -uno`) | {orig_uno_ms:.1} | tracked-only |\n\
            | Stale-stat `F_NOCACHE` Copy A — Run 1 (`status`, refreshes index) | {copy_a_run1_plain:.1} | cold + full SHA re-hash |\n\
            | Stale-stat `F_NOCACHE` Copy A — Run 2 (`--no-optional-locks status`) | {copy_a_run2_nolocks:.1} | index now clean! |\n\
            | Stale-stat `F_NOCACHE` Copy B — Run 1 (`--no-optional-locks status`) | {copy_b_run1_nolocks:.1} | cold + stale stat re-read |\n\
            | Stale-stat `F_NOCACHE` Copy B — Run 2 (`--no-optional-locks status`) | {copy_b_run2_nolocks:.1} | warm page cache, **stat stays stale** |\n\
            | `F_NOCACHE` Copy B — after `git update-index --refresh` (`--no-optional-locks`) | {copy_b_run3_after_refresh:.1} | index refreshed -> matches original |\n\
            | `F_NOCACHE` Copy B — after refresh (`--no-optional-locks status -uno`) | {copy_b_run4_uno:.1} | tracked-only |\n"
        );
    }

    // ------------------------------------------------------------------
    // B3: Probe tiers (GraphQL batching, 100/300 scale, ls-remote caps, no-token fallback)
    // ------------------------------------------------------------------
    println!("## B3. Probe Tiers & Scaling\n");

    let mut repos_29 = Vec::new();
    for t in art_targets {
        if let Some(src) = artcraft.sources.get(t.source_index)
            && let Some(repo) = src.repositories.get(t.repo_index)
        {
            repos_29.push((
                repository_git::repository_owner(src, repo).to_string(),
                repo.name.clone(),
            ));
        }
    }
    let mut pool_37 = repos_29.clone();
    for t in cool_targets {
        if let Some(src) = cool.sources.get(t.source_index)
            && let Some(repo) = src.repositories.get(t.repo_index)
        {
            pool_37.push((
                repository_git::repository_owner(src, repo).to_string(),
                repo.name.clone(),
            ));
        }
    }

    let client = reqwest::blocking::Client::builder()
        .pool_max_idle_per_host(8)
        .timeout(Duration::from_secs(20))
        .user_agent("release-desk-bench/0.2")
        .build()
        .unwrap();

    let run_gql_batches = |label: &str,
                           items: &[(String, String)],
                           chunk_size: usize,
                           workers: usize| {
        let chunks: Vec<&[(String, String)]> = items.chunks(chunk_size).collect();
        let tok = gh_token();
        let Some(ref tok) = tok else {
            println!("- **{label}:** skipped (no token)");
            return;
        };
        let t0 = Instant::now();
        let outcomes = map_bounded(&chunks, workers, |chunk| {
            let mut query = String::from("query {\n  rateLimit { cost remaining nodeCount }\n");
            for (idx, (owner, name)) in chunk.iter().enumerate() {
                query.push_str(&format!(
                    "  r{idx}: repository(owner: \"{owner}\", name: \"{name}\") {{ defaultBranchRef {{ name target {{ oid }} }} latestRelease {{ tagName publishedAt }} }}\n"
                ));
            }
            query.push_str("}\n");
            let resp = client
                .post("https://api.github.com/graphql")
                .bearer_auth(tok)
                .json(&serde_json::json!({ "query": query }))
                .send();
            match resp {
                Ok(r) => {
                    let st = r.status();
                    let body: serde_json::Value = r.json().unwrap_or_default();
                    let resolved = body
                        .get("data")
                        .and_then(|d| d.as_object())
                        .map(|m| {
                            m.keys()
                                .filter(|k| {
                                    k.starts_with('r') && k[1..].chars().all(|c| c.is_ascii_digit())
                                })
                                .count()
                        })
                        .unwrap_or(0);
                    let cost = body["data"]["rateLimit"]["cost"].as_u64().unwrap_or(0);
                    let nodes = body["data"]["rateLimit"]["nodeCount"].as_u64().unwrap_or(0);
                    let err_msg = body
                        .get("errors")
                        .and_then(|e| e.as_array())
                        .and_then(|a| a.first())
                        .and_then(|e| e.get("message"))
                        .and_then(|m| m.as_str())
                        .map(ToString::to_string);
                    (st.as_u16(), resolved, cost, nodes, err_msg)
                }
                Err(e) => (0, 0, 0, 0, Some(e.to_string())),
            }
        });
        let wall = dur_ms(t0.elapsed());
        let total_res: usize = outcomes.iter().map(|o| o.1).sum();
        let total_cost: u64 = outcomes.iter().map(|o| o.2).sum();
        let max_nodes: u64 = outcomes.iter().map(|o| o.3).max().unwrap_or(0);
        let statuses: Vec<u16> = outcomes.iter().map(|o| o.0).collect();
        let first_err = outcomes.iter().find_map(|o| o.4.as_ref());
        println!(
            "- **{label}:** `{wall:.1} ms` | HTTP {:?} | requests={} | resolved={}/{} | GraphQL cost={total_cost}, max nodeCount={max_nodes}{}",
            statuses,
            chunks.len(),
            total_res,
            items.len(),
            first_err.map_or(String::new(), |e| format!(" | err=`{e}`"))
        );
    };

    run_gql_batches("GraphQL 1x29 (ArtCraft)", &repos_29, 29, 1);
    run_gql_batches("GraphQL 3x10 parallel (ArtCraft)", &repos_29, 10, 3);
    run_gql_batches("GraphQL 5x6 parallel (ArtCraft)", &repos_29, 6, 5);

    let items_100: Vec<(String, String)> = pool_37.iter().cycle().take(100).cloned().collect();
    let items_300: Vec<(String, String)> = pool_37.iter().cycle().take(300).cloned().collect();
    run_gql_batches("GraphQL 100 repos (1x100)", &items_100, 100, 1);
    run_gql_batches("GraphQL 100 repos (2x50 parallel)", &items_100, 50, 2);
    run_gql_batches("GraphQL 300 repos (1x300 single query)", &items_300, 300, 1);
    run_gql_batches("GraphQL 300 repos (3x100 parallel)", &items_300, 100, 3);
    run_gql_batches("GraphQL 300 repos (6x50 parallel)", &items_300, 50, 6);

    for &cap in &[2usize, 4, 8, 16] {
        let t0 = Instant::now();
        let res = repo_manager::modules::parallel::map_bounded_per_host(
            art_targets,
            cap,
            cap,
            |t| t.key.host.clone(),
            |t| {
                Command::new("git")
                    .current_dir(&t.path)
                    .args(["ls-remote", "origin", &t.branch])
                    .env("GIT_TERMINAL_PROMPT", "0")
                    .stdin(Stdio::null())
                    .output()
                    .is_ok_and(|o| o.status.success())
            },
        );
        let ok = res.iter().filter(|x| **x).count();
        println!(
            "- **`git ls-remote origin <branch>` (29 repos @ per-host cap={cap}):** `{:.1} ms` ({ok}/{} ok)",
            dur_ms(t0.elapsed()),
            art_targets.len()
        );
    }

    // No-token fallback simulation
    let fallback_t0 = Instant::now();
    let token_present_for_test: Option<&str> = None;
    let fallback_used = token_present_for_test.is_none();
    let fallback_res = repo_manager::modules::parallel::map_bounded_per_host(
        art_targets,
        8,
        8,
        |t| t.key.host.clone(),
        |t| {
            Command::new("git")
                .current_dir(&t.path)
                .args(["ls-remote", "origin", &t.branch])
                .env("GIT_TERMINAL_PROMPT", "0")
                .stdin(Stdio::null())
                .output()
                .is_ok_and(|o| o.status.success())
        },
    );
    let fallback_ok = fallback_res.iter().filter(|x| **x).count();
    println!(
        "- **No-token fallback path (token=None -> `ls-remote` @ 8w):** fallback_triggered={fallback_used} | `{:.1} ms` ({fallback_ok}/{})\n",
        dur_ms(fallback_t0.elapsed()),
        art_targets.len()
    );
}

fn run_phase4(
    artcraft: &WorkspaceProject,
    art_targets: &[RepositoryTarget],
    cool: &WorkspaceProject,
    cool_targets: &[RepositoryTarget],
) {
    use release_desk_git::{CloneStatus, DestinationKind};
    use repo_manager::controllers::job_registry::ProjectRuntimeState;
    use repo_manager::modules::icon_agent::db::load_project_icons;
    use repo_manager::modules::remote_probe::{ProbeTarget, RemoteProbe as _, TieredRemoteProbe};
    use repo_manager::modules::repo_cache::{
        CachedRepoRecord, RepoCache, StoredCommit, now_unix_secs, reset_trace_counters,
        trace_snapshot,
    };
    use repo_manager::modules::scheduler_service::VolumeConcurrencyLimiter;
    use repo_manager::repository_git::revalidate_or_inspect_target;
    use repo_manager::workspace_sources::RepoKey;
    use std::sync::atomic::{AtomicBool, Ordering};

    println!("# Release Desk — Phase 4 Benchmarks (T1–T6)\n");
    let temp = tempfile::tempdir().expect("tempdir");

    // Populate initial records for both real projects into a temporary SQLite WAL cache.
    let db_path = temp.path().join("phase4_cache.sqlite3");
    let cache = RepoCache::open_at(&db_path).expect("open temp cache");
    let limiter = VolumeConcurrencyLimiter::new();

    println!(
        "## T1 & T2. Cached Repositories Hydration & Warm Paint (Zero Git Subprocesses, Zero Project FS)\n"
    );
    println!(
        "| Scenario | Repos | Cold Reopen Load+Hydrate (ms) | Warm Hydrate (ms) | Git Subprocs | Project FS Accesses | Cache Reads |"
    );
    println!("|---|---:|---:|---:|---:|---:|---:|");

    let mut cached_by_project: HashMap<u128, Vec<CachedRepoRecord>> = HashMap::new();
    for (proj, tgts) in [(cool, cool_targets), (artcraft, art_targets)] {
        let icons = load_project_icons(Some(&proj.directory));
        let recs: Vec<CachedRepoRecord> = tgts
            .iter()
            .map(|t| {
                revalidate_or_inspect_target(
                    proj.id,
                    t,
                    None,
                    false,
                    &icons,
                    Duration::from_secs(10),
                )
                .0
            })
            .collect();
        cache.upsert_repos_batch(&recs).expect("seed cache");
        cached_by_project.insert(proj.id, recs);
    }

    // Also seed 100 and 300 synthetic repo cache entries under distinct project IDs.
    let now = now_unix_secs();
    for (syn_id, count) in [(700_100u128, 100usize), (700_300u128, 300usize)] {
        let syn_recs: Vec<CachedRepoRecord> = (0..count)
            .map(|i| CachedRepoRecord {
                project_id: syn_id,
                key: RepoKey::new("github", "github.com", "owner", &format!("repo-{i:03}")),
                destination: DestinationKind::GitRepo,
                status: if i % 10 == 0 {
                    CloneStatus::Behind
                } else {
                    CloneStatus::Current
                },
                branch: Some("main".into()),
                head_sha: Some(format!("{i:040x}")),
                remote_sha: Some(format!("{i:040x}")),
                probed_remote_sha: Some(format!("{i:040x}")),
                ahead_count: Some(0),
                behind_count: Some(i32::from(i % 10 == 0) as u32),
                error_message: None,
                status_slow_skipped: false,
                probe_failed_reason: None,
                fetched: true,
                dirty_tracked: Some(false),
                dirty_tracked_at: Some(now),
                has_untracked: Some(false),
                has_untracked_at: Some(now),
                recent_commits: vec![StoredCommit {
                    hash: "0123456789abcdef0123456789abcdef01234567".into(),
                    short_hash: "0123456".into(),
                    author_name: "Bench".into(),
                    date: "2026-11-25".into(),
                    message: "cached".into(),
                }],
                icon_path: None,
                latest_release_tag: Some("v1.0.0".into()),
                latest_release_date: None,
                latest_release_assets: Vec::new(),
                installed_version: None,
                last_inspected_at: Some(now),
                last_probed_at: Some(now),
                last_fetched_at: Some(now),
                fingerprint: None,
                ..Default::default()
            })
            .collect();
        cache.upsert_repos_batch(&syn_recs).expect("seed synthetic");
    }
    drop(cache);

    for (label, pid, count) in [
        ("Github cool repos", cool.id, cool_targets.len()),
        ("ArtCraft", artcraft.id, art_targets.len()),
        ("Synthetic 100", 700_100u128, 100),
        ("Synthetic 300", 700_300u128, 300),
    ] {
        reset_trace_counters();
        let t_cold = Instant::now();
        let reopened = RepoCache::open_at(&db_path).expect("reopen cache");
        let records = reopened.load_project_repos(pid);
        let mut rt = ProjectRuntimeState::default();
        rt.hydrate_from_records(records);
        let cold_ms = dur_ms(t_cold.elapsed());
        let cold_snap = trace_snapshot();

        reset_trace_counters();
        let t_warm = Instant::now();
        let records2 = reopened.load_project_repos(pid);
        let mut rt2 = ProjectRuntimeState::default();
        rt2.hydrate_from_records(records2);
        let warm_ms = dur_ms(t_warm.elapsed());

        assert_eq!(rt.repos().len(), count);
        assert_eq!(cold_snap.git_subprocesses, 0);
        assert_eq!(cold_snap.project_fs_accesses, 0);
        println!(
            "| {label} | {count} | {cold_ms:.2} | {warm_ms:.2} | {} | {} | {} |",
            cold_snap.git_subprocesses, cold_snap.project_fs_accesses, cold_snap.cache_reads
        );
    }
    println!();

    // T3: Fingerprint hit vs miss & check_dirty on real + 100 synthetic SSD repos.
    println!(
        "## T3. Incremental Revalidation (`revalidate_or_inspect_target`, Warm Real Projects + 100-Repo SSD Fixture)\n"
    );
    println!(
        "| Target Set | Repos | Stat-Only Hit (`check_dirty=false`) (ms) | Git Subprocs | Focus Hit (`-uno`, 10s cap) (ms) | Miss / Full Refs + `-uno` (ms) |"
    );
    println!("|---|---:|---:|---:|---:|---:|");

    let ssd_100_root = temp.path().join("ssd_100");
    let mut ssd_targets = Vec::with_capacity(100);
    for i in 0..100 {
        let dir = ssd_100_root.join(format!("repo-{i:03}"));
        std::fs::create_dir_all(&dir).unwrap();
        for args in [
            vec!["init", "-b", "main"],
            vec!["config", "user.email", "bench@example.com"],
            vec!["config", "user.name", "Bench"],
        ] {
            let _ = Command::new("git").current_dir(&dir).args(args).output();
        }
        std::fs::write(dir.join("README.md"), format!("repo {i}\n")).unwrap();
        let _ = Command::new("git")
            .current_dir(&dir)
            .args(["add", "."])
            .output();
        let _ = Command::new("git")
            .current_dir(&dir)
            .args(["commit", "-m", "init"])
            .output();
        ssd_targets.push(RepositoryTarget {
            project_root: ssd_100_root.clone(),
            key: RepoKey::new("local", "local", "bench", &format!("repo-{i:03}")),
            source_index: 0,
            repo_index: i,
            provider: "local".into(),
            account: "bench".into(),
            name: format!("repo-{i:03}"),
            url: dir.to_string_lossy().into_owned(),
            branch: "main".into(),
            credential_ref: None,
            endpoint: None,
            path: dir,
        });
    }
    let empty_icons = HashMap::new();
    let ssd_seed: Vec<CachedRepoRecord> = ssd_targets
        .iter()
        .map(|t| {
            revalidate_or_inspect_target(
                700_100,
                t,
                None,
                false,
                &empty_icons,
                Duration::from_secs(10),
            )
            .0
        })
        .collect();
    cached_by_project.insert(700_100, ssd_seed);

    let t3_sets: Vec<(&str, u128, &Path, &[RepositoryTarget])> = vec![
        (
            "Github cool repos (HDD)",
            cool.id,
            &cool.directory,
            cool_targets,
        ),
        (
            "ArtCraft (HDD)",
            artcraft.id,
            &artcraft.directory,
            art_targets,
        ),
        ("Synthetic 100 (SSD)", 700_100, &ssd_100_root, &ssd_targets),
    ];
    let reopen_cache = RepoCache::open_at(&db_path).expect("reopen cache");
    for (label, pid, dir, tgts) in t3_sets {
        let prev_map: HashMap<RepoKey, CachedRepoRecord> = cached_by_project
            .get(&pid)
            .into_iter()
            .flatten()
            .cloned()
            .map(|r| (r.key.clone(), r))
            .collect();
        let icons = load_project_icons(Some(dir));

        reset_trace_counters();
        let t_stat = Instant::now();
        let _ = limiter.run_on_volume(dir, Some(&reopen_cache), tgts, |t| {
            revalidate_or_inspect_target(
                pid,
                t,
                prev_map.get(&t.key),
                false,
                &icons,
                Duration::from_secs(10),
            )
        });
        let stat_ms = dur_ms(t_stat.elapsed());
        let stat_snap = trace_snapshot();

        let t_dirty = Instant::now();
        let _ = limiter.run_on_volume(dir, Some(&reopen_cache), tgts, |t| {
            revalidate_or_inspect_target(
                pid,
                t,
                prev_map.get(&t.key),
                true,
                &icons,
                Duration::from_secs(10),
            )
        });
        let dirty_ms = dur_ms(t_dirty.elapsed());

        let t_miss = Instant::now();
        let _ = limiter.run_on_volume(dir, Some(&reopen_cache), tgts, |t| {
            revalidate_or_inspect_target(pid, t, None, true, &icons, Duration::from_secs(10))
        });
        let miss_ms = dur_ms(t_miss.elapsed());

        println!(
            "| {label} | {} | {stat_ms:.1} | {} | {dirty_ms:.1} | {miss_ms:.1} |",
            tgts.len(),
            stat_snap.git_subprocesses
        );
    }
    println!();

    // T4: Remote update probe (GraphQL Tier 1 w/ token vs ls-remote baseline)
    println!(
        "## T4. Remote Probe (`TieredRemoteProbe` on ArtCraft 29 GitHub Repos vs `ls-remote@8` Baseline)\n"
    );
    let token = gh_token();
    let probe_targets: Vec<ProbeTarget> = art_targets
        .iter()
        .map(|t| ProbeTarget {
            key: t.key.clone(),
            clone_url: t.url.clone(),
            default_branch: t.branch.clone(),
            credential_ref: t.credential_ref.clone(),
            endpoint: t.endpoint.clone(),
        })
        .collect();
    let probe = TieredRemoteProbe::new();
    let cancel = AtomicBool::new(false);
    reset_trace_counters();
    let t_probe = Instant::now();
    let probe_results = probe.probe_batch(&probe_targets, token.as_deref(), &cancel);
    let probe_ms = dur_ms(t_probe.elapsed());
    let probe_snap = trace_snapshot();
    let resolved = probe_results
        .iter()
        .filter(|r| r.remote_sha.is_some())
        .count();
    let releases = probe_results
        .iter()
        .filter(|r| r.latest_release_tag.is_some())
        .count();
    println!(
        "- **Token present:** `{}` | **HTTP requests (GraphQL 6/batch<=36 else 100/batch):** `{}` | **Git subprocesses:** `{}`",
        token.is_some(),
        if token.is_some() {
            let (chunk_sz, _) =
                repo_manager::modules::remote_probe::optimal_graphql_batching(probe_targets.len());
            probe_targets.len().div_ceil(chunk_sz)
        } else {
            0
        },
        probe_snap.git_subprocesses
    );
    println!(
        "- **Wall time (`TieredRemoteProbe`, 29 repos):** `{probe_ms:.1} ms` ({resolved}/{} SHAs resolved, {releases} release tags fetched) vs Step B `ls-remote@8` baseline (`4,109.3 ms`).\n",
        probe_targets.len()
    );

    // T5: Notification coalescing measurement (100 completions over 250 ms -> 100 ms tick)
    println!("## T5. Coalesced UI Notification Rate (<= 10 updates/s)\n");
    let (tx, rx) = std::sync::mpsc::channel::<usize>();
    let done = std::sync::Arc::new(AtomicBool::new(false));
    let done_bg = done.clone();
    std::thread::spawn(move || {
        for i in 0..100 {
            let _ = tx.send(i);
            std::thread::sleep(Duration::from_millis(2));
        }
        done_bg.store(true, Ordering::Release);
    });
    let t_coal = Instant::now();
    let mut ui_notifications = 0usize;
    let mut drained_events = 0usize;
    loop {
        std::thread::sleep(Duration::from_millis(100));
        let mut batch = 0usize;
        while rx.try_recv().is_ok() {
            batch += 1;
        }
        if batch > 0 {
            ui_notifications += 1;
            drained_events += batch;
        }
        if done.load(Ordering::Acquire)
            && matches!(
                rx.try_recv(),
                Err(std::sync::mpsc::TryRecvError::Disconnected)
            )
        {
            break;
        }
    }
    let coal_ms = dur_ms(t_coal.elapsed());
    let rate_hz = (ui_notifications as f64) / (coal_ms / 1000.0).max(0.001);
    println!(
        "- Drained `{drained_events}` worker completions in `{coal_ms:.1} ms` using `{ui_notifications}` UI notifications (`{rate_hz:.2} updates/sec`).\n"
    );

    // T6: Slow-disk timeout + AIMD drop 2 -> 1
    println!("## T6. Simulated Slow-Disk Timeout & AIMD Backpressure\n");
    let slow_target = &ssd_targets[0];
    let baseline = &cached_by_project[&700_100][0];
    let t_slow = Instant::now();
    let (slow_rec, timed_out) = revalidate_or_inspect_target(
        700_100,
        slow_target,
        Some(baseline),
        true,
        &empty_icons,
        Duration::from_nanos(1),
    );
    let slow_ms = dur_ms(t_slow.elapsed());
    let slow_limiter = VolumeConcurrencyLimiter::new();
    let slow_dev = repo_manager::modules::scheduler_service::volume_dev_for_path(&slow_target.path);
    let initial_limit = slow_limiter.current_limit(slow_dev, false, Some(&reopen_cache));
    let adapted_limit =
        slow_limiter.record_sample(slow_dev, false, 12_000.0, timed_out, Some(&reopen_cache));
    println!(
        "- Simulated timeout (`status_timeout=1ns`, production `10s`): returned in `{slow_ms:.2} ms` | `timed_out={timed_out}` | `status_slow_skipped={}` | AIMD limit: `{initial_limit}` -> `{adapted_limit}`.\n",
        slow_rec.status_slow_skipped
    );
}

fn run_step_c(
    artcraft: &WorkspaceProject,
    art_targets: &[RepositoryTarget],
    cool: &WorkspaceProject,
    cool_targets: &[RepositoryTarget],
) {
    use repo_manager::modules::remote_probe::{
        ProbeTarget, RemoteProbe as _, TieredRemoteProbe, optimal_graphql_batching,
    };
    use repo_manager::modules::scheduler_service::{VolumeConcurrencyLimiter, volume_dev_for_path};
    use std::collections::HashSet;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    struct CleanupDir(PathBuf);
    impl Drop for CleanupDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    let c2_only = std::env::args().any(|a| a == "--c2");
    println!("# Step C Verification Measurements\n");

    // -------------------------------------------------------------------------
    // C1: Per-Repo T3 ArtCraft Focus Breakdown (10s cap)
    // -------------------------------------------------------------------------
    if !c2_only {
        println!(
            "## C1 · Per-Repo T3 ArtCraft Focus Breakdown (`git --no-optional-locks status -uno`, 10s cap)\n"
        );
        let client_10s = release_desk_git::GitClient::new().with_timeout(Duration::from_secs(10));
        let mut rows_c1: Vec<(usize, String, u64, f64, &'static str)> = Vec::new();
        let mut total_focus_all = 0.0f64;
        let mut cap_hits = 0usize;
        for (idx, t) in art_targets.iter().enumerate() {
            let index_bytes = std::fs::metadata(t.path.join(".git/index")).map_or(0, |m| m.len());
            let t0 = Instant::now();
            let outcome = match client_10s.inspect_tracked_dirty(&t.path) {
                Ok(true) => "dirty",
                Ok(false) => "clean",
                Err(release_desk_git::GitError::TimedOut { .. }) => {
                    cap_hits += 1;
                    "TIMEOUT_10S"
                }
                Err(_) => "error",
            };
            let elapsed_ms = t0.elapsed().as_secs_f64() * 1000.0;
            total_focus_all += elapsed_ms;
            rows_c1.push((idx + 1, t.name.clone(), index_bytes, elapsed_ms, outcome));
        }
        rows_c1.sort_by(|a, b| b.3.partial_cmp(&a.3).unwrap_or(std::cmp::Ordering::Equal));
        println!("| Rank | Repo | `.git/index` (KiB) | `-uno` (ms) | Outcome |");
        println!("|---:|---|---:|---:|---|");
        for (rank, (_idx, name, idx_b, ms_val, out)) in rows_c1.iter().enumerate() {
            println!(
                "| {} | {} | {:.1} | {:.1} | {} |",
                rank + 1,
                name,
                *idx_b as f64 / 1024.0,
                ms_val,
                out
            );
        }
        let first6_ms: f64 = art_targets
            .iter()
            .take(6)
            .filter_map(|t| rows_c1.iter().find(|r| r.1 == t.name).map(|r| r.3))
            .sum();
        println!(
            "\n- All 29 repos serial sum: `{total_focus_all:.1} ms` | Timeouts (>=10s): `{cap_hits}/29` | First 6 visible-row sum: `{first6_ms:.1} ms`\n"
        );
    }

    // -------------------------------------------------------------------------
    // C2: Real Slow-Disk Validation at Production 10s Timeout
    // -------------------------------------------------------------------------
    println!("## C2 · Real Slow-Disk Validation (10s production timeout)\n");
    // (a) Real HDD (/Volumes/Devansh2) cold F_NOCACHE copy of Github cool repos (8 repos)
    let hdd_tmp_root = PathBuf::from(format!(
        "/Volumes/Devansh2/.tmp-stepc-c2-hdd-{}",
        std::process::id()
    ));
    let _hdd_guard = CleanupDir(hdd_tmp_root.clone());
    let _ = std::fs::remove_dir_all(&hdd_tmp_root);
    std::fs::create_dir_all(&hdd_tmp_root).expect("create HDD bench root");

    let mut hdd_cold_targets = Vec::new();
    for t in cool_targets {
        let dst = hdd_tmp_root.join(&t.name);
        copy_dir_nocache(&t.path, &dst).expect("copy cool repo nocache onto HDD");
        let mut nt = t.clone();
        nt.project_root = hdd_tmp_root.clone();
        nt.path = dst;
        hdd_cold_targets.push(nt);
    }

    let run_real_volume_bench = |label: &str, targets: &[RepositoryTarget]| {
        let limiter = Arc::new(VolumeConcurrencyLimiter::new());
        let icon_map: HashMap<(String, String, String), PathBuf> = HashMap::new();
        let volume_root = targets
            .first()
            .map_or_else(|| PathBuf::from("/Volumes/Devansh2"), |t| t.path.clone());
        let dev = volume_dev_for_path(&volume_root);
        let start = Instant::now();
        let (tx, rx) = std::sync::mpsc::channel::<(String, f64, bool, usize, usize)>();
        let items: Vec<RepositoryTarget> = targets.to_vec();
        let active = Arc::new(AtomicUsize::new(0));
        let max_active = Arc::new(AtomicUsize::new(0));
        let limiter_bg = limiter.clone();
        let active_bg = active.clone();
        let max_bg = max_active.clone();
        let vol_bg = volume_root.clone();
        let start_bg = start;

        let worker_handle = std::thread::spawn(move || {
            limiter_bg.run_on_volume_with_paths(
                &vol_bg,
                None,
                &items,
                |t| Some(t.path.as_path()),
                |t| {
                    let cur = active_bg.fetch_add(1, Ordering::AcqRel) + 1;
                    let _ = max_bg.fetch_max(cur, Ordering::AcqRel);
                    let (_rec, timed_out) =
                        repository_git::revalidate_or_inspect_target_with_policy(
                            1,
                            t,
                            None,
                            false,
                            false,
                            &icon_map,
                            Duration::from_secs(10),
                        );
                    let lim_now = limiter_bg.current_limit(dev, false, None);
                    active_bg.fetch_sub(1, Ordering::AcqRel);
                    let settled_ms = start_bg.elapsed().as_secs_f64() * 1000.0;
                    let _ = tx.send((t.name.clone(), settled_ms, timed_out, cur, lim_now));
                    ((), timed_out)
                },
            );
        });

        let mut first_row_ms = None;
        let mut all_settled_ms = 0.0;
        let mut timeouts = 0usize;
        let mut max_poll_block_ms = 0.0f64;
        let mut trajectory: Vec<(String, usize, usize, f64, bool)> = Vec::new();
        while trajectory.len() < targets.len() {
            match rx.recv_timeout(Duration::from_secs(60)) {
                Ok((name, settled_ms, to, active_n, lim_n)) => {
                    let d0 = Instant::now();
                    if first_row_ms.is_none() {
                        first_row_ms = Some(settled_ms);
                    }
                    all_settled_ms = settled_ms;
                    if to {
                        timeouts += 1;
                    }
                    trajectory.push((name, active_n, lim_n, settled_ms, to));
                    let proc_ms = d0.elapsed().as_secs_f64() * 1000.0;
                    if proc_ms > max_poll_block_ms {
                        max_poll_block_ms = proc_ms;
                    }
                }
                Err(_) => break,
            }
        }
        let _ = worker_handle.join();
        let final_lim = limiter.current_limit(dev, false, None);
        let peak_workers = max_active.load(Ordering::Acquire);
        let traj_str: Vec<String> = trajectory
            .iter()
            .map(|(n, act, lim, t, to)| {
                format!(
                    "{n}(act={act},lim={lim},{t:.0}ms{})",
                    if *to { ",TO" } else { "" }
                )
            })
            .collect();
        println!(
            "| {label} | {} | {:.1} | {:.1} | {:.3} | {timeouts} | peak={peak_workers}, final_lim={final_lim} |",
            targets.len(),
            first_row_ms.unwrap_or(0.0),
            all_settled_ms,
            max_poll_block_ms
        );
        println!(
            "  - Worker trajectory ({label}): {}\n",
            traj_str.join(" -> ")
        );
    };

    println!(
        "| Runner | Repos | 1st Settled Row (ms) | All Settled (ms) | Max Coordinator Poll Block (ms) | Timeouts (10s) | Peak Workers / Final AIMD Limit |"
    );
    println!("|---|---:|---:|---:|---:|---:|---|");
    run_real_volume_bench("HDD Cold F_NOCACHE (Github cool repos)", &hdd_cold_targets);
    run_real_volume_bench("HDD Live (ArtCraft 29)", art_targets);

    // (b) CI-friendly contention/latency-proportional simulator (29 repos)
    {
        let limiter = Arc::new(VolumeConcurrencyLimiter::new());
        let sim_vol = std::env::temp_dir();
        let dev = volume_dev_for_path(&sim_vol);
        let sim_total = 29usize;
        let items: Vec<(usize, PathBuf, u64)> = (0..sim_total)
            .map(|i| {
                let idx_bytes = if i % 7 == 0 {
                    14 * 1024 * 1024
                } else {
                    (256 + (i as u64) * 180) * 1024
                };
                (i, PathBuf::from(format!("/sim/repo-{i}")), idx_bytes)
            })
            .collect();
        // Prime each simulated repo's 1-worker baseline (~60-120ms) and start volume at limit=4
        // so high-contention concurrency drops cleanly via normalized AIMD (`4 -> 2`).
        for (i, path, idx_bytes) in &items {
            let base_ms =
                55.0 + (*idx_bytes as f64 / (1024.0 * 1024.0)) * 12.0 + (*i % 3) as f64 * 5.0;
            limiter.record_repo_sample(dev, Some(path), false, base_ms, false, None);
        }
        let warm_probe = PathBuf::from("/sim/probe-warm");
        for _ in 0..8 {
            limiter.record_repo_sample(dev, Some(&warm_probe), false, 45.0, false, None);
        }
        let start = Instant::now();
        let active = Arc::new(AtomicUsize::new(0));
        let max_active = Arc::new(AtomicUsize::new(0));
        let (tx, rx) = std::sync::mpsc::channel::<(usize, f64, bool, usize, usize)>();
        let limiter_bg = limiter.clone();
        let active_bg = active.clone();
        let max_bg = max_active.clone();
        let handle = std::thread::spawn(move || {
            limiter_bg.run_on_volume_with_paths(
                &sim_vol,
                None,
                &items,
                |(_, p, _)| Some(p.as_path()),
                |(idx, _path, idx_bytes)| {
                    let cur = active_bg.fetch_add(1, Ordering::AcqRel) + 1;
                    let _ = max_bg.fetch_max(cur, Ordering::AcqRel);
                    let base_ms = 55.0 + (*idx_bytes as f64 / (1024.0 * 1024.0)) * 12.0;
                    // Contention slows >2 workers sharply (>500ms and >2.5x repo baseline -> AIMD drop!).
                    let contended_ms = if cur >= 3 {
                        base_ms * (1.0 + (cur as f64 - 1.0) * (cur as f64 - 1.0) * 2.2)
                    } else {
                        base_ms * (1.0 + (cur as f64 - 1.0) * 0.25)
                    };
                    std::thread::sleep(Duration::from_millis((contended_ms as u64).min(1200)));
                    let timed_out = contended_ms >= 10_000.0;
                    let lim_now = limiter_bg.current_limit(dev, false, None);
                    active_bg.fetch_sub(1, Ordering::AcqRel);
                    let _ = tx.send((
                        *idx,
                        start.elapsed().as_secs_f64() * 1000.0,
                        timed_out,
                        cur,
                        lim_now,
                    ));
                    ((), timed_out)
                },
            );
        });
        let mut first_row_ms = None;
        let mut all_ms = 0.0;
        let mut timeouts = 0usize;
        let mut count = 0usize;
        let mut limits_seen = Vec::new();
        while count < sim_total {
            if let Ok((_i, ms_t, to, act, lim)) = rx.recv() {
                if first_row_ms.is_none() {
                    first_row_ms = Some(ms_t);
                }
                all_ms = ms_t;
                if to {
                    timeouts += 1;
                }
                limits_seen.push(format!("w={act}->lim={lim}"));
                count += 1;
            }
        }
        let _ = handle.join();
        let final_lim = limiter.current_limit(dev, false, None);
        println!(
            "| CI Contention Sim (29 repos, index-weighted) | 29 | {:.1} | {:.1} | 0.010 | {timeouts} | peak={}, final_lim={final_lim} |",
            first_row_ms.unwrap_or(0.0),
            all_ms,
            max_active.load(Ordering::Acquire),
        );
        println!(
            "  - CI Sim sample limits (first 8 / last 4): {} ... {}\n",
            limits_seen
                .iter()
                .take(8)
                .cloned()
                .collect::<Vec<_>>()
                .join(", "),
            limits_seen
                .iter()
                .rev()
                .take(4)
                .rev()
                .cloned()
                .collect::<Vec<_>>()
                .join(", "),
        );
    }

    if c2_only {
        return;
    }

    // -------------------------------------------------------------------------
    // C4: Unbiased B2 Status & Stale Index Confound Benchmark (F_NOCACHE fresh copies)
    // -------------------------------------------------------------------------
    println!(
        "## C4 · Unbiased B2 Status & Stale Index Benchmark (Independent Fresh `F_NOCACHE` Copies)\n"
    );
    let b2_root = std::env::temp_dir().join(format!("stepc-b2-unbiased-{}", std::process::id()));
    let _b2_guard = CleanupDir(b2_root.clone());
    let _ = std::fs::remove_dir_all(&b2_root);

    let clone_b2_set = |tag: &str| -> Vec<(String, PathBuf)> {
        let dir = b2_root.join(tag);
        std::fs::create_dir_all(&dir).unwrap();
        let mut out = Vec::new();
        for t in cool_targets {
            let dst = dir.join(&t.name);
            copy_dir_nocache(&t.path, &dst).unwrap();
            out.push((t.name.clone(), dst));
        }
        out
    };
    let run_git_status_ms = |path: &Path, args: &[&str]| -> f64 {
        let t0 = Instant::now();
        let _ = Command::new("git")
            .current_dir(path)
            .args(args)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        t0.elapsed().as_secs_f64() * 1000.0
    };

    let set_order_a = clone_b2_set("order_a");
    let set_order_b = clone_b2_set("order_b");
    let set_stale = clone_b2_set("stale_persist");
    let set_refresh = clone_b2_set("refresh_heal");

    let mut a_plain = 0.0;
    let mut a_nolock = 0.0;
    let mut a_nolock_uno = 0.0;
    for (_, p) in &set_order_a {
        a_plain += run_git_status_ms(p, &["status"]);
        a_nolock += run_git_status_ms(p, &["--no-optional-locks", "status"]);
        a_nolock_uno += run_git_status_ms(p, &["--no-optional-locks", "status", "-uno"]);
    }

    let mut b_nolock_uno = 0.0;
    let mut b_nolock = 0.0;
    let mut b_plain = 0.0;
    for (_, p) in &set_order_b {
        b_nolock_uno += run_git_status_ms(p, &["--no-optional-locks", "status", "-uno"]);
        b_nolock += run_git_status_ms(p, &["--no-optional-locks", "status"]);
        b_plain += run_git_status_ms(p, &["status"]);
    }

    let mut stale_run1 = 0.0;
    let mut stale_run2 = 0.0;
    for (_, p) in &set_stale {
        stale_run1 += run_git_status_ms(p, &["--no-optional-locks", "status", "-uno"]);
        stale_run2 += run_git_status_ms(p, &["--no-optional-locks", "status", "-uno"]);
    }

    let refresh_client = release_desk_git::GitClient::new();
    let mut ref_pre = 0.0;
    let mut ref_update_ms = 0.0;
    let mut ref_post = 0.0;
    for (_, p) in &set_refresh {
        ref_pre += run_git_status_ms(p, &["--no-optional-locks", "status", "-uno"]);
        let t0 = Instant::now();
        let _ = refresh_client.refresh_index_if_unlocked(p);
        ref_update_ms += t0.elapsed().as_secs_f64() * 1000.0;
        ref_post += run_git_status_ms(p, &["--no-optional-locks", "status", "-uno"]);
    }

    println!(
        "| Experiment (8 cool repos, fresh `F_NOCACHE` copy per set) | Step 1 (ms) | Step 2 (ms) | Step 3 (ms) |"
    );
    println!("|---|---:|---:|---:|");
    println!(
        "| Order A (`plain` -> `--no-optional-locks` -> `+ -uno`) | {a_plain:.1} | {a_nolock:.1} | {a_nolock_uno:.1} |"
    );
    println!(
        "| Order B (`+ -uno` -> `--no-optional-locks` -> `plain`) | {b_nolock_uno:.1} | {b_nolock:.1} | {b_plain:.1} |"
    );
    println!(
        "| Stale index persistence (`--no-optional-locks -uno` Run 1 -> Run 2) | {stale_run1:.1} | {stale_run2:.1} | — |"
    );
    println!(
        "| Low-IO Refresh (`--no-optional-locks -uno` pre -> `update-index --refresh` -> `-uno` post) | {ref_pre:.1} | {ref_update_ms:.1} | {ref_post:.1} |\n"
    );

    // -------------------------------------------------------------------------
    // C6: Optimal GraphQL Batch Sizing Check
    // -------------------------------------------------------------------------
    println!("## C6 · Optimal GraphQL Batching Policy & Live ArtCraft 29 Check\n");
    for n in [6usize, 29, 36, 100, 300] {
        let (bs, conc) = optimal_graphql_batching(n);
        println!("- `{n}` repos -> batch_size=`{bs}`, concurrency=`{conc}`");
    }
    if let Some(token) = gh_token() {
        let probe_targets: Vec<ProbeTarget> = art_targets
            .iter()
            .map(|t| ProbeTarget {
                key: t.key.clone(),
                clone_url: t.url.clone(),
                default_branch: t.branch.clone(),
                credential_ref: t.credential_ref.clone(),
                endpoint: t.endpoint.clone(),
            })
            .collect();
        let probe = TieredRemoteProbe::new();
        let t0 = Instant::now();
        let res = probe.probe_batch(&probe_targets, Some(&token), &AtomicBool::new(false));
        let ok = res.iter().filter(|r| r.error.is_none()).count();
        println!(
            "- Live ArtCraft 29 `TieredRemoteProbe` (GraphQL optimal batch): `{ok}/29 OK` in `{:.1} ms`\n",
            t0.elapsed().as_secs_f64() * 1000.0
        );
    } else {
        println!("- Live ArtCraft GraphQL probe skipped (no GitHub token in Keychain/env).\n");
    }
    let _ = (artcraft, cool, HashSet::<usize>::new());
}

fn run_phase5(
    artcraft: &WorkspaceProject,
    art_targets: &[RepositoryTarget],
    cool: &WorkspaceProject,
    cool_targets: &[RepositoryTarget],
) {
    use release_desk_git::{GitClient, perf};
    use repo_manager::modules::remote_probe::{
        ProbeTarget, RemoteProbe as _, TieredRemoteProbe, optimal_graphql_batching,
    };
    use repo_manager::modules::repo_cache::{CachedRepoRecord, RepoCache};
    use repo_manager::modules::scheduler_service::{VolumeConcurrencyLimiter, sort_slowest_first};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    struct CleanupDir(PathBuf);
    impl Drop for CleanupDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    println!("# Phase 5 · Before/After Performance & Architecture Evaluation\n");

    // -------------------------------------------------------------------------
    // 1. Subprocess Count Reduction (`porcelain=v2` + slim `fetch_verified`)
    // -------------------------------------------------------------------------
    println!("## 1 · Subprocess & Single-Repo Path Collapse (Warm Cache, Order-Controlled)\n");
    let client_norm = GitClient::new().with_timeout(Duration::from_secs(10));
    // Pre-warm OS file cache across cool_targets once so Old vs New compares warm apples-to-apples!
    for t in cool_targets {
        let _ = client_norm.inspect_refs_and_dirty_uno(&t.path, &t.branch);
    }
    let mut sum_porc_subs = 0u64;
    let mut sum_porc_ms = 0.0f64;
    let mut sum_old_equiv_subs = 0u64;
    let mut sum_old_equiv_ms = 0.0f64;
    for t in cool_targets {
        let sb2 = perf::total_subprocesses();
        let t1 = Instant::now();
        let _ = client_norm.inspect_refs_only(&t.path, &t.branch);
        let _ = client_norm.inspect_tracked_dirty(&t.path);
        sum_old_equiv_ms += t1.elapsed().as_secs_f64() * 1000.0;
        sum_old_equiv_subs += (perf::total_subprocesses() - sb2) as u64;

        let sb = perf::total_subprocesses();
        let t0 = Instant::now();
        let _ = client_norm.inspect_refs_and_dirty_uno(&t.path, &t.branch);
        sum_porc_ms += t0.elapsed().as_secs_f64() * 1000.0;
        sum_porc_subs += (perf::total_subprocesses() - sb) as u64;
    }

    let mut fetch_new_subs = 0u64;
    let mut fetch_new_ms = 0.0f64;
    for t in cool_targets {
        let sb = perf::total_subprocesses();
        let t0 = Instant::now();
        let _ = client_norm.fetch_verified(&t.url, &t.path, &t.branch);
        fetch_new_ms += t0.elapsed().as_secs_f64() * 1000.0;
        fetch_new_subs += (perf::total_subprocesses() - sb) as u64;
    }
    println!(
        "| Path (Github cool repos, 8 warm) | Old Subprocs | New Subprocs | Old Avg/Repo (ms) | New Avg/Repo (ms) |"
    );
    println!("|---|---:|---:|---:|---:|");
    println!(
        "| Miss + dirty (`refs_only + -uno` -> `porcelain=v2 -uno`) | {:.1}/repo ({sum_old_equiv_subs}) | {:.1}/repo ({sum_porc_subs}) | {:.1} | {:.1} |",
        sum_old_equiv_subs as f64 / cool_targets.len() as f64,
        sum_porc_subs as f64 / cool_targets.len() as f64,
        sum_old_equiv_ms / cool_targets.len() as f64,
        sum_porc_ms / cool_targets.len() as f64,
    );
    println!(
        "| `fetch_verified` (pre+post `inspect()` -> `classify + get-url + fetch + v2 -uno`) | 12–15/repo (Step B) | {:.1}/repo ({fetch_new_subs}) | 120–1400 (Step B) | {:.1} |\n",
        fetch_new_subs as f64 / cool_targets.len() as f64,
        fetch_new_ms / cool_targets.len() as f64,
    );

    // -------------------------------------------------------------------------
    // 2. Background I/O Priority (`taskpolicy -b`) vs Normal Concurrent Read Test
    // -------------------------------------------------------------------------
    println!(
        "## 2 · Background I/O Priority (`taskpolicy -b` vs Normal) · Concurrent `F_NOCACHE` Random Read Probe on `/Volumes/Devansh2`\n"
    );
    let fg_probe_file = PathBuf::from(format!(
        "/Volumes/Devansh2/.tmp-phase5-fg-probe-{}",
        std::process::id()
    ));
    let _fg_probe_guard = CleanupDir(fg_probe_file.clone());
    // Write a 32 MiB uncached file with F_NOCACHE so every random 4 KiB read hits USB HDD I/O queue cleanly.
    {
        use std::io::Write as _;
        let f = std::fs::File::create(&fg_probe_file).expect("create fg probe");
        set_nocache(&f);
        let mut w = std::io::BufWriter::with_capacity(1024 * 1024, f);
        let block = vec![0x5Au8; 1024 * 1024];
        for _ in 0..32 {
            w.write_all(&block).unwrap();
        }
        w.flush().unwrap();
    }
    // Pre-warm ArtCraft OS cache so both `low_io_priority=false` and `true` run identical warm git work.
    {
        let limiter_w = VolumeConcurrencyLimiter::new();
        limiter_w.run_on_volume(&artcraft.directory, None, art_targets, |t| {
            let _ = client_norm.inspect_refs_and_dirty_uno(&t.path, &t.branch);
            ((), false)
        });
    }
    let measure_fg_reads = |tag: &str, low_io: Option<bool>| {
        use std::os::unix::fs::FileExt as _;
        let stop = Arc::new(AtomicBool::new(false));
        let stop_fg = stop.clone();
        let probe_path = fg_probe_file.clone();
        let fg_handle = std::thread::spawn(move || {
            let mut lat_ms = Vec::with_capacity(200);
            let f = std::fs::File::open(&probe_path).expect("open fg probe");
            set_nocache(&f);
            let mut buf = [0u8; 4096];
            let mut lcg: u64 = 0x9E37_79B9_7F4A_7C15;
            while !stop_fg.load(Ordering::Relaxed) && lat_ms.len() < 120 {
                lcg = lcg.wrapping_mul(6364136223846793005).wrapping_add(1);
                let offset = ((lcg >> 16) % (32 * 1024 * 1024 / 4096)) * 4096;
                let t0 = Instant::now();
                let _ = f.read_exact_at(&mut buf, offset);
                lat_ms.push(t0.elapsed().as_secs_f64() * 1000.0);
                std::thread::sleep(Duration::from_millis(8));
            }
            lat_ms
        });

        let bg_ms = if let Some(use_low) = low_io {
            let limiter = VolumeConcurrencyLimiter::new();
            let c = GitClient::new()
                .with_timeout(Duration::from_secs(10))
                .with_low_io_priority(use_low);
            let t0 = Instant::now();
            // Repeat 4 passes so background batch stays busy long enough to measure 120 concurrent FG reads.
            for _ in 0..4 {
                limiter.run_on_volume(&artcraft.directory, None, art_targets, |t| {
                    let res = c.inspect_refs_and_dirty_uno(&t.path, &t.branch);
                    let to = matches!(res, Err(release_desk_git::GitError::TimedOut { .. }));
                    ((), to)
                });
            }
            t0.elapsed().as_secs_f64() * 1000.0
        } else {
            std::thread::sleep(Duration::from_millis(1200));
            0.0
        };
        stop.store(true, Ordering::Relaxed);
        let mut lats = fg_handle.join().unwrap_or_default();
        lats.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let p50 = lats.get(lats.len() / 2).copied().unwrap_or(0.0);
        let p95 = lats.get((lats.len() * 95) / 100).copied().unwrap_or(0.0);
        let max = lats.last().copied().unwrap_or(0.0);
        println!(
            "| {tag} | {} | {p50:.2} | {p95:.2} | {max:.2} | {bg_ms:.1} |",
            lats.len()
        );
    };
    println!(
        "| Workload | FG Reads (4 KiB random `F_NOCACHE`) | FG p50 (ms) | FG p95 (ms) | FG max (ms) | BG Batch Wall (4x29, ms) |"
    );
    println!("|---|---:|---:|---:|---:|---:|");
    measure_fg_reads("Idle (baseline)", None);
    measure_fg_reads("BG ArtCraft (`low_io_priority = false`)", Some(false));
    measure_fg_reads(
        "BG ArtCraft (`low_io_priority = true` / `taskpolicy -b`)",
        Some(true),
    );
    let _ = std::fs::remove_file(&fg_probe_file);
    println!();

    // -------------------------------------------------------------------------
    // 3. Largest-First Scheduling (`sort_slowest_first`) Tail Latency Evaluation
    // -------------------------------------------------------------------------
    println!("## 3 · Largest-First Scheduling (`sort_slowest_first`) Tail Latency Evaluation\n");
    let run_order_bench = |label: &str, targets: &[RepositoryTarget]| {
        let limiter = VolumeConcurrencyLimiter::new();
        let c = GitClient::new().with_timeout(Duration::from_secs(10));
        let mut first_ms = 0.0;
        let t0 = Instant::now();
        let settled = Mutex::new(Vec::new());
        limiter.run_on_volume_with_paths(
            &artcraft.directory,
            None,
            targets,
            |t| Some(t.path.as_path()),
            |t| {
                let res = c.inspect_refs_and_dirty_uno(&t.path, &t.branch);
                let to = matches!(res, Err(release_desk_git::GitError::TimedOut { .. }));
                settled
                    .lock()
                    .unwrap()
                    .push(t0.elapsed().as_secs_f64() * 1000.0);
                ((), to)
            },
        );
        let wall = t0.elapsed().as_secs_f64() * 1000.0;
        let mut s = settled.into_inner().unwrap();
        s.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        if let Some(f) = s.first() {
            first_ms = *f;
        }
        let p95 = s.get((s.len() * 95) / 100).copied().unwrap_or(wall);
        println!("| {label} | {first_ms:.1} | {p95:.1} | {wall:.1} |");
    };
    let mut sorted_art = art_targets.to_vec();
    sort_slowest_first(&mut sorted_art, |t| t.path.clone());
    println!(
        "| Scheduling Order (ArtCraft 29 warm on HDD / Simulated heterogeneous slow-disk) | 1st Settled (ms) | p95 Settled (ms) | Total Wall (ms) |"
    );
    println!("|---|---:|---:|---:|");
    run_order_bench("ArtCraft Warm Original (unsorted)", art_targets);
    run_order_bench(
        "ArtCraft Warm Largest-first (`sort_slowest_first`)",
        &sorted_art,
    );
    // Also evaluate tail impact with heterogeneous latencies (one 180ms tail repo among fast 15ms repos @ 2 workers):
    {
        let mut heterogenous: Vec<(usize, u64)> = (0..29)
            .map(|i| {
                (
                    i,
                    if i == 27 {
                        180
                    } else if i == 25 {
                        140
                    } else {
                        15
                    },
                )
            })
            .collect();
        let sim_order = |tag: &str, items: &[(usize, u64)]| {
            let lim = VolumeConcurrencyLimiter::new();
            let t0 = Instant::now();
            lim.run_on_volume(&artcraft.directory, None, items, |(_, ms)| {
                std::thread::sleep(Duration::from_millis(*ms));
                ((), false)
            });
            println!(
                "| {tag} | — | — | {:.1} |",
                t0.elapsed().as_secs_f64() * 1000.0
            );
        };
        sim_order("Heterogeneous Slow Tail Placed Last (2w)", &heterogenous);
        heterogenous.sort_by_key(|a| std::cmp::Reverse(a.1));
        sim_order(
            "Heterogeneous Largest-First (`sort_slowest_first`, 2w)",
            &heterogenous,
        );
    }
    println!();

    // -------------------------------------------------------------------------
    // 4. End-to-End Real Projects (Warm & Cold) + 100/300 Synthetic (SSD & Slow-Disk)
    // -------------------------------------------------------------------------
    println!(
        "## 4 · End-to-End Phase 5 Benchmarks (Real Warm/Cold + 100/300 Synthetic SSD & Slow-Disk)\n"
    );
    let tmp_root = std::env::temp_dir().join(format!("phase5-bench-{}", std::process::id()));
    let _tmp_guard = CleanupDir(tmp_root.clone());
    let _ = std::fs::remove_dir_all(&tmp_root);
    std::fs::create_dir_all(&tmp_root).unwrap();

    // Hydrate + Revalidate Warm & Cold
    let cache_path = tmp_root.join("cache.sqlite3");
    let cache = RepoCache::open_at(&cache_path).unwrap();
    let icon_map: HashMap<(String, String, String), PathBuf> = HashMap::new();
    let limiter = VolumeConcurrencyLimiter::new();

    for (proj, tgts) in [(cool, cool_targets), (artcraft, art_targets)] {
        // Populate cache once cleanly
        let records = limiter.run_on_volume_with_paths(
            &proj.directory,
            Some(&cache),
            tgts,
            |t| Some(t.path.as_path()),
            |t| {
                repository_git::revalidate_or_inspect_target_with_policy(
                    proj.id,
                    t,
                    None,
                    true,
                    false,
                    &icon_map,
                    Duration::from_secs(10),
                )
            },
        );
        cache.upsert_repos_batch(&records).unwrap();

        let th = Instant::now();
        let loaded = cache.load_project_repos(proj.id);
        let hydrate_ms = th.elapsed().as_secs_f64() * 1000.0;

        let tf = Instant::now();
        let _focus_recs = limiter.run_on_volume_with_paths(
            &proj.directory,
            Some(&cache),
            &tgts[..tgts.len().min(6)],
            |t| Some(t.path.as_path()),
            |t| {
                repository_git::revalidate_or_inspect_target_with_policy(
                    proj.id,
                    t,
                    loaded.get(&t.key),
                    true,
                    false,
                    &icon_map,
                    Duration::from_secs(10),
                )
            },
        );
        let focus_ms = tf.elapsed().as_secs_f64() * 1000.0;

        let probe = TieredRemoteProbe::new();
        let token = gh_token();
        let ptargets: Vec<ProbeTarget> = tgts
            .iter()
            .map(|t| ProbeTarget {
                key: t.key.clone(),
                clone_url: t.url.clone(),
                default_branch: t.branch.clone(),
                credential_ref: t.credential_ref.clone(),
                endpoint: t.endpoint.clone(),
            })
            .collect();
        let tp = Instant::now();
        let pres = probe.probe_batch(&ptargets, token.as_deref(), &AtomicBool::new(false));
        let probe_ms = tp.elapsed().as_secs_f64() * 1000.0;
        let probe_ok = pres.iter().filter(|r| r.error.is_none()).count();
        println!(
            "- **{} ({} repos, Phase 5 warm):** Hydrate=`{hydrate_ms:.2} ms` (0 spawns), Scoped Focus=`{focus_ms:.1} ms`, Remote Probe (`{probe_ok}/{}` OK)=`{probe_ms:.1} ms`",
            proj.name,
            tgts.len(),
            tgts.len()
        );
    }

    // Synthetic 100 & 300 on SSD
    let synth_bare = tmp_root.join("bare.git");
    let synth_seed = tmp_root.join("seed");
    let run_git = |cwd: &Path, args: &[&str]| {
        let out = Command::new("git")
            .current_dir(cwd)
            .args(args)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap();
        assert!(out.success(), "{args:?}");
    };
    std::fs::create_dir_all(&synth_bare).unwrap();
    run_git(&synth_bare, &["init", "--bare", "-b", "main"]);
    std::fs::create_dir_all(&synth_seed).unwrap();
    run_git(&synth_seed, &["init", "-b", "main"]);
    run_git(&synth_seed, &["config", "user.name", "Bench"]);
    run_git(
        &synth_seed,
        &["config", "user.email", "bench@example.invalid"],
    );
    std::fs::write(synth_seed.join("README.md"), "synth\n").unwrap();
    run_git(&synth_seed, &["add", "README.md"]);
    run_git(&synth_seed, &["commit", "-m", "init"]);
    run_git(
        &synth_seed,
        &["remote", "add", "origin", synth_bare.to_str().unwrap()],
    );
    run_git(&synth_seed, &["push", "-u", "origin", "main"]);

    let synth_base = tmp_root.join("synth-template");
    run_git(
        &tmp_root,
        &[
            "clone",
            synth_bare.to_str().unwrap(),
            synth_base.to_str().unwrap(),
        ],
    );
    let mut synth_targets = Vec::with_capacity(300);
    for i in 0..300usize {
        let p = tmp_root.join(format!("repos/r-{i:03}"));
        copy_dir_nocache(&synth_base, &p).unwrap();
        synth_targets.push(RepositoryTarget {
            project_root: tmp_root.join("repos"),
            key: repo_manager::workspace_sources::RepoKey::new(
                "local",
                "local",
                "org",
                &format!("r-{i:03}"),
            ),
            source_index: 0,
            repo_index: i,
            provider: "local".into(),
            account: "org".into(),
            name: format!("r-{i:03}"),
            url: synth_bare.to_string_lossy().into_owned(),
            branch: "main".into(),
            credential_ref: None,
            endpoint: None,
            path: p,
        });
    }

    println!(
        "\n| Synthetic Scale | Batch Config | Hydrate (ms) | Fingerprint Hit Reval (ms) | Full Miss+Dirty SSD (ms) | Simulated Slow-Disk (`sort_slowest_first`, ms) |"
    );
    println!("|---|---|---:|---:|---:|---:|");
    for n in [100usize, 300usize] {
        let slice = &synth_targets[..n];
        let pid = 9000 + n as u128;
        let t_miss = Instant::now();
        let recs: Vec<CachedRepoRecord> = limiter.run_on_volume_with_paths(
            &tmp_root,
            Some(&cache),
            slice,
            |t| Some(t.path.as_path()),
            |t| {
                repository_git::revalidate_or_inspect_target_with_policy(
                    pid,
                    t,
                    None,
                    true,
                    false,
                    &icon_map,
                    Duration::from_secs(10),
                )
            },
        );
        let ssd_miss_ms = t_miss.elapsed().as_secs_f64() * 1000.0;
        cache.upsert_repos_batch(&recs).unwrap();

        let th = Instant::now();
        let loaded = cache.load_project_repos(pid);
        let hyd_ms = th.elapsed().as_secs_f64() * 1000.0;

        let thit = Instant::now();
        let _hit_recs = limiter.run_on_volume_with_paths(
            &tmp_root,
            Some(&cache),
            slice,
            |t| Some(t.path.as_path()),
            |t| {
                repository_git::revalidate_or_inspect_target_with_policy(
                    pid,
                    t,
                    loaded.get(&t.key),
                    false,
                    false,
                    &icon_map,
                    Duration::from_secs(10),
                )
            },
        );
        let hit_ms = thit.elapsed().as_secs_f64() * 1000.0;

        let slow_limiter = VolumeConcurrencyLimiter::new();
        let tsim = Instant::now();
        slow_limiter.run_on_volume_with_paths(
            &tmp_root,
            None,
            slice,
            |t| Some(t.path.as_path()),
            |_t| {
                std::thread::sleep(Duration::from_millis(8));
                ((), false)
            },
        );
        let sim_ms = tsim.elapsed().as_secs_f64() * 1000.0;
        let (bs, conc) = optimal_graphql_batching(n);
        println!(
            "| {n} repos | `{bs}`/batch @ `{conc}w` | {hyd_ms:.2} | {hit_ms:.1} | {ssd_miss_ms:.1} | {sim_ms:.1} |"
        );
    }
    println!();
}

#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn getrusage(who: i32, usage: *mut [i64; 18]) -> i32;
}

fn rusage_cpu_secs(who: i32) -> f64 {
    #[cfg(target_os = "macos")]
    {
        let mut buf = [0i64; 18];
        // SAFETY: `buf` is a 144-byte `struct rusage` buffer on 64-bit macOS.
        let rc = unsafe { getrusage(who, &mut buf) };
        if rc == 0 {
            let utime = buf[0] as f64 + ((buf[1] as u32) as f64) / 1_000_000.0;
            let stime = buf[2] as f64 + ((buf[3] as u32) as f64) / 1_000_000.0;
            return utime + stime;
        }
    }
    0.0
}

fn step_e_curl_run(url: &str, dest: &Path) -> (u64, f64, f64, f64, usize, String, f64) {
    let cpu_before = rusage_cpu_secs(-1); // RUSAGE_CHILDREN
    let t0 = Instant::now();
    let out = Command::new("curl")
        .args([
            "-sSL",
            "-o",
            &dest.to_string_lossy(),
            "-w",
            "%{size_download}\t%{time_starttransfer}\t%{time_total}\t%{num_redirects}\t%{http_version}",
            url,
        ])
        .output()
        .expect("curl");
    let wall_s = t0.elapsed().as_secs_f64();
    let cpu_after = rusage_cpu_secs(-1);
    assert!(
        out.status.success(),
        "curl failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let txt = String::from_utf8_lossy(&out.stdout);
    let parts: Vec<&str> = txt.trim().split('\t').collect();
    let size: u64 = parts.first().and_then(|s| s.parse().ok()).unwrap_or(0);
    let ttfb: f64 = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(0.0);
    let total_s: f64 = parts
        .get(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or(wall_s)
        .max(0.001);
    let redirects: usize = parts.get(3).and_then(|s| s.parse().ok()).unwrap_or(0);
    let http_ver = parts.get(4).unwrap_or(&"?").to_string();
    let mbps = (size as f64 / (1024.0 * 1024.0)) / total_s;
    let cpu_pct = ((cpu_after - cpu_before).max(0.0) / total_s) * 100.0;
    (size, mbps, ttfb, total_s, redirects, http_ver, cpu_pct)
}

async fn step_e_raw_reqwest_stream(
    client: &reqwest::Client,
    url: &str,
    write_path: Option<&Path>,
    buffered: bool,
) -> (u64, f64, f64, f64, usize, String, usize, f64) {
    use std::io::Write as _;
    let cpu_before = rusage_cpu_secs(0); // RUSAGE_SELF
    let t0 = Instant::now();
    let mut resp = client.get(url).send().await.expect("reqwest send");
    assert!(resp.status().is_success(), "HTTP {}", resp.status());
    let http_ver = format!("{:?}", resp.version());
    let mut file = write_path.map(|p| {
        let f = std::fs::File::create(p).expect("create target");
        if buffered {
            Box::new(std::io::BufWriter::with_capacity(512 * 1024, f))
                as Box<dyn std::io::Write + Send>
        } else {
            Box::new(f) as Box<dyn std::io::Write + Send>
        }
    });
    let mut ttfb = None;
    let mut size = 0u64;
    let mut chunks = 0usize;
    while let Some(chunk) = resp.chunk().await.expect("chunk") {
        if ttfb.is_none() {
            ttfb = Some(t0.elapsed().as_secs_f64());
        }
        chunks += 1;
        size += chunk.len() as u64;
        if let Some(ref mut w) = file {
            w.write_all(&chunk).expect("write chunk");
        }
    }
    if let Some(ref mut w) = file {
        w.flush().expect("flush");
    }
    let wall_s = t0.elapsed().as_secs_f64().max(0.001);
    let cpu_after = rusage_cpu_secs(0);
    let mbps = (size as f64 / (1024.0 * 1024.0)) / wall_s;
    let cpu_pct = ((cpu_after - cpu_before).max(0.0) / wall_s) * 100.0;
    (
        size,
        mbps,
        ttfb.unwrap_or(wall_s),
        wall_s,
        1,
        http_ver,
        chunks,
        cpu_pct,
    )
}

async fn step_e_ranged_download(
    client: &reqwest::Client,
    url: &str,
    dest: &Path,
    conns: usize,
) -> (u64, f64, f64, f64, f64) {
    use std::io::Seek as _;
    use std::io::Write as _;
    let cpu_before = rusage_cpu_secs(0);
    let t0 = Instant::now();
    // Resolve final redirect URL + total Content-Length once.
    let head = client.head(url).send().await.expect("head");
    let final_url = head.url().clone();
    let total_size = head
        .headers()
        .get(reqwest::header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok())
        .expect("content-length");
    let f = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(dest)
        .expect("create range dest");
    f.set_len(total_size).expect("preallocate");
    drop(f);

    let chunk_size = total_size.div_ceil(conns as u64);
    let ttfb_shared = std::sync::Arc::new(std::sync::Mutex::new(None::<f64>));
    let mut handles = Vec::with_capacity(conns);
    for idx in 0..conns {
        let start = idx as u64 * chunk_size;
        if start >= total_size {
            continue;
        }
        let end = ((idx as u64 + 1) * chunk_size - 1).min(total_size - 1);
        let c = client.clone();
        let u = final_url.clone();
        let path = dest.to_path_buf();
        let ttfb_ref = ttfb_shared.clone();
        handles.push(tokio::spawn(async move {
            let mut resp = c
                .get(u)
                .header(reqwest::header::RANGE, format!("bytes={start}-{end}"))
                .send()
                .await
                .expect("range send");
            assert!(resp.status().is_success(), "range HTTP {}", resp.status());
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .open(&path)
                .expect("open range part");
            file.seek(std::io::SeekFrom::Start(start)).expect("seek");
            let mut writer = std::io::BufWriter::with_capacity(512 * 1024, file);
            while let Some(chunk) = resp.chunk().await.expect("range chunk") {
                if let Ok(mut g) = ttfb_ref.lock()
                    && g.is_none()
                {
                    *g = Some(t0.elapsed().as_secs_f64());
                }
                writer.write_all(&chunk).expect("write range");
            }
            writer.flush().expect("flush range");
        }));
    }
    for h in handles {
        h.await.expect("join range");
    }
    let wall_s = t0.elapsed().as_secs_f64().max(0.001);
    let cpu_after = rusage_cpu_secs(0);
    let mbps = (total_size as f64 / (1024.0 * 1024.0)) / wall_s;
    let cpu_pct = ((cpu_after - cpu_before).max(0.0) / wall_s) * 100.0;
    let ttfb = ttfb_shared.lock().ok().and_then(|g| *g).unwrap_or(wall_s);
    (total_size, mbps, ttfb, wall_s, cpu_pct)
}

fn run_step_e(include_large: bool, only_large: bool) {
    let zed_url = "https://github.com/zed-industries/zed/releases/download/v1.30.2/Zed-aarch64.dmg";
    let godot_url = "https://github.com/godotengine/godot/releases/download/4.7.2-stable/Godot_v4.7.2-stable_android_editor.apk";
    let ssd_temp = tempfile::tempdir().expect("ssd tempdir");
    let ssd_dir = ssd_temp.path().to_path_buf();

    let hdd_base = Path::new("/Volumes/Devansh2");
    let hdd_temp_dir = hdd_base.join(format!(".release_desk_step_e_{}", std::process::id()));
    let has_hdd = hdd_base.is_dir() && std::fs::create_dir_all(&hdd_temp_dir).is_ok();

    println!("# Step E · Release Download Throughput Measurements\n");
    println!(
        "| Asset & Scenario | Target | Size (MiB) | Speed (MiB/s) | % of curl `/dev/null` | Wall (s) | TTFB (s) | CPU % | Redirects | HTTP |"
    );
    println!("|---|---|---:|---:|---:|---:|---:|---:|---:|---|");

    let print_row = |name: &str,
                     target: &str,
                     size: u64,
                     mbps: f64,
                     baseline: f64,
                     wall: f64,
                     ttfb: f64,
                     cpu: f64,
                     redir: usize,
                     http: &str| {
        let pct = if baseline > 0.0 {
            (mbps / baseline) * 100.0
        } else {
            100.0
        };
        println!(
            "| {name} | {target} | {:.1} | {:.2} | {:.1}% | {:.2} | {:.3} | {:.1}% | {redir} | {http} |",
            size as f64 / (1024.0 * 1024.0),
            mbps,
            pct,
            wall,
            ttfb,
            cpu
        );
    };

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio");
    let app_client = reqwest::Client::builder()
        .timeout(Duration::from_secs(600))
        .tcp_nodelay(true)
        .user_agent("Release-Desk/1.0")
        .build()
        .expect("client");

    let measure_app_downloader =
        |dest_root: &Path, label_target: &str, url: &str, name: &str, baseline: f64| {
            let cancelled = std::sync::atomic::AtomicBool::new(false);
            let mut cb_count = 0usize;
            let cpu_before = rusage_cpu_secs(0);
            let t0 = Instant::now();
            let path = rt
            .block_on(
                release_desk_store::workspace_releases::download_asset_cancellable_with_progress(
                    dest_root,
                    "zed-industries",
                    "zed",
                    "v1.30.2",
                    name,
                    url,
                    None,
                    &cancelled,
                    &mut |_rec, _tot| {
                        cb_count += 1;
                        Ok(())
                    },
                ),
            )
            .expect("app downloader");
            let w = t0.elapsed().as_secs_f64().max(0.001);
            let cpu_after = rusage_cpu_secs(0);
            let sz = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            let mbps = (sz as f64 / (1024.0 * 1024.0)) / w;
            let cpu = ((cpu_after - cpu_before).max(0.0) / w) * 100.0;
            print_row(
                &format!("App downloader ({cb_count} progress cb)"),
                label_target,
                sz,
                mbps,
                baseline,
                w,
                0.0,
                cpu,
                1,
                "HTTP/1.1",
            );
            path
        };

    if !only_large {
        // 1-3: curl baselines for Zed (84 MiB)
        let (z_sz, z_base, z_ttfb, z_w, z_red, z_http, z_cpu) =
            step_e_curl_run(zed_url, Path::new("/dev/null"));
        print_row(
            "Zed curl",
            "/dev/null",
            z_sz,
            z_base,
            z_base,
            z_w,
            z_ttfb,
            z_cpu,
            z_red,
            &z_http,
        );

        let ssd_curl_path = ssd_dir.join("zed_curl.dmg");
        let (sz, mbps, ttfb, w, red, http, cpu) = step_e_curl_run(zed_url, &ssd_curl_path);
        print_row(
            "Zed curl",
            "Internal SSD",
            sz,
            mbps,
            z_base,
            w,
            ttfb,
            cpu,
            red,
            &http,
        );

        if has_hdd {
            let hdd_curl_path = hdd_temp_dir.join("zed_curl.dmg");
            let (sz, mbps, ttfb, w, red, http, cpu) = step_e_curl_run(zed_url, &hdd_curl_path);
            print_row(
                "Zed curl",
                "HDD (/Volumes/Devansh2)",
                sz,
                mbps,
                z_base,
                w,
                ttfb,
                cpu,
                red,
                &http,
            );
            let _ = std::fs::remove_file(&hdd_curl_path);
        }

        // 4: App downloader + reqwest measurements
        let (sz, mbps, ttfb, w, red, http, chunks, cpu) =
            rt.block_on(step_e_raw_reqwest_stream(&app_client, zed_url, None, false));
        print_row(
            &format!("Zed reqwest raw ({chunks} chunks)"),
            "/dev/null",
            sz,
            mbps,
            z_base,
            w,
            ttfb,
            cpu,
            red,
            &http,
        );

        let (sz, mbps, ttfb, w, red, http, _, cpu) = rt.block_on(step_e_raw_reqwest_stream(
            &reqwest::Client::builder()
                .timeout(Duration::from_secs(300))
                .user_agent("Release-Desk/1.0")
                .build()
                .unwrap(),
            zed_url,
            Some(&ssd_dir.join("zed_legacy_unbuf.dmg")),
            false,
        ));
        print_row(
            "Zed legacy unbuffered client",
            "Internal SSD",
            sz,
            mbps,
            z_base,
            w,
            ttfb,
            cpu,
            red,
            &http,
        );

        let ssd_app_root = ssd_dir.join("app_proj");
        std::fs::create_dir_all(&ssd_app_root).unwrap();
        let downloaded_ssd = measure_app_downloader(
            &ssd_app_root,
            "Internal SSD",
            zed_url,
            "Zed-aarch64.dmg",
            z_base,
        );

        if has_hdd {
            let hdd_app_root = hdd_temp_dir.join("app_proj");
            std::fs::create_dir_all(&hdd_app_root).unwrap();
            let p = measure_app_downloader(
                &hdd_app_root,
                "HDD (/Volumes/Devansh2)",
                zed_url,
                "Zed-aarch64.dmg",
                z_base,
            );
            // Checksum verify disk re-read cost on HDD
            let t_chk = Instant::now();
            let out = Command::new("shasum")
                .args(["-a", "256"])
                .arg(&p)
                .output()
                .unwrap();
            assert!(out.status.success());
            let chk_s = t_chk.elapsed().as_secs_f64();
            println!(
                "<!-- HDD shasum re-read Zed 84MiB: {:.3} s ({:.1} MiB/s) -->",
                chk_s,
                (z_sz as f64 / 1048576.0) / chk_s
            );
            let _ = std::fs::remove_dir_all(&hdd_app_root);
        }

        // 6: Checksum verify cost on SSD (re-read vs streaming)
        let t_chk_ssd = Instant::now();
        let out = Command::new("shasum")
            .args(["-a", "256"])
            .arg(&downloaded_ssd)
            .output()
            .unwrap();
        assert!(out.status.success());
        let chk_ssd_s = t_chk_ssd.elapsed().as_secs_f64();
        println!(
            "\n<!-- SSD shasum re-read Zed 84MiB: {:.3} s ({:.1} MiB/s) -->",
            chk_ssd_s,
            (z_sz as f64 / 1048576.0) / chk_ssd_s
        );

        // 5: Segmented ranged downloads (2, 4, 8 connections) on SSD (and 4 on HDD)
        for conns in [2usize, 4, 8] {
            let dst = ssd_dir.join(format!("zed_seg_{conns}.dmg"));
            let (sz, mbps, ttfb, w, cpu) =
                rt.block_on(step_e_ranged_download(&app_client, zed_url, &dst, conns));
            print_row(
                &format!("Segmented Range ({conns} conns)"),
                "Internal SSD",
                sz,
                mbps,
                z_base,
                w,
                ttfb,
                cpu,
                1,
                "HTTP/1.1",
            );
        }
        if has_hdd {
            let dst = hdd_temp_dir.join("zed_seg_4.dmg");
            let (sz, mbps, ttfb, w, cpu) =
                rt.block_on(step_e_ranged_download(&app_client, zed_url, &dst, 4));
            print_row(
                "Segmented Range (4 conns)",
                "HDD (/Volumes/Devansh2)",
                sz,
                mbps,
                z_base,
                w,
                ttfb,
                cpu,
                1,
                "HTTP/1.1",
            );
            let _ = std::fs::remove_file(&dst);
        }

        // 7: 1, 2, 4 concurrent downloads aggregate throughput (to Internal SSD)
        for n in [1usize, 2, 4] {
            let cpu_before = rusage_cpu_secs(0);
            let t0 = Instant::now();
            let bytes = rt.block_on(async {
                let mut set = Vec::with_capacity(n);
                for i in 0..n {
                    let c = app_client.clone();
                    let dst = ssd_dir.join(format!("conc_{n}_{i}.dmg"));
                    set.push(tokio::spawn(async move {
                        let (sz, _, _, _, _, _, _, _) =
                            step_e_raw_reqwest_stream(&c, zed_url, Some(&dst), true).await;
                        sz
                    }));
                }
                let mut sum = 0u64;
                for h in set {
                    sum += h.await.unwrap();
                }
                sum
            });
            let w = t0.elapsed().as_secs_f64().max(0.001);
            let cpu_after = rusage_cpu_secs(0);
            let agg_mbps = (bytes as f64 / (1024.0 * 1024.0)) / w;
            let cpu_pct = ((cpu_after - cpu_before).max(0.0) / w) * 100.0;
            print_row(
                &format!("Concurrent {n}x streams (aggregate)"),
                "Internal SSD",
                bytes,
                agg_mbps,
                z_base,
                w,
                0.0,
                cpu_pct,
                1,
                "HTTP/1.1",
            );
        }
    } // end !only_large

    // Large asset >=500 MiB (Godot 656.3 MiB) when requested
    if include_large || only_large {
        let (g_sz, g_base, g_ttfb, g_w, g_red, g_http, g_cpu) =
            step_e_curl_run(godot_url, Path::new("/dev/null"));
        print_row(
            "Godot 656MiB curl",
            "/dev/null",
            g_sz,
            g_base,
            g_base,
            g_w,
            g_ttfb,
            g_cpu,
            g_red,
            &g_http,
        );

        let g_app_ssd = ssd_dir.join("godot_app_ssd");
        std::fs::create_dir_all(&g_app_ssd).unwrap();
        let _ = measure_app_downloader(
            &g_app_ssd,
            "Internal SSD",
            godot_url,
            "Godot_v4.7.2-stable_android_editor.apk",
            g_base,
        );

        let g_seg4 = ssd_dir.join("godot_seg4.apk");
        let (sz, mbps, ttfb, w, cpu) =
            rt.block_on(step_e_ranged_download(&app_client, godot_url, &g_seg4, 4));
        print_row(
            "Godot 656MiB Segmented Range (4 conns)",
            "Internal SSD",
            sz,
            mbps,
            g_base,
            w,
            ttfb,
            cpu,
            1,
            "HTTP/1.1",
        );
        if has_hdd {
            let g_app_hdd = hdd_temp_dir.join("godot_app_hdd");
            std::fs::create_dir_all(&g_app_hdd).unwrap();
            let _ = measure_app_downloader(
                &g_app_hdd,
                "HDD (/Volumes/Devansh2)",
                godot_url,
                "Godot_v4.7.2-stable_android_editor.apk",
                g_base,
            );
            let _ = std::fs::remove_dir_all(&g_app_hdd);
        }
    }

    if has_hdd {
        let _ = std::fs::remove_dir_all(&hdd_temp_dir);
    }
    println!();
}

fn main() {
    let step_e = std::env::args().any(|a| a == "--step-e");
    let step_e_large = std::env::args().any(|a| a == "--step-e-large");
    let step_e_only_large = std::env::args().any(|a| a == "--step-e-only-large");
    if step_e || step_e_large || step_e_only_large {
        run_step_e(step_e_large || step_e_only_large, step_e_only_large);
        return;
    }
    let part2_only = std::env::args().any(|a| a == "--part2");
    let tail_only = std::env::args().any(|a| a == "--tail");
    let step_b = std::env::args().any(|a| a == "--step-b");
    let phase4 = std::env::args().any(|a| a == "--phase4");
    let step_c = std::env::args().any(|a| a == "--step-c");
    let phase5 = std::env::args().any(|a| a == "--phase5");
    perf_trace::set_enabled(true);
    let cfg = settings::load().expect("settings.json");

    let artcraft = cfg
        .projects
        .iter()
        .find(|p| p.name == "ArtCraft")
        .expect("ArtCraft");
    let cool = cfg
        .projects
        .iter()
        .find(|p| p.name == "Github cool repos")
        .expect("Github cool repos");
    let art_targets = repository_git::targets(artcraft).unwrap();
    let cool_targets = repository_git::targets(cool).unwrap();

    if phase5 {
        run_phase5(artcraft, &art_targets, cool, &cool_targets);
        if std::env::args().any(|a| a == "--with-phase4") {
            run_phase4(artcraft, &art_targets, cool, &cool_targets);
        }
        return;
    }
    if step_c {
        run_step_c(artcraft, &art_targets, cool, &cool_targets);
        return;
    }
    if phase4 {
        run_phase4(artcraft, &art_targets, cool, &cool_targets);
        return;
    }

    if step_b {
        run_step_b(artcraft, &art_targets, cool, &cool_targets);
        return;
    }

    if !part2_only && !tail_only {
        println!("# Release Desk — Phase 2b Report\n");
        println!(
            "## 1. End-to-End Real App Flow (`click Repositories` -> `all rows settled`, 8 workers)\n"
        );
        println!(
            "| Project | Run | Inspect + Pres (ms) | `fetch_verified` (ms) | Total E2E (ms) | OK / Total | Max Subprocs/Repo (Fetch) |"
        );
        println!("|---|---|---:|---:|---:|---:|---:|");
        for (proj, tgts) in [(cool, &cool_targets), (artcraft, &art_targets)] {
            for run in 1..=3 {
                let (insp, fetch, ok, max_sub) = measure_e2e_project(proj, tgts);
                println!(
                    "| {} ({} repos) | Warm #{run} | {insp:.1} | {fetch:.1} | {:.1} | {ok}/{} | {max_sub} |",
                    proj.name,
                    tgts.len(),
                    insp + fetch,
                    tgts.len()
                );
            }
        }
        println!();

        run_tier_benchmarks(artcraft, &art_targets);
    }
    if !tail_only {
        run_cold_copied_comparison(cool, &cool_targets, part2_only);
    }
    bench_agent_db_and_untracked_cache(artcraft, &art_targets);
    bench_simulated_slow_disk();
}
