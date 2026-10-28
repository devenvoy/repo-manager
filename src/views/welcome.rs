//! First-run setup and library initialization view.

use crate::components::motion_button::Button;
use gpui_kit::component::button::ButtonVariants as _;
use gpui_kit::component::form::{Field, Form};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::{Context, Entity, IntoElement, ParentElement as _, Styled as _, div};

pub fn render_welcome<T: 'static>(
    library_input: &Entity<InputState>,
    path_input: &Entity<InputState>,
    cx: &Context<T>,
    on_create: impl Fn(&mut T, &mut Context<T>) + Copy + 'static,
) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap_4()
        .p_6()
        .child("Your releases, one organized place.")
        .child(
            "Choose where Release Desk should create its managed library. \
             ArtCraft stays where it is and its data.json is linked read-only.",
        )
        .child(
            Form::new()
                .child(
                    Field::new()
                        .label("Managed library parent folder")
                        .child(Input::new(library_input)),
                )
                .child(
                    Field::new()
                        .label("ArtCraft snapshot")
                        .child(Input::new(path_input)),
                ),
        )
        .child(
            Button::new("create-library")
                .primary()
                .label("Create library and open ArtCraft")
                .on_click(cx.listener(move |this, _, _, cx| on_create(this, cx))),
        )
}
