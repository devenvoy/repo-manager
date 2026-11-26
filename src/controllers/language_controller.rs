//! Load repository language statistics only when their panel is opened.

use crate::app::{ReleaseDesk, project_auth_token};
use crate::views::project_repository_status::LanguageBreakdownState;
use crate::workspace_sources::RepoKey;
use gpui_kit::{Context, Window};
use std::time::Duration;

impl ReleaseDesk {
    pub(crate) fn toggle_repository_languages(
        &mut self,
        key: RepoKey,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(project) = self
            .selected_project_id
            .and_then(|id| self.projects.iter().find(|project| project.id == id))
            .cloned()
        else {
            return;
        };
        let project_id = project.id;
        let runtime = self.jobs.project_mut(project_id);
        if !runtime.languages_expanded.insert(key.clone()) {
            runtime.languages_expanded.remove(&key);
            cx.notify();
            return;
        }
        if matches!(
            runtime.language_breakdowns.get(&key),
            Some(LanguageBreakdownState::Loading | LanguageBreakdownState::Loaded(_))
        ) {
            cx.notify();
            return;
        }

        let Some((_, _, source, repo)) = project.find_repo_by_key(&key) else {
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

        self.jobs
            .project_mut(project_id)
            .language_breakdowns
            .insert(key.clone(), LanguageBreakdownState::Loading);
        cx.notify();
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
                            if this.projects.iter().any(|p| p.id == project_id) {
                                let state = match result {
                                    Ok(languages) => LanguageBreakdownState::Loaded(languages),
                                    Err(error) => LanguageBreakdownState::Error(error.to_string()),
                                };
                                this.jobs
                                    .project_mut(project_id)
                                    .language_breakdowns
                                    .insert(key, state);
                                cx.notify();
                            }
                        });
                        return;
                    }
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        let _ = this.update_in(cx, |this, _, cx| {
                            if this.projects.iter().any(|p| p.id == project_id) {
                                this.jobs
                                    .project_mut(project_id)
                                    .language_breakdowns
                                    .insert(
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
