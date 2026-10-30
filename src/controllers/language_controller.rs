//! Load repository language statistics only when their panel is opened.

use crate::app::{ReleaseDesk, project_auth_token};
use crate::views::project_repository_status::LanguageBreakdownState;
use gpui_kit::{Context, Window};
use std::time::Duration;

impl ReleaseDesk {
    pub(crate) fn toggle_repository_languages(
        &mut self,
        source_index: usize,
        repo_index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let key = (source_index, repo_index);
        if !self.languages_expanded.insert(key) {
            self.languages_expanded.remove(&key);
            cx.notify();
            return;
        }
        if matches!(
            self.language_breakdowns.get(&key),
            Some(LanguageBreakdownState::Loading | LanguageBreakdownState::Loaded(_))
        ) {
            cx.notify();
            return;
        }

        let Some(project) = self
            .selected_project_id
            .and_then(|id| self.projects.iter().find(|project| project.id == id))
        else {
            return;
        };
        let Some(source) = project.sources.get(source_index) else {
            return;
        };
        let Some(repo) = source.repositories.get(repo_index) else {
            return;
        };
        let provider = source.provider.clone();
        let full_name = if repo.full_name.is_empty() {
            format!("{}/{}", source.handle, repo.name)
        } else {
            repo.full_name.clone()
        };
        let use_token = provider == project.provider.as_str();
        let auth_project = project.clone();
        let secret_store = self.secret_store.clone();

        self.language_breakdowns
            .insert(key, LanguageBreakdownState::Loading);
        cx.notify();
        let revision = self.workspace_revision;
        let project_id = project.id;
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let result = (|| {
                let token = if use_token {
                    project_auth_token(&auth_project, &secret_store)?
                } else {
                    None
                };
                release_desk_store::source_fetcher::fetch_repository_languages(
                    &provider,
                    &full_name,
                    token.as_deref(),
                )
                .map_err(|error| error.to_string())
            })();
            let _ = tx.send(result);
        });
        cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(100))
                    .await;
                match rx.try_recv() {
                    Ok(result) => {
                        let _ = this.update_in(cx, |this, _, cx| {
                            if this.workspace_revision == revision
                                && this.selected_project_id == Some(project_id)
                            {
                                let state = match result {
                                    Ok(languages) => LanguageBreakdownState::Loaded(languages),
                                    Err(error) => LanguageBreakdownState::Error(error.to_string()),
                                };
                                this.language_breakdowns.insert(key, state);
                                cx.notify();
                            }
                        });
                        return;
                    }
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        let _ = this.update_in(cx, |this, _, cx| {
                            if this.workspace_revision == revision {
                                this.language_breakdowns.insert(
                                    key,
                                    LanguageBreakdownState::Error(
                                        "Language request stopped unexpectedly".into(),
                                    ),
                                );
                                cx.notify();
                            }
                        });
                        return;
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => {}
                }
            }
        })
        .detach();
    }
}
