//! Three-step project setup shared by new and edited projects.

use crate::components::provider_icon::provider_icon;
use crate::components::toggle_button;
use crate::workspace::ProviderChoice;
use gpui_kit::base::StyledExt;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::form::{Field, Form};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::{Context, Entity, IntoElement, ParentElement as _, Styled as _, Window, div, px};
use release_desk_core::project::DiskLayout;

pub struct ProjectSetupProps<'a> {
    pub editing: bool,
    pub step: usize,
    pub provider: ProviderChoice,
    pub disk_layout: DiskLayout,
    pub name: &'a Entity<InputState>,
    pub directory: &'a Entity<InputState>,
    pub account: &'a Entity<InputState>,
    pub custom_host: &'a Entity<InputState>,
    pub token: &'a Entity<InputState>,
}

pub fn render_project_setup<
    T: 'static,
    FProvider: Fn(ProviderChoice, &mut T, &mut Context<T>) + Copy + 'static,
    FLayout: Fn(DiskLayout, &mut T, &mut Context<T>) + Copy + 'static,
    FNext: Fn(&mut T, &mut Context<T>) + Copy + 'static,
    FBack: Fn(&mut T, &mut Context<T>) + Copy + 'static,
    FBrowse: Fn(&mut T, &mut Window, &mut Context<T>) + Copy + 'static,
>(
    props: ProjectSetupProps<'_>,
    cx: &Context<T>,
    on_provider: FProvider,
    on_layout: FLayout,
    on_next: FNext,
    on_back: FBack,
    on_browse: FBrowse,
) -> impl IntoElement {
    let title = match props.step {
        0 => "Name and folder",
        1 => "Choose a Git provider",
        _ => "Workspace preferences",
    };
    let mut content = div().flex().flex_col().gap_5();

    match props.step {
        0 => {
            content = content
                .child(
                    Form::new()
                        .child(Field::new().label("Project name").child(Input::new(props.name)))
                        .child(Field::new().label("Directory on disk").child(Input::new(props.directory))),
                )
                .child(Button::new("browse-directory").label("Choose folder…").on_click(cx.listener(move |this, _, window, cx| on_browse(this, window, cx))))
                .child(div().text_size(px(11.)).text_color(cx.theme().muted_foreground).child("Choose an existing directory. Each project can use a different disk or folder."));
        }
        1 => {
            let mut providers = div().flex().flex_wrap().gap_3();
            for provider in ProviderChoice::ALL {
                let selected = props.provider == provider;
                let mut btn = Button::new(format!("provider-{provider:?}"))
                    .label(provider.label().to_string())
                    .icon(provider_icon(provider));
                if selected {
                    btn = btn.primary();
                } else {
                    btn = btn.ghost();
                }
                providers =
                    providers.child(btn.on_click(
                        cx.listener(move |this, _, _, cx| on_provider(provider, this, cx)),
                    ));
            }
            content = content.child(providers).child(
                Form::new().child(
                    Field::new()
                        .label("Account or organization")
                        .child(Input::new(props.account)),
                ),
            );
            if props.provider == ProviderChoice::Custom {
                content = content.child(
                    Form::new().child(
                        Field::new()
                            .label("Custom Git server HTTPS URL")
                            .child(Input::new(props.custom_host)),
                    ),
                );
            }
            content = content.child(
                Form::new().child(
                    Field::new()
                        .label("Personal access token (optional)")
                        .child(Input::new(props.token)),
                ),
            );
            content = content.child(div().text_size(px(11.)).text_color(cx.theme().muted_foreground).child("Tokens are stored in the OS secure store for this project. Leave blank to connect later."));
        }
        _ => {
            content = content
                .child(div().text_size(px(12.)).child("Choose where cloned repositories will be placed. This does not control how the app lists them."))
                .child(
                    div()
                        .flex()
                        .gap_3()
                        .child(
                            toggle_button(
                                "layout-grouped",
                                "Grouped · owner / repository",
                                props.disk_layout == DiskLayout::Grouped,
                            )
                            .on_click(cx.listener(move |this, _, _, cx| {
                                on_layout(DiskLayout::Grouped, this, cx)
                            })),
                        )
                        .child(
                            toggle_button(
                                "layout-flat",
                                "Flat · repository",
                                props.disk_layout == DiskLayout::Flat,
                            )
                            .on_click(cx.listener(move |this, _, _, cx| {
                                on_layout(DiskLayout::Flat, this, cx)
                            })),
                        ),
                );
        }
    }

    div()
        .flex()
        .flex_col()
        .gap_6()
        .p_6()
        .overflow_y_scrollbar()
        .child(
            div()
                .text_size(px(11.))
                .font_bold()
                .text_color(cx.theme().primary)
                .child(format!(
                    "{} · STEP {} OF 3",
                    if props.editing {
                        "EDIT PROJECT"
                    } else {
                        "NEW PROJECT"
                    },
                    props.step + 1
                )),
        )
        .child(div().text_size(px(27.)).font_bold().child(title))
        .child(content)
        .child(
            div()
                .flex()
                .gap_3()
                .child(
                    Button::new("setup-back")
                        .label(if props.step == 0 { "Cancel" } else { "Back" })
                        .on_click(cx.listener(move |this, _, _, cx| on_back(this, cx))),
                )
                .child(
                    Button::new("setup-next")
                        .label(if props.step == 2 {
                            if props.editing {
                                "Save project"
                            } else {
                                "Create project"
                            }
                        } else {
                            "Continue"
                        })
                        .primary()
                        .on_click(cx.listener(move |this, _, _, cx| on_next(this, cx))),
                ),
        )
}
