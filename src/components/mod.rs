//! UI shell components.

pub mod app_icon;
pub mod header;
pub mod motion_button;
pub mod provider_icon;
pub mod sidebar;
pub mod ui_icons;

pub use header::render_header;
pub use sidebar::render_sidebar;

use gpui_kit::component::button::ButtonVariants as _;
use motion_button::Button;

pub fn toggle_button(
    id: impl Into<gpui_kit::ElementId>,
    label: impl Into<gpui_kit::SharedString>,
    active: bool,
) -> Button {
    let btn = Button::new(id).label(label);
    if active { btn.primary() } else { btn.ghost() }
}
