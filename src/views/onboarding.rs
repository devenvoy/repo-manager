//! First-run onboarding view displaying Terms of Service, Privacy Policy, and default paths.

use gpui_kit::base::StyledExt;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::{Context, IntoElement, ParentElement as _, Styled as _, div, px};

pub fn render_onboarding<T: 'static>(
    library_path: &str,
    applications_path: &str,
    cx: &Context<T>,
    on_agree: impl Fn(&mut T, &mut Context<T>) + Copy + 'static,
) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .p_8()
        .w_full()
        .h_full()
        .child(
            div()
                .max_w(px(560.))
                .p_8()
                .rounded_2xl()
                .bg(cx.theme().secondary)
                .border_1()
                .border_color(cx.theme().border)
                .flex()
                .flex_col()
                .gap_6()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .text_center()
                        .child(
                            div()
                                .text_size(px(28.))
                                .font_bold()
                                .child("Welcome to Release Desk"),
                        )
                        .child(
                            div()
                                .text_size(px(13.))
                                .text_color(cx.theme().muted_foreground)
                                .child("A local-first desktop control center for Git fleets and runnable applications."),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_3()
                        .p_4()
                        .rounded_xl()
                        .bg(cx.theme().muted)
                        .child(
                            div()
                                .text_size(px(12.))
                                .font_bold()
                                .child("Privacy & Local-First Principles:"),
                        )
                        .child(
                            div()
                                .text_size(px(11.))
                                .text_color(cx.theme().muted_foreground)
                                .child("• All credentials are encrypted in your macOS Keychain / platform store."),
                        )
                        .child(
                            div()
                                .text_size(px(11.))
                                .text_color(cx.theme().muted_foreground)
                                .child("• Your code, releases, and repository data stay 100% on your local machine."),
                        )
                        .child(
                            div()
                                .text_size(px(11.))
                                .text_color(cx.theme().muted_foreground)
                                .child("• Automated .release-desk/data.json backups ensure complete offline recoverability."),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .text_size(px(11.))
                        .text_color(cx.theme().muted_foreground)
                        .child(format!("• Library Location: {library_path}"))
                        .child(format!("• Install Directory: {applications_path}")),
                )
                .child(
                    Button::new("agree-continue-btn")
                        .label("Agree & Get Started")
                        .primary()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            on_agree(this, cx);
                        })),
                ),
        )
}
