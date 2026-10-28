//! Modular CSS extension methods for GPUI elements.
//!
//! Provides reusable CSS-like styling classes (such as `.css_card()`, `.css_pill()`,
//! `.css_modal()`, and `.css_page_scroll()`) directly on any `Styled` element,
//! matching modern utility-first CSS frameworks like Tailwind CSS.

use gpui_kit::base::StyledExt;
use gpui_kit::component::Theme;
use gpui_kit::component::scroll::{Scrollable, ScrollableElement};
use gpui_kit::{Styled, px, rgb};

/// Extension trait providing modular, reusable CSS styling classes.
pub trait CssExt: Styled + StyledExt + ScrollableElement + Sized {
    /// `.card` - Standard elevated container with rounded borders and background.
    fn css_card(self, theme: &Theme) -> Self {
        self.rounded_xl()
            .border_1()
            .border_color(theme.border)
            .bg(theme.background)
    }

    /// `.card-padded` - Standard card with comfortable p-5 padding.
    fn css_card_padded(self, theme: &Theme) -> Self {
        self.css_card(theme).p_5()
    }

    /// `.card-subtle` - Secondary subtle surface for nested cards.
    fn css_card_subtle(self, theme: &Theme) -> Self {
        self.rounded_xl()
            .border_1()
            .border_color(theme.border)
            .bg(theme.secondary.opacity(0.1))
            .p_4()
    }

    /// `.card-interactive` - Interactive card with hover effect and active highlight.
    fn css_card_interactive(self, theme: &Theme, selected: bool) -> Self {
        self.rounded_xl()
            .border_1()
            .border_color(if selected {
                theme.primary.opacity(0.6)
            } else {
                theme.border
            })
            .bg(if selected {
                theme.primary.opacity(0.05)
            } else {
                theme.background
            })
            .hover(|s| s.border_color(theme.primary.opacity(0.4)))
    }

    /// `.slot-card` - 3-column release slot card.
    fn css_slot_card(self, theme: &Theme) -> Self {
        self.flex_1()
            .min_w(px(200.))
            .p_4()
            .rounded_xl()
            .border_1()
            .border_color(theme.border)
            .bg(theme.background)
            .flex()
            .flex_col()
            .gap_2()
    }

    /// `.modal-backdrop` - Fullscreen dimmed backdrop for modal dialogs.
    fn css_modal_backdrop(self) -> Self {
        self.absolute()
            .inset_0()
            .bg(rgb(0x000000).opacity(0.65))
            .flex()
            .items_center()
            .justify_center()
            .p_6()
    }

    /// `.modal-dialog` - Standard rounded modal dialog card.
    fn css_modal_dialog(self, theme: &Theme) -> Self {
        self.w(px(720.))
            .max_w_full()
            .h(px(640.))
            .max_h_full()
            .rounded_2xl()
            .border_1()
            .border_color(theme.border)
            .bg(theme.background)
            .shadow_lg()
            .flex()
            .flex_col()
            .overflow_hidden()
    }

    /// `.rail` - Sidebar master rail for master-detail views.
    fn css_rail(self, theme: &Theme) -> Self {
        self.w(px(320.))
            .flex_shrink_0()
            .border_r_1()
            .border_color(theme.border)
            .bg(theme.background)
            .flex()
            .flex_col()
    }

    /// `.detail-pane` - Right pane detail view with independent scroll.
    fn css_detail_pane(self) -> Scrollable<Self> {
        self.flex_1()
            .min_w_0()
            .p_6()
            .flex()
            .flex_col()
            .gap_5()
            .h(px(640.))
            .overflow_y_scrollbar()
    }

    /// `.page-scroll` - Standard top-level scrollable page container.
    fn css_page_scroll(self) -> Scrollable<Self> {
        self.flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .size_full()
            .gap_6()
            .p_6()
            .overflow_y_scrollbar()
    }

    /// `.pill-primary` - Primary accent pill.
    fn css_pill_primary(self, theme: &Theme) -> Self {
        self.px_2()
            .py_0p5()
            .rounded_full()
            .bg(theme.primary.opacity(0.12))
            .text_color(theme.primary)
            .text_size(px(10.))
            .font_bold()
    }

    /// `.pill-success` - Green status indicator pill.
    fn css_pill_success(self, theme: &Theme) -> Self {
        self.px_2()
            .py_0p5()
            .rounded_full()
            .bg(theme.success.opacity(0.14))
            .text_color(theme.success)
            .text_size(px(10.))
            .font_bold()
    }

    /// `.pill-warning` - Amber update/caution indicator pill.
    fn css_pill_warning(self, theme: &Theme) -> Self {
        self.px_2()
            .py_0p5()
            .rounded_full()
            .bg(theme.warning.opacity(0.14))
            .text_color(theme.warning)
            .text_size(px(10.))
            .font_bold()
    }

    /// `.pill-danger` - Red error indicator pill.
    fn css_pill_danger(self, theme: &Theme) -> Self {
        self.px_2()
            .py_0p5()
            .rounded_full()
            .bg(theme.danger.opacity(0.12))
            .text_color(theme.danger)
            .text_size(px(10.))
            .font_bold()
    }

    /// `.pill-neutral` - Muted gray badge pill.
    fn css_pill_neutral(self, theme: &Theme) -> Self {
        self.px_2()
            .py_0p5()
            .rounded_full()
            .bg(theme.secondary.opacity(0.15))
            .text_color(theme.muted_foreground)
            .text_size(px(10.))
            .font_bold()
    }

    /// `.chip` - Compact metadata chip (e.g. branch, format).
    fn css_chip(self, theme: &Theme) -> Self {
        self.px_1p5()
            .py_0p5()
            .rounded_md()
            .bg(theme.secondary.opacity(0.12))
            .text_color(theme.muted_foreground)
            .text_size(px(9.))
    }

    /// `.section-eyebrow` - Small uppercase category / tracker header.
    fn css_section_eyebrow(self, theme: &Theme) -> Self {
        self.text_size(px(10.))
            .font_bold()
            .text_color(theme.primary)
    }

    /// `.heading-title` - Prominent page section heading.
    fn css_heading_title(self) -> Self {
        self.text_size(px(29.)).font_bold()
    }

    /// `.body-muted` - Standard descriptive body text.
    fn css_body_muted(self, theme: &Theme) -> Self {
        self.text_size(px(13.)).text_color(theme.muted_foreground)
    }

    /// `.row-item` - Interactive list item with selected state highlight.
    fn css_row_item(self, theme: &Theme, selected: bool) -> Self {
        self.p_3()
            .rounded_xl()
            .border_1()
            .border_color(if selected {
                theme.primary.opacity(0.4)
            } else {
                theme.border
            })
            .bg(if selected {
                theme.primary.opacity(0.04)
            } else {
                theme.background
            })
    }

    /// `.empty-state` - Container for empty states with dashed border.
    fn css_empty_state(self, theme: &Theme) -> Self {
        self.p_8()
            .rounded_xl()
            .border_1()
            .border_color(theme.border)
            .bg(theme.background)
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_3()
    }

    /// `.stat-num` - Large prominent statistic number.
    fn css_stat_num(self) -> Self {
        self.text_size(px(25.)).font_bold()
    }

    /// `.stat-label` - Muted label under a statistic number.
    fn css_stat_label(self, theme: &Theme) -> Self {
        self.text_size(px(12.)).text_color(theme.muted_foreground)
    }
}

impl<S: Styled + StyledExt + ScrollableElement> CssExt for S {}
