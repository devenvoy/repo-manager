//! Fictional app icons for the desktop design preview.

use gpui_kit::component::Icon;
use gpui_kit::{Styled as _, px};

pub fn app_icon(id: &str) -> Icon {
    let data: &[u8] = match id {
        "prism" => include_bytes!("../../resources/app-icons/prism.svg"),
        "quill" => include_bytes!("../../resources/app-icons/quill.svg"),
        "atlas" => include_bytes!("../../resources/app-icons/atlas.svg"),
        "orbit" => include_bytes!("../../resources/app-icons/orbit.svg"),
        _ => include_bytes!("../../resources/app-icons/relay.svg"),
    };
    Icon::default().data(data).w(px(44.)).h(px(44.))
}
