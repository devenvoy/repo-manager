//! Three-step project setup shared by new and edited projects.

mod provider;

use crate::components::motion_button::Button;
use crate::components::provider_icon::provider_icon;
use crate::components::toggle_button;
use crate::workspace::{AuthModeChoice, BrowserAuth, ProviderChoice, SavedAccountInfo};
use gpui_kit::base::{Disableable as _, StyledExt};
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::button::ButtonVariants as _;
use gpui_kit::component::form::{Field, Form};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::{Context, Entity, IntoElement, ParentElement as _, Styled as _, Window, div, px};
use release_desk_core::project::DiskLayout;

pub struct ProjectSetupProps<'a> {
    pub editing: bool,
    pub step: usize,
    pub provider: ProviderChoice,
    pub supported_providers: Vec<ProviderChoice>,
    pub disk_layout: DiskLayout,
    pub name: &'a Entity<InputState>,
    pub directory: &'a Entity<InputState>,
    pub custom_host: &'a Entity<InputState>,
    pub oauth_client_id: &'a Entity<InputState>,
    pub token: &'a Entity<InputState>,
    pub system_credential: Option<(String, String)>,
    pub saved_accounts: Vec<SavedAccountInfo>,
    pub selected_saved_account: Option<String>,
    pub auth_mode: AuthModeChoice,
    pub auth_status_message: Option<String>,
    pub has_existing_credential: bool,
    pub device_user_code: Option<String>,
}

#[derive(Clone, Copy)]
pub struct ProjectSetupActions<T> {
    pub on_provider: fn(&mut T, ProviderChoice, &mut Window, &mut Context<T>),
    pub on_layout: fn(&mut T, DiskLayout, &mut Context<T>),
    pub on_next: fn(&mut T, &mut Context<T>),
    pub on_back: fn(&mut T, &mut Context<T>),
    pub on_browse: fn(&mut T, &mut Window, &mut Context<T>),
    pub on_connect_system_credential: fn(&mut T, &mut Window, &mut Context<T>),
    pub on_find_system_credential: fn(&mut T, &mut Context<T>),
    pub on_disconnect_credential: fn(&mut T, &mut Window, &mut Context<T>),
    pub on_select_account: fn(&mut T, String, &mut Window, &mut Context<T>),
    pub on_select_auth_tab: fn(&mut T, AuthModeChoice, &mut Context<T>),
    pub on_auth_browser: fn(&mut T, &mut Window, &mut Context<T>),
    pub on_cancel_browser_auth: fn(&mut T, &mut Context<T>),
    pub on_copy_code: fn(&mut T, String, &mut Context<T>),
    pub on_auth_webview: fn(&mut T, &mut Window, &mut Context<T>),
    pub on_save_token: fn(&mut T, &mut Window, &mut Context<T>),
    pub on_open_token_url: fn(&mut T, &mut Window, &mut Context<T>),
}

pub fn render_project_setup<T: 'static>(
    props: ProjectSetupProps<'_>,
    cx: &Context<T>,
    actions: ProjectSetupActions<T>,
) -> impl IntoElement {
    let ProjectSetupActions {
        on_layout,
        on_next,
        on_back,
        on_browse,
        ..
    } = actions;
    let title = match props.step {
        0 => "Project name & storage layout",
        _ => "Git Provider & API Access",
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
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_3()
                        .child(
                            Button::new("browse-directory")
                                .label("Choose folder…")
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    on_browse(this, window, cx)
                                })),
                        )
                        .child(
                            div()
                                .text_size(px(11.))
                                .text_color(cx.theme().muted_foreground)
                                .child("Each project points to an independent directory on disk."),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .mt_2()
                        .p_4()
                        .rounded_lg()
                        .bg(cx.theme().secondary)
                        .border_1()
                        .border_color(cx.theme().border)
                        .child(
                            div()
                                .font_bold()
                                .text_size(px(13.))
                                .child("On-Disk Repository Organization"),
                        )
                        .child(
                            div()
                                .text_size(px(11.))
                                .text_color(cx.theme().muted_foreground)
                                .child(
                                    "Choose how cloned repository folders will be structured on disk inside this project:",
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .gap_3()
                                .mt_1()
                                .child(
                                    toggle_button(
                                        "layout-grouped",
                                        "Grouped · owner / repository",
                                        props.disk_layout == DiskLayout::Grouped,
                                    )
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        on_layout(this, DiskLayout::Grouped, cx)
                                    })),
                                )
                                .child(
                                    toggle_button(
                                        "layout-flat",
                                        "Flat · repository",
                                        props.disk_layout == DiskLayout::Flat,
                                    )
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        on_layout(this, DiskLayout::Flat, cx)
                                    })),
                                ),
                        ),
                );
        }
        _ => {
            content = content.child(provider::render_provider_step(&props, cx, &actions));
        }
    }

    div()
        .flex()
        .flex_col()
        .size_full()
        .min_w_0()
        .min_h_0()
        .overflow_y_scrollbar()
        .p_6()
        .child(
            div()
                .flex()
                .flex_col()
                .w_full()
                .max_w(px(860.))
                .min_w_0()
                .gap_6()
                .child(
                    div()
                        .text_size(px(11.))
                        .font_bold()
                        .text_color(cx.theme().primary)
                        .child(format!(
                            "{} · STEP {} OF 2",
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
                                .label(if props.step == 1 {
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
                ),
        )
}
