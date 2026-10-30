# Connecting project providers

Create a project with an existing local directory, select a provider, then add a source in the project's Sources page. API tokens are checked with the selected provider before Release Desk stores them in the OS Keychain. Native browser sign-in uses provider-specific OAuth adapters; existing Git credentials can also be discovered without requiring Git Credential Manager.

| Provider | Browser sign-in | Other connection methods |
| --- | --- | --- |
| GitHub | Release Desk device sign-in, in browser or app webview, when configured in the app build | Existing Git credential or access token |
| GitLab.com | Release Desk OAuth authorization code with PKCE, when configured in the app build | GitLab access token or discovered Git credential |
| Gitea and Codeberg | Release Desk OAuth authorization code with PKCE, when configured in the app build | Provider access token or discovered Git credential |
| Forgejo and custom Gitea-compatible servers | No app-wide OAuth client; use an access token or discovered Git credential | Provider access token or discovered Git credential |
| Bitbucket Cloud | No native desktop OAuth; a confidential consumer requires a secure server-side exchange | Atlassian API token or discovered Git credential |
| Azure DevOps | Release Desk Microsoft Entra delegated sign-in with PKCE, when configured in the app build | Azure DevOps PAT or discovered Git credential |
| AWS CodeCommit | AWS CLI profile or IAM Identity Center sign-in outside the app | Configured AWS credentials |

Users never need to register an OAuth application or enter a client ID. Release Desk's public OAuth client IDs are registered by the application maintainers and included in configured builds using `RELEASE_DESK_GITHUB_OAUTH_CLIENT_ID`, `RELEASE_DESK_GITLAB_OAUTH_CLIENT_ID`, `RELEASE_DESK_AZURE_DEVOPS_OAUTH_CLIENT_ID`, `RELEASE_DESK_GITEA_OAUTH_CLIENT_ID`, and `RELEASE_DESK_CODEBERG_OAUTH_CLIENT_ID`. If a provider client is not configured in a build, setup hides its browser sign-in option and leaves token or existing-Git-credential options available. These client IDs are public identifiers, not secrets.

GitLab's registered callback is `http://127.0.0.1:7171/oauth/callback`. Gitea and Codeberg use a loopback callback on `127.0.0.1` with a dynamic port. Release Desk uses PKCE and validates the returned `state` before exchanging the authorization code. Arbitrary Forgejo/custom instances cannot share one app registration because each server issues its own client ID.

Bitbucket OAuth requires a confidential consumer secret. A desktop client cannot keep that secret private, so Release Desk does not expose a user-entered client ID/secret flow. Use an Atlassian API token for now; a future Bitbucket OAuth integration needs a secure server-side token exchange.

Azure DevOps uses Release Desk's Microsoft Entra delegated app for work or school accounts. The application maintainers register `http://localhost` as a Mobile and desktop redirect URI. Microsoft accounts that are not connected to an Entra tenant are not supported by this flow.

For any token-based provider, **Find existing Git account** checks the selected HTTPS host for a previously configured Git credential. The credential is validated against that provider before connection. Self-hosted providers require their server address first.

| Provider | Project setup | Source input | Credential |
| --- | --- | --- | --- |
| GitHub | Select GitHub | User, organization, or repository URL | Device sign-in, API token, or discovered Git credential |
| GitLab | Select GitLab | User, nested group, or repository URL | Native PKCE OAuth, API token, or discovered Git credential |
| Bitbucket Cloud | Select Bitbucket | Workspace slug or `https://bitbucket.org/workspace/repo` | Scoped Atlassian API token or discovered Git credential; native OAuth requires a server-side exchange |
| Gitea | Select Gitea | User, organization, or repository URL on `gitea.com` | Native PKCE OAuth, API token, or discovered Git credential |
| Forgejo | Enter the Forgejo server HTTPS origin | User, organization, or repository URL on that server | Native PKCE OAuth, API token, or discovered Git credential |
| Codeberg | Select Codeberg | User, organization, or repository URL on `codeberg.org` | Native PKCE OAuth, API token, or discovered Git credential |
| Azure DevOps | Enter `https://dev.azure.com/organization` | Project name or `https://dev.azure.com/organization/project/_git/repo` | Microsoft Entra OAuth, Azure DevOps PAT, or discovered Git credential |
| AWS CodeCommit | Enter an AWS region | `all` or an exact repository name | Configured AWS CLI profile with CodeCommit list, get, and Git pull access |
| Custom Gitea-compatible server | Enter the server HTTPS origin | User, organization, or repository URL on that server | Server API token |

Bitbucket app passwords are retired. Use an Atlassian API token with scopes; do not paste an Atlassian account password. CodeCommit needs the `aws` command on `PATH` and uses the CLI's configured IAM credentials for discovery and Git. A custom server must expose a Gitea/Forgejo-compatible `/api/v1` API. It is not a generic integration with every Git server.

Connections may be omitted for public repositories. Private discovery, cloning, and release downloads require credentials with the relevant provider permissions. Azure and CodeCommit release installers need a repository `release-desk-store.json` declaration until a provider-specific pipeline artifact mapping is configured. Bitbucket Downloads are files and do not supply semantic release metadata.

See the [provider release flows](provider-releases.md) for artifact behavior. The provider API references used for this integration are in the [integration plan](provider-integration-plan.md).
