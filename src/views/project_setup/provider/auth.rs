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
    let has_browser = props.browser_auth_available;
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
                .child("Release Desk browser sign-in is not configured for this provider. Use a scoped access token or a detected Git account."),
        );
    }

    if is_browser && props.provider != ProviderChoice::GitHub {
        let (description, callback_hint) = match props.provider.browser_auth() {
            BrowserAuth::GitLabPkce => (
                "Continue with Release Desk's GitLab sign-in. Release Desk uses PKCE and returns to this app through a local callback.",
                "Release Desk's registered callback: http://127.0.0.1:7171/oauth/callback",
            ),
            BrowserAuth::AzureEntra => (
                "Continue with your work or school account using Release Desk's Microsoft Entra integration.",
                "Release Desk's registered desktop redirect URI: http://localhost",
            ),
            BrowserAuth::ForgePkce => (
                "Continue with Release Desk's sign-in for this forge. Release Desk uses PKCE and a local callback.",
                "Release Desk's registered integration handles the loopback callback.",
            ),
            _ => (
                "This provider does not have a native browser OAuth flow configured.",
                "",
            ),
        };
        let mut browser_card = div()
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
                    .child(description),
            );
        browser_card = browser_card
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(cx.theme().muted_foreground)
                    .child(callback_hint),
            )
            .child(
                Button::new("btn-start-provider-oauth")
                    .label("Continue in Browser")
                    .primary()
                    .on_click(
                        cx.listener(move |this, _, window, cx| on_auth_browser(this, window, cx)),
                    ),
            );
        content = content.child(browser_card);
        if props
            .auth_status_message
            .as_deref()
            .is_some_and(|message| message.contains("secure callback"))
        {
            content = content.child(
                Button::new("btn-cancel-provider-oauth")
                    .label("Cancel sign-in")
                    .ghost()
                    .on_click(cx.listener(move |this, _, _, cx| on_cancel_browser_auth(this, cx))),
            );
        }
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
