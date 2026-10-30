# Connecting project providers

Create a project with an existing local directory, select a provider, then add a source in the project's Sources page. API tokens are checked with the selected provider before Release Desk stores them in the OS Keychain. Available login methods depend on the provider protocol and installed helpers.

| Provider | Browser sign-in | Other connection methods |
| --- | --- | --- |
| GitHub | Built-in device flow, in browser or app webview | Existing Git credential or access token |
| GitLab, Bitbucket Cloud, Azure DevOps | Git Credential Manager, when installed | Existing Git credential or access token |
| Gitea, Forgejo, Codeberg, custom forge | Not yet integrated into the app | Existing Git credential or access token |
| AWS CodeCommit | AWS CLI profile or IAM Identity Center sign-in outside the app | Configured AWS credentials |

Git Credential Manager owns browser interaction and token renewal. Release Desk requests a current credential when it calls the provider API and configures Git to use the same manager for clone and fetch. It does not copy a short-lived OAuth token into its own Keychain. Install and configure [Git Credential Manager](https://github.com/git-ecosystem/git-credential-manager) before choosing that option. Forgejo and Gitea support OAuth with public-client PKCE, but a complete callback, refresh, and server registration flow is still required before the app can offer it. Bitbucket OAuth requires an app client secret, so a desktop-only built-in OAuth flow would not be appropriate. Azure DevOps recommends Microsoft Entra; Git Credential Manager handles that interaction.

For any token-based provider, **Find existing Git account** checks the selected HTTPS host for a previously configured Git credential. The credential is validated against that provider before connection. Self-hosted providers require their server address first.

| Provider | Project setup | Source input | Credential |
| --- | --- | --- | --- |
| GitHub | Select GitHub | User, organization, or repository URL | Device sign-in, API token, or discovered Git credential |
| GitLab | Select GitLab | User, nested group, or repository URL | API token or discovered Git credential |
| Bitbucket Cloud | Select Bitbucket | Workspace slug or `https://bitbucket.org/workspace/repo` | Scoped Atlassian API token with repository and workspace read access |
| Gitea | Select Gitea | User, organization, or repository URL on `gitea.com` | Gitea API token with repository and user read access |
| Forgejo | Enter the Forgejo server HTTPS origin | User, organization, or repository URL on that server | Forgejo API token with repository and user read access |
| Codeberg | Select Codeberg | User, organization, or repository URL on `codeberg.org` | Codeberg API token |
| Azure DevOps | Enter `https://dev.azure.com/organization` | Project name or `https://dev.azure.com/organization/project/_git/repo` | Azure DevOps PAT with code read access; existing Git credentials can also be used for Git |
| AWS CodeCommit | Enter an AWS region | `all` or an exact repository name | Configured AWS CLI profile with CodeCommit list, get, and Git pull access |
| Custom Gitea-compatible server | Enter the server HTTPS origin | User, organization, or repository URL on that server | Server API token |

Bitbucket app passwords are retired. Use an Atlassian API token with scopes; do not paste an Atlassian account password. CodeCommit needs the `aws` command on `PATH` and uses the CLI's configured IAM credentials for discovery and Git. A custom server must expose a Gitea/Forgejo-compatible `/api/v1` API. It is not a generic integration with every Git server.

Connections may be omitted for public repositories. Private discovery, cloning, and release downloads require credentials with the relevant provider permissions. Azure and CodeCommit release installers need a repository `release-desk-store.json` declaration until a provider-specific pipeline artifact mapping is configured. Bitbucket Downloads are files and do not supply semantic release metadata.

See the [provider release flows](provider-releases.md) for artifact behavior. The provider API references used for this integration are in the [integration plan](provider-integration-plan.md).
