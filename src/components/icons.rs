//! Application icons backed by GPUI Kit's bundled Lucide SVG assets.

use gpui_kit::assets::IconName;
use gpui_kit::component::Icon;
use gpui_kit::{Styled as _, px, white};

pub type UiIcon = Icon;

macro_rules! ui_icon {
    ($fn_name:ident, $name:ident, $size:expr) => {
        pub fn $fn_name() -> UiIcon {
            Icon::new(IconName::$name).w(px($size)).h(px($size))
        }
    };
}

ui_icon!(grid_icon, LayoutGrid, 16.);
ui_icon!(store_icon, Store, 16.);
ui_icon!(settings_icon, Settings, 16.);
ui_icon!(folder_icon, Folder, 16.);
ui_icon!(plus_icon, Plus, 14.);
ui_icon!(check_icon, Check, 14.);
ui_icon!(shield_icon, ShieldCheck, 15.);
ui_icon!(arrow_right_icon, ArrowRight, 13.);
ui_icon!(arrow_left_icon, ArrowLeft, 13.);
ui_icon!(download_icon, Download, 14.);
ui_icon!(layers_icon, Layers, 15.);
ui_icon!(repository_icon, BookText, 17.);
ui_icon!(branch_icon, GitBranch, 15.);
ui_icon!(star_icon, Star, 14.);
ui_icon!(fork_icon, GitFork, 14.);
ui_icon!(refresh_icon, RefreshCw, 15.);
ui_icon!(trash_icon, Trash, 15.);
ui_icon!(lock_icon, Lock, 14.);
ui_icon!(external_link_icon, ExternalLink, 14.);
ui_icon!(copy_icon, Copy, 14.);
ui_icon!(globe_icon, Globe, 14.);
ui_icon!(chevron_down_icon, ChevronDown, 14.);
ui_icon!(chevron_right_icon, ChevronRight, 14.);
ui_icon!(alert_circle_icon, CircleAlert, 14.);
ui_icon!(git_commit_icon, GitCommitHorizontal, 14.);
ui_icon!(terminal_icon, Terminal, 14.);
ui_icon!(code_icon, Code, 14.);
ui_icon!(hammer_icon, Hammer, 14.);
ui_icon!(android_icon, Bot, 14.);

pub fn brand_logo() -> UiIcon {
    layers_icon().w(px(18.)).h(px(18.)).text_color(white())
}
