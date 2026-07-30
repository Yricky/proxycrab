use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{Result, anyhow};
use brotli::Decompressor;
use flate2::read::GzDecoder;
use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::{
    model::{
        BodyPayload, CaptureDetail, CaptureError, CaptureOutcome, CaptureSummary, ErrorStage,
        HeaderValues, InterceptorExecution, InterceptorKind, InterceptorRun, Modification,
        RequestData, ResponseData,
    },
    workspace::now_millis,
};

const BODY_DETAIL_LIMIT: u64 = 16 * 1024 * 1024;

#[derive(Debug, Clone, Copy)]
pub enum BodySide {
    Request,
    Response,
}

#[derive(Clone)]
pub struct CaptureStore {
    session_id: u64,
    database_path: PathBuf,
    blob_directory: PathBuf,
}

impl CaptureStore {
    pub fn open(session_id: u64, session_directory: &Path) -> Result<Self> {
        let blob_directory = session_directory.join("blob");
        fs::create_dir_all(&blob_directory)?;
        let store = Self {
            session_id,
            database_path: session_directory.join("captures.db"),
            blob_directory,
        };
        store.initialize()?;
        Ok(store)
    }

    pub fn begin(&self, source: &str, request: &RequestData, stage: &str) -> Result<u64> {
        let now = now_millis();
        let connection = self.connection()?;
        connection.execute(
            "INSERT INTO captures (
                source, method, uri, req_version, req_headers, outcome, stage, created_at, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, 'in_progress', ?6, ?7, ?7)",
            params![
                source,
                request.method,
                request.uri,
                request.version,
                serde_json::to_string(&request.headers)?,
                stage,
                now as i64,
            ],
        )?;
        Ok(connection.last_insert_rowid() as u64)
    }

    pub fn update_request(
        &self,
        id: u64,
        request: &RequestData,
        _modifications: &[Modification],
    ) -> Result<()> {
        self.connection()?.execute(
            "UPDATE captures SET
                method=?2, uri=?3, req_version=?4, req_headers=?5,
                stage='request',
                updated_at=MAX(updated_at + 1, ?6)
             WHERE id=?1",
            params![
                id as i64,
                request.method,
                request.uri,
                request.version,
                serde_json::to_string(&request.headers)?,
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
        if bytes.is_empty() {
            return Ok(());
        }
        fs::write(self.body_path(id, side, modified), bytes)?;
        self.connection()?.execute(
            "UPDATE captures SET updated_at=MAX(updated_at + 1, ?2) WHERE id=?1",
            params![id as i64, now_millis() as i64],
        )?;
        Ok(())
    }

    pub fn record_interceptor_run(&self, id: u64, run: &InterceptorRun) -> Result<()> {
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
                capture_id, phase, position, name, script_hash, modifications, error
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                id as i64,
                interceptor_phase_name(run.phase),
                run.position as i64,
                run.name,
                run.script_hash,
                serde_json::to_string(&run.modifications)?,
                run.error,
            ],
        )?;
        transaction.execute(
            "UPDATE captures SET updated_at=MAX(updated_at + 1, ?2) WHERE id=?1",
            params![id as i64, now_millis() as i64],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn list(&self, limit: usize, after_id: Option<u64>) -> Result<Vec<CaptureSummary>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT id, source, method, uri, req_version, req_headers,
                    resp_status, resp_version, resp_headers, outcome, stage,
                    error_stage, error_kind, error_message, created_at, updated_at
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
                    error_stage, error_kind, error_message, created_at, updated_at
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
                    error_stage, error_kind, error_message, created_at, updated_at
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
                    error_stage, error_kind, error_message, created_at, updated_at
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
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT id, source, method, uri, req_version, req_headers,
                    resp_status, resp_version, resp_headers, outcome, stage,
                    error_stage, error_kind, error_message, created_at, updated_at
             FROM captures WHERE id=?1",
        )?;
        statement
            .query_row(params![id as i64], |row| {
                let summary = self.summary_from_row(row)?;
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
                Ok(CaptureDetail {
                    summary,
                    request_body,
                    response_body,
                    request_interceptors: self
                        .interceptor_executions(id, InterceptorKind::Request)
                        .unwrap_or_default(),
                    response_interceptors: self
                        .interceptor_executions(id, InterceptorKind::Response)
                        .unwrap_or_default(),
                })
            })
            .optional()
            .map_err(Into::into)
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

    fn initialize(&self) -> Result<()> {
        self.connection()?.execute_batch(
            "CREATE TABLE IF NOT EXISTS captures (
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
                req_modifications TEXT NOT NULL DEFAULT '[]',
                resp_modifications TEXT NOT NULL DEFAULT '[]'
            );
            CREATE INDEX IF NOT EXISTS captures_created_at ON captures(created_at);
            CREATE TABLE IF NOT EXISTS interceptor_script_contents (
                hash TEXT PRIMARY KEY,
                content TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS capture_interceptor_runs (
                capture_id INTEGER NOT NULL,
                phase TEXT NOT NULL CHECK (phase IN ('request', 'response')),
                position INTEGER NOT NULL,
                name TEXT NOT NULL,
                script_hash TEXT NOT NULL,
                modifications TEXT NOT NULL DEFAULT '[]',
                error TEXT,
                PRIMARY KEY (capture_id, phase, position),
                FOREIGN KEY (capture_id) REFERENCES captures(id) ON DELETE CASCADE,
                FOREIGN KEY (script_hash) REFERENCES interceptor_script_contents(hash)
            );
            CREATE INDEX IF NOT EXISTS capture_interceptor_runs_capture
            ON capture_interceptor_runs(capture_id, phase, position);",
        )?;
        Ok(())
    }

    fn connection(&self) -> Result<Connection> {
        let connection = Connection::open(&self.database_path)?;
        connection.busy_timeout(Duration::from_secs(5))?;
        connection.execute_batch("PRAGMA foreign_keys = ON;")?;
        Ok(connection)
    }

    fn interceptor_executions(
        &self,
        id: u64,
        phase: InterceptorKind,
    ) -> Result<Vec<InterceptorExecution>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT runs.position, runs.name, runs.script_hash, contents.content,
                    runs.modifications, runs.error
             FROM capture_interceptor_runs AS runs
             JOIN interceptor_script_contents AS contents
               ON contents.hash = runs.script_hash
             WHERE runs.capture_id=?1 AND runs.phase=?2
             ORDER BY runs.position ASC",
        )?;
        let executions =
            statement.query_map(params![id as i64, interceptor_phase_name(phase)], |row| {
                let modifications: String = row.get(4)?;
                Ok(InterceptorExecution {
                    phase,
                    position: row.get::<_, i64>(0)? as usize,
                    name: row.get(1)?,
                    script_hash: row.get(2)?,
                    content: row.get(3)?,
                    modifications: serde_json::from_str(&modifications).unwrap_or_default(),
                    error: row.get(5)?,
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

        Ok(CaptureSummary {
            id: row.get::<_, i64>(0)? as u64,
            session_id: self.session_id,
            source: row.get(1)?,
            request: RequestData {
                method: row.get(2)?,
                uri: row.get(3)?,
                version: row.get(4)?,
                headers: serde_json::from_str(&request_headers).unwrap_or_default(),
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
            });
        }
        let mut bytes = fs::read(&path)?;
        if let Some(encoding) = first_header(headers, "content-encoding") {
            bytes = decode_body(&bytes, encoding, BODY_DETAIL_LIMIT).unwrap_or(bytes);
        }
        if bytes.is_empty() {
            return Ok(BodyPayload::Empty);
        }
        let content_type = first_header(headers, "content-type").unwrap_or_default();
        if content_type.contains("json")
            && let Ok(content) = serde_json::from_slice(&bytes)
        {
            return Ok(BodyPayload::Json { content });
        }
        if is_textual(content_type)
            && let Ok(content) = String::from_utf8(bytes.clone())
        {
            return Ok(BodyPayload::Text { content });
        }
        Ok(BodyPayload::Binary {
            size: bytes.len() as u64,
        })
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

fn decode_body(bytes: &[u8], encoding: &str, limit: u64) -> Result<Vec<u8>> {
    let mut decoded = Vec::new();
    match encoding.trim().to_ascii_lowercase().as_str() {
        "gzip" => GzDecoder::new(bytes)
            .take(limit + 1)
            .read_to_end(&mut decoded)?,
        "br" => Decompressor::new(bytes, 4096)
            .take(limit + 1)
            .read_to_end(&mut decoded)?,
        _ => return Err(anyhow!("unsupported content encoding")),
    };
    if decoded.len() as u64 > limit {
        return Err(anyhow!("decoded body exceeds detail limit"));
    }
    Ok(decoded)
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
    use tempfile::tempdir;

    use crate::model::{
        CaptureError, CaptureOutcome, ErrorStage, HeaderValues, InterceptorKind, InterceptorRun,
        RequestData, script_content_hash,
    };

    use super::{BodySide, CaptureStore, decode_body};

    fn request(uri: &str) -> RequestData {
        RequestData {
            method: "GET".into(),
            uri: uri.into(),
            version: "HTTP/1.1".into(),
            headers: HeaderValues::new(),
        }
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

    #[test]
    fn interceptor_source_is_deduplicated_and_runs_are_ordered() {
        let root = tempdir().unwrap();
        let store = CaptureStore::open(7, root.path()).unwrap();
        let id = store
            .begin("127.0.0.1", &request("http://example.com"), "request")
            .unwrap();
        let source = "req.headers:set(\"x-test\", \"1\")";
        let hash = script_content_hash(source);

        for (position, name) in ["first", "second"].into_iter().enumerate() {
            store
                .record_interceptor_run(
                    id,
                    &InterceptorRun {
                        phase: InterceptorKind::Request,
                        position,
                        name: name.into(),
                        script_hash: hash.clone(),
                        content: source.into(),
                        modifications: vec![],
                        error: None,
                    },
                )
                .unwrap();
        }

        let detail = store.get(id).unwrap().unwrap();
        assert_eq!(detail.request_interceptors.len(), 2);
        assert_eq!(detail.request_interceptors[0].name, "first");
        assert_eq!(detail.request_interceptors[0].content, source);
        assert_eq!(store.interceptor_source_count().unwrap(), 1);
    }
}
