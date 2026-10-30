# OAuth setup report

## Summary

Release Desk owns the OAuth applications used by its customers. Users should choose a provider, sign in on that provider's page, and approve Release Desk. They should not create OAuth applications or enter a client ID. Client IDs are public application identifiers; client secrets must remain private.

The implementation has provider-specific adapters and a shared authorization-code flow with PKCE for GitLab, Gitea/Codeberg, and Microsoft Entra/Azure DevOps. GitHub uses its device authorization flow. Provider login controls are only available when their app-owned client ID is included in the build. Token and existing Git credential options remain available when browser login is unavailable.

## Provider registration checklist

| Provider | Where the Release Desk maintainer registers it | Build configuration | Callback / notes |
| --- | --- | --- | --- |
| GitHub | GitHub **Settings → Developer settings → OAuth apps → New OAuth App** | `RELEASE_DESK_GITHUB_OAUTH_CLIENT_ID` | Enable Device Flow for the current GitHub device authorization flow. [GitHub guide](https://docs.github.com/en/apps/oauth-apps/building-oauth-apps/creating-an-oauth-app) |
| GitLab.com | GitLab **Edit profile → Access → Applications → Add new application** | `RELEASE_DESK_GITLAB_OAUTH_CLIENT_ID` | Register `http://127.0.0.1:7171/oauth/callback` and the required API scopes. [GitLab guide](https://docs.gitlab.com/integration/oauth_provider/) |
| Azure DevOps | Microsoft Entra admin center **App registrations → New registration** | `RELEASE_DESK_AZURE_DEVOPS_OAUTH_CLIENT_ID` | Configure a public mobile/desktop client and `http://localhost` redirect; this flow targets organizational accounts. [Microsoft guide](https://learn.microsoft.com/en-us/entra/identity-platform/quickstart-register-app?tabs=client-secret) |
| Gitea | On the Gitea instance, register an OAuth application in account settings | `RELEASE_DESK_GITEA_OAUTH_CLIENT_ID` | One Release Desk app registration per host; loopback callback uses a dynamic port. A single client ID does not work across arbitrary self-hosted instances. |
| Codeberg | Codeberg **User settings → Applications → Create new OAuth2 application** | `RELEASE_DESK_CODEBERG_OAUTH_CLIENT_ID` | Configure the loopback callback accepted by Release Desk. [Forgejo/Codeberg guide](https://forgejo.org/docs/v17.0/user/authentication/oauth2-provider/) |
| Forgejo/custom instances | The user would need to register Release Desk on that specific instance | Not supported as a bundled client | Keep API token and discovered Git credential methods. Each server issues a different client ID. |
| Bitbucket Cloud | A workspace administrator can add an OAuth consumer under **Workspace settings → Apps and features → OAuth consumers** | None for current desktop login | Bitbucket issues a key and secret. Do not put the secret in a desktop build; support OAuth after a secure server-side token exchange exists. Use an Atlassian API token for now. [Atlassian guide](https://support.atlassian.com/bitbucket-cloud/docs/use-oauth-on-bitbucket-cloud/) |

## Current build status

The five client ID environment variables above are not present in the current shell, and no OAuth client IDs are configured for this checkout. As a result, browser sign-in for those providers is not enabled by this local build until maintainers register the apps and provide the matching IDs at compile time. The app intentionally does not prompt end users for IDs.

## Maintainer rollout steps

1. Create the supported provider app registrations under Release Desk's organization or service accounts and set the exact redirect URI and minimum required scopes.
2. Set the corresponding `RELEASE_DESK_*_OAUTH_CLIENT_ID` variables in the trusted release build environment. These IDs are public; do not add credentials to source control.
3. Build and distribute the app, then confirm each configured provider shows browser sign-in and completes authorization, callback validation, token exchange, account validation, and keychain storage.
4. Keep client secrets out of desktop builds. For providers requiring confidential clients, implement and operate a backend exchange before exposing that flow.

OAuth app registration itself generally does not have a separate fee, though account eligibility, organization policy, tenant permissions, or provider plan limitations may apply. Microsoft registration requires an Azure account and app-registration permissions; verify current provider terms before release.
