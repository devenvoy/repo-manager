//! Provider selection and connection controls for project setup.

mod auth;

use super::*;

pub(super) fn render_provider_step<T: 'static>(
    props: &ProjectSetupProps<'_>,
    cx: &Context<T>,
    actions: &ProjectSetupActions<T>,
) -> impl IntoElement {
    let on_provider = actions.on_provider;
    let on_connect_system_credential = actions.on_connect_system_credential;
    let on_disconnect_credential = actions.on_disconnect_credential;
    let on_select_account = actions.on_select_account;
    let mut content = div().flex().flex_col().gap_5();
    content = content.child(
        div()
            .text_size(px(12.))
            .text_color(cx.theme().muted_foreground)
            .child("Choose a provider, then connect an account and add its repositories after creating the project."),
    );

    // Provider switcher
    let mut providers = div().flex().flex_wrap().gap_2();
    for provider in ProviderChoice::ALL {
        let selected = props.provider == provider;
        let supported = props.supported_providers.contains(&provider);
        let mut btn = Button::new(format!("provider-{provider:?}"))
            .label(if supported {
                provider.label().to_string()
            } else {
                format!("{} · unavailable", provider.label())
            })
            .icon(provider_icon(provider));
        if selected {
            btn = btn.primary();
        } else {
            btn = btn.ghost();
        }
        providers = providers.child(btn.disabled(!supported).on_click(
            cx.listener(move |this, _, window, cx| on_provider(this, provider, window, cx)),
        ));
    }
    content = content.child(providers);

    if props.provider.requires_endpoint() {
        content = content.child(
            Form::new().child(
                Field::new()
                    .label(props.provider.endpoint_label())
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
    let uses_aws_credentials = props.provider == ProviderChoice::AwsCodeCommit;
    let is_connected = !uses_aws_credentials
        && (props.selected_saved_account.is_some() || props.has_existing_credential);

    if is_connected {
        let current_user = props
            .selected_saved_account
            .clone()
            .unwrap_or_else(|| "Saved Keychain Account".into());
        content = content.child(
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
                    div().flex().items_center().gap_2().child(
                        div()
                            .text_size(px(13.))
                            .font_bold()
                            .child(format!("Connected: @{current_user}")),
                    ),
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
        // If a system credential (e.g. example from macOS Keychain / gh CLI) was detected:
        if !uses_aws_credentials {
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
                                            "Detected system account: @{user_label}"
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
        }

        // If saved Keychain accounts exist from other projects:
        if !uses_aws_credentials && !props.saved_accounts.is_empty() {
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

        if props.provider == ProviderChoice::AwsCodeCommit {
            content = content.child(
                div()
                    .text_size(px(12.))
                    .text_color(cx.theme().muted_foreground)
                    .child("CodeCommit uses your configured AWS CLI profile and IAM credentials. Enter a region above, then add the source ‘all’ after creating the project."),
            );
        } else {
            content = content.child(auth::render_auth_methods(props, cx, actions));
        }
    }

    content = content.child(
        div()
            .text_size(px(11.))
            .text_color(cx.theme().muted_foreground)
            .child(
                "Note: Authentication is optional. You can create the project and connect credentials later. Upstream users and repositories to clone are added inside the project workspace.",
            ),
    );
    content
}
