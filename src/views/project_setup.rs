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
use gpui_kit::{
    Context, Entity, InteractiveElement as _, IntoElement, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _, Window, div, px,
};
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
    pub saved_accounts: Vec<SavedAccountInfo>,
    pub selected_saved_account: Option<String>,
    pub auth_mode: AuthModeChoice,
    pub auth_status_message: Option<String>,
    pub has_existing_credential: bool,
}

#[derive(Clone, Copy)]
pub struct ProjectSetupActions<T> {
    pub on_provider: fn(&mut T, ProviderChoice, &mut Window, &mut Context<T>),
    pub on_layout: fn(&mut T, DiskLayout, &mut Context<T>),
    pub on_next: fn(&mut T, &mut Context<T>),
    pub on_back: fn(&mut T, &mut Context<T>),
    pub on_browse: fn(&mut T, &mut Window, &mut Context<T>),
    pub on_select_account: fn(&mut T, String, &mut Window, &mut Context<T>),
    pub on_toggle_new_account: fn(&mut T, &mut Window, &mut Context<T>),
    pub on_auth_browser: fn(&mut T, &mut Window, &mut Context<T>),
    pub on_auth_webview: fn(&mut T, &mut Window, &mut Context<T>),
    pub on_toggle_token_mode: fn(&mut T, &mut Context<T>),
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
        on_select_account,
        on_toggle_new_account,
        on_auth_browser,
        on_auth_webview,
        on_toggle_token_mode,
    } = actions;
    let title = match props.step {
        0 => "Name and folder",
        1 => "Choose a Git provider & account",
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
            } else if props.editing && props.has_existing_credential {
                content = content.child(
                    div()
                        .p_3()
                        .rounded(px(8.))
                        .bg(cx.theme().secondary)
                        .border_1()
                        .border_color(cx.theme().border)
                        .child(
                            div()
                                .text_size(px(12.))
                                .text_color(cx.theme().muted_foreground)
                                .child("This project has authenticated credentials safely stored in macOS Keychain. You do not need to re-enter your token unless you wish to replace it."),
                        ),
                );
            }

            // Section 1: Connected Accounts from Keychain
            if !props.saved_accounts.is_empty() {
                let mut accounts_section = div().flex().flex_col().gap_2();
                accounts_section = accounts_section.child(
                    div()
                        .text_size(px(11.))
                        .font_bold()
                        .text_color(cx.theme().muted_foreground)
                        .child("CONNECTED KEYCHAIN ACCOUNTS (1-CLICK SELECT)"),
                );

                let mut accounts_grid = div().flex().flex_wrap().gap_3();
                for acc in &props.saved_accounts {
                    let is_selected = props.selected_saved_account.as_deref() == Some(&acc.handle)
                        && props.auth_mode == AuthModeChoice::SavedAccount;
                    let handle_clone = acc.handle.clone();
                    let card = div()
                        .id(format!("saved-acc-card-{}", acc.handle))
                        .p_3()
                        .rounded(px(8.))
                        .border_1()
                        .border_color(if is_selected {
                            cx.theme().primary
                        } else {
                            cx.theme().border
                        })
                        .bg(if is_selected {
                            cx.theme().primary.opacity(0.08)
                        } else {
                            cx.theme().secondary
                        })
                        .flex()
                        .flex_col()
                        .gap_1()
                        .cursor_pointer()
                        .on_click(cx.listener(move |this, _, window, cx| {
                            on_select_account(this, handle_clone.clone(), window, cx)
                        }))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .justify_between()
                                .gap_3()
                                .child(
                                    div()
                                        .text_size(px(13.))
                                        .font_bold()
                                        .child(format!("@{}", acc.handle)),
                                )
                                .child(
                                    div()
                                        .text_size(px(10.))
                                        .p_1()
                                        .rounded(px(4.))
                                        .bg(if is_selected {
                                            cx.theme().primary.opacity(0.18)
                                        } else {
                                            cx.theme().muted_foreground.opacity(0.12)
                                        })
                                        .text_color(if is_selected {
                                            cx.theme().primary
                                        } else {
                                            cx.theme().muted_foreground
                                        })
                                        .child(if is_selected {
                                            "Selected"
                                        } else {
                                            "Keychain Ready"
                                        }),
                                ),
                        )
                        .child(
                            div()
                                .text_size(px(11.))
                                .text_color(cx.theme().muted_foreground)
                                .child("Reuses stored macOS Keychain token · No prompt needed"),
                        );
                    accounts_grid = accounts_grid.child(card);
                }

                let toggle_label = if props.auth_mode == AuthModeChoice::SavedAccount {
                    "+ Connect a different account"
                } else {
                    "Use connected account instead"
                };

                accounts_section = accounts_section.child(accounts_grid).child(
                    div().pt_1().child(
                        Button::new("toggle-new-account-mode")
                            .label(toggle_label)
                            .ghost()
                            .on_click(cx.listener(move |this, _, window, cx| {
                                on_toggle_new_account(this, window, cx)
                            })),
                    ),
                );
                content = content.child(accounts_section);
            }

            // Section 2: Account Handle & Auth Methods (for new account or if no saved accounts)
            if props.saved_accounts.is_empty() || props.auth_mode != AuthModeChoice::SavedAccount {
                let mut auth_flow = div().flex().flex_col().gap_3();

                auth_flow = auth_flow.child(
                    Form::new().child(
                        Field::new()
                            .label("Account or organization handle")
                            .child(Input::new(props.account)),
                    ),
                );

                if props.provider == ProviderChoice::Custom {
                    auth_flow = auth_flow.child(
                        Form::new().child(
                            Field::new()
                                .label("Custom Git server HTTPS URL")
                                .child(Input::new(props.custom_host)),
                        ),
                    );
                }

                let mut methods = div().flex().flex_col().gap_3();
                methods = methods.child(
                    div()
                        .text_size(px(11.))
                        .font_bold()
                        .text_color(cx.theme().muted_foreground)
                        .child("AUTHENTICATION OPTIONS"),
                );

                // Option 1: Browser OAuth redirect
                let browser_card = div()
                    .p_3()
                    .rounded(px(8.))
                    .border_1()
                    .border_color(if props.auth_mode == AuthModeChoice::OAuthBrowser {
                        cx.theme().primary
                    } else {
                        cx.theme().border
                    })
                    .bg(cx.theme().secondary)
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
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .child(
                                        div()
                                            .text_size(px(13.))
                                            .font_bold()
                                            .child("Sign in with Browser (OAuth)"),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(10.))
                                            .bg(cx.theme().primary.opacity(0.15))
                                            .text_color(cx.theme().primary)
                                            .p_1()
                                            .rounded(px(4.))
                                            .child("Recommended"),
                                    ),
                            )
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .text_color(cx.theme().muted_foreground)
                                    .child("Opens your default browser to authorize. Token is saved in macOS Keychain."),
                            ),
                    )
                    .child(
                        Button::new("auth-browser-btn")
                            .label("Sign in with Browser")
                            .primary()
                            .on_click(cx.listener(move |this, _, window, cx| {
                                on_auth_browser(this, window, cx)
                            })),
                    );
                methods = methods.child(browser_card);

                // Option 2: Embedded Webview
                let webview_card = div()
                    .p_3()
                    .rounded(px(8.))
                    .border_1()
                    .border_color(if props.auth_mode == AuthModeChoice::OAuthWebview {
                        cx.theme().primary
                    } else {
                        cx.theme().border
                    })
                    .bg(cx.theme().secondary)
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
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .child(
                                        div()
                                            .text_size(px(13.))
                                            .font_bold()
                                            .child("Sign in with In-App Webview"),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(10.))
                                            .bg(cx.theme().muted_foreground.opacity(0.12))
                                            .text_color(cx.theme().muted_foreground)
                                            .p_1()
                                            .rounded(px(4.))
                                            .child("In-App Modal"),
                                    ),
                            )
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .text_color(cx.theme().muted_foreground)
                                    .child("Opens an embedded modal webview panel without switching apps."),
                            ),
                    )
                    .child(
                        Button::new("auth-webview-btn")
                            .label("Launch Webview")
                            .ghost()
                            .on_click(cx.listener(move |this, _, window, cx| {
                                on_auth_webview(this, window, cx)
                            })),
                    );
                methods = methods.child(webview_card);

                // Option 3: Manual Personal Access Token
                let is_token_active = props.auth_mode == AuthModeChoice::Token;
                let mut token_card = div()
                    .p_3()
                    .rounded(px(8.))
                    .border_1()
                    .border_color(if is_token_active {
                        cx.theme().primary
                    } else {
                        cx.theme().border
                    })
                    .bg(cx.theme().secondary)
                    .flex()
                    .flex_col()
                    .gap_2()
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
                                            .child("Personal Access Token (PAT)"),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(11.))
                                            .text_color(cx.theme().muted_foreground)
                                            .child("For fine-grained tokens, CI tokens, or self-hosted Git servers."),
                                    ),
                            )
                            .child(
                                Button::new("toggle-token-btn")
                                    .label(if is_token_active { "Hide token field" } else { "Enter token" })
                                    .ghost()
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        on_toggle_token_mode(this, cx)
                                    })),
                            ),
                    );

                if is_token_active {
                    token_card = token_card
                        .child(
                            Form::new().child(
                                Field::new()
                                    .label("Personal access token")
                                    .child(Input::new(props.token)),
                            ),
                        )
                        .child(
                            div()
                                .text_size(px(11.))
                                .text_color(cx.theme().muted_foreground)
                                .child("Stored securely in macOS Keychain. Release Desk will remember this account so you won't need to re-enter it."),
                        );
                }
                methods = methods.child(token_card);
                auth_flow = auth_flow.child(methods);
                content = content.child(auth_flow);
            }
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
