//! SQLite schema migrations and transaction management.
//!
//! Applies versioned migrations with checksum verification, WAL mode,
//! and foreign key enforcement on every connection.

use rusqlite::{Connection, Transaction};
use std::fmt;

/// Migration error types.
#[derive(Debug)]
pub enum MigrationError {
    Sql(rusqlite::Error),
    CorruptedHistory {
        version: i64,
        expected: &'static str,
        actual: String,
    },
}

impl fmt::Display for MigrationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sql(err) => write!(f, "SQLite error during migration: {err}"),
            Self::CorruptedHistory {
                version,
                expected,
                actual,
            } => {
                write!(
                    f,
                    "migration history corrupted at version {version}: expected checksum {expected}, found {actual}"
                )
            }
        }
    }
}

impl std::error::Error for MigrationError {}

impl From<rusqlite::Error> for MigrationError {
    fn from(err: rusqlite::Error) -> Self {
        Self::Sql(err)
    }
}

struct Migration {
    version: i64,
    #[allow(dead_code)]
    description: &'static str,
    checksum: &'static str,
    sql: &'static str,
}

const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        description: "Initial core schema for projects, repositories, tracking, jobs, and PRs",
        checksum: "v1_core_schema",
        sql: r#"
        CREATE TABLE IF NOT EXISTS projects (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL UNIQUE,
            account_login TEXT NOT NULL,
            account_kind TEXT NOT NULL,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
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
        "#,
    },
    Migration {
        version: 2,
        description: "Add clone_location to projects table",
        checksum: "v2_add_clone_location",
        sql: r#"
            ALTER TABLE projects ADD COLUMN clone_location TEXT;
        "#,
    },
    Migration {
        version: 3,
        description: "Add disk_layout and provider to projects, and add project_credentials and project_sources tables",
        checksum: "v3_project_sources_and_credentials",
        sql: r#"
            ALTER TABLE projects ADD COLUMN disk_layout TEXT NOT NULL DEFAULT 'grouped';
            ALTER TABLE projects ADD COLUMN provider TEXT NOT NULL DEFAULT 'github';

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
    },
];

/// Apply standard SQLite connection pragmas (WAL mode, foreign keys, busy timeout).
pub fn configure_connection(conn: &Connection) -> Result<(), MigrationError> {
    conn.execute_batch(
        r#"
        PRAGMA journal_mode = WAL;
        PRAGMA foreign_keys = ON;
        PRAGMA busy_timeout = 5000;
        "#,
    )?;
    Ok(())
}

/// Run all pending migrations inside transactions.
pub fn migrate(conn: &mut Connection) -> Result<(), MigrationError> {
    configure_connection(conn)?;

    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS schema_migrations (
            version INTEGER PRIMARY KEY,
            applied_at TEXT NOT NULL,
            checksum TEXT NOT NULL
        );
        "#,
    )?;

    for migration in MIGRATIONS {
        let applied_checksum: Option<String> = conn
            .query_row(
                "SELECT checksum FROM schema_migrations WHERE version = ?1",
                [migration.version],
                |row| row.get(0),
            )
            .ok();

        if let Some(existing_checksum) = applied_checksum {
            if existing_checksum != migration.checksum {
                return Err(MigrationError::CorruptedHistory {
                    version: migration.version,
                    expected: migration.checksum,
                    actual: existing_checksum,
                });
            }
            continue;
        }

        // Apply new migration inside transaction
        let tx = conn.transaction()?;
        apply_migration(&tx, migration)?;
        tx.commit()?;
    }

    Ok(())
}

fn apply_migration(tx: &Transaction<'_>, migration: &Migration) -> Result<(), MigrationError> {
    tx.execute_batch(migration.sql)?;
    let now = "2026-10-27T00:00:00Z";
    tx.execute(
        "INSERT INTO schema_migrations (version, applied_at, checksum) VALUES (?1, ?2, ?3)",
        rusqlite::params![migration.version, now, migration.checksum],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_apply_cleanly_and_idempotently() {
        let mut conn = Connection::open_in_memory().unwrap();
        migrate(&mut conn).unwrap();

        // Run second time; should be idempotent
        migrate(&mut conn).unwrap();

        let count: i64 = conn
            .query_row("SELECT count(*) FROM schema_migrations", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 3);
    }
}
