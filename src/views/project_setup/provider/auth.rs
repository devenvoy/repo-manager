//! Available authentication methods and method-specific controls.

use super::*;

pub(super) fn render_auth_methods<T: 'static>(
    props: &ProjectSetupProps<'_>,
    cx: &Context<T>,
    actions: &ProjectSetupActions<T>,
) -> impl IntoElement {
    let on_select_auth_tab = actions.on_select_auth_tab;
    let on_auth_browser = actions.on_auth_browser;
    let on_cancel_browser_auth = actions.on_cancel_browser_auth;
    let on_copy_code = actions.on_copy_code;
    let on_auth_webview = actions.on_auth_webview;
    let on_save_token = actions.on_save_token;
    let on_open_token_url = actions.on_open_token_url;
    let on_find_system_credential = actions.on_find_system_credential;
    let mut content = div().flex().flex_col().gap_3();
    // Authentication Methods Switcher
    let has_browser = props.provider.browser_auth() != BrowserAuth::Unavailable;
    let is_browser = has_browser && props.auth_mode == AuthModeChoice::OAuthBrowser;
    let is_webview =
        props.provider == ProviderChoice::GitHub && props.auth_mode == AuthModeChoice::OAuthWebview;
    let is_token = props.auth_mode == AuthModeChoice::Token;

    let mut tab_bar = div().flex().gap_2();
    if has_browser {
        tab_bar = tab_bar.child(
            toggle_button("auth-tab-browser", "Browser Login", is_browser).on_click(cx.listener(
                move |this, _, _, cx| on_select_auth_tab(this, AuthModeChoice::OAuthBrowser, cx),
            )),
        );
    }
    if props.provider == ProviderChoice::GitHub {
        tab_bar = tab_bar.child(
            toggle_button("auth-tab-webview", "In-App Webview", is_webview).on_click(cx.listener(
                move |this, _, _, cx| on_select_auth_tab(this, AuthModeChoice::OAuthWebview, cx),
            )),
        );
    }
    let token_label = if props.provider == ProviderChoice::Bitbucket {
        "Atlassian API Token"
    } else {
        "Access Token"
    };
    tab_bar = tab_bar.child(
        toggle_button("auth-tab-token", token_label, is_token).on_click(
            cx.listener(move |this, _, _, cx| on_select_auth_tab(this, AuthModeChoice::Token, cx)),
        ),
    );

    content = content.child(tab_bar);
    content = content.child(
        Button::new("btn-find-system-git-credential")
            .label("Find existing Git account")
            .ghost()
            .on_click(cx.listener(move |this, _, _, cx| on_find_system_credential(this, cx))),
    );

    if !has_browser {
        content = content.child(
            div()
                .text_size(px(11.))
                .text_color(cx.theme().muted_foreground)
                .child("Browser sign-in is not configured for this server. Use an access token or a detected Git account."),
        );
    }

    if is_browser && props.provider != ProviderChoice::GitHub {
        content = content.child(
            div()
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
                        .child(format!(
                            "Sign in to {} in your browser through Git Credential Manager. It stores and refreshes the credential; Release Desk requests it when needed. Git Credential Manager must be installed.",
                            props.provider.label()
                        )),
                )
                .child(
                    Button::new("btn-start-managed-browser-login")
                        .label("Sign in with Git Credential Manager")
                        .primary()
                        .on_click(cx.listener(move |this, _, window, cx| {
                            on_auth_browser(this, window, cx)
                        })),
                ),
        );
    } else if is_browser {
        let mut browser_box = div()
            .p_4()
            .rounded(px(8.))
            .bg(cx.theme().secondary)
            .border_1()
            .border_color(cx.theme().border)
            .flex()
            .flex_col()
            .gap_3();

        if let Some(ref code) = props.device_user_code {
            let c_clone = code.clone();
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
                                    .child("GitHub OAuth (Device Flow)"),
                            )
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .text_color(cx.theme().muted_foreground)
                                    .child("Enter this code in your browser to authorize Release Desk:"),
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
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(
                        div()
                            .px_4()
                            .py_2()
                            .rounded(px(6.))
                            .bg(cx.theme().background)
                            .border_1()
                            .border_color(cx.theme().border)
                            .text_size(px(18.))
                            .font_bold()
                            .text_color(cx.theme().primary)
                            .child(code.clone()),
                    )
                    .child(
                        Button::new("btn-copy-device-code")
                            .label("Copy Code")
                            .outline()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                on_copy_code(this, c_clone.clone(), cx)
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
                        "Uses GitHub's device authorization flow. Complete sign-in on github.com in your browser.",
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
                            "OAuth Client ID (optional, defaults to official GitHub CLI Client ID)",
                        )
                        .child(Input::new(props.oauth_client_id)),
                ),
            );
        }
        content = content.child(browser_box);
    } else if is_webview {
        let mut webview_box = div()
            .p_4()
            .rounded(px(8.))
            .bg(cx.theme().secondary)
            .border_1()
            .border_color(cx.theme().border)
            .flex()
            .flex_col()
            .gap_3();

        if let Some(ref code) = props.device_user_code {
            let c_clone = code.clone();
            webview_box = webview_box
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
                                        .child("In-App Webview Active"),
                                )
                                .child(
                                    div()
                                        .text_size(px(11.))
                                        .text_color(cx.theme().muted_foreground)
                                        .child(
                                            "Enter this code in the opened webview modal window:",
                                        ),
                                ),
                        )
                        .child(
                            Button::new("btn-cancel-webview-oauth")
                                .label("Cancel")
                                .ghost()
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    on_cancel_browser_auth(this, cx)
                                })),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_3()
                        .child(
                            div()
                                .px_4()
                                .py_2()
                                .rounded(px(6.))
                                .bg(cx.theme().background)
                                .border_1()
                                .border_color(cx.theme().border)
                                .text_size(px(18.))
                                .font_bold()
                                .text_color(cx.theme().primary)
                                .child(code.clone()),
                        )
                        .child(
                            Button::new("btn-copy-device-code-wv")
                                .label("Copy Code")
                                .outline()
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    on_copy_code(this, c_clone.clone(), cx)
                                })),
                        ),
                );
        } else {
            webview_box = webview_box
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(cx.theme().muted_foreground)
                    .child(
                        "Opens an embedded native WebKit modal window without leaving Release Desk.",
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
        }
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
                        .label("Provider API token")
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
                            .label("Create token on provider website")
                            .ghost()
                            .on_click(cx.listener(move |this, _, window, cx| {
                                on_open_token_url(this, window, cx)
                            })),
                    ),
            );
        content = content.child(token_box);
    }
    content
}
