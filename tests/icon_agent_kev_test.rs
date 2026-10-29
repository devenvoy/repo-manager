//! Integration tests for the Kev-0.8b Decision Model Tool and Agent Pipeline.
//!
//! Verifies:
//! 1. Icon selection ranks valid candidate files.
//! 2. Kev tool filters candidates down to the Top 10.
//! 3. The authentic engraved animal icon (e.g. `photocraft-1024.png`) beats the umbrella ARTCRAFT banner.
//! 4. Kev's #1 suggested icon is the final selection.
//! 5. Selected icon is stored in `.release-desk/icons/` and recorded in `.release-desk/agent.db`.

use repo_manager::modules::icon_agent::kev::KevClient;
use repo_manager::modules::icon_agent::scanner::scan_repo;
use repo_manager::modules::icon_agent::{resolve_icon_for, scan_and_cache_icon};
use std::path::Path;

fn create_sample_png(path: &Path, width: u32, height: u32) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    image::RgbaImage::new(width, height).save(path).unwrap();
}

#[test]
fn test_full_size_svg_beats_small_svg_variant() {
    let temp = tempfile::tempdir().unwrap();
    let icon_dir = temp.path().join("assets/app-icon");
    std::fs::create_dir_all(&icon_dir).unwrap();
    std::fs::write(
        icon_dir.join("designcraft-small.svg"),
        r#"<svg viewBox="0 0 512 512"><circle cx="256" cy="256" r="50"/></svg>"#,
    )
    .unwrap();
    std::fs::write(
        icon_dir.join("designcraft.svg"),
        r#"<svg viewBox="0 0 512 512"><circle cx="256" cy="256" r="200"/><path d="M0 0 L512 512"/></svg>"#,
    )
    .unwrap();
    create_sample_png(&icon_dir.join("designcraft-1024.png"), 1024, 1024);

    let candidates = scan_repo(temp.path(), "designcraft");
    let mut kev = KevClient::new("http://127.0.0.1:0", "jaredpalmer/kev-0.8b");
    kev.timeout_ms = 100;

    let result = kev
        .rank_and_filter_top10(&candidates, "designcraft")
        .expect("Kev decision");
    assert_eq!(result.total_scanned, 3);
    assert_eq!(result.top_choice.file_name, "designcraft.svg");
    assert_eq!(result.top_10.len(), 1);

    let small_only = candidates
        .iter()
        .filter(|candidate| candidate.file_name == "designcraft-small.svg")
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(
        kev.rank_and_filter_top10(&small_only, "designcraft")
            .expect("small asset is usable alone")
            .top_choice
            .file_name,
        "designcraft-small.svg"
    );
}

#[test]
fn test_kev_tool_photocraft_fox_beats_artcraft_banner() {
    let temp = tempfile::tempdir().unwrap();
    let repo_dir = temp.path().join("PhotoCraft");

    // 1. Suite umbrella banner (1200x300 horizontal "ARTCRAFT" banner)
    let banner_svg = repo_dir.join("assets/logo.svg");
    std::fs::create_dir_all(banner_svg.parent().unwrap()).unwrap();
    std::fs::write(
        &banner_svg,
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1200 300"><text>ARTCRAFT</text></svg>"#,
    )
    .unwrap();

    // 2. Real square app icon: 1024x1024 blue tile with nine-tailed fox engraving
    let fox_icon = repo_dir.join("assets/app-icon/photocraft-1024.png");
    create_sample_png(&fox_icon, 1024, 1024);

    // 3. Documentation screenshot (1920x1080)
    let screenshot = repo_dir.join("docs/screenshot.png");
    create_sample_png(&screenshot, 1920, 1080);

    // 4. Hero banner
    let hero = repo_dir.join("assets/hero-banner.png");
    create_sample_png(&hero, 800, 200);

    // Run Scanner Tool
    let candidates = scan_repo(&repo_dir, "photocraft");
    assert!(!candidates.is_empty(), "Scanner must find image candidates");

    // Run Kev Decision Tool
    let kev = KevClient::default();
    let kev_result = kev
        .rank_and_filter_top10(&candidates, "photocraft")
        .expect("Kev decision evaluation");

    // Assert Kev #1 choice is the authentic 1024x1024 fox icon
    assert_eq!(
        kev_result.top_choice.file_name, "photocraft-1024.png",
        "Kev decision model must select photocraft-1024.png as top choice"
    );
    // Top 10 must contain valid candidates and not exceed 10
    assert!(kev_result.top_10.len() <= 10);
    assert_eq!(
        kev_result.top_10[0].candidate.file_name,
        "photocraft-1024.png"
    );
}

#[test]
fn test_kev_model_first_file_is_final_selection() {
    let temp = tempfile::tempdir().unwrap();
    let repo_dir = temp.path().join("VectorCraft");

    // Authentic icon: Red tile with dragon engraving
    let dragon_icon = repo_dir.join("assets/app-icon/vectorcraft-1024.png");
    create_sample_png(&dragon_icon, 1024, 1024);

    // Umbrella banner
    let header = repo_dir.join("assets/artcraft-header.png");
    create_sample_png(&header, 1000, 250);

    let candidates = scan_repo(&repo_dir, "vectorcraft");

    let kev = KevClient::default();
    let selected = kev
        .rank_and_filter_top10(&candidates, "vectorcraft")
        .expect("icon selection")
        .top_choice;

    // Kev's top choice is final without another model call.
    assert_eq!(
        selected.file_name, "vectorcraft-1024.png",
        "Final selected icon must be the first file suggested by Kev model"
    );
    assert_eq!(
        selected.relative_path,
        "assets/app-icon/vectorcraft-1024.png"
    );
}

#[test]
fn test_end_to_end_scan_and_cache_into_hidden_release_desk_folder() {
    let temp = tempfile::tempdir().unwrap();
    let proj_dir = temp.path();
    let repo_dir = proj_dir.join("FilmCraft");

    // Authentic icon: Purple tile with owl engraving
    let owl_icon = repo_dir.join("assets/app-icon/filmcraft-1024.png");
    create_sample_png(&owl_icon, 1024, 1024);

    // Umbrella banner
    let banner = repo_dir.join("assets/banner.png");
    create_sample_png(&banner, 900, 250);

    // Pipeline with Kev
    let kev = KevClient::default();

    let cached = scan_and_cache_icon(
        Some(proj_dir),
        "github",
        "storytold",
        "filmcraft",
        &repo_dir,
        &kev,
    );

    assert!(cached.is_some(), "Must successfully scan and cache icon");
    let cached_path = cached.unwrap();

    // Verify it is saved in .release-desk/icons/
    assert!(
        cached_path.is_file(),
        "Cached icon file must exist on disk: {}",
        cached_path.display()
    );
    assert!(
        cached_path.starts_with(proj_dir.join(".release-desk").join("icons")),
        "Cached file must reside in project's .release-desk/icons/"
    );

    // Verify instant binding from SQLite agent.db
    let resolved = resolve_icon_for(Some(proj_dir), "github", "storytold", "filmcraft");
    assert_eq!(
        resolved,
        Some(cached_path),
        "Fast path query must immediately return the cached icon"
    );
}
