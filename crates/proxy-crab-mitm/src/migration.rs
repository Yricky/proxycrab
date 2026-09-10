use std::{fs, path::Path, time::Duration};

use anyhow::{Context, Result, bail};
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};

use crate::{
    asset::AssetStore,
    model::{BodySourceType, CaptureModifications, HeaderValues},
    workspace::{read_json, write_json_atomic},
};

const WORKSPACE_SCHEMA_FILE: &str = "workspace_schema.json";
pub(crate) const CURRENT_WORKSPACE_SCHEMA_VERSION: u32 = 3;

#[derive(Debug, Default, Serialize, Deserialize)]
struct WorkspaceSchema {
    version: u32,
}

pub(crate) fn migrate_workspace(root: &Path) -> Result<()> {
    let schema_path = root.join(WORKSPACE_SCHEMA_FILE);
    let mut version = if schema_path.exists() {
        read_json::<WorkspaceSchema>(&schema_path)?.version
    } else {
        0
    };
    if version > CURRENT_WORKSPACE_SCHEMA_VERSION {
        bail!(
            "workspace schema version {version} is newer than supported version {}",
            CURRENT_WORKSPACE_SCHEMA_VERSION
        );
    }

    while version < CURRENT_WORKSPACE_SCHEMA_VERSION {
        match version {
            0 => migrate_v0_to_v1(root)?,
            1 => migrate_v1_to_v2(root)?,
            2 => migrate_v2_to_v3(root)?,
            _ => bail!("no migration registered for workspace schema version {version}"),
        }
        version += 1;
        write_json_atomic(&schema_path, &WorkspaceSchema { version })?;
    }
    Ok(())
}

/// Workspace schema v1 adds persistent request tags to every Session capture database and
/// replaces the one-row-per-interceptor-position history table with an ID-based execution table.
/// The new execution model stores saved and temporary runs at the same chain position, records
/// completion state, and preserves their creation order without changing capture/blob layout.
fn migrate_v0_to_v1(root: &Path) -> Result<()> {
    for directory in session_directories(root) {
        if !directory.exists() {
            continue;
        }
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let database = entry.path().join("captures.db");
            if database.exists() {
                migrate_capture_database_v1(&database).with_context(|| {
                    format!("failed to migrate capture database {}", database.display())
                })?;
            }
        }
    }
    Ok(())
}

fn migrate_capture_database_v1(path: &Path) -> Result<()> {
    let mut connection = Connection::open(path)?;
    connection.busy_timeout(Duration::from_secs(5))?;
    connection.execute_batch("PRAGMA foreign_keys = ON;")?;
    if !table_exists(&connection, "captures")? {
        return Ok(());
    }
    if !table_has_column(&connection, "captures", "req_tags")? {
        connection.execute(
            "ALTER TABLE captures ADD COLUMN req_tags TEXT NOT NULL DEFAULT '{}'",
            [],
        )?;
    }
    if !table_exists(&connection, "capture_interceptor_runs")? {
        return Ok(());
    }
    if !table_has_column(&connection, "capture_interceptor_runs", "id")? {
        migrate_interceptor_runs_v1(&mut connection)?;
    } else {
        connection.execute(
            "CREATE INDEX IF NOT EXISTS capture_interceptor_runs_capture
             ON capture_interceptor_runs(capture_id, phase, id)",
            [],
        )?;
    }
    Ok(())
}

/// Workspace schema v2 changes every Session capture database from SQLite's rollback journal
/// storage model to WAL. The logical tables and blob layout stay unchanged; the persistent WAL
/// mode allows management readers to overlap with MITM capture writers.
fn migrate_v1_to_v2(root: &Path) -> Result<()> {
    for directory in session_directories(root) {
        if !directory.exists() {
            continue;
        }
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let database = entry.path().join("captures.db");
            if database.exists() {
                let connection = Connection::open(&database)?;
                connection.busy_timeout(Duration::from_secs(5))?;
                connection
                    .execute_batch("PRAGMA journal_mode = WAL;")
                    .with_context(|| {
                        format!(
                            "failed to enable WAL for capture database {}",
                            database.display()
                        )
                    })?;
            }
        }
    }
    Ok(())
}

/// Workspace schema v3 stores the final request and response body source in the existing
/// capture modification columns, moves legacy modified body blobs into immutable migration
/// assets, and removes obsolete interceptor snapshots and file-replacement history.
fn migrate_v2_to_v3(root: &Path) -> Result<()> {
    let assets = AssetStore::open(root)?;
    for directory in session_directories(root) {
        if !directory.exists() {
            continue;
        }
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let session_id = entry
                .file_name()
                .to_string_lossy()
                .parse::<u64>()
                .with_context(|| format!("invalid Session directory {}", entry.path().display()))?;
            let database = entry.path().join("captures.db");
            if !database.exists() {
                continue;
            }
            migrate_capture_database_v3(&database, &entry.path().join("blob"), session_id, &assets)
                .with_context(|| {
                    format!("failed to migrate capture database {}", database.display())
                })?;
        }
    }
    Ok(())
}

fn migrate_capture_database_v3(
    path: &Path,
    blob_directory: &Path,
    session_id: u64,
    assets: &AssetStore,
) -> Result<()> {
    let connection = Connection::open(path)?;
    connection.busy_timeout(Duration::from_secs(5))?;
    connection.execute_batch("PRAGMA foreign_keys = ON;")?;
    if !table_exists(&connection, "captures")? {
        return Ok(());
    }

    let captures = {
        let mut statement = connection.prepare(
            "SELECT id, req_headers, resp_headers, req_modifications,
                    resp_modifications, created_at
             FROM captures ORDER BY id",
        )?;
        statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)? as u64,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, i64>(5)? as u64,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?
    };

    for (
        capture_id,
        request_headers,
        response_headers,
        request_state,
        response_state,
        created_at,
    ) in captures
    {
        migrate_final_body(
            &connection,
            assets,
            blob_directory,
            session_id,
            capture_id,
            "request",
            "req_modifications",
            &request_headers,
            &request_state,
            created_at,
        )?;
        migrate_final_body(
            &connection,
            assets,
            blob_directory,
            session_id,
            capture_id,
            "response",
            "resp_modifications",
            response_headers.as_deref().unwrap_or("{}"),
            &response_state,
            created_at,
        )?;
    }

    if table_exists(&connection, "capture_interceptor_runs")? {
        let runs = {
            let mut statement =
                connection.prepare("SELECT id, modifications FROM capture_interceptor_runs")?;
            statement
                .query_map([], |row| {
                    Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };
        for (id, modifications) in runs {
            let mut modifications: Vec<serde_json::Value> = serde_json::from_str(&modifications)?;
            modifications.retain(|item| {
                !matches!(
                    item.get("kind").and_then(serde_json::Value::as_str),
                    Some("snapshot" | "body_replace_file")
                )
            });
            connection.execute(
                "UPDATE capture_interceptor_runs SET modifications=?2 WHERE id=?1",
                params![id, serde_json::to_string(&modifications)?],
            )?;
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn migrate_final_body(
    connection: &Connection,
    assets: &AssetStore,
    blob_directory: &Path,
    session_id: u64,
    capture_id: u64,
    side: &str,
    column: &str,
    headers: &str,
    current: &str,
    created_at: u64,
) -> Result<()> {
    let modified = blob_directory.join(format!("{capture_id}-{side}.body.modified"));
    let existing = serde_json::from_str::<CaptureModifications>(current).ok();
    let final_body = if modified.exists() {
        if let Some(BodySourceType::Asset { asset_id }) =
            existing.as_ref().map(|state| &state.final_body)
            && assets.get(asset_id)?.is_some()
        {
            BodySourceType::Asset {
                asset_id: asset_id.clone(),
            }
        } else {
            let headers: HeaderValues = serde_json::from_str(headers)?;
            let content_type = headers
                .iter()
                .find(|(name, _)| name.eq_ignore_ascii_case("content-type"))
                .and_then(|(_, values)| values.first())
                .cloned()
                .unwrap_or_default();
            let filename = modified
                .file_name()
                .expect("modified body path has a filename")
                .to_string_lossy();
            let metadata = assets.import_file_with_suffix(
                &format!("mig/{session_id}/{filename}"),
                &modified,
                content_type,
                created_at,
            )?;
            BodySourceType::Asset {
                asset_id: metadata.id,
            }
        }
    } else {
        existing
            .map(|state| state.final_body)
            .unwrap_or(BodySourceType::Original)
    };
    let sql = format!("UPDATE captures SET {column}=?2 WHERE id=?1");
    connection.execute(
        &sql,
        params![
            capture_id as i64,
            serde_json::to_string(&CaptureModifications { final_body })?
        ],
    )?;
    if modified.exists() {
        fs::remove_file(modified)?;
    }
    Ok(())
}

fn session_directories(root: &Path) -> [std::path::PathBuf; 2] {
    [root.join("sessions"), root.join("sessions_archived")]
}

fn migrate_interceptor_runs_v1(connection: &mut Connection) -> Result<()> {
    let transaction = connection.transaction()?;
    transaction.execute_batch(
        "DROP INDEX IF EXISTS capture_interceptor_runs_capture;
         CREATE TABLE capture_interceptor_runs_v2 (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            capture_id INTEGER NOT NULL,
            phase TEXT NOT NULL CHECK (phase IN ('request', 'response')),
            position INTEGER NOT NULL,
            origin TEXT NOT NULL CHECK (origin IN ('saved', 'temporary')),
            name TEXT NOT NULL,
            script_hash TEXT NOT NULL,
            modifications TEXT NOT NULL DEFAULT '[]',
            error TEXT,
            completed INTEGER NOT NULL DEFAULT 1,
            created_at INTEGER NOT NULL,
            FOREIGN KEY (capture_id) REFERENCES captures(id) ON DELETE CASCADE,
            FOREIGN KEY (script_hash) REFERENCES interceptor_script_contents(hash)
         );
         INSERT INTO capture_interceptor_runs_v2 (
            capture_id, phase, position, origin, name, script_hash,
            modifications, error, completed, created_at
         )
         SELECT runs.capture_id, runs.phase, runs.position, 'saved', runs.name,
                runs.script_hash, runs.modifications, runs.error, 1, captures.created_at
         FROM capture_interceptor_runs AS runs
         JOIN captures ON captures.id = runs.capture_id
         ORDER BY runs.capture_id, runs.phase, runs.position;
         DROP TABLE capture_interceptor_runs;
         ALTER TABLE capture_interceptor_runs_v2 RENAME TO capture_interceptor_runs;
         CREATE INDEX capture_interceptor_runs_capture
         ON capture_interceptor_runs(capture_id, phase, id);",
    )?;
    transaction.commit()?;
    Ok(())
}

fn table_exists(connection: &Connection, table: &str) -> Result<bool> {
    Ok(connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
        [table],
        |row| row.get(0),
    )?)
}

fn table_has_column(connection: &Connection, table: &str, column: &str) -> Result<bool> {
    let mut statement = connection.prepare(&format!("PRAGMA table_info({table})"))?;
    let names = statement
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(names.iter().any(|name| name == column))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use rusqlite::{Connection, params};
    use tempfile::tempdir;

    use super::{CURRENT_WORKSPACE_SCHEMA_VERSION, WorkspaceSchema, migrate_workspace};
    use crate::{
        model::{BodySourceType, CaptureModifications},
        workspace::{read_json, write_json_atomic},
    };

    #[test]
    fn new_workspace_is_labeled_with_current_schema_version() {
        let root = tempdir().unwrap();
        migrate_workspace(root.path()).unwrap();
        let schema: WorkspaceSchema =
            read_json(&root.path().join("workspace_schema.json")).unwrap();
        assert_eq!(schema.version, CURRENT_WORKSPACE_SCHEMA_VERSION);
    }

    #[test]
    fn failed_migration_does_not_advance_workspace_version() {
        let root = tempdir().unwrap();
        let session = root.path().join("sessions/1");
        fs::create_dir_all(&session).unwrap();
        fs::write(session.join("captures.db"), b"not a sqlite database").unwrap();

        assert!(migrate_workspace(root.path()).is_err());
        assert!(!root.path().join("workspace_schema.json").exists());
    }

    #[test]
    fn newer_workspace_version_is_rejected() {
        let root = tempdir().unwrap();
        write_json_atomic(
            &root.path().join("workspace_schema.json"),
            &WorkspaceSchema {
                version: CURRENT_WORKSPACE_SCHEMA_VERSION + 1,
            },
        )
        .unwrap();

        let error = migrate_workspace(root.path()).unwrap_err();
        assert!(error.to_string().contains("newer than supported"));
    }

    #[test]
    fn v2_migration_enables_wal_for_existing_capture_databases() {
        let root = tempdir().unwrap();
        let session = root.path().join("sessions/1");
        fs::create_dir_all(&session).unwrap();
        let database = session.join("captures.db");
        Connection::open(&database)
            .unwrap()
            .execute_batch(
                "CREATE TABLE captures (
                    id INTEGER PRIMARY KEY,
                    req_headers TEXT NOT NULL DEFAULT '{}',
                    resp_headers TEXT,
                    req_modifications TEXT NOT NULL DEFAULT '[]',
                    resp_modifications TEXT NOT NULL DEFAULT '[]',
                    created_at INTEGER NOT NULL DEFAULT 0
                );",
            )
            .unwrap();
        write_json_atomic(
            &root.path().join("workspace_schema.json"),
            &WorkspaceSchema { version: 1 },
        )
        .unwrap();

        migrate_workspace(root.path()).unwrap();

        let connection = Connection::open(database).unwrap();
        let journal_mode: String = connection
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))
            .unwrap();
        assert_eq!(journal_mode, "wal");
        let schema: WorkspaceSchema =
            read_json(&root.path().join("workspace_schema.json")).unwrap();
        assert_eq!(schema.version, CURRENT_WORKSPACE_SCHEMA_VERSION);
    }

    #[test]
    fn v3_migration_moves_modified_bodies_to_suffixed_assets_and_cleans_history() {
        let root = tempdir().unwrap();
        let session = root.path().join("sessions/7");
        let blob = session.join("blob");
        fs::create_dir_all(&blob).unwrap();
        fs::create_dir_all(root.path().join("assets/mig/7")).unwrap();
        fs::write(
            root.path().join("assets/mig/7/1-request.body.modified"),
            b"occupied",
        )
        .unwrap();
        fs::write(blob.join("1-request.body.modified"), b"final request").unwrap();
        fs::write(blob.join("1-response.body.modified"), b"final response").unwrap();

        let database = session.join("captures.db");
        let connection = Connection::open(&database).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE captures (
                    id INTEGER PRIMARY KEY,
                    req_headers TEXT NOT NULL,
                    resp_headers TEXT,
                    req_modifications TEXT NOT NULL DEFAULT '[]',
                    resp_modifications TEXT NOT NULL DEFAULT '[]',
                    created_at INTEGER NOT NULL
                );
                CREATE TABLE capture_interceptor_runs (
                    id INTEGER PRIMARY KEY,
                    modifications TEXT NOT NULL
                );",
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO captures (
                    id, req_headers, resp_headers, req_modifications,
                    resp_modifications, created_at
                 ) VALUES (1, ?1, ?2, '[]', '[]', 123)",
                params![
                    r#"{"content-type":["text/plain"]}"#,
                    r#"{"content-type":["application/json"]}"#
                ],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO capture_interceptor_runs (id, modifications) VALUES (1, ?1)",
                [r#"[{"kind":"snapshot","headers":{}},{"kind":"body_replace_file","path":"x"},{"kind":"method_set","method":"POST"}]"#],
            )
            .unwrap();
        drop(connection);
        write_json_atomic(
            &root.path().join("workspace_schema.json"),
            &WorkspaceSchema { version: 2 },
        )
        .unwrap();

        migrate_workspace(root.path()).unwrap();

        let connection = Connection::open(database).unwrap();
        let (request, response): (String, String) = connection
            .query_row(
                "SELECT req_modifications, resp_modifications FROM captures WHERE id=1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        let request: CaptureModifications = serde_json::from_str(&request).unwrap();
        let response: CaptureModifications = serde_json::from_str(&response).unwrap();
        assert_eq!(
            request.final_body,
            BodySourceType::Asset {
                asset_id: "mig/7/1-request.body.modified-1".into()
            }
        );
        assert_eq!(
            response.final_body,
            BodySourceType::Asset {
                asset_id: "mig/7/1-response.body.modified".into()
            }
        );
        assert_eq!(
            fs::read(root.path().join("assets/mig/7/1-request.body.modified-1")).unwrap(),
            b"final request"
        );
        assert!(!blob.join("1-request.body.modified").exists());
        assert!(!blob.join("1-response.body.modified").exists());
        let history: String = connection
            .query_row(
                "SELECT modifications FROM capture_interceptor_runs WHERE id=1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(history, r#"[{"kind":"method_set","method":"POST"}]"#);
    }
}
