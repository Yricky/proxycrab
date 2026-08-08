use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard},
    time::Duration,
};

use anyhow::{Result, anyhow};
use brotli::Decompressor;
use flate2::read::{GzDecoder, ZlibDecoder};
use rusqlite::{Connection, OptionalExtension, Row, params};
use tokio::io::AsyncWriteExt;

use crate::{
    model::{
        BodyPayload, CaptureDetail, CaptureError, CaptureOutcome, CaptureSummary, ErrorStage,
        HeaderValues, InterceptorExecution, InterceptorExecutionOrigin, InterceptorKind,
        InterceptorRun, Modification, RequestData, RequestTags, ResponseData,
    },
    workspace::now_millis,
};

pub(crate) const BODY_DETAIL_LIMIT: u64 = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodySide {
    Request,
    Response,
}

#[derive(Debug, Clone)]
pub enum BodySourceData {
    File(PathBuf),
    Bytes(Vec<u8>),
}

#[derive(Debug, Clone)]
pub struct BodySource {
    pub data: BodySourceData,
    pub path: Option<String>,
    pub stored_size: u64,
    pub content_type: Option<String>,
    pub content_encodings: Vec<String>,
}

pub(crate) struct CaptureBodyWriter {
    store: CaptureStore,
    id: u64,
    writer: tokio::fs::File,
    path: PathBuf,
}

impl CaptureBodyWriter {
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
    pub(crate) async fn write_all(&mut self, bytes: &[u8]) -> Result<()> {
        self.writer.write_all(bytes).await?;
        Ok(())
    }

    pub(crate) async fn finish(mut self) -> Result<()> {
        self.writer.flush().await?;
        self.store.touch(self.id)
    }
}

#[derive(Clone)]
pub struct CaptureStore {
    session_id: u64,
    connection: Arc<Mutex<Connection>>,
    blob_directory: PathBuf,
}

impl CaptureStore {
    pub fn open(session_id: u64, session_directory: &Path) -> Result<Self> {
        let blob_directory = session_directory.join("blob");
        fs::create_dir_all(&blob_directory)?;
        let connection = Connection::open(session_directory.join("captures.db"))?;
        connection.busy_timeout(Duration::from_secs(5))?;
        connection.execute_batch(
            "PRAGMA foreign_keys = ON;
             PRAGMA synchronous = NORMAL;",
        )?;
        let store = Self {
            session_id,
            connection: Arc::new(Mutex::new(connection)),
            blob_directory,
        };
        store.initialize()?;
        Ok(store)
    }

    pub(crate) fn session_id(&self) -> u64 {
        self.session_id
    }

    pub fn begin(&self, source: &str, request: &RequestData, stage: &str) -> Result<u64> {
        let now = now_millis();
        let connection = self.connection()?;
        connection.execute(
            "INSERT INTO captures (
                source, method, uri, req_version, req_headers, req_tags,
                outcome, stage, created_at, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'in_progress', ?7, ?8, ?8)",
            params![
                source,
                request.method,
                request.uri,
                request.version,
                serde_json::to_string(&request.headers)?,
                serde_json::to_string(&request.tags)?,
                stage,
                now as i64,
            ],
        )?;
        Ok(connection.last_insert_rowid() as u64)
    }

    pub fn contains(&self, id: u64) -> Result<bool> {
        Ok(self.connection()?.query_row(
            "SELECT EXISTS(SELECT 1 FROM captures WHERE id=?1)",
            params![id as i64],
            |row| row.get(0),
        )?)
    }

    pub fn update_request(
        &self,
        id: u64,
        request: &RequestData,
        _modifications: &[Modification],
    ) -> Result<()> {
        self.connection()?.execute(
            "UPDATE captures SET
                method=?2, uri=?3, req_version=?4, req_headers=?5, req_tags=?6,
                stage='request', updated_at=MAX(updated_at + 1, ?7)
             WHERE id=?1",
            params![
                id as i64,
                request.method,
                request.uri,
                request.version,
                serde_json::to_string(&request.headers)?,
                serde_json::to_string(&request.tags)?,
                now_millis() as i64,
            ],
        )?;
        Ok(())
    }

    pub fn update_tags(&self, id: u64, tags: &RequestTags) -> Result<()> {
        self.connection()?.execute(
            "UPDATE captures SET req_tags=?2, updated_at=MAX(updated_at + 1, ?3) WHERE id=?1",
            params![id as i64, serde_json::to_string(tags)?, now_millis() as i64,],
        )?;
        Ok(())
    }

    pub fn update_response(&self, id: u64, response: &ResponseData) -> Result<()> {
        self.connection()?.execute(
            "UPDATE captures SET
                resp_status=?2, resp_version=?3, resp_headers=?4,
                stage='response', updated_at=MAX(updated_at + 1, ?5)
             WHERE id=?1",
            params![
                id as i64,
                response.status,
                response.version,
                serde_json::to_string(&response.headers)?,
                now_millis() as i64,
            ],
        )?;
        Ok(())
    }

    pub fn complete(
        &self,
        id: u64,
        response: &ResponseData,
        _modifications: &[Modification],
    ) -> Result<()> {
        self.connection()?.execute(
            "UPDATE captures SET
                resp_status=?2, resp_version=?3, resp_headers=?4,
                outcome='success', stage='completed',
                updated_at=MAX(updated_at + 1, ?5)
             WHERE id=?1",
            params![
                id as i64,
                response.status,
                response.version,
                serde_json::to_string(&response.headers)?,
                now_millis() as i64,
            ],
        )?;
        Ok(())
    }

    pub fn mitm_established(&self, id: u64) -> Result<()> {
        self.connection()?.execute(
            "UPDATE captures SET
                resp_status=200, resp_version='HTTP/1.1', resp_headers='{}',
                outcome='success', stage='tls_mitm',
                updated_at=MAX(updated_at + 1, ?2)
             WHERE id=?1",
            params![id as i64, now_millis() as i64],
        )?;
        Ok(())
    }

    pub fn fail(&self, id: u64, error: &CaptureError) -> Result<()> {
        self.connection()?.execute(
            "UPDATE captures SET outcome='failed', stage=?2, error_stage=?3,
                error_kind=?4, error_message=?5,
                updated_at=MAX(updated_at + 1, ?6) WHERE id=?1",
            params![
                id as i64,
                error_stage_name(error.stage),
                error_stage_name(error.stage),
                error.kind,
                error.message,
                now_millis() as i64,
            ],
        )?;
        Ok(())
    }

    pub fn note_error(&self, id: u64, error: &CaptureError) -> Result<()> {
        self.connection()?.execute(
            "UPDATE captures SET error_stage=?2, error_kind=?3,
                error_message=?4, updated_at=MAX(updated_at + 1, ?5) WHERE id=?1",
            params![
                id as i64,
                error_stage_name(error.stage),
                error.kind,
                error.message,
                now_millis() as i64,
            ],
        )?;
        Ok(())
    }

    pub fn tunneled(&self, id: u64) -> Result<()> {
        self.connection()?.execute(
            "UPDATE captures SET outcome='tunneled', stage='tunnel',
                updated_at=MAX(updated_at + 1, ?2) WHERE id=?1",
            params![id as i64, now_millis() as i64],
        )?;
        Ok(())
    }

    pub fn discard(&self, id: u64) -> Result<()> {
        self.connection()?
            .execute("DELETE FROM captures WHERE id=?1", params![id as i64])?;
        for side in [BodySide::Request, BodySide::Response] {
            for modified in [false, true] {
                let path = self.body_path(id, side, modified);
                if let Err(error) = fs::remove_file(path)
                    && error.kind() != std::io::ErrorKind::NotFound
                {
                    return Err(error.into());
                }
            }
        }
        Ok(())
    }

    pub fn save_body(&self, id: u64, side: BodySide, modified: bool, bytes: &[u8]) -> Result<()> {
        fs::write(self.body_path(id, side, modified), bytes)?;
        self.touch(id)
    }

    pub(crate) async fn create_body_writer(
        &self,
        id: u64,
        side: BodySide,
        modified: bool,
    ) -> Result<CaptureBodyWriter> {
        let path = self.body_path(id, side, modified);
        let file = tokio::fs::File::create(&path).await?;
        Ok(CaptureBodyWriter {
            store: self.clone(),
            id,
            writer: file,
            path,
        })
    }

    fn touch(&self, id: u64) -> Result<()> {
        self.connection()?.execute(
            "UPDATE captures SET updated_at=MAX(updated_at + 1, ?2) WHERE id=?1",
            params![id as i64, now_millis() as i64],
        )?;
        Ok(())
    }

    pub fn body_source(&self, id: u64, side: BodySide) -> Result<Option<BodySource>> {
        let row = self
            .connection()?
            .query_row(
                "SELECT req_headers, resp_status, resp_headers FROM captures WHERE id=?1",
                params![id as i64],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, Option<u16>>(1)?,
                        row.get::<_, Option<String>>(2)?,
                    ))
                },
            )
            .optional()?;
        let Some((request_headers, response_status, response_headers)) = row else {
            return Ok(None);
        };
        let headers = match side {
            BodySide::Request => serde_json::from_str::<HeaderValues>(&request_headers)?,
            BodySide::Response => {
                if response_status.is_none() {
                    return Ok(None);
                }
                serde_json::from_str::<HeaderValues>(response_headers.as_deref().unwrap_or("{}"))?
            }
        };
        let modified = self.body_path(id, side, true).exists();
        let path = self.body_path(id, side, modified);
        let metadata = match fs::metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        Ok(Some(BodySource {
            data: BodySourceData::File(path.clone()),
            path: Some(path.to_string_lossy().into_owned()),
            stored_size: metadata.len(),
            content_type: first_header(&headers, "content-type").map(str::to_owned),
            content_encodings: content_encodings(&headers),
        }))
    }

    pub fn begin_interceptor_run(&self, id: u64, run: &InterceptorRun) -> Result<u64> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        transaction.execute(
            "INSERT INTO interceptor_script_contents (hash, content)
             VALUES (?1, ?2)
             ON CONFLICT(hash) DO NOTHING",
            params![run.script_hash, run.content],
        )?;
        let stored: String = transaction.query_row(
            "SELECT content FROM interceptor_script_contents WHERE hash=?1",
            params![run.script_hash],
            |row| row.get(0),
        )?;
        if stored != run.content {
            return Err(anyhow!("interceptor script hash collision"));
        }
        transaction.execute(
            "INSERT INTO capture_interceptor_runs (
                capture_id, phase, position, origin, name, script_hash,
                modifications, error, completed, created_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                id as i64,
                interceptor_phase_name(run.phase),
                run.position as i64,
                interceptor_origin_name(run.origin),
                run.name,
                run.script_hash,
                serde_json::to_string(&run.modifications)?,
                run.error,
                run.completed,
                now_millis() as i64,
            ],
        )?;
        let execution_id = transaction.last_insert_rowid() as u64;
        transaction.execute(
            "UPDATE captures SET updated_at=MAX(updated_at + 1, ?2) WHERE id=?1",
            params![id as i64, now_millis() as i64],
        )?;
        transaction.commit()?;
        Ok(execution_id)
    }

    pub fn update_interceptor_run(
        &self,
        execution_id: u64,
        modifications: &[Modification],
        error: Option<&str>,
        completed: bool,
    ) -> Result<()> {
        let connection = self.connection()?;
        let capture_id = connection.query_row(
            "SELECT capture_id FROM capture_interceptor_runs WHERE id=?1",
            params![execution_id as i64],
            |row| row.get::<_, i64>(0),
        )?;
        connection.execute(
            "UPDATE capture_interceptor_runs
             SET modifications=?2, error=?3, completed=?4 WHERE id=?1",
            params![
                execution_id as i64,
                serde_json::to_string(modifications)?,
                error,
                completed,
            ],
        )?;
        connection.execute(
            "UPDATE captures SET updated_at=MAX(updated_at + 1, ?2) WHERE id=?1",
            params![capture_id, now_millis() as i64],
        )?;
        Ok(())
    }

    pub fn record_interceptor_run(&self, id: u64, run: &InterceptorRun) -> Result<()> {
        self.begin_interceptor_run(id, run).map(|_| ())
    }

    pub fn list(&self, limit: usize, after_id: Option<u64>) -> Result<Vec<CaptureSummary>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT id, source, method, uri, req_version, req_headers,
                    resp_status, resp_version, resp_headers, outcome, stage,
                    error_stage, error_kind, error_message, created_at, updated_at, req_tags
             FROM captures WHERE (?1 IS NULL OR id > ?1)
             ORDER BY id DESC LIMIT ?2",
        )?;
        let rows = statement
            .query_map(params![after_id.map(|id| id as i64), limit as i64], |row| {
                self.summary_from_row(row)
            })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn list_before(&self, limit: usize, before_id: Option<u64>) -> Result<Vec<CaptureSummary>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT id, source, method, uri, req_version, req_headers,
                    resp_status, resp_version, resp_headers, outcome, stage,
                    error_stage, error_kind, error_message, created_at, updated_at, req_tags
             FROM captures WHERE (?1 IS NULL OR id < ?1)
             ORDER BY id DESC LIMIT ?2",
        )?;
        let rows = statement.query_map(
            params![before_id.map(|id| id as i64), limit as i64],
            |row| self.summary_from_row(row),
        )?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn list_range(
        &self,
        limit: usize,
        min_id: Option<u64>,
        max_id: Option<u64>,
        ascending: bool,
    ) -> Result<Vec<CaptureSummary>> {
        let connection = self.connection()?;
        let order = if ascending { "ASC" } else { "DESC" };
        let sql = format!(
            "SELECT id, source, method, uri, req_version, req_headers,
                    resp_status, resp_version, resp_headers, outcome, stage,
                    error_stage, error_kind, error_message, created_at, updated_at, req_tags
             FROM captures
             WHERE (?1 IS NULL OR id > ?1) AND (?2 IS NULL OR id < ?2)
             ORDER BY id {order} LIMIT ?3"
        );
        let mut statement = connection.prepare(&sql)?;
        let rows = statement.query_map(
            params![
                min_id.map(|id| id as i64),
                max_id.map(|id| id as i64),
                limit as i64
            ],
            |row| self.summary_from_row(row),
        )?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn get_many(&self, ids: &[u64]) -> Result<Vec<CaptureSummary>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let placeholders = std::iter::repeat_n("?", ids.len())
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!(
            "SELECT id, source, method, uri, req_version, req_headers,
                    resp_status, resp_version, resp_headers, outcome, stage,
                    error_stage, error_kind, error_message, created_at, updated_at, req_tags
             FROM captures WHERE id IN ({placeholders})"
        );
        let connection = self.connection()?;
        let mut statement = connection.prepare(&sql)?;
        let values = ids.iter().map(|id| *id as i64).collect::<Vec<_>>();
        let rows = statement.query_map(rusqlite::params_from_iter(values), |row| {
            self.summary_from_row(row)
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn get(&self, id: u64) -> Result<Option<CaptureDetail>> {
        let summary = {
            let connection = self.connection()?;
            let mut statement = connection.prepare(
                "SELECT id, source, method, uri, req_version, req_headers,
                        resp_status, resp_version, resp_headers, outcome, stage,
                        error_stage, error_kind, error_message, created_at, updated_at, req_tags
                 FROM captures WHERE id=?1",
            )?;
            statement
                .query_row(params![id as i64], |row| self.summary_from_row(row))
                .optional()?
        };
        let Some(summary) = summary else {
            return Ok(None);
        };
        let request_body = self
            .read_body(
                id,
                BodySide::Request,
                &summary.request.headers,
                self.body_path(id, BodySide::Request, true).exists(),
            )
            .unwrap_or(BodyPayload::Empty);
        let response_body = self
            .read_body(
                id,
                BodySide::Response,
                summary
                    .response
                    .as_ref()
                    .map(|response| &response.headers)
                    .unwrap_or(&HeaderValues::new()),
                self.body_path(id, BodySide::Response, true).exists(),
            )
            .unwrap_or(BodyPayload::Empty);
        Ok(Some(CaptureDetail {
            summary,
            request_body,
            response_body,
            request_interceptors: self
                .interceptor_executions(id, InterceptorKind::Request)
                .unwrap_or_default(),
            response_interceptors: self
                .interceptor_executions(id, InterceptorKind::Response)
                .unwrap_or_default(),
        }))
    }

    pub fn mark_in_progress_as_shutdown(&self) -> Result<usize> {
        Ok(self.connection()?.execute(
            "UPDATE captures SET outcome='failed', stage='connect',
                error_stage='connect', error_kind='proxy_shutdown',
                error_message='proxy stopped before the request completed',
                updated_at=MAX(updated_at + 1, ?1)
             WHERE outcome='in_progress'",
            params![now_millis() as i64],
        )?)
    }

    pub fn mark_in_progress_through_as_shutdown(&self, max_id: u64) -> Result<usize> {
        Ok(self.connection()?.execute(
            "UPDATE captures SET outcome='failed', stage='connect',
                error_stage='connect', error_kind='proxy_shutdown',
                error_message='proxy stopped before the request completed',
                updated_at=MAX(updated_at + 1, ?1)
             WHERE id<=?2 AND outcome='in_progress'",
            params![now_millis() as i64, max_id as i64],
        )?)
    }

    fn initialize(&self) -> Result<()> {
        let connection = self.connection()?;
        connection.execute_batch(
            "PRAGMA journal_mode = WAL;
            CREATE TABLE IF NOT EXISTS captures (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                source TEXT NOT NULL,
                method TEXT NOT NULL,
                uri TEXT NOT NULL,
                req_version TEXT NOT NULL,
                req_headers TEXT NOT NULL,
                resp_status INTEGER,
                resp_version TEXT,
                resp_headers TEXT,
                outcome TEXT NOT NULL,
                stage TEXT NOT NULL,
                error_stage TEXT,
                error_kind TEXT,
                error_message TEXT,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                req_tags TEXT NOT NULL DEFAULT '{}',
                req_modifications TEXT NOT NULL DEFAULT '[]',
                resp_modifications TEXT NOT NULL DEFAULT '[]'
            );
            CREATE INDEX IF NOT EXISTS captures_created_at ON captures(created_at);
            CREATE TABLE IF NOT EXISTS interceptor_script_contents (
                hash TEXT PRIMARY KEY,
                content TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS capture_interceptor_runs (
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
            );",
        )?;
        connection.execute(
            "CREATE INDEX IF NOT EXISTS capture_interceptor_runs_capture
             ON capture_interceptor_runs(capture_id, phase, id)",
            [],
        )?;
        Ok(())
    }

    fn connection(&self) -> Result<MutexGuard<'_, Connection>> {
        self.connection
            .lock()
            .map_err(|_| anyhow!("capture database connection lock poisoned"))
    }

    fn interceptor_executions(
        &self,
        id: u64,
        phase: InterceptorKind,
    ) -> Result<Vec<InterceptorExecution>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT runs.id, runs.origin, runs.completed, runs.position, runs.name,
                    runs.script_hash, contents.content, runs.modifications, runs.error
             FROM capture_interceptor_runs AS runs
             JOIN interceptor_script_contents AS contents
               ON contents.hash = runs.script_hash
             WHERE runs.capture_id=?1 AND runs.phase=?2
             ORDER BY runs.id ASC",
        )?;
        let executions =
            statement.query_map(params![id as i64, interceptor_phase_name(phase)], |row| {
                let modifications: String = row.get(7)?;
                Ok(InterceptorExecution {
                    execution_id: row.get::<_, i64>(0)? as u64,
                    origin: parse_interceptor_origin(&row.get::<_, String>(1)?),
                    completed: row.get(2)?,
                    phase,
                    position: row.get::<_, i64>(3)? as usize,
                    name: row.get(4)?,
                    script_hash: row.get(5)?,
                    content: row.get(6)?,
                    modifications: serde_json::from_str(&modifications).unwrap_or_default(),
                    error: row.get(8)?,
                })
            })?;
        executions
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    #[cfg(test)]
    fn interceptor_source_count(&self) -> Result<usize> {
        Ok(self.connection()?.query_row(
            "SELECT COUNT(*) FROM interceptor_script_contents",
            [],
            |row| row.get::<_, i64>(0),
        )? as usize)
    }

    fn summary_from_row(&self, row: &Row<'_>) -> rusqlite::Result<CaptureSummary> {
        let request_headers: String = row.get(5)?;
        let response_status: Option<u16> = row.get(6)?;
        let response_version: Option<String> = row.get(7)?;
        let response_headers: Option<String> = row.get(8)?;
        let outcome: String = row.get(9)?;
        let error_stage: Option<String> = row.get(11)?;
        let error_kind: Option<String> = row.get(12)?;
        let error_message: Option<String> = row.get(13)?;
        let request_tags: String = row.get(16)?;

        Ok(CaptureSummary {
            id: row.get::<_, i64>(0)? as u64,
            session_id: self.session_id,
            source: row.get(1)?,
            request: RequestData {
                method: row.get(2)?,
                uri: row.get(3)?,
                version: row.get(4)?,
                headers: serde_json::from_str(&request_headers).unwrap_or_default(),
                tags: serde_json::from_str(&request_tags).unwrap_or_default(),
            },
            response: response_status.map(|status| ResponseData {
                status,
                version: response_version.unwrap_or_default(),
                headers: response_headers
                    .and_then(|headers| serde_json::from_str(&headers).ok())
                    .unwrap_or_default(),
            }),
            outcome: match outcome.as_str() {
                "success" => CaptureOutcome::Success,
                "failed" => CaptureOutcome::Failed,
                "tunneled" => CaptureOutcome::Tunneled,
                _ => CaptureOutcome::InProgress,
            },
            stage: row.get(10)?,
            error: match (error_stage, error_kind, error_message) {
                (Some(stage), Some(kind), Some(message)) => Some(CaptureError {
                    stage: parse_error_stage(&stage),
                    kind,
                    message,
                }),
                _ => None,
            },
            created_at: row.get::<_, i64>(14)? as u64,
            updated_at: row.get::<_, i64>(15)? as u64,
        })
    }

    fn body_path(&self, id: u64, side: BodySide, modified: bool) -> PathBuf {
        let side = match side {
            BodySide::Request => "request",
            BodySide::Response => "response",
        };
        let suffix = if modified { ".modified" } else { "" };
        self.blob_directory
            .join(format!("{id}-{side}.body{suffix}"))
    }

    fn read_body(
        &self,
        id: u64,
        side: BodySide,
        headers: &HeaderValues,
        modified: bool,
    ) -> Result<BodyPayload> {
        let path = self.body_path(id, side, modified);
        let metadata = match fs::metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(BodyPayload::Empty);
            }
            Err(error) => return Err(error.into()),
        };
        if metadata.len() > BODY_DETAIL_LIMIT {
            return Ok(BodyPayload::Large {
                size: metadata.len(),
                path: Some(path.to_string_lossy().into_owned()),
            });
        }
        let mut bytes = fs::read(&path)?;
        let encodings = content_encodings(headers);
        if !encodings.is_empty() {
            bytes = match decode_body(&bytes, &encodings.join(","), BODY_DETAIL_LIMIT) {
                Ok(bytes) => bytes,
                Err(DecodeBodyError::TooLarge) => {
                    return Ok(BodyPayload::Large {
                        size: metadata.len(),
                        path: Some(path.to_string_lossy().into_owned()),
                    });
                }
                Err(DecodeBodyError::Failed) => {
                    return Ok(BodyPayload::Binary {
                        size: metadata.len(),
                        path: Some(path.to_string_lossy().into_owned()),
                    });
                }
            };
        }
        Ok(body_payload(
            &bytes,
            headers,
            metadata.len(),
            Some(path.to_string_lossy().into_owned()),
        ))
    }
}

pub(crate) fn body_payload(
    bytes: &[u8],
    headers: &HeaderValues,
    stored_size: u64,
    path: Option<String>,
) -> BodyPayload {
    if bytes.len() as u64 > BODY_DETAIL_LIMIT {
        return BodyPayload::Large {
            size: stored_size,
            path,
        };
    }
    if bytes.is_empty() {
        return BodyPayload::Empty;
    }
    let content_type = first_header(headers, "content-type").unwrap_or_default();
    if content_type.contains("json")
        && let Ok(content) = serde_json::from_slice(bytes)
    {
        return BodyPayload::Json {
            content,
            size: stored_size,
            path,
        };
    }
    if is_textual(content_type)
        && let Ok(content) = String::from_utf8(bytes.to_vec())
    {
        return BodyPayload::Text {
            content,
            size: stored_size,
            path,
        };
    }
    BodyPayload::Binary {
        size: stored_size,
        path,
    }
}

fn first_header<'a>(headers: &'a HeaderValues, name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .and_then(|(_, values)| values.first())
        .map(String::as_str)
}

fn is_textual(content_type: &str) -> bool {
    content_type.starts_with("text/")
        || ["json", "xml", "javascript", "x-www-form-urlencoded"]
            .iter()
            .any(|kind| content_type.contains(kind))
}

fn content_encodings(headers: &HeaderValues) -> Vec<String> {
    headers
        .iter()
        .filter(|(key, _)| key.eq_ignore_ascii_case("content-encoding"))
        .flat_map(|(_, values)| values)
        .flat_map(|value| value.split(','))
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty() && value != "identity")
        .collect()
}

#[derive(Debug)]
enum DecodeBodyError {
    TooLarge,
    Failed,
}

fn decode_body(
    bytes: &[u8],
    encoding: &str,
    limit: u64,
) -> std::result::Result<Vec<u8>, DecodeBodyError> {
    let mut current = bytes.to_vec();
    let encodings = encoding
        .split(',')
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty() && value != "identity")
        .collect::<Vec<_>>();
    for encoding in encodings.iter().rev() {
        let mut decoded = Vec::new();
        let result = match encoding.as_str() {
            "gzip" => GzDecoder::new(current.as_slice())
                .take(limit + 1)
                .read_to_end(&mut decoded)
                .map_err(anyhow::Error::from),
            "br" => Decompressor::new(current.as_slice(), 4096)
                .take(limit + 1)
                .read_to_end(&mut decoded)
                .map_err(anyhow::Error::from),
            "deflate" => ZlibDecoder::new(current.as_slice())
                .take(limit + 1)
                .read_to_end(&mut decoded)
                .map_err(anyhow::Error::from),
            "zstd" => zstd::stream::read::Decoder::new(current.as_slice())
                .map_err(anyhow::Error::from)
                .and_then(|reader| {
                    reader
                        .take(limit + 1)
                        .read_to_end(&mut decoded)
                        .map_err(anyhow::Error::from)
                })
                .map(|_| 0),
            other => {
                let _ = other;
                return Err(DecodeBodyError::Failed);
            }
        };
        result.map_err(|_| DecodeBodyError::Failed)?;
        if decoded.len() as u64 > limit {
            return Err(DecodeBodyError::TooLarge);
        }
        current = decoded;
    }
    Ok(current)
}

fn error_stage_name(stage: ErrorStage) -> &'static str {
    match stage {
        ErrorStage::Connect => "connect",
        ErrorStage::TlsHandshake => "tls_handshake",
        ErrorStage::RequestBody => "request_body",
        ErrorStage::Interceptor => "interceptor",
        ErrorStage::Upstream => "upstream",
        ErrorStage::ResponseBody => "response_body",
    }
}

fn interceptor_phase_name(phase: InterceptorKind) -> &'static str {
    match phase {
        InterceptorKind::Request => "request",
        InterceptorKind::Response => "response",
    }
}

fn interceptor_origin_name(origin: InterceptorExecutionOrigin) -> &'static str {
    match origin {
        InterceptorExecutionOrigin::Saved => "saved",
        InterceptorExecutionOrigin::Temporary => "temporary",
    }
}

fn parse_interceptor_origin(origin: &str) -> InterceptorExecutionOrigin {
    match origin {
        "temporary" => InterceptorExecutionOrigin::Temporary,
        _ => InterceptorExecutionOrigin::Saved,
    }
}

fn parse_error_stage(stage: &str) -> ErrorStage {
    match stage {
        "tls_handshake" => ErrorStage::TlsHandshake,
        "request_body" => ErrorStage::RequestBody,
        "interceptor" => ErrorStage::Interceptor,
        "upstream" => ErrorStage::Upstream,
        "response_body" => ErrorStage::ResponseBody,
        _ => ErrorStage::Connect,
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use flate2::{Compression, write::GzEncoder};
    use rusqlite::{Connection, params};
    use tempfile::tempdir;

    use crate::model::{
        BodyPayload, CaptureError, CaptureOutcome, ErrorStage, HeaderValues,
        InterceptorExecutionOrigin, InterceptorKind, InterceptorRun, RequestData,
        script_content_hash,
    };

    use super::{BodySide, BodySourceData, CaptureStore, decode_body};

    fn request(uri: &str) -> RequestData {
        RequestData {
            method: "GET".into(),
            uri: uri.into(),
            version: "HTTP/1.1".into(),
            headers: HeaderValues::new(),
            tags: Default::default(),
        }
    }

    #[test]
    fn clones_share_one_connection() {
        let root = tempdir().unwrap();
        let store = CaptureStore::open(7, root.path()).unwrap();

        assert!(std::sync::Arc::ptr_eq(
            &store.connection,
            &store.clone().connection,
        ));
    }

    #[test]
    fn persists_diagnostic_failure() {
        let root = tempdir().unwrap();
        let store = CaptureStore::open(7, root.path()).unwrap();
        let request = RequestData {
            method: "CONNECT".into(),
            uri: "example.com:443".into(),
            version: "HTTP/1.1".into(),
            headers: HeaderValues::new(),
            tags: Default::default(),
        };
        let id = store.begin("127.0.0.1", &request, "connect").unwrap();
        store
            .fail(
                id,
                &CaptureError {
                    stage: ErrorStage::TlsHandshake,
                    kind: "client_rejected_ca".into(),
                    message: "unknown ca".into(),
                },
            )
            .unwrap();

        let detail = store.get(id).unwrap().unwrap();
        assert_eq!(detail.summary.outcome, CaptureOutcome::Failed);
        assert_eq!(detail.summary.error.unwrap().kind, "client_rejected_ca");
    }

    #[test]
    fn compressed_body_decode_is_output_bounded() {
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(&vec![b'a'; 4096]).unwrap();
        let compressed = encoder.finish().unwrap();
        assert!(decode_body(&compressed, "gzip", 128).is_err());
    }

    #[test]
    fn detail_embeds_at_most_64_kib_and_exposes_capture_path() {
        let root = tempdir().unwrap();
        let store = CaptureStore::open(7, root.path()).unwrap();
        let mut request = request("http://example.com/body");
        request.headers.insert(
            "content-type".into(),
            vec!["text/plain; charset=utf-8".into()],
        );

        let small_id = store.begin("127.0.0.1", &request, "request").unwrap();
        store
            .save_body(small_id, BodySide::Request, false, &vec![b'a'; 64 * 1024])
            .unwrap();
        let small = store.get(small_id).unwrap().unwrap();
        assert!(matches!(
            small.request_body,
            BodyPayload::Text { size: 65_536, ref path, .. }
                if path.as_deref().is_some_and(|path| path.ends_with("-request.body"))
        ));

        let large_id = store.begin("127.0.0.1", &request, "request").unwrap();
        store
            .save_body(
                large_id,
                BodySide::Request,
                false,
                &vec![b'a'; 64 * 1024 + 1],
            )
            .unwrap();
        let large = store.get(large_id).unwrap().unwrap();
        assert!(matches!(
            large.request_body,
            BodyPayload::Large { size: 65_537, ref path }
                if path.as_deref().is_some_and(|path| path.ends_with("-request.body"))
        ));
    }

    #[test]
    fn range_queries_use_exclusive_bounds_in_both_directions() {
        let root = tempdir().unwrap();
        let store = CaptureStore::open(7, root.path()).unwrap();
        let ids = (0..5)
            .map(|index| {
                store
                    .begin(
                        "127.0.0.1",
                        &request(&format!("http://example.com/{index}")),
                        "request",
                    )
                    .unwrap()
            })
            .collect::<Vec<_>>();

        let older = store
            .list_range(10, None, Some(ids[4]), false)
            .unwrap()
            .into_iter()
            .map(|item| item.id)
            .collect::<Vec<_>>();
        let newer = store
            .list_range(10, Some(ids[1]), None, true)
            .unwrap()
            .into_iter()
            .map(|item| item.id)
            .collect::<Vec<_>>();

        assert_eq!(older, vec![ids[3], ids[2], ids[1], ids[0]]);
        assert_eq!(newer, vec![ids[2], ids[3], ids[4]]);
    }

    #[test]
    fn body_writes_advance_the_log_version_monotonically() {
        let root = tempdir().unwrap();
        let store = CaptureStore::open(7, root.path()).unwrap();
        let id = store
            .begin("127.0.0.1", &request("http://example.com"), "request")
            .unwrap();
        let before = store.get(id).unwrap().unwrap().summary.updated_at;

        store
            .save_body(id, BodySide::Request, false, b"body")
            .unwrap();
        let after = store.get(id).unwrap().unwrap().summary.updated_at;

        assert!(after > before);
    }

    #[tokio::test]
    async fn body_writer_appends_without_a_size_limit() {
        let root = tempdir().unwrap();
        let store = CaptureStore::open(7, root.path()).unwrap();
        let id = store
            .begin("127.0.0.1", &request("http://example.com"), "request")
            .unwrap();
        store
            .update_response(
                id,
                &crate::model::ResponseData {
                    status: 200,
                    version: "HTTP/1.1".into(),
                    headers: HeaderValues::new(),
                },
            )
            .unwrap();
        let mut writer = store
            .create_body_writer(id, BodySide::Response, false)
            .await
            .unwrap();
        let chunk = vec![b'x'; 1024 * 1024];
        for _ in 0..65 {
            writer.write_all(&chunk).await.unwrap();
        }
        writer.finish().await.unwrap();

        let source = store.body_source(id, BodySide::Response).unwrap().unwrap();
        assert_eq!(source.stored_size, 65 * 1024 * 1024);
    }

    #[tokio::test]
    async fn body_writer_exposes_small_appends_before_finish() {
        let root = tempdir().unwrap();
        let store = CaptureStore::open(7, root.path()).unwrap();
        let mut request = request("http://example.com");
        request
            .headers
            .insert("content-type".into(), vec!["text/plain".into()]);
        let id = store.begin("127.0.0.1", &request, "request").unwrap();
        let mut writer = store
            .create_body_writer(id, BodySide::Request, false)
            .await
            .unwrap();

        writer.write_all(b"partial frame").await.unwrap();

        let source = store.body_source(id, BodySide::Request).unwrap().unwrap();
        assert_eq!(source.stored_size, 13);
        let BodySourceData::File(path) = source.data else {
            panic!("streaming capture body should be backed by a file");
        };
        assert_eq!(tokio::fs::read(path).await.unwrap(), b"partial frame");
        let detail = store.get(id).unwrap().unwrap();
        assert!(matches!(
            detail.request_body,
            BodyPayload::Text { ref content, size: 13, .. } if content == "partial frame"
        ));
    }

    #[test]
    fn interceptor_source_is_deduplicated_and_runs_are_ordered() {
        let root = tempdir().unwrap();
        let store = CaptureStore::open(7, root.path()).unwrap();
        let mut request = request("http://example.com");
        request.tags.insert("team".into(), "checkout".into());
        let id = store.begin("127.0.0.1", &request, "request").unwrap();
        let source = "req.headers:set(\"x-test\", \"1\")";
        let hash = script_content_hash(source);

        for (index, (origin, name)) in [
            (InterceptorExecutionOrigin::Saved, "first"),
            (InterceptorExecutionOrigin::Temporary, "临时脚本"),
            (InterceptorExecutionOrigin::Temporary, "临时脚本"),
        ]
        .into_iter()
        .enumerate()
        {
            store
                .record_interceptor_run(
                    id,
                    &InterceptorRun {
                        origin,
                        completed: true,
                        phase: InterceptorKind::Request,
                        position: 0,
                        name: name.into(),
                        script_hash: hash.clone(),
                        content: source.into(),
                        modifications: vec![crate::model::Modification::TagSet {
                            key: "run".into(),
                            value: index.to_string(),
                        }],
                        error: None,
                    },
                )
                .unwrap();
        }

        let detail = store.get(id).unwrap().unwrap();
        assert_eq!(detail.summary.request.tags["team"], "checkout");
        assert_eq!(detail.request_interceptors.len(), 3);
        assert_eq!(detail.request_interceptors[0].name, "first");
        assert_eq!(detail.request_interceptors[0].content, source);
        assert_eq!(
            detail.request_interceptors[0].origin,
            InterceptorExecutionOrigin::Saved
        );
        assert_eq!(
            detail.request_interceptors[1].origin,
            InterceptorExecutionOrigin::Temporary
        );
        assert_eq!(
            detail.request_interceptors[2].origin,
            InterceptorExecutionOrigin::Temporary
        );
        assert!(
            detail
                .request_interceptors
                .iter()
                .all(|run| run.execution_id > 0)
        );
        assert_eq!(store.interceptor_source_count().unwrap(), 1);
    }

    #[test]
    fn migrates_legacy_interceptor_history_and_adds_tags() {
        let root = tempdir().unwrap();
        let session = root.path().join("sessions/1");
        std::fs::create_dir_all(&session).unwrap();
        let database_path = session.join("captures.db");
        let connection = Connection::open(&database_path).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE captures (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    source TEXT NOT NULL, method TEXT NOT NULL, uri TEXT NOT NULL,
                    req_version TEXT NOT NULL, req_headers TEXT NOT NULL,
                    resp_status INTEGER, resp_version TEXT, resp_headers TEXT,
                    outcome TEXT NOT NULL, stage TEXT NOT NULL, error_stage TEXT,
                    error_kind TEXT, error_message TEXT, created_at INTEGER NOT NULL,
                    updated_at INTEGER NOT NULL, req_modifications TEXT NOT NULL DEFAULT '[]',
                    resp_modifications TEXT NOT NULL DEFAULT '[]'
                 );
                 CREATE TABLE interceptor_script_contents (
                    hash TEXT PRIMARY KEY, content TEXT NOT NULL
                 );
                 CREATE TABLE capture_interceptor_runs (
                    capture_id INTEGER NOT NULL, phase TEXT NOT NULL, position INTEGER NOT NULL,
                    name TEXT NOT NULL, script_hash TEXT NOT NULL,
                    modifications TEXT NOT NULL DEFAULT '[]', error TEXT,
                    PRIMARY KEY (capture_id, phase, position)
                 );",
            )
            .unwrap();
        let source = "req.headers:set('x-legacy', '1')";
        let hash = script_content_hash(source);
        connection
            .execute(
                "INSERT INTO captures (
                    source, method, uri, req_version, req_headers, outcome, stage,
                    created_at, updated_at
                 ) VALUES ('127.0.0.1', 'GET', 'http://example.com', 'HTTP/1.1',
                           '{}', 'success', 'completed', 1, 1)",
                [],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO interceptor_script_contents (hash, content) VALUES (?1, ?2)",
                params![hash, source],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO capture_interceptor_runs (
                    capture_id, phase, position, name, script_hash, modifications, error
                 ) VALUES (1, 'request', 0, 'legacy', ?1, '[]', NULL)",
                params![hash],
            )
            .unwrap();
        drop(connection);

        crate::migration::migrate_workspace(root.path()).unwrap();
        let store = CaptureStore::open(7, &session).unwrap();
        let detail = store.get(1).unwrap().unwrap();
        assert!(detail.summary.request.tags.is_empty());
        assert_eq!(detail.request_interceptors.len(), 1);
        assert_eq!(detail.request_interceptors[0].name, "legacy");
        assert_eq!(detail.request_interceptors[0].content, source);
        assert_eq!(
            detail.request_interceptors[0].origin,
            InterceptorExecutionOrigin::Saved
        );
        assert!(detail.request_interceptors[0].completed);
    }
}
