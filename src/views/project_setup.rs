//! Three-step project setup shared by new and edited projects.

use crate::components::provider_icon::provider_icon;
use crate::components::toggle_button;
use crate::workspace::{AuthModeChoice, ProviderChoice, SavedAccountInfo};
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
    pub custom_host: &'a Entity<InputState>,
    pub oauth_client_id: &'a Entity<InputState>,
    pub token: &'a Entity<InputState>,
    pub system_credential: Option<(String, String)>,
    pub saved_accounts: Vec<SavedAccountInfo>,
    pub selected_saved_account: Option<String>,
    pub auth_mode: AuthModeChoice,
    pub auth_status_message: Option<String>,
    pub has_existing_credential: bool,
    pub is_oauth_listening: bool,
}

#[derive(Clone, Copy)]
pub struct ProjectSetupActions<T> {
    pub on_provider: fn(&mut T, ProviderChoice, &mut Window, &mut Context<T>),
    pub on_layout: fn(&mut T, DiskLayout, &mut Context<T>),
    pub on_next: fn(&mut T, &mut Context<T>),
    pub on_back: fn(&mut T, &mut Context<T>),
    pub on_browse: fn(&mut T, &mut Window, &mut Context<T>),
    pub on_connect_system_credential: fn(&mut T, &mut Window, &mut Context<T>),
    pub on_disconnect_credential: fn(&mut T, &mut Window, &mut Context<T>),
    pub on_select_account: fn(&mut T, String, &mut Window, &mut Context<T>),
    pub on_select_auth_tab: fn(&mut T, AuthModeChoice, &mut Context<T>),
    pub on_auth_browser: fn(&mut T, &mut Window, &mut Context<T>),
    pub on_cancel_browser_auth: fn(&mut T, &mut Context<T>),
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
        on_provider,
        on_layout,
        on_next,
        on_back,
        on_browse,
        on_connect_system_credential,
        on_disconnect_credential,
        on_select_account,
        on_select_auth_tab,
        on_auth_browser,
        on_cancel_browser_auth,
        on_auth_webview,
        on_save_token,
        on_open_token_url,
    } = actions;
    let title = match props.step {
        0 => "Name and folder",
        1 => "Git Provider & API Access",
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
            content = content.child(
                div()
                    .text_size(px(12.))
                    .text_color(cx.theme().muted_foreground)
                    .child("Select the Git provider for this project. Authenticating raises your API rate limit (from 60/hr to 5,000/hr) so Release Desk won't be blocked when fetching user repositories. You will add specific users and repositories to clone in the project workspace after creating it."),
            );

            // Provider switcher
            let mut providers = div().flex().flex_wrap().gap_2();
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
                    providers.child(btn.on_click(cx.listener(move |this, _, window, cx| {
                        on_provider(this, provider, window, cx)
                    })));
            }
            content = content.child(providers);

            // If custom provider, custom host URL field
            if props.provider == ProviderChoice::Custom {
                content = content.child(
                    Form::new().child(
                        Field::new()
                            .label("Custom Git server HTTPS URL")
                            .child(Input::new(props.custom_host)),
                    ),
                );
            }

            // Active auth status feedback banner
            if let Some(ref status_msg) = props.auth_status_message {
                content = content.child(
                    div()
                        .p_3()
                        .rounded(px(8.))
                        .bg(cx.theme().primary.opacity(0.12))
                        .border_1()
                        .border_color(cx.theme().primary.opacity(0.35))
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .text_size(px(13.))
                                .font_bold()
                                .text_color(cx.theme().primary)
                                .child(status_msg.clone()),
                        ),
                );
            }

            // Connection state display
            let is_connected =
                props.selected_saved_account.is_some() || props.has_existing_credential;

            if is_connected {
                let current_user = props
                    .selected_saved_account
                    .clone()
                    .unwrap_or_else(|| "Saved Keychain Account".into());
                content =
                    content.child(
                        div()
                            .p_3()
                            .rounded(px(8.))
                            .bg(cx.theme().primary.opacity(0.08))
                            .border_1()
                            .border_color(cx.theme().primary.opacity(0.3))
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap_3()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .child(
                                        div()
                                            .text_size(px(14.))
                                            .text_color(cx.theme().primary)
                                            .child("✓"),
                                    )
                                    .child(div().text_size(px(13.)).font_bold().child(format!(
                                        "Connected as @{current_user} (5,000 req/hr)"
                                    ))),
                            )
                            .child(
                                Button::new("btn-disconnect-cred")
                                    .label("Disconnect / Change")
                                    .ghost()
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        on_disconnect_credential(this, window, cx)
                                    })),
                            ),
                    );
            } else {
                // If a system credential (e.g. devenvoy from macOS Keychain / gh CLI) was detected:
                if let Some((ref username, _)) = props.system_credential {
                    let user_label = username.clone();
                    content = content.child(
                        div()
                            .p_3()
                            .rounded(px(8.))
                            .bg(cx.theme().secondary)
                            .border_1()
                            .border_color(cx.theme().border)
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap_3()
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap_1()
                                    .child(
                                        div()
                                            .text_size(px(13.))
                                            .font_bold()
                                            .child(format!(
                                                "⚡ Detected system account: @{user_label}"
                                            )),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(11.))
                                            .text_color(cx.theme().muted_foreground)
                                            .child(
                                                "Available in your macOS Git Keychain. Click to connect this account, or choose an option below.",
                                            ),
                                    ),
                            )
                            .child(
                                Button::new("btn-connect-system-cred")
                                    .label(format!("Use @{user_label}"))
                                    .primary()
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        on_connect_system_credential(this, window, cx)
                                    })),
                            ),
                    );
                }

                // If saved Keychain accounts exist from other projects:
                if !props.saved_accounts.is_empty() {
                    let mut saved_row = div().flex().items_center().gap_2().flex_wrap();
                    saved_row = saved_row.child(
                        div()
                            .text_size(px(11.))
                            .font_bold()
                            .text_color(cx.theme().muted_foreground)
                            .child("Saved Accounts:"),
                    );
                    for acc in &props.saved_accounts {
                        let h_clone = acc.handle.clone();
                        saved_row = saved_row.child(
                            Button::new(format!("btn-use-saved-{}", acc.handle))
                                .label(format!("@{h_clone}"))
                                .ghost()
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    on_select_account(this, h_clone.clone(), window, cx)
                                })),
                        );
                    }
                    content = content.child(saved_row);
                }

                // Authentication Methods Switcher
                let is_browser = props.auth_mode == AuthModeChoice::OAuthBrowser;
                let is_webview = props.auth_mode == AuthModeChoice::OAuthWebview;
                let is_token = props.auth_mode == AuthModeChoice::Token;

                let tab_bar = div()
                    .flex()
                    .gap_2()
                    .child(
                        toggle_button(
                            "auth-tab-browser",
                            "🌐 Browser OAuth (127.0.0.1:4567)",
                            is_browser,
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            on_select_auth_tab(this, AuthModeChoice::OAuthBrowser, cx)
                        })),
                    )
                    .child(
                        toggle_button("auth-tab-webview", "🖥️ In-App Webview", is_webview)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                on_select_auth_tab(this, AuthModeChoice::OAuthWebview, cx)
                            })),
                    )
                    .child(
                        toggle_button("auth-tab-token", "🔑 Personal Access Token", is_token)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                on_select_auth_tab(this, AuthModeChoice::Token, cx)
                            })),
                    );

                content = content.child(tab_bar);

                if is_browser {
                    let mut browser_box = div()
                        .p_4()
                        .rounded(px(8.))
                        .bg(cx.theme().secondary)
                        .border_1()
                        .border_color(cx.theme().border)
                        .flex()
                        .flex_col()
                        .gap_3();

                    if props.is_oauth_listening {
                        browser_box = browser_box
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .child(
                                        div()
                                            .flex()
                                            .flex_col()
                                            .gap_1()
                                            .child(
                                                div()
                                                    .text_size(px(13.))
                                                    .font_bold()
                                                    .text_color(cx.theme().primary)
                                                    .child("⏳ Waiting for browser callback..."),
                                            )
                                            .child(
                                                div()
                                                    .text_size(px(11.))
                                                    .text_color(cx.theme().muted_foreground)
                                                    .child(
                                                        "Listening on http://127.0.0.1:4567/callback. Approve Release Desk in your browser.",
                                                    ),
                                            ),
                                    )
                                    .child(
                                        Button::new("btn-cancel-oauth")
                                            .label("Cancel")
                                            .ghost()
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                on_cancel_browser_auth(this, cx)
                                            })),
                                    ),
                            );
                    } else {
                        browser_box = browser_box
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(cx.theme().muted_foreground)
                                    .child(
                                        "Starts a local redirect listener on http://127.0.0.1:4567/callback and opens your system browser to sign in.",
                                    ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_3()
                                    .child(
                                        Button::new("btn-start-browser-oauth")
                                            .label("Start Browser Sign-In")
                                            .primary()
                                            .on_click(cx.listener(move |this, _, window, cx| {
                                                on_auth_browser(this, window, cx)
                                            })),
                                    ),
                            )
                            .child(
                                Form::new().child(
                                    Field::new()
                                        .label(
                                            "OAuth Client ID (optional, for custom registered OAuth apps)",
                                        )
                                        .child(Input::new(props.oauth_client_id)),
                                ),
                            );
                    }
                    content = content.child(browser_box);
                } else if is_webview {
                    let webview_box = div()
                        .p_4()
                        .rounded(px(8.))
                        .bg(cx.theme().secondary)
                        .border_1()
                        .border_color(cx.theme().border)
                        .flex()
                        .flex_col()
                        .gap_3()
                        .child(
                            div()
                                .text_size(px(12.))
                                .text_color(cx.theme().muted_foreground)
                                .child(
                                    "Opens an embedded webview modal panel without leaving Release Desk.",
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_3()
                                .child(
                                    Button::new("btn-start-webview-oauth")
                                        .label("Open Webview Sign-In")
                                        .primary()
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            on_auth_webview(this, window, cx)
                                        })),
                                ),
                        );
                    content = content.child(webview_box);
                } else if is_token {
                    let token_box = div()
                        .p_4()
                        .rounded(px(8.))
                        .bg(cx.theme().secondary)
                        .border_1()
                        .border_color(cx.theme().border)
                        .flex()
                        .flex_col()
                        .gap_3()
                        .child(
                            Form::new().child(
                                Field::new()
                                    .label("Personal Access Token or App Password")
                                    .child(Input::new(props.token)),
                            ),
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .justify_between()
                                .child(
                                    Button::new("btn-save-pat")
                                        .label("Save Token to Keychain")
                                        .primary()
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            on_save_token(this, window, cx)
                                        })),
                                )
                                .child(
                                    Button::new("btn-open-token-gen")
                                        .label("↗ Create token on provider site")
                                        .ghost()
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            on_open_token_url(this, window, cx)
                                        })),
                                ),
                        );
                    content = content.child(token_box);
                }
            }

            content = content.child(
                div()
                    .text_size(px(11.))
                    .text_color(cx.theme().muted_foreground)
                    .child(
                        "Note: Authentication is optional. You can click Continue to use public access (60 requests/hr). Upstream users and repositories to clone are added inside the project workspace.",
                    ),
            );
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
