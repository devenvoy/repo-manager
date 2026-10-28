# Planning state

The previous completion and feature-parity claims were inaccurate and are superseded. The active roadmap is `.planning/ROADMAP.md`.

The desired workflow is: add a source → discover and track its accessible repository catalogue → browse → select repositories to manage → clone or update with visible real Git state → inspect and download real releases. Projects can use different folders, providers and sources. The supplied HTML and shell script are behavioral and visual references, not instructions to execute.

Current status: implementation in progress. The source picker and project workspace are prototypes; clone, refresh, release download, provider coverage, and visual verification must be demonstrated before they can be marked complete.

Implemented in the current working tree: GitHub/GitLab source discovery, pagination, search and selection preservation; reviewed clone-only operations; explicit Git remote checks and guarded fast-forward; fetched GitHub/GitLab release metadata; public and selected private asset download paths; and a project-scoped Git credential helper. Offline tests pass, but live provider, private credential, download, and desktop interaction checks remain outstanding. Browser/webview OAuth token exchange is unavailable and reports an error. The actual UI still needs visual review, operational progress and cancellation, and download integrity work. Do not mark feature parity or a software release complete.
