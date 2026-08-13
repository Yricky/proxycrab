use std::{
    path::Path,
    sync::{Arc, Mutex, MutexGuard},
    time::Duration,
};

use anyhow::{Context, Result, anyhow, bail};
use rusqlite::{Connection, OptionalExtension, params, params_from_iter};
use serde::{Deserialize, Serialize};

use crate::workspace::now_millis;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BypassOutcome {
    InProgress,
    Success,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BypassEntry {
    pub id: u64,
    pub created_at: u64,
    pub updated_at: u64,
    pub source: String,
    pub method: String,
    pub uri: String,
    pub version: String,
    pub reason: String,
    pub outcome: BypassOutcome,
    pub response_status: Option<u16>,
    pub error: Option<String>,
    pub upload_bytes: Option<u64>,
    pub download_bytes: Option<u64>,
}

#[derive(Clone)]
pub struct BypassStore {
    connection: Arc<Mutex<Connection>>,
}

impl BypassStore {
    pub fn open(workspace: &Path) -> Result<Self> {
        let connection = Connection::open(workspace.join("bypass.db"))?;
        connection.busy_timeout(Duration::from_secs(5))?;
        connection.execute_batch(
            "PRAGMA foreign_keys=ON;
             PRAGMA synchronous=NORMAL;",
        )?;
        let store = Self {
            connection: Arc::new(Mutex::new(connection)),
        };
        let connection = store.connection()?;
        connection.execute_batch(
            "PRAGMA journal_mode=WAL;
             CREATE TABLE IF NOT EXISTS bypass_entries (
               id INTEGER PRIMARY KEY AUTOINCREMENT,
               created_at INTEGER NOT NULL,
               updated_at INTEGER NOT NULL,
               source TEXT NOT NULL,
               method TEXT NOT NULL,
               uri TEXT NOT NULL,
               version TEXT NOT NULL,
               reason TEXT NOT NULL,
               outcome TEXT NOT NULL,
               response_status INTEGER,
               error TEXT,
               upload_bytes INTEGER,
               download_bytes INTEGER
             );
             CREATE INDEX IF NOT EXISTS bypass_entries_updated
             ON bypass_entries(updated_at DESC, id DESC);",
        )?;
        drop(connection);
        Ok(store)
    }

    fn connection(&self) -> Result<MutexGuard<'_, Connection>> {
        self.connection
            .lock()
            .map_err(|_| anyhow!("bypass database connection lock poisoned"))
    }

    pub fn begin(
        &self,
        source: &str,
        method: &str,
        uri: &str,
        version: &str,
        reason: &str,
    ) -> Result<u64> {
        let now = now_millis();
        let connection = self.connection()?;
        connection.execute(
            "INSERT INTO bypass_entries (
               created_at, updated_at, source, method, uri, version, reason, outcome
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'in_progress')",
            params![now as i64, now as i64, source, method, uri, version, reason],
        )?;
        Ok(connection.last_insert_rowid() as u64)
    }

    pub fn complete(
        &self,
        id: u64,
        response_status: Option<u16>,
        upload_bytes: Option<u64>,
        download_bytes: Option<u64>,
    ) -> Result<()> {
        self.finish(
            id,
            "success",
            response_status,
            None,
            upload_bytes,
            download_bytes,
        )
    }

    pub fn fail(
        &self,
        id: u64,
        error: &str,
        upload_bytes: Option<u64>,
        download_bytes: Option<u64>,
    ) -> Result<()> {
        self.finish(
            id,
            "failed",
            None,
            Some(error),
            upload_bytes,
            download_bytes,
        )
    }

    fn finish(
        &self,
        id: u64,
        outcome: &str,
        response_status: Option<u16>,
        error: Option<&str>,
        upload_bytes: Option<u64>,
        download_bytes: Option<u64>,
    ) -> Result<()> {
        let connection = self.connection()?;
        let changed = connection.execute(
            "UPDATE bypass_entries
             SET updated_at=?2, outcome=?3, response_status=?4, error=?5,
                 upload_bytes=?6, download_bytes=?7
             WHERE id=?1",
            params![
                id as i64,
                now_millis() as i64,
                outcome,
                response_status.map(i64::from),
                error,
                upload_bytes.map(|value| value as i64),
                download_bytes.map(|value| value as i64),
            ],
        )?;
        if changed == 0 {
            bail!("bypass entry {id} not found");
        }
        Ok(())
    }

    pub fn list(&self, limit: usize, before_id: Option<u64>) -> Result<Vec<BypassEntry>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT id, created_at, updated_at, source, method, uri, version, reason,
                    outcome, response_status, error, upload_bytes, download_bytes
             FROM bypass_entries
             WHERE (?1 IS NULL OR id < ?1)
             ORDER BY id DESC
             LIMIT ?2",
        )?;
        let rows = statement.query_map(
            params![before_id.map(|value| value as i64), limit as i64],
            map_entry,
        )?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .context("failed to read bypass entries")
    }

    pub fn delete(&self, id: u64, started_at: Option<u64>) -> Result<()> {
        self.delete_many(&[id], started_at).map(|_| ())
    }

    pub fn delete_many(&self, ids: &[u64], started_at: Option<u64>) -> Result<usize> {
        if ids.is_empty() {
            return Ok(0);
        }
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let placeholders = (1..=ids.len())
            .map(|index| format!("?{index}"))
            .collect::<Vec<_>>()
            .join(",");
        let started_at_parameter = ids.len() + 1;
        let active: Option<i64> = transaction
            .query_row(
                &format!(
                    "SELECT id FROM bypass_entries
                     WHERE id IN ({placeholders}) AND outcome='in_progress'
                       AND ?{started_at_parameter} IS NOT NULL
                       AND created_at>=?{started_at_parameter}
                     LIMIT 1"
                ),
                params_from_iter(
                    ids.iter()
                        .map(|id| Some(*id as i64))
                        .chain(std::iter::once(started_at.map(|value| value as i64))),
                ),
                |row| row.get(0),
            )
            .optional()?;
        if let Some(id) = active {
            bail!("bypass entry {id} is still in progress");
        }
        let deleted = transaction.execute(
            &format!("DELETE FROM bypass_entries WHERE id IN ({placeholders})"),
            params_from_iter(ids.iter().map(|id| *id as i64)),
        )?;
        transaction.commit()?;
        Ok(deleted)
    }

    pub fn clear_deletable(&self, started_at: Option<u64>) -> Result<usize> {
        let connection = self.connection()?;
        Ok(connection.execute(
            "DELETE FROM bypass_entries
             WHERE outcome != 'in_progress' OR ?1 IS NULL OR created_at<?1",
            [started_at.map(|value| value as i64)],
        )?)
    }

    pub fn mark_in_progress_range_as_shutdown(&self, min_id: u64, max_id: u64) -> Result<()> {
        let connection = self.connection()?;
        connection.execute(
            "UPDATE bypass_entries
             SET updated_at=?1, outcome='failed', error='proxy_shutdown'
             WHERE id>=?2 AND id<=?3 AND outcome='in_progress'",
            params![now_millis() as i64, min_id as i64, max_id as i64],
        )?;
        Ok(())
    }
}

fn map_entry(row: &rusqlite::Row<'_>) -> rusqlite::Result<BypassEntry> {
    let outcome: String = row.get(8)?;
    Ok(BypassEntry {
        id: row.get::<_, i64>(0)? as u64,
        created_at: row.get::<_, i64>(1)? as u64,
        updated_at: row.get::<_, i64>(2)? as u64,
        source: row.get(3)?,
        method: row.get(4)?,
        uri: row.get(5)?,
        version: row.get(6)?,
        reason: row.get(7)?,
        outcome: match outcome.as_str() {
            "success" => BypassOutcome::Success,
            "failed" => BypassOutcome::Failed,
            _ => BypassOutcome::InProgress,
        },
        response_status: row.get::<_, Option<i64>>(9)?.map(|value| value as u16),
        error: row.get(10)?,
        upload_bytes: row.get::<_, Option<i64>>(11)?.map(|value| value as u64),
        download_bytes: row.get::<_, Option<i64>>(12)?.map(|value| value as u64),
    })
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::{BypassOutcome, BypassStore};

    #[test]
    fn clones_share_one_connection() {
        let root = tempdir().unwrap();
        let store = BypassStore::open(root.path()).unwrap();

        assert!(std::sync::Arc::ptr_eq(
            &store.connection,
            &store.clone().connection,
        ));
    }

    #[test]
    fn lifecycle_and_deletion_rules() {
        let root = tempdir().unwrap();
        let store = BypassStore::open(root.path()).unwrap();
        let pending = store
            .begin(
                "127.0.0.1:1",
                "CONNECT",
                "example.com:443",
                "HTTP/1.1",
                "nil",
            )
            .unwrap();
        assert!(store.delete(pending, Some(0)).is_err());
        store.complete(pending, None, Some(12), Some(34)).unwrap();
        let rows = store.list(10, None).unwrap();
        assert_eq!(rows[0].outcome, BypassOutcome::Success);
        assert_eq!(rows[0].upload_bytes, Some(12));
        assert_eq!(store.clear_deletable(Some(0)).unwrap(), 1);
        assert!(store.list(10, None).unwrap().is_empty());
    }

    #[test]
    fn reopening_preserves_in_progress_outcome() {
        let root = tempdir().unwrap();
        let store = BypassStore::open(root.path()).unwrap();
        store
            .begin(
                "127.0.0.1:1",
                "GET",
                "http://example.com",
                "HTTP/1.1",
                "no_active_session",
            )
            .unwrap();
        drop(store);

        let reopened = BypassStore::open(root.path()).unwrap();
        assert_eq!(
            reopened.list(10, None).unwrap()[0].outcome,
            BypassOutcome::InProgress
        );
    }

    #[test]
    fn stale_in_progress_entries_are_deletable() {
        let root = tempdir().unwrap();
        let store = BypassStore::open(root.path()).unwrap();
        let stale = store
            .begin(
                "127.0.0.1:1",
                "GET",
                "http://stale.example.com",
                "HTTP/1.1",
                "no_active_session",
            )
            .unwrap();
        let created_at = store.list(10, None).unwrap()[0].created_at;

        assert!(store.delete(stale, Some(created_at)).is_err());
        assert_eq!(
            store.delete_many(&[stale], Some(created_at + 1)).unwrap(),
            1
        );

        let stopped = store
            .begin(
                "127.0.0.1:1",
                "GET",
                "http://stopped.example.com",
                "HTTP/1.1",
                "no_active_session",
            )
            .unwrap();
        store.delete(stopped, None).unwrap();
    }

    #[test]
    fn clear_deletable_retains_current_run_in_progress_entries() {
        let root = tempdir().unwrap();
        let store = BypassStore::open(root.path()).unwrap();
        let stale = store
            .begin("old", "GET", "http://old", "HTTP/1.1", "nil")
            .unwrap();
        let stale_created_at = store.list(10, None).unwrap()[0].created_at;
        std::thread::sleep(std::time::Duration::from_millis(2));
        let completed = store
            .begin("done", "GET", "http://done", "HTTP/1.1", "nil")
            .unwrap();
        store.complete(completed, None, None, None).unwrap();
        let current = store
            .begin("new", "GET", "http://new", "HTTP/1.1", "nil")
            .unwrap();

        assert_eq!(
            store.clear_deletable(Some(stale_created_at + 1)).unwrap(),
            2
        );
        let rows = store.list(10, None).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, current);
        assert_ne!(rows[0].id, stale);
    }

    #[test]
    fn generation_shutdown_only_finalizes_ids_inside_its_range() {
        let root = tempdir().unwrap();
        let store = BypassStore::open(root.path()).unwrap();
        let stale = store
            .begin("old", "GET", "http://old", "HTTP/1.1", "nil")
            .unwrap();
        let generation = store
            .begin("current", "GET", "http://current", "HTTP/1.1", "nil")
            .unwrap();
        let later = store
            .begin("later", "GET", "http://later", "HTTP/1.1", "nil")
            .unwrap();

        store
            .mark_in_progress_range_as_shutdown(generation, generation)
            .unwrap();
        let rows = store.list(10, None).unwrap();
        assert_eq!(
            rows.iter().find(|row| row.id == stale).unwrap().outcome,
            BypassOutcome::InProgress
        );
        assert_eq!(
            rows.iter()
                .find(|row| row.id == generation)
                .unwrap()
                .outcome,
            BypassOutcome::Failed
        );
        assert_eq!(
            rows.iter().find(|row| row.id == later).unwrap().outcome,
            BypassOutcome::InProgress
        );
    }
}
