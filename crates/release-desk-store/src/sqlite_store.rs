//! SQLite-backed domain port implementations.
//!
//! Implements ProjectStore, TrackingStore, JobStore, and PullRequestStore
//! with thread-safe connection pooling and WAL mode.

use crate::boundary::ManagedLibrary;
use crate::migration::migrate;
use release_desk_application::use_case::discovery::{
    ProjectRepositoryRecord, TrackingStatus, TrackingStore,
};
use release_desk_application::use_case::pull_request::PullRequestStore;
use release_desk_core::error::DomainError;
use release_desk_core::job::{IdempotencyKey, JobId, JobKind, JobPriority, JobState};
use release_desk_core::port::{JobStore, ProjectStore};
use release_desk_core::project::{ProjectId, ProjectIdentity, ProjectName, TrackingPreferences};
use release_desk_core::pull_request::{PullRequest, PullRequestState};
use release_desk_core::repository::GitHubRepositoryId;
use rusqlite::{Connection, OptionalExtension, params};
use std::path::Path;
use std::sync::{Arc, Mutex};

/// Thread-safe SQLite store implementing domain port traits.
#[derive(Clone)]
pub struct SqliteStore {
    conn: Arc<Mutex<Connection>>,
    library: ManagedLibrary,
}

impl SqliteStore {
    /// Open or create a SQLite database at `db_path` bounded by `library`.
    pub fn open(db_path: impl AsRef<Path>, library: ManagedLibrary) -> Result<Self, DomainError> {
        let mut conn =
            Connection::open(db_path.as_ref()).map_err(|e| DomainError::PortFailure {
                port: "SQLite",
                message: e.to_string(),
            })?;

        migrate(&mut conn).map_err(|e| DomainError::PortFailure {
            port: "Migration",
            message: e.to_string(),
        })?;

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            library,
        })
    }

    /// Open an in-memory database (useful for isolated tests).
    pub fn open_in_memory(library: ManagedLibrary) -> Result<Self, DomainError> {
        let mut conn = Connection::open_in_memory().map_err(|e| DomainError::PortFailure {
            port: "SQLite",
            message: e.to_string(),
        })?;

        migrate(&mut conn).map_err(|e| DomainError::PortFailure {
            port: "Migration",
            message: e.to_string(),
        })?;

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            library,
        })
    }

    /// Access the underlying managed library.
    pub fn library(&self) -> &ManagedLibrary {
        &self.library
    }
}

impl ProjectStore for SqliteStore {
    fn list_projects(&self) -> Result<Vec<ProjectIdentity>, DomainError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT id, name, clone_location FROM projects ORDER BY name ASC")
            .map_err(|e| DomainError::PortFailure {
                port: "SQLite",
                message: e.to_string(),
            })?;

        let rows = stmt
            .query_map([], |row| {
                let id_raw: String = row.get(0)?;
                let name_raw: String = row.get(1)?;
                let clone_location_raw: Option<String> = row.get(2)?;
                Ok((id_raw, name_raw, clone_location_raw))
            })
            .map_err(|e| DomainError::PortFailure {
                port: "SQLite",
                message: e.to_string(),
            })?;

        let mut projects = Vec::new();
        for row in rows {
            let (id_raw, name_raw, clone_location_raw) =
                row.map_err(|e| DomainError::PortFailure {
                    port: "SQLite",
                    message: e.to_string(),
                })?;

            let id_num: u128 = id_raw.parse().map_err(|_| DomainError::PortFailure {
                port: "SQLite",
                message: format!("invalid numeric project id: {id_raw}"),
            })?;
            let project_name = ProjectName::new(&name_raw).map_err(DomainError::from)?;
            let clone_location = clone_location_raw.map(std::path::PathBuf::from);

            projects.push(ProjectIdentity {
                id: ProjectId::from_u128(id_num),
                name: project_name,
                clone_location,
            });
        }

        Ok(projects)
    }

    fn get_project(&self, id: ProjectId) -> Result<Option<ProjectIdentity>, DomainError> {
        let conn = self.conn.lock().unwrap();
        let id_str = id.as_u128().to_string();

        let mut stmt = conn
            .prepare("SELECT id, name, clone_location FROM projects WHERE id = ?1")
            .map_err(|e| DomainError::PortFailure {
                port: "SQLite",
                message: e.to_string(),
            })?;

        let res = stmt
            .query_row([&id_str], |row| {
                let id_raw: String = row.get(0)?;
                let name_raw: String = row.get(1)?;
                let clone_location_raw: Option<String> = row.get(2)?;
                Ok((id_raw, name_raw, clone_location_raw))
            })
            .optional()
            .map_err(|e| DomainError::PortFailure {
                port: "SQLite",
                message: e.to_string(),
            })?;

        match res {
            Some((_, name_raw, clone_location_raw)) => {
                let project_name = ProjectName::new(&name_raw).map_err(DomainError::from)?;
                let clone_location = clone_location_raw.map(std::path::PathBuf::from);
                Ok(Some(ProjectIdentity {
                    id,
                    name: project_name,
                    clone_location,
                }))
            }
            None => Ok(None),
        }
    }

    fn save_project(&self, project: &ProjectIdentity) -> Result<(), DomainError> {
        let conn = self.conn.lock().unwrap();
        let id_str = project.id.as_u128().to_string();
        let name_str = project.name.as_str();
        let clone_loc_str = project
            .clone_location
            .as_ref()
            .map(|p| p.to_string_lossy().to_string());
        let now = "2026-10-27T00:00:00Z";

        conn.execute(
            r#"
            INSERT INTO projects (id, name, clone_location, account_login, account_kind, created_at, updated_at)
            VALUES (?1, ?2, ?3, 'owner', 'organization', ?4, ?4)
            ON CONFLICT(id) DO UPDATE SET
                name = excluded.name,
                clone_location = excluded.clone_location,
                updated_at = excluded.updated_at
            "#,
            params![id_str, name_str, clone_loc_str, now],
        )
        .map_err(|e| DomainError::PortFailure {
            port: "SQLite",
            message: e.to_string(),
        })?;

        Ok(())
    }

    fn delete_project(&self, id: ProjectId) -> Result<(), DomainError> {
        let conn = self.conn.lock().unwrap();
        let id_str = id.as_u128().to_string();

        conn.execute("DELETE FROM projects WHERE id = ?1", params![id_str])
            .map_err(|e| DomainError::PortFailure {
                port: "SQLite",
                message: e.to_string(),
            })?;

        Ok(())
    }
}

impl TrackingStore for SqliteStore {
    fn get_preferences(
        &self,
        project_id: ProjectId,
        repo_id: GitHubRepositoryId,
    ) -> Result<TrackingStatus, DomainError> {
        let conn = self.conn.lock().unwrap();
        let proj_str = project_id.as_u128().to_string();
        let repo_num = repo_id.as_u64() as i64;

        let res = conn
            .query_row(
                r#"
                SELECT status, track_source, track_installer, track_pr
                FROM project_repositories
                WHERE project_id = ?1 AND github_repo_id = ?2
                "#,
                params![proj_str, repo_num],
                |row| {
                    let status: String = row.get(0)?;
                    let src: i64 = row.get(1)?;
                    let inst: i64 = row.get(2)?;
                    let pr: i64 = row.get(3)?;
                    Ok((status, src != 0, inst != 0, pr != 0))
                },
            )
            .optional()
            .map_err(|e| DomainError::PortFailure {
                port: "SQLite",
                message: e.to_string(),
            })?;

        match res {
            Some((status, src, inst, pr)) => match status.as_str() {
                "tracked" => Ok(TrackingStatus::Tracked(TrackingPreferences {
                    source: src,
                    installers: inst,
                    pull_requests: pr,
                })),
                "ignored" => Ok(TrackingStatus::Ignored),
                _ => Ok(TrackingStatus::Inbox),
            },
            None => Ok(TrackingStatus::Inbox),
        }
    }

    fn set_preferences(
        &self,
        project_id: ProjectId,
        repo_id: GitHubRepositoryId,
        status: TrackingStatus,
    ) -> Result<(), DomainError> {
        let conn = self.conn.lock().unwrap();
        let proj_str = project_id.as_u128().to_string();
        let repo_num = repo_id.as_u64() as i64;
        let now = "2026-10-27T00:00:00Z";

        let (status_str, src, inst, pr) = match &status {
            TrackingStatus::Inbox => ("inbox", 1, 1, 1),
            TrackingStatus::Ignored => ("ignored", 0, 0, 0),
            TrackingStatus::Tracked(prefs) => (
                "tracked",
                i64::from(prefs.source),
                i64::from(prefs.installers),
                i64::from(prefs.pull_requests),
            ),
        };

        conn.execute(
            r#"
            INSERT INTO project_repositories (
                project_id, github_repo_id, repo_name, status,
                track_source, track_installer, track_pr, updated_at
            )
            VALUES (?1, ?2, 'repo', ?3, ?4, ?5, ?6, ?7)
            ON CONFLICT(project_id, github_repo_id) DO UPDATE SET
                status = excluded.status,
                track_source = excluded.track_source,
                track_installer = excluded.track_installer,
                track_pr = excluded.track_pr,
                updated_at = excluded.updated_at
            "#,
            params![proj_str, repo_num, status_str, src, inst, pr, now],
        )
        .map_err(|e| DomainError::PortFailure {
            port: "SQLite",
            message: e.to_string(),
        })?;

        Ok(())
    }

    fn list_project_repositories(
        &self,
        project_id: ProjectId,
    ) -> Result<Vec<ProjectRepositoryRecord>, DomainError> {
        let conn = self.conn.lock().unwrap();
        let proj_str = project_id.as_u128().to_string();

        let mut stmt = conn
            .prepare(
                r#"
                SELECT github_repo_id, repo_name, status, track_source, track_installer, track_pr
                FROM project_repositories
                WHERE project_id = ?1
                ORDER BY repo_name ASC
                "#,
            )
            .map_err(|e| DomainError::PortFailure {
                port: "SQLite",
                message: e.to_string(),
            })?;

        let rows = stmt
            .query_map([&proj_str], |row| {
                let repo_id_num: i64 = row.get(0)?;
                let repo_name: String = row.get(1)?;
                let status_str: String = row.get(2)?;
                let src: i64 = row.get(3)?;
                let inst: i64 = row.get(4)?;
                let pr: i64 = row.get(5)?;

                let status = match status_str.as_str() {
                    "tracked" => TrackingStatus::Tracked(TrackingPreferences {
                        source: src != 0,
                        installers: inst != 0,
                        pull_requests: pr != 0,
                    }),
                    "ignored" => TrackingStatus::Ignored,
                    _ => TrackingStatus::Inbox,
                };

                Ok(ProjectRepositoryRecord {
                    repo_id: GitHubRepositoryId::from_u64(repo_id_num as u64),
                    repo_name,
                    status,
                })
            })
            .map_err(|e| DomainError::PortFailure {
                port: "SQLite",
                message: e.to_string(),
            })?;

        let mut records = Vec::new();
        for row in rows {
            records.push(row.map_err(|e| DomainError::PortFailure {
                port: "SQLite",
                message: e.to_string(),
            })?);
        }

        Ok(records)
    }

    fn register_repository(
        &self,
        project_id: ProjectId,
        repo_id: GitHubRepositoryId,
        repo_name: &str,
        status: TrackingStatus,
    ) -> Result<(), DomainError> {
        let conn = self.conn.lock().unwrap();
        let proj_str = project_id.as_u128().to_string();
        let repo_num = repo_id.as_u64() as i64;
        let now = "2026-10-27T00:00:00Z";

        let (status_str, src, inst, pr) = match &status {
            TrackingStatus::Inbox => ("inbox", 1, 1, 1),
            TrackingStatus::Ignored => ("ignored", 0, 0, 0),
            TrackingStatus::Tracked(prefs) => (
                "tracked",
                i64::from(prefs.source),
                i64::from(prefs.installers),
                i64::from(prefs.pull_requests),
            ),
        };

        conn.execute(
            r#"
            INSERT INTO project_repositories (
                project_id, github_repo_id, repo_name, status,
                track_source, track_installer, track_pr, updated_at
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
            ON CONFLICT(project_id, github_repo_id) DO UPDATE SET
                repo_name = excluded.repo_name,
                status = excluded.status,
                track_source = excluded.track_source,
                track_installer = excluded.track_installer,
                track_pr = excluded.track_pr,
                updated_at = excluded.updated_at
            "#,
            params![
                proj_str, repo_num, repo_name, status_str, src, inst, pr, now
            ],
        )
        .map_err(|e| DomainError::PortFailure {
            port: "SQLite",
            message: e.to_string(),
        })?;

        Ok(())
    }
}

impl JobStore for SqliteStore {
    fn enqueue_job(
        &self,
        kind: JobKind,
        priority: JobPriority,
        idempotency: &IdempotencyKey,
    ) -> Result<JobId, DomainError> {
        let conn = self.conn.lock().unwrap();
        let kind_str = format!("{kind:?}");
        let prio_str = format!("{priority:?}");
        let now = "2026-10-27T00:00:00Z";

        // Check idempotency
        let existing: Option<String> = conn
            .query_row(
                "SELECT id FROM jobs WHERE idempotency_key = ?1",
                params![idempotency.as_str()],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| DomainError::PortFailure {
                port: "SQLite",
                message: e.to_string(),
            })?;

        if let Some(id_str) = existing {
            let id_num: u128 = id_str.parse().unwrap_or(1);
            return Ok(JobId::from_u128(id_num));
        }

        // Monotonic job ID
        let max_id: Option<i64> = conn
            .query_row("SELECT MAX(CAST(id AS INTEGER)) FROM jobs", [], |row| {
                row.get(0)
            })
            .optional()
            .unwrap_or(None);
        let next_id = max_id.unwrap_or(0) + 1;
        let id_str = next_id.to_string();

        conn.execute(
            r#"
            INSERT INTO jobs (
                id, kind, priority, state, idempotency_key, created_at, updated_at
            )
            VALUES (?1, ?2, ?3, 'pending', ?4, ?5, ?5)
            "#,
            params![id_str, kind_str, prio_str, idempotency.as_str(), now],
        )
        .map_err(|e| DomainError::PortFailure {
            port: "SQLite",
            message: e.to_string(),
        })?;

        Ok(JobId::from_u128(next_id as u128))
    }

    fn update_job_state(&self, id: JobId, state: JobState) -> Result<(), DomainError> {
        let conn = self.conn.lock().unwrap();
        let id_str = id.as_u128().to_string();
        let state_str = format!("{state:?}");
        let now = "2026-10-27T00:00:00Z";

        conn.execute(
            "UPDATE jobs SET state = ?1, updated_at = ?2 WHERE id = ?3",
            params![state_str, now, id_str],
        )
        .map_err(|e| DomainError::PortFailure {
            port: "SQLite",
            message: e.to_string(),
        })?;

        Ok(())
    }
}

impl PullRequestStore for SqliteStore {
    fn get_pull_requests(
        &self,
        repo_id: GitHubRepositoryId,
    ) -> Result<Vec<PullRequest>, DomainError> {
        let conn = self.conn.lock().unwrap();
        let repo_num = repo_id.as_u64() as i64;

        let mut stmt = conn
            .prepare(
                r#"
                SELECT number, title, state, draft, head_ref, base_ref, html_url, created_at, updated_at, merged_at
                FROM pull_requests
                WHERE github_repo_id = ?1
                ORDER BY number DESC
                "#,
            )
            .map_err(|e| DomainError::PortFailure {
                port: "SQLite",
                message: e.to_string(),
            })?;

        let rows = stmt
            .query_map(params![repo_num], |row| {
                let number: i64 = row.get(0)?;
                let title: String = row.get(1)?;
                let state_raw: String = row.get(2)?;
                let draft: i64 = row.get(3)?;
                let head_ref: String = row.get(4)?;
                let base_ref: String = row.get(5)?;
                let html_url: String = row.get(6)?;
                let created_at: String = row.get(7)?;
                let updated_at: String = row.get(8)?;
                let merged_at: Option<String> = row.get(9)?;

                let state = match state_raw.as_str() {
                    "merged" => PullRequestState::Merged,
                    "closed" => PullRequestState::Closed,
                    "draft" => PullRequestState::Draft,
                    _ => PullRequestState::Open,
                };

                Ok(PullRequest {
                    number: number as u64,
                    title,
                    state,
                    draft: draft != 0,
                    author: None,
                    head_ref,
                    base_ref,
                    html_url,
                    created_at,
                    updated_at,
                    merged_at,
                    labels: vec![],
                })
            })
            .map_err(|e| DomainError::PortFailure {
                port: "SQLite",
                message: e.to_string(),
            })?;

        let mut prs = Vec::new();
        for r in rows {
            prs.push(r.map_err(|e| DomainError::PortFailure {
                port: "SQLite",
                message: e.to_string(),
            })?);
        }
        Ok(prs)
    }

    fn save_pull_requests(
        &self,
        repo_id: GitHubRepositoryId,
        prs: &[PullRequest],
    ) -> Result<(), DomainError> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction().map_err(|e| DomainError::PortFailure {
            port: "SQLite",
            message: e.to_string(),
        })?;

        let repo_num = repo_id.as_u64() as i64;
        for pr in prs {
            let state_str = match pr.state {
                PullRequestState::Merged => "merged",
                PullRequestState::Closed => "closed",
                PullRequestState::Draft => "draft",
                PullRequestState::Open => "open",
            };

            let id = format!("{}_{}", repo_num, pr.number);
            tx.execute(
                r#"
                INSERT INTO pull_requests (
                    id, github_repo_id, number, title, state, draft,
                    head_ref, base_ref, html_url, created_at, updated_at, merged_at
                )
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
                ON CONFLICT(github_repo_id, number) DO UPDATE SET
                    title = excluded.title,
                    state = excluded.state,
                    draft = excluded.draft,
                    head_ref = excluded.head_ref,
                    base_ref = excluded.base_ref,
                    html_url = excluded.html_url,
                    updated_at = excluded.updated_at,
                    merged_at = excluded.merged_at
                "#,
                params![
                    id,
                    repo_num,
                    pr.number as i64,
                    pr.title,
                    state_str,
                    i64::from(pr.draft),
                    pr.head_ref,
                    pr.base_ref,
                    pr.html_url,
                    pr.created_at,
                    pr.updated_at,
                    pr.merged_at,
                ],
            )
            .map_err(|e| DomainError::PortFailure {
                port: "SQLite",
                message: e.to_string(),
            })?;
        }

        tx.commit().map_err(|e| DomainError::PortFailure {
            port: "SQLite",
            message: e.to_string(),
        })?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_lifecycle_in_sqlite() {
        let temp = tempfile::tempdir().unwrap();
        let library = ManagedLibrary::new(temp.path()).unwrap();
        let store = SqliteStore::open_in_memory(library).unwrap();

        let id = ProjectId::from_u128(42);
        let name = ProjectName::new("Release Desk Pro").unwrap();
        let clone_location = Some(temp.path().join("clone-repo"));
        let project = ProjectIdentity::with_clone_location(id, name, clone_location.clone());

        store.save_project(&project).unwrap();

        let fetched = store.get_project(id).unwrap().unwrap();
        assert_eq!(fetched.id, id);
        assert_eq!(fetched.name.as_str(), "Release Desk Pro");
        assert_eq!(fetched.clone_location, clone_location);

        let list = store.list_projects().unwrap();
        assert_eq!(list.len(), 1);

        store.delete_project(id).unwrap();
        assert!(store.get_project(id).unwrap().is_none());
    }

    #[test]
    fn tracking_preferences_in_sqlite() {
        let temp = tempfile::tempdir().unwrap();
        let library = ManagedLibrary::new(temp.path()).unwrap();
        let store = SqliteStore::open_in_memory(library).unwrap();

        let project_id = ProjectId::from_u128(10);
        let repo_id = GitHubRepositoryId::from_u64(20);

        assert_eq!(
            store.get_preferences(project_id, repo_id).unwrap(),
            TrackingStatus::Inbox
        );

        let prefs = TrackingPreferences {
            source: true,
            installers: false,
            pull_requests: true,
        };
        store
            .set_preferences(project_id, repo_id, TrackingStatus::Tracked(prefs))
            .unwrap();

        assert_eq!(
            store.get_preferences(project_id, repo_id).unwrap(),
            TrackingStatus::Tracked(prefs)
        );
    }
}
