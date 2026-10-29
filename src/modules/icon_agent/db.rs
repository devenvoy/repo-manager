//! SQLite-backed icon metadata store.
//!
//! Stores icon file paths and scan metadata in `.release-desk/agent.db`.
//! NOTE: Actual image binaries are stored as files on disk; the database
//! only holds file paths, resolution, and verification records.

use super::cache::release_desk_dir;
use rusqlite::Connection;
use std::path::{Path, PathBuf};

/// A persisted icon record.
#[derive(Clone, Debug)]
pub struct IconRecord {
    pub provider: String,
    pub owner: String,
    pub repo_name: String,
    pub source_path: String,
    pub cached_path: String,
    pub resolution: u32,
    pub confirmed_by: String,
    pub scanned_at: String,
}

const SCHEMA_VERSION: i32 = 1;

pub fn db_path(project_dir: Option<&Path>) -> Option<PathBuf> {
    Some(release_desk_dir(project_dir)?.join("agent.db"))
}

fn init_schema(conn: &Connection) -> Result<(), rusqlite::Error> {
    let current_version: i32 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap_or(0);

    if current_version < SCHEMA_VERSION {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS icon_cache (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                provider TEXT NOT NULL,
                owner TEXT NOT NULL,
                repo_name TEXT NOT NULL,
                source_path TEXT NOT NULL,
                cached_path TEXT NOT NULL,
                resolution INTEGER NOT NULL DEFAULT 0,
                confirmed_by TEXT NOT NULL DEFAULT 'scanner',
                scanned_at TEXT NOT NULL,
                UNIQUE(provider, owner, repo_name)
            );",
        )?;

        conn.execute_batch(&format!("PRAGMA user_version = {SCHEMA_VERSION};"))?;
    }

    Ok(())
}

/// Open or initialize the icon agent database.
pub fn open_icon_db() -> Option<Connection> {
    open_icon_db_for(None)
}

/// Open or initialize the icon agent database for a project or global.
pub fn open_icon_db_for(project_dir: Option<&Path>) -> Option<Connection> {
    let path = db_path(project_dir)?;

    let conn = Connection::open(&path).ok()?;
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA busy_timeout = 3000;",
    )
    .ok()?;
    init_schema(&conn).ok()?;

    Some(conn)
}

/// Query the cached icon for a repository.
pub fn query_icon(provider: &str, owner: &str, repo_name: &str) -> Option<IconRecord> {
    query_icon_for(None, provider, owner, repo_name)
}

/// Query the cached icon for a repository, checking the project database first,
/// then falling back to the global database.
pub fn query_icon_for(
    project_dir: Option<&Path>,
    provider: &str,
    owner: &str,
    repo_name: &str,
) -> Option<IconRecord> {
    // 1. Try project-specific db
    if project_dir.is_some() {
        if let Some(conn) = open_icon_db_read_only(project_dir) {
            if let Some(record) = query_single_db(&conn, provider, owner, repo_name) {
                return Some(record);
            }
        }
    }

    // Project lookups must never return another project's icon.
    if project_dir.is_some() {
        return None;
    }
    // 2. Fall back to global db
    let conn = open_icon_db_read_only(None)?;
    query_single_db(&conn, provider, owner, repo_name)
}

fn open_icon_db_read_only(project_dir: Option<&Path>) -> Option<Connection> {
    let path = if let Some(project_dir) = project_dir {
        project_dir.join(".release-desk/agent.db")
    } else {
        db_path(None)?
    };
    if !path.is_file() {
        return None;
    }
    let conn =
        Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).ok()?;
    conn.busy_timeout(std::time::Duration::from_secs(3)).ok()?;
    Some(conn)
}

fn query_single_db(
    conn: &Connection,
    provider: &str,
    owner: &str,
    repo_name: &str,
) -> Option<IconRecord> {
    let prov = provider.to_lowercase();
    let own = owner.to_lowercase();
    let name = repo_name.to_lowercase();

    // Exact match
    conn
        .query_row(
            "SELECT provider, owner, repo_name, source_path, cached_path, resolution, confirmed_by, scanned_at
             FROM icon_cache
             WHERE provider = ?1 AND owner = ?2 AND repo_name = ?3
             LIMIT 1",
            rusqlite::params![prov, own, name],
            |row| {
                Ok(IconRecord {
                    provider: row.get(0)?,
                    owner: row.get(1)?,
                    repo_name: row.get(2)?,
                    source_path: row.get(3)?,
                    cached_path: row.get(4)?,
                    resolution: row.get::<_, i64>(5)? as u32,
                    confirmed_by: row.get(6)?,
                    scanned_at: row.get(7)?,
                })
            },
        )
        .ok()
}

/// Record a discovered icon in the database (upsert).
pub fn record_icon(
    provider: &str,
    owner: &str,
    repo_name: &str,
    source_path: &str,
    cached_path: &Path,
    resolution: u32,
) {
    record_icon_for(
        None,
        provider,
        owner,
        repo_name,
        source_path,
        cached_path,
        resolution,
    );
}

/// Marker for automatic choices made with the current icon policy.
pub const AUTO_SELECTION_MARKER: &str = "scanner:v2";

/// Record a discovered icon in a specific project's database and global fallback.
pub fn record_icon_for(
    project_dir: Option<&Path>,
    provider: &str,
    owner: &str,
    repo_name: &str,
    source_path: &str,
    cached_path: &Path,
    resolution: u32,
) {
    let _ = record_icon_with_confirmation(
        project_dir,
        provider,
        owner,
        repo_name,
        source_path,
        cached_path,
        resolution,
        AUTO_SELECTION_MARKER,
    );
}

/// Record a Kev or local ranking result.
#[allow(clippy::too_many_arguments)]
pub fn record_ai_confirmed_icon(
    project_dir: Option<&Path>,
    provider: &str,
    owner: &str,
    repo_name: &str,
    source_path: &str,
    cached_path: &Path,
    resolution: u32,
    model_name: &str,
) -> Result<(), String> {
    record_icon_with_confirmation(
        project_dir,
        provider,
        owner,
        repo_name,
        source_path,
        cached_path,
        resolution,
        &format!("ai:{model_name}:v2"),
    )
}

#[allow(clippy::too_many_arguments)]
fn record_icon_with_confirmation(
    project_dir: Option<&Path>,
    provider: &str,
    owner: &str,
    repo_name: &str,
    source_path: &str,
    cached_path: &Path,
    resolution: u32,
    confirmed_by: &str,
) -> Result<(), String> {
    let now = chrono_now();
    let cached_str = cached_path.to_string_lossy().to_string();
    let write_to = |conn: &Connection| {
        conn.execute(
            "INSERT INTO icon_cache (provider, owner, repo_name, source_path, cached_path, resolution, confirmed_by, scanned_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(provider, owner, repo_name) DO UPDATE SET
                 source_path = excluded.source_path,
                 cached_path = excluded.cached_path,
                 resolution = excluded.resolution,
                 confirmed_by = excluded.confirmed_by,
                 scanned_at = excluded.scanned_at",
            rusqlite::params![
                provider.to_lowercase(),
                owner.to_lowercase(),
                repo_name.to_lowercase(),
                source_path,
                cached_str,
                resolution as i64,
                confirmed_by,
                now,
            ],
        )
        .map(|_| ())
        .map_err(|error| format!("could not save the icon database record: {error}"))
    };
    let conn = open_icon_db_for(project_dir)
        .ok_or_else(|| "could not open the icon database".to_string())?;
    write_to(&conn)
}

/// Delete a cached icon record.
pub fn delete_icon(project_dir: Option<&Path>, provider: &str, owner: &str, repo_name: &str) {
    let delete_from = |conn: &Connection| {
        let _ = conn.execute(
            "DELETE FROM icon_cache WHERE provider = ?1 AND owner = ?2 AND repo_name = ?3",
            rusqlite::params![
                provider.to_lowercase(),
                owner.to_lowercase(),
                repo_name.to_lowercase(),
            ],
        );
    };

    if project_dir.is_some() {
        if let Some(conn) = open_icon_db_for(project_dir) {
            delete_from(&conn);
        }
    }
    if project_dir.is_none() {
        if let Some(conn) = open_icon_db_for(None) {
            delete_from(&conn);
        }
    }
}

/// List all cached icon records.
pub fn list_all_icons(project_dir: Option<&Path>) -> Vec<IconRecord> {
    let Some(conn) = open_icon_db_for(project_dir) else {
        return Vec::new();
    };

    let mut stmt = match conn.prepare(
        "SELECT provider, owner, repo_name, source_path, cached_path, resolution, confirmed_by, scanned_at
         FROM icon_cache ORDER BY repo_name ASC",
    ) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };

    let rows = stmt
        .query_map([], |row| {
            Ok(IconRecord {
                provider: row.get(0)?,
                owner: row.get(1)?,
                repo_name: row.get(2)?,
                source_path: row.get(3)?,
                cached_path: row.get(4)?,
                resolution: row.get::<_, i64>(5)? as u32,
                confirmed_by: row.get(6)?,
                scanned_at: row.get(7)?,
            })
        })
        .ok();

    rows.map(|r| r.flatten().collect()).unwrap_or_default()
}

fn chrono_now() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{secs}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icon_db_lifecycle() {
        let temp = tempfile::tempdir().unwrap();
        let db_file = temp.path().join("test-agent.db");
        let conn = Connection::open(&db_file).unwrap();
        conn.execute_batch("PRAGMA journal_mode = WAL;").unwrap();
        init_schema(&conn).unwrap();

        conn.execute(
            "INSERT INTO icon_cache (provider, owner, repo_name, source_path, cached_path, resolution, confirmed_by, scanned_at)
             VALUES ('github', 'storytold', 'photocraft', 'assets/app-icon/photocraft-1024.png', '/cache/photocraft.png', 1024, 'scanner', '12345')",
            [],
        ).unwrap();

        let record: IconRecord = conn
            .query_row(
                "SELECT provider, owner, repo_name, source_path, cached_path, resolution, confirmed_by, scanned_at FROM icon_cache WHERE repo_name = 'photocraft'",
                [],
                |row| {
                    Ok(IconRecord {
                        provider: row.get(0)?,
                        owner: row.get(1)?,
                        repo_name: row.get(2)?,
                        source_path: row.get(3)?,
                        cached_path: row.get(4)?,
                        resolution: row.get::<_, i64>(5)? as u32,
                        confirmed_by: row.get(6)?,
                        scanned_at: row.get(7)?,
                    })
                },
            )
            .unwrap();

        assert_eq!(record.repo_name, "photocraft");
        assert_eq!(record.resolution, 1024);
        assert_eq!(record.source_path, "assets/app-icon/photocraft-1024.png");
    }
}
