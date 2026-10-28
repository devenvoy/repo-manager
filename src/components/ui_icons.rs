//! Common UI icons bundled as inline SVG for native GPUI rendering.

use gpui_kit::component::Icon;
use gpui_kit::{Styled as _, px};

pub fn brand_logo() -> Icon {
    let svg = br##"<svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="white" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="m12 3 9 5-9 5-9-5 9-5Zm-9 9 9 5 9-5M3 16l9 5 9-5"/></svg>"##;
    Icon::default().data(svg).w(px(18.)).h(px(18.))
}

pub fn grid_icon() -> Icon {
    let svg = br##"<svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="3" width="7" height="7" rx="1.5"/><rect x="14" y="3" width="7" height="7" rx="1.5"/><rect x="3" y="14" width="7" height="7" rx="1.5"/><rect x="14" y="14" width="7" height="7" rx="1.5"/></svg>"##;
    Icon::default().data(svg).w(px(16.)).h(px(16.))
}

pub fn store_icon() -> Icon {
    let svg = br##"<svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M3 10h18l-1-5H4l-1 5Z"/><path d="M5 10v10h14V10M9 20v-6h6v6"/></svg>"##;
    Icon::default().data(svg).w(px(16.)).h(px(16.))
}

pub fn settings_icon() -> Icon {
    let svg = br##"<svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1 0 2.83 2 2 0 0 1-2.83 0l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-2 2 2 2 0 0 1-2-2v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83 0 2 2 0 0 1 0-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1-2-2 2 2 0 0 1 2-2h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 0-2.83 2 2 0 0 1 2.83 0l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 2-2 2 2 0 0 1 2 2v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 0 2 2 0 0 1 0 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 2 2 2 2 0 0 1-2 2h-.09a1.65 1.65 0 0 0-1.51 1Z"/></svg>"##;
    Icon::default().data(svg).w(px(16.)).h(px(16.))
}

pub fn folder_icon() -> Icon {
    let svg = br##"<svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M3 7V5a2 2 0 0 1 2-2h5l2 3h7a2 2 0 0 1 2 2v11a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V7Z"/></svg>"##;
    Icon::default().data(svg).w(px(16.)).h(px(16.))
}

pub fn plus_icon() -> Icon {
    let svg = br##"<svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 5v14M5 12h14"/></svg>"##;
    Icon::default().data(svg).w(px(14.)).h(px(14.))
}

pub fn check_icon() -> Icon {
    let svg = br##"<svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><path d="m4 12 5 5L20 6"/></svg>"##;
    Icon::default().data(svg).w(px(14.)).h(px(14.))
}

pub fn shield_icon() -> Icon {
    let svg = br##"<svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M12 22s8-4 8-11V5l-8-3-8 3v6c0 7 8 11 8 11Z"/><path d="m9 12 2 2 4-4"/></svg>"##;
    Icon::default().data(svg).w(px(15.)).h(px(15.))
}

pub fn arrow_right_icon() -> Icon {
    let svg = br##"<svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="m9 18 6-6-6-6"/></svg>"##;
    Icon::default().data(svg).w(px(13.)).h(px(13.))
}

pub fn arrow_left_icon() -> Icon {
    let svg = br##"<svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="m15 18-6-6 6-6"/></svg>"##;
    Icon::default().data(svg).w(px(13.)).h(px(13.))
}

pub fn download_icon() -> Icon {
    let svg = br##"<svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M12 3v12m-4-4 4 4 4-4M4 17v4h16v-4"/></svg>"##;
    Icon::default().data(svg).w(px(14.)).h(px(14.))
}

pub fn layers_icon() -> Icon {
    let svg = br##"<svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="m12 3 9 5-9 5-9-5 9-5Zm-9 9 9 5 9-5M3 16l9 5 9-5"/></svg>"##;
    Icon::default().data(svg).w(px(15.)).h(px(15.))
}

pub fn repository_icon() -> Icon {
    let svg = br##"<svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><rect x="4" y="3" width="16" height="18" rx="2"/><path d="M8 7h8M8 11h8M8 15h5"/></svg>"##;
    Icon::default().data(svg).w(px(17.)).h(px(17.))
}

pub fn branch_icon() -> Icon {
    let svg = br##"<svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><circle cx="6" cy="4" r="2"/><circle cx="6" cy="20" r="2"/><circle cx="18" cy="6" r="2"/><path d="M6 6v12M18 8a7 7 0 0 1-7 7H6"/></svg>"##;
    Icon::default().data(svg).w(px(15.)).h(px(15.))
}

pub fn star_icon() -> Icon {
    let svg = br##"<svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linejoin="round"><path d="m12 2 3 6.3 7 1-5 4.9 1.2 6.8L12 17.8 5.8 21 7 14.2 2 9.3l7-1L12 2Z"/></svg>"##;
    Icon::default().data(svg).w(px(14.)).h(px(14.))
}

pub fn fork_icon() -> Icon {
    let svg = br##"<svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><circle cx="6" cy="5" r="2"/><circle cx="18" cy="5" r="2"/><circle cx="12" cy="19" r="2"/><path d="M6 7v4a5 5 0 0 0 5 5h2a5 5 0 0 0 5-5V7"/></svg>"##;
    Icon::default().data(svg).w(px(14.)).h(px(14.))
}

pub fn refresh_icon() -> Icon {
    let svg = br##"<svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M20 11a8 8 0 0 0-14.7-4.4L3 9m0-5v5h5M4 13a8 8 0 0 0 14.7 4.4L21 15m0 5v-5h-5"/></svg>"##;
    Icon::default().data(svg).w(px(15.)).h(px(15.))
}

pub fn trash_icon() -> Icon {
    let svg = br##"<svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M4 7h16M9 7V4h6v3M6 7l1 13h10l1-13M10 11v5m4-5v5"/></svg>"##;
    Icon::default().data(svg).w(px(15.)).h(px(15.))
}

pub fn lock_icon() -> Icon {
    let svg = br##"<svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><rect x="5" y="10" width="14" height="11" rx="2"/><path d="M8 10V7a4 4 0 0 1 8 0v3"/></svg>"##;
    Icon::default().data(svg).w(px(14.)).h(px(14.))
}

pub fn external_link_icon() -> Icon {
    let svg = br##"<svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6"/><polyline points="15 3 21 3 21 9"/><line x1="10" y1="14" x2="21" y2="3"/></svg>"##;
    Icon::default().data(svg).w(px(14.)).h(px(14.))
}

pub fn copy_icon() -> Icon {
    let svg = br##"<svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><rect x="9" y="9" width="13" height="13" rx="2" ry="2"/><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/></svg>"##;
    Icon::default().data(svg).w(px(14.)).h(px(14.))
}

pub fn globe_icon() -> Icon {
    let svg = br##"<svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="10"/><line x1="2" y1="12" x2="22" y2="12"/><path d="M12 2a15.3 15.3 0 0 1 4 10 15.3 15.3 0 0 1-4 10 15.3 15.3 0 0 1-4-10 15.3 15.3 0 0 1 4-10z"/></svg>"##;
    Icon::default().data(svg).w(px(14.)).h(px(14.))
}

pub fn chevron_down_icon() -> Icon {
    let svg = br##"<svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="m6 9 6 6 6-6"/></svg>"##;
    Icon::default().data(svg).w(px(14.)).h(px(14.))
}

pub fn chevron_right_icon() -> Icon {
    let svg = br##"<svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="m9 18 6-6-6-6"/></svg>"##;
    Icon::default().data(svg).w(px(14.)).h(px(14.))
}

pub fn alert_circle_icon() -> Icon {
    let svg = br##"<svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="10"/><line x1="12" y1="8" x2="12" y2="12"/><line x1="12" y1="16" x2="12.01" y2="16"/></svg>"##;
    Icon::default().data(svg).w(px(14.)).h(px(14.))
}

pub fn git_commit_icon() -> Icon {
    let svg = br##"<svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="3"/><line x1="3" y1="12" x2="9" y2="12"/><line x1="15" y1="12" x2="21" y2="12"/></svg>"##;
    Icon::default().data(svg).w(px(14.)).h(px(14.))
}

pub fn terminal_icon() -> Icon {
    let svg = br##"<svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="4 17 10 11 4 5"/><line x1="12" y1="19" x2="20" y2="19"/></svg>"##;
    Icon::default().data(svg).w(px(14.)).h(px(14.))
}

pub fn code_icon() -> Icon {
    let svg = br##"<svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="16 18 22 12 16 6"/><polyline points="8 6 2 12 8 18"/></svg>"##;
    Icon::default().data(svg).w(px(14.)).h(px(14.))
}

pub fn hammer_icon() -> Icon {
    let svg = br##"<svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="m15 12-8.5 8.5c-.83.83-2.17.83-3 0 0 0 0 0 0 0a2.12 2.12 0 0 1 0-3L12 9"/><path d="M17.64 15 22 10.64"/><path d="m20.91 3.26-6.36 6.36"/><path d="m4.93 19.07 4.24-4.24"/></svg>"##;
    Icon::default().data(svg).w(px(14.)).h(px(14.))
}

pub fn android_icon() -> Icon {
    let svg = br##"<svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><rect x="4" y="9" width="16" height="12" rx="3"/><path d="M8 5l2 4M16 5l-2 4"/><circle cx="9" cy="13" r="1"/><circle cx="15" cy="13" r="1"/></svg>"##;
    Icon::default().data(svg).w(px(14.)).h(px(14.))
}
