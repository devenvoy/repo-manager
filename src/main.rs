//! Release Desk native desktop shell entrypoint.

use gpui_kit::AppContext as _;
use gpui_kit::WindowOptions;
use image::ImageFormat;
use repo_manager::ReleaseDesk;
use std::io::Cursor;
use std::sync::Arc;

/// Embedded window icon used as the taskbar/dock/window icon on all platforms.
/// macOS bundle icon is `resources/AppIcon.icns` (Info.plist CFBundleIconFile).
/// Windows .exe icon is `resources/app.ico` embedded via `resources/app.rc`.
fn window_icon() -> Option<Arc<image::RgbaImage>> {
    let bytes: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/resources/app-icon-256.png"
    ));
    let img = image::load(Cursor::new(bytes), ImageFormat::Png)
        .ok()?
        .into_rgba8();
    Some(Arc::new(img))
}

fn main() {
    if let Some(code) = repo_manager::git_credential_helper::dispatch() {
        std::process::exit(code);
    }
    gpui_kit::application()
        .with_assets(gpui_kit::assets::AllAssets)
        .run(|cx| {
            gpui_kit::init(cx);
            gpui_kit::open_window(
                WindowOptions {
                    icon: window_icon(),
                    ..Default::default()
                },
                cx,
                |window, cx| cx.new(|cx| ReleaseDesk::new(window, cx)),
            )
            .expect("failed to open Release Desk window");
        });
}
