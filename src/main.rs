//! Release Desk native desktop shell entrypoint.

use gpui_kit::AppContext as _;
use gpui_kit::WindowOptions;
use repo_manager::ReleaseDesk;

fn main() {
    if let Some(code) = repo_manager::git_credential_helper::dispatch() {
        std::process::exit(code);
    }
    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(|cx| {
            gpui_kit::init(cx);
            gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
                cx.new(|cx| ReleaseDesk::new(window, cx))
            })
            .expect("failed to open Release Desk window");
        });
}
