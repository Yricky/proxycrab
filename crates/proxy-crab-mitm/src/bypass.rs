use std::{
    path::Path,
    sync::{Arc, Mutex},
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
        connection.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA foreign_keys=ON;
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
        let store = Self {
            connection: Arc::new(Mutex::new(connection)),
        };
        store.mark_in_progress_as_shutdown()?;
        Ok(store)
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
        let connection = self
            .connection
            .lock()
            .map_err(|_| anyhow!("bypass database lock poisoned"))?;
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
        let connection = self
            .connection
            .lock()
            .map_err(|_| anyhow!("bypass database lock poisoned"))?;
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
        let connection = self
            .connection
            .lock()
            .map_err(|_| anyhow!("bypass database lock poisoned"))?;
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

    pub fn delete(&self, id: u64) -> Result<()> {
        self.delete_many(&[id]).map(|_| ())
    }

    pub fn delete_many(&self, ids: &[u64]) -> Result<usize> {
        if ids.is_empty() {
            return Ok(0);
        }
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| anyhow!("bypass database lock poisoned"))?;
        let transaction = connection.transaction()?;
        let placeholders = (1..=ids.len())
            .map(|index| format!("?{index}"))
            .collect::<Vec<_>>()
            .join(",");
        let in_progress: Option<i64> = transaction
            .query_row(
                &format!(
                    "SELECT id FROM bypass_entries
                     WHERE id IN ({placeholders}) AND outcome='in_progress'
                     LIMIT 1"
                ),
                params_from_iter(ids.iter().map(|id| *id as i64)),
                |row| row.get(0),
            )
            .optional()?;
        if let Some(id) = in_progress {
            bail!("bypass entry {id} is still in progress");
        }
        let deleted = transaction.execute(
            &format!("DELETE FROM bypass_entries WHERE id IN ({placeholders})"),
            params_from_iter(ids.iter().map(|id| *id as i64)),
        )?;
        transaction.commit()?;
        Ok(deleted)
    }

    pub fn clear_terminal(&self) -> Result<usize> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| anyhow!("bypass database lock poisoned"))?;
        Ok(connection.execute(
            "DELETE FROM bypass_entries WHERE outcome != 'in_progress'",
            [],
        )?)
    }

    pub fn mark_in_progress_as_shutdown(&self) -> Result<()> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| anyhow!("bypass database lock poisoned"))?;
        connection.execute(
            "UPDATE bypass_entries
             SET updated_at=?1, outcome='failed', error='proxy_shutdown'
             WHERE outcome='in_progress'",
            [now_millis() as i64],
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
        assert!(store.delete(pending).is_err());
        store.complete(pending, None, Some(12), Some(34)).unwrap();
        let rows = store.list(10, None).unwrap();
        assert_eq!(rows[0].outcome, BypassOutcome::Success);
        assert_eq!(rows[0].upload_bytes, Some(12));
        assert_eq!(store.clear_terminal().unwrap(), 1);
        assert!(store.list(10, None).unwrap().is_empty());
    }

    #[test]
    fn startup_marks_in_progress_as_failed() {
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
            BypassOutcome::Failed
        );
    }
}
