//! Initialize the current SQLite schema for fresh installations.

use rusqlite::Connection;

/// Initialize missing current-schema tables without migrating existing databases.
pub fn initialize(conn: &Connection) -> rusqlite::Result<()> {
    let expected: &[(&str, &[&str])] = &[
        (
            "projects",
            &[
                "id",
                "name",
                "account_login",
                "account_kind",
                "created_at",
                "updated_at",
                "clone_location",
                "disk_layout",
                "provider",
            ],
        ),
        (
            "project_repositories",
            &[
                "project_id",
                "github_repo_id",
                "repo_name",
                "status",
                "track_source",
                "track_installer",
                "track_pr",
                "updated_at",
            ],
        ),
        (
            "jobs",
            &[
                "id",
                "project_id",
                "kind",
                "priority",
                "state",
                "idempotency_key",
                "error_message",
                "created_at",
                "updated_at",
            ],
        ),
        (
            "pull_requests",
            &[
                "id",
                "github_repo_id",
                "number",
                "title",
                "state",
                "draft",
                "author",
                "head_ref",
                "base_ref",
                "html_url",
                "created_at",
                "updated_at",
                "merged_at",
            ],
        ),
        (
            "installer_files",
            &[
                "id",
                "project_id",
                "repo_id",
                "managed_path",
                "role",
                "sha256",
                "size",
                "created_at",
            ],
        ),
        (
            "schedules",
            &[
                "id",
                "project_id",
                "job_kind",
                "recurrence_cron",
                "enabled",
                "last_run",
                "next_run",
            ],
        ),
        (
            "project_credentials",
            &[
                "project_id",
                "provider",
                "auth_method",
                "encrypted_secret",
                "account_login",
                "created_at",
                "updated_at",
            ],
        ),
        (
            "project_sources",
            &[
                "id",
                "project_id",
                "provider",
                "login",
                "kind",
                "display_name",
                "avatar_url",
                "bio",
                "public_repos_count",
                "created_at",
            ],
        ),
    ];
    for (table, required) in expected {
        // Identifiers come from the static list above, not user input.
        let mut statement = conn.prepare(&format!("PRAGMA table_info({table})"))?;
        let columns = statement
            .query_map([], |row| row.get::<_, String>(1))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        if !columns.is_empty()
            && required
                .iter()
                .any(|name| !columns.iter().any(|column| column == name))
        {
            return Err(rusqlite::Error::InvalidParameterName(format!(
                "Existing {table} table has an unsupported schema; no data was changed"
            )));
        }
    }
    conn.execute_batch(
        "PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON; PRAGMA busy_timeout = 5000;",
    )?;
    let transaction = conn.unchecked_transaction()?;
    transaction.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS projects (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL UNIQUE,
            account_login TEXT NOT NULL,
            account_kind TEXT NOT NULL,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            clone_location TEXT,
            disk_layout TEXT NOT NULL DEFAULT 'grouped',
            provider TEXT NOT NULL DEFAULT 'github'
        );

        CREATE TABLE IF NOT EXISTS project_repositories (
            project_id TEXT NOT NULL,
            github_repo_id INTEGER NOT NULL,
            repo_name TEXT NOT NULL,
            status TEXT NOT NULL,
            track_source INTEGER NOT NULL DEFAULT 1,
            track_installer INTEGER NOT NULL DEFAULT 1,
            track_pr INTEGER NOT NULL DEFAULT 1,
            updated_at TEXT NOT NULL,
            PRIMARY KEY (project_id, github_repo_id)
        );

        CREATE INDEX IF NOT EXISTS idx_project_repositories_project_id
            ON project_repositories(project_id);

        CREATE TABLE IF NOT EXISTS jobs (
            id TEXT PRIMARY KEY,
            project_id TEXT,
            kind TEXT NOT NULL,
            priority TEXT NOT NULL,
            state TEXT NOT NULL,
            idempotency_key TEXT UNIQUE,
            error_message TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_jobs_state_priority
            ON jobs(state, priority);

        CREATE TABLE IF NOT EXISTS pull_requests (
            id TEXT PRIMARY KEY,
            github_repo_id INTEGER NOT NULL,
            number INTEGER NOT NULL,
            title TEXT NOT NULL,
            state TEXT NOT NULL,
            draft INTEGER NOT NULL,
            author TEXT,
            head_ref TEXT NOT NULL,
            base_ref TEXT NOT NULL,
            html_url TEXT NOT NULL,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            merged_at TEXT,
            UNIQUE(github_repo_id, number)
        );

        CREATE INDEX IF NOT EXISTS idx_pull_requests_repo_state
            ON pull_requests(github_repo_id, state);

        CREATE TABLE IF NOT EXISTS installer_files (
            id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL,
            repo_id INTEGER NOT NULL,
            managed_path TEXT NOT NULL UNIQUE,
            role TEXT NOT NULL,
            sha256 TEXT NOT NULL,
            size INTEGER NOT NULL,
            created_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS schedules (
            id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL,
            job_kind TEXT NOT NULL,
            recurrence_cron TEXT NOT NULL,
            enabled INTEGER NOT NULL DEFAULT 1,
            last_run TEXT,
            next_run TEXT
        );
                    CREATE TABLE IF NOT EXISTS project_credentials (
                project_id TEXT NOT NULL,
                provider TEXT NOT NULL,
                auth_method TEXT NOT NULL,
                encrypted_secret TEXT NOT NULL,
                account_login TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                PRIMARY KEY (project_id, provider)
            );

            CREATE TABLE IF NOT EXISTS project_sources (
                id TEXT PRIMARY KEY,
                project_id TEXT NOT NULL,
                provider TEXT NOT NULL,
                login TEXT NOT NULL,
                kind TEXT NOT NULL,
                display_name TEXT,
                avatar_url TEXT,
                bio TEXT,
                public_repos_count INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL,
                UNIQUE(project_id, provider, login)
            );
        "#,
    )?;
    transaction.commit()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::initialize;
    #[test]
    fn initializes_current_schema_idempotently_and_preserves_existing_rows() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        initialize(&conn).unwrap();
        conn.execute("INSERT INTO projects (id, name, account_login, account_kind, created_at, updated_at) VALUES ('1', 'sample', 'owner', 'user', 'now', 'now')", []).unwrap();
        initialize(&conn).unwrap();
        let count: i64 = conn
            .query_row("SELECT count(*) FROM projects", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }
    #[test]
    fn older_schema_is_rejected_without_modifying_data() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE projects (id TEXT PRIMARY KEY); INSERT INTO projects VALUES ('keep');",
        )
        .unwrap();
        assert!(initialize(&conn).is_err());
        let id: String = conn
            .query_row("SELECT id FROM projects", [], |row| row.get(0))
            .unwrap();
        assert_eq!(id, "keep");
    }
}
