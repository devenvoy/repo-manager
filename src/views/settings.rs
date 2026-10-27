//! Workspace settings and preferences view.

use crate::settings::{Appearance, Settings};
use gpui_kit::base::StyledExt;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::form::{Field, Form};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::{Context, Entity, IntoElement, ParentElement as _, Styled as _, Window, div, px};

pub fn render_settings<T: 'static>(
    settings: &Settings,
    extension_input: &Entity<InputState>,
    cx: &Context<T>,
    on_toggle_prereleases: impl Fn(&mut T, bool, &mut Context<T>) + Copy + 'static,
    on_toggle_archived: impl Fn(&mut T, bool, &mut Context<T>) + Copy + 'static,
    on_set_appearance: impl Fn(&mut T, Appearance, &Window, &mut Context<T>) + Copy + 'static,
    on_save: impl Fn(&mut T, &mut Context<T>) + Copy + 'static,
) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap_4()
        .p_6()
        .overflow_y_scrollbar()
        .size_full()
        .child(div().font_bold().text_size(px(24.)).child("Settings"))
        .child("Personalize this workspace. ArtCraft snapshot stays strictly read-only.")
        .child(
            Form::new()
                .child(
                    Field::new()
                        .label("Installer file extension")
                        .child(Input::new(extension_input)),
                )
                .child(
                    Field::new().label_indent(false).child(
                        Checkbox::new("prereleases")
                            .label("Include pre-releases")
                            .checked(settings.include_prereleases)
                            .on_change(cx.listener(move |this, value, _, cx| {
                                on_toggle_prereleases(this, *value, cx);
                            })),
                    ),
                )
                .child(
                    Field::new().label_indent(false).child(
                        Checkbox::new("archived")
                            .label("Show archived repositories")
                            .checked(settings.show_archived)
                            .on_change(cx.listener(move |this, value, _, cx| {
                                on_toggle_archived(this, *value, cx);
                            })),
                    ),
                ),
        )
        .child("Appearance")
        .child(
            div()
                .flex()
                .gap_2()
                .child(
                    Button::new("appearance-system")
                        .label(if settings.appearance == Appearance::System {
                            "● System"
                        } else {
                            "System"
                        })
                        .on_click(cx.listener(move |this, _, window, cx| {
                            on_set_appearance(this, Appearance::System, window, cx);
                        })),
                )
                .child(
                    Button::new("appearance-light")
                        .label(if settings.appearance == Appearance::Light {
                            "● Light"
                        } else {
                            "Light"
                        })
                        .on_click(cx.listener(move |this, _, window, cx| {
                            on_set_appearance(this, Appearance::Light, window, cx);
                        })),
                )
                .child(
                    Button::new("appearance-dark")
                        .label(if settings.appearance == Appearance::Dark {
                            "● Dark"
                        } else {
                            "Dark"
                        })
                        .on_click(cx.listener(move |this, _, window, cx| {
                            on_set_appearance(this, Appearance::Dark, window, cx);
                        })),
                ),
        )
        .child(
            Button::new("save-settings")
                .primary()
                .label("Save settings")
                .on_click(cx.listener(move |this, _, _, cx| on_save(this, cx))),
        )
}
