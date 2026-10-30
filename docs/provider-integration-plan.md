# Complete provider integration plan

## Goal and definition of done

Enable project creation and the complete account-to-repository workflow for GitHub, GitLab, Bitbucket Cloud, Gitea, Forgejo, Codeberg, Azure DevOps, AWS CodeCommit, and a configured custom Git server. A provider is selectable only when account connection, source discovery, private clone/fetch, and supported release behavior work as advertised. Deliver all providers in one coordinated change set, with individual adapters and capability-gated UI rather than a blanket UI switch.

The project directory remains local. “Project creation” here means connecting a host and selecting its existing repositories; it does not create a remote repository.

## Current implementation and remaining architecture work

- Project setup now enables providers with a registered source adapter. `provider_sources` contains provider-specific discovery for GitHub, GitLab, Bitbucket, Gitea/Forgejo/Codeberg/custom Gitea-compatible servers, Azure DevOps, and AWS CodeCommit.
- Authentication has provider-specific adapters for configured Release Desk clients: GitHub device authorization, GitLab/Gitea/Codeberg PKCE, Microsoft Entra PKCE, validated provider tokens, and AWS CLI credentials. Client IDs are app-owned build configuration, never user input. Bitbucket confidential OAuth is withheld until a secure server-side exchange exists.
- The desktop still dispatches directly through `provider_sources`; it does not yet use the application `ProviderRegistry`. The CLI registry and desktop discovery path therefore remain separate.
- `GitHostGateway` combines repository listing, pull requests, and releases, while the desktop also has source and release adapters. Keep existing paths stable during migration, then consolidate only after both clients use shared application use cases.
- Continue auditing host-scoped credential handling and URL parsing for every adapter. Never infer a provider from substring matches or send credentials across origins.
- Release fetching covers GitHub, GitLab, Bitbucket Downloads, and Gitea/Forgejo-compatible APIs. Azure and CodeCommit use declared artifacts because neither has a universal repository release-asset API.

## Architecture

Keep domain objects and use cases free of `reqwest`, GPUI, Keychain, AWS SDK, and provider JSON. The application layer owns the workflow and a registry of configured provider instances. The store crate owns HTTP/API adapters and credential validation. The Git crate receives a host-scoped credential policy. The desktop and CLI call the same application use cases.

```text
Desktop / CLI
  -> ConnectProvider, DiscoverSource, RefreshSource, CloneSelected, FetchReleases
       -> ProviderRegistry (keyed by provider kind + canonical endpoint/tenant/region)
            -> DiscoveryPort        -> GitHub / GitLab / Bitbucket / Forge / Azure / CodeCommit adapters
            -> AuthPort             -> provider-specific token/OAuth/AWS strategies
            -> ReleasePort          -> existing release adapters or declared artifacts
       -> SecretStorePort           -> OS Keychain reference
       -> GitPort + CredentialPolicy -> verified HTTPS host and repository path
```

Use **small ports**, not one large `ProviderAdapter` trait whose methods fail for unsupported features. A provider registration bundles the ports and capabilities it actually implements. Define the contracts near `release-desk-core/src/port.rs` (or application-owned ports if they are only used by application use cases):

```rust
pub trait SourceDiscoveryPort: Send + Sync {
    fn resolve_target(&self, input: &str, instance: &ProviderInstance)
        -> Result<SourceTarget, ProviderError>;
    fn discover(&self, target: &SourceTarget, credential: Option<&Credential>)
        -> Result<DiscoveredSource, ProviderError>;
}

pub trait ConnectionPort: Send + Sync {
    fn supported_methods(&self) -> &'static [AuthMethodKind];
    fn validate(&self, credential: &Credential, instance: &ProviderInstance)
        -> Result<ConnectedIdentity, ProviderError>;
}

pub trait ReleaseCatalogPort: Send + Sync {
    fn releases(&self, repository: &ProviderRepository, credential: Option<&Credential>)
        -> Result<Vec<WorkspaceRelease>, ProviderError>;
}
```

These are contract sketches, not signatures to copy verbatim. Preserve the existing release model only after moving it out of infrastructure or defining an application model. `ProviderInstance` must hold canonical HTTPS origin plus host-specific coordinates: Azure organization/project, AWS region/account, or a custom forge origin. `SourceTarget` must distinguish user, organization/workspace, and exact repository; GitLab nested groups and Azure project paths cannot be reduced to a single `owner` string. `ProviderRepository` needs stable provider ID, full namespace, canonical clone URL, default branch, and private status. Do not derive IDs from page indexes or coerce provider IDs into `GitHubRepositoryId`.

Use a typed `ProviderError` with validation, missing/expired credentials, forbidden scope, not found, rate limit, transport, and malformed response variants. Present actionable messages in the UI; retry only transient transport/rate-limit failures. The registry should expose capabilities from registered implementations, not a manually maintained `matches!` expression. Model support at feature granularity (connection, source discovery, private Git, release assets, manifest fallback), so the UI can truthfully describe partial support.

## Provider adapters and authentication

| Provider | Discovery target and adapter | Connection and Git credential | Release path |
| --- | --- | --- | --- |
| GitHub | User/org/repository, existing API mapper | Existing device flow or validated token; HTTPS helper scoped to `github.com` | Existing Releases API |
| GitLab | User, nested group, or repository | Release Desk public-client PKCE when configured in the app build, or validated token; HTTPS helper scoped to configured GitLab origin | Existing Releases API |
| Bitbucket Cloud | Workspace/repository; follow opaque `next` links, use slug and UUID | Scoped Atlassian API token; confidential OAuth requires a server-side exchange; Git helper uses API-token username convention | Existing Downloads adapter; label as downloads, not semantic releases |
| Gitea | User/org/repository on `gitea.com` | Release Desk public-client PKCE when configured in the app build, or scoped API token; origin-scoped Git helper | Existing `/api/v1` releases |
| Forgejo | User/org/repository on a **configured Forgejo origin** | Scoped API token; per-instance OAuth app is not bundled | Forgejo-compatible release API at that origin |
| Codeberg | User/org/repository at `codeberg.org`, using Forgejo adapter configuration | Release Desk public-client PKCE when configured in the app build, or Codeberg API token; Git helper scoped to `codeberg.org` | Forgejo-compatible releases at Codeberg |
| Azure DevOps | Organization/project/repository via Core Projects and Git Repositories APIs | Release Desk Microsoft Entra delegated OAuth with PKCE when configured, or validated PAT; Git remote policy uses `dev.azure.com`/verified legacy origin | Declared artifact manifest until pipeline artifact mapping exists |
| AWS CodeCommit | AWS account/region/repository via `ListRepositories` and repository metadata | IAM credential chain and AWS SDK/CLI signing; Git credential helper or `git-remote-codecommit`, not generic PAT | Declared signed artifact URL until a pipeline mapping exists |
| Custom Git server | User/org/repository only for a detected, supported API flavor (initially Gitea/Forgejo); otherwise exact Git URL import | Explicit HTTPS origin and matching auth strategy; never infer GitHub credentials | Flavor adapter or declared artifact manifest |

Bitbucket API tokens replace app passwords. Explain required read scopes and never assume a saved token is valid. Forgejo must not silently map to Codeberg. The `token_creation_url` fallback to GitHub for unrelated providers has been removed. Browser OAuth is shown only when an exchange implementation and a Release Desk-owned client configuration exist. End users authorize Release Desk; they do not register OAuth apps or provide client IDs. Bitbucket OAuth requires a confidential consumer secret, so it needs a secure server-side exchange before it can be offered.

## Coordinated implementation sequence

1. **Characterize and protect the current flow.** Add focused fixtures for GitHub/GitLab profile parsing, source refresh, private clone credential scope, releases, persisted project settings, and failure messages. Preserve existing serialized `ProviderChoice` values and Keychain references.
2. **Add the shared model and registry.** Create provider instance, target, repository, credential reference, capabilities, and typed errors. Refactor the application registry to register per-instance discovery, connection, and release ports. Wire both desktop and CLI to the same use cases. Keep old APIs behind temporary shims until callers migrate.
3. **Move source parsing and discovery into adapters.** Replace `parse_input_target` and the large provider match in `source_fetcher.rs` with provider-owned parsing/mapping. Implement all listed adapters. Support complete pagination and precise user/org/repository semantics. Reject malformed URLs, embedded credentials, insecure origins, and cross-host token reuse.
4. **Unify connection handling.** Make setup ask each adapter for its supported methods and provider-specific fields. Validate credentials against the provider before saving the connection; store secrets in Keychain and persist only references and non-secret coordinates. Implement OAuth code exchange where offered, including state/PKCE as required, or hide the option. AWS uses the credential chain rather than a token field.
5. **Generalize private Git safely.** Replace the two-host `GitCredentialContext` switch and substring host guess with a policy derived from the canonical clone URL and provider instance. Scope helper responses by HTTPS origin and repository path; never put secrets in remote URLs or logs. Verify clone, fetch, pull, and cross-host rejection for every adapter.
6. **Unify releases and downloads.** Route release discovery through the same registry. Reuse existing implementations, fix host propagation for self-hosted Forgejo/Gitea, preserve Bitbucket Downloads semantics, and retain manifest fallback for Azure, CodeCommit, and unknown custom servers. Reuse existing redirect/origin protections for private downloads.
7. **Enable the UI from capabilities.** Remove the hard-coded enabled list, show an explicit reason for any missing feature, and use provider-specific labels, token links, account identifiers, and setup fields. A saved connection must be revalidated when used, with an actionable reconnect path.
8. **Remove duplicate flows and finish documentation.** Delete obsolete desktop/CLI provider switches only after all callers use the registry. Document account requirements, supported release behavior, token scopes, and custom-server limitations.

## Test and release gate

- Contract tests run against each adapter with fixture HTTP responses: public/private listing, 0/1/multiple pages, malformed payload, 401/403/404/429, missing scopes, nested namespaces, and exact repository URLs.
- Integration tests cover the same user journey for each provider: connect, create project, discover, select, clone, fetch/pull, refresh, and release view. Use disposable accounts or a documented manual smoke test for live APIs; fixture tests alone cannot prove a remote credential works.
- Credential tests prove a secret is never sent to a different origin, redirect, account, or repository path. Cover saved Keychain references and migration from existing settings.
- UI tests assert that every visible connection method is functional and every selectable provider has a working discovery path; provider capability state, wording, and errors remain consistent.
- Run `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, and `cargo test`. Then perform live smoke tests with minimal-scope credentials for all providers before removing “unavailable.”

## Primary documentation checked

- [Bitbucket REST authentication and pagination](https://developer.atlassian.com/cloud/bitbucket/rest/), [workspace repository endpoint](https://developer.atlassian.com/cloud/bitbucket/rest/api-group-repositories), [API token creation](https://support.atlassian.com/bitbucket-cloud/docs/create-an-api-token/), and [app-password retirement](https://support.atlassian.com/bitbucket-cloud/docs/revoke-an-app-password/).
- [Forgejo API authentication and pagination](https://forgejo.org/docs/latest/user/api/usage/), [Gitea user repository API](https://docs.gitea.com/enterprise/api/23/operations/user-list-repos/), and [Codeberg access tokens](https://docs.codeberg.org/advanced/access-token/).
- [Azure DevOps repository API](https://learn.microsoft.com/en-us/rest/api/azure/devops/git/repositories/list?view=azure-devops-rest-7.1) and [Azure Repos authentication](https://learn.microsoft.com/en-us/azure/devops/repos/git/auth-overview?view=azure-devops).
- [CodeCommit ListRepositories](https://docs.aws.amazon.com/codecommit/latest/APIReference/API_ListRepositories.html), [AWS API signing](https://docs.aws.amazon.com/IAM/latest/UserGuide/reference_sigv.html), and [CodeCommit availability history](https://docs.aws.amazon.com/codecommit/latest/userguide/history.html).
