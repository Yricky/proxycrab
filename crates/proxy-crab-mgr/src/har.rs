use std::collections::BTreeMap;

use base64::{Engine, engine::general_purpose::STANDARD};
use proxy_crab_mitm::{
    model::{CaptureDetail, HeaderValues},
    storage::{BodySource, BodySourceData},
};
use serde::Serialize;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use tokio::io::AsyncReadExt;
use url::Url;

use crate::{dto::ManagerResult, http::body_reader};

pub(crate) struct HarCapture {
    pub detail: CaptureDetail,
    pub request_body: Option<BodySource>,
    pub response_body: Option<BodySource>,
}

#[derive(Serialize)]
struct HarRoot {
    log: HarLog,
}

#[derive(Serialize)]
struct HarLog {
    version: &'static str,
    creator: HarCreator,
    entries: Vec<HarEntry>,
}

#[derive(Serialize)]
struct HarCreator {
    name: &'static str,
    version: &'static str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct HarEntry {
    started_date_time: String,
    time: u64,
    request: HarRequest,
    response: HarResponse,
    cache: HarCache,
    timings: HarTimings,
    #[serde(rename = "_proxyCrab")]
    proxy_crab: ProxyCrabMetadata,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct HarRequest {
    method: String,
    url: String,
    http_version: String,
    headers: Vec<HarNameValue>,
    query_string: Vec<HarNameValue>,
    cookies: Vec<HarCookie>,
    headers_size: i64,
    body_size: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    post_data: Option<HarPostData>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct HarPostData {
    mime_type: String,
    text: String,
    #[serde(rename = "_encoding", skip_serializing_if = "Option::is_none")]
    encoding: Option<&'static str>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct HarResponse {
    status: u16,
    status_text: String,
    http_version: String,
    headers: Vec<HarNameValue>,
    cookies: Vec<HarCookie>,
    content: HarContent,
    #[serde(rename = "redirectURL")]
    redirect_url: String,
    headers_size: i64,
    body_size: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct HarContent {
    size: usize,
    mime_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    encoding: Option<&'static str>,
}

#[derive(Serialize)]
struct HarNameValue {
    name: String,
    value: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct HarCookie {
    name: String,
    value: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    domain: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    http_only: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    secure: Option<bool>,
}

#[derive(Serialize)]
struct HarCache {}

#[derive(Serialize)]
struct HarTimings {
    blocked: i64,
    dns: i64,
    connect: i64,
    ssl: i64,
    send: i64,
    wait: i64,
    receive: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ProxyCrabMetadata {
    log_id: u64,
    session_id: u64,
    tags: BTreeMap<String, String>,
    stage: String,
    source_addr: String,
    created_at: u64,
    updated_at: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    request_body_decoded: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    response_body_decoded: Option<bool>,
}

struct LoadedBody {
    bytes: Vec<u8>,
    stored_size: u64,
    mime_type: String,
    decode_failed: bool,
}

pub(crate) async fn serialize(captures: Vec<HarCapture>) -> ManagerResult<Vec<u8>> {
    let mut entries = Vec::with_capacity(captures.len());
    for capture in captures {
        entries.push(entry(capture).await?);
    }
    serde_json::to_vec(&HarRoot {
        log: HarLog {
            version: "1.2",
            creator: HarCreator {
                name: "ProxyCrab",
                version: env!("CARGO_PKG_VERSION"),
            },
            entries,
        },
    })
    .map_err(|error| crate::dto::ManagerError::internal(error.to_string()))
}

async fn entry(capture: HarCapture) -> ManagerResult<HarEntry> {
    let request_body = load_body(capture.request_body).await?;
    let response_body = load_body(capture.response_body).await?;
    let summary = capture.detail.summary;
    let response = summary
        .response
        .expect("the Manager only passes captures with responses to HAR serialization");
    let request_post_data = post_data(&request_body);
    let response_content = content(&response_body);
    let request_decode_failed = request_body.decode_failed.then_some(false);
    let response_decode_failed = response_body.decode_failed.then_some(false);
    let request_headers = summary.request.headers;
    let response_headers = response.headers;
    let started_date_time = timestamp(summary.created_at)?;

    Ok(HarEntry {
        started_date_time,
        time: summary.updated_at.saturating_sub(summary.created_at),
        request: HarRequest {
            method: summary.request.method,
            query_string: query_string(&summary.request.uri),
            cookies: request_cookies(&request_headers),
            headers: headers(&request_headers),
            url: summary.request.uri,
            http_version: summary.request.version,
            headers_size: -1,
            body_size: request_body.stored_size,
            post_data: request_post_data,
        },
        response: HarResponse {
            status: response.status,
            status_text: status_text(response.status),
            http_version: response.version,
            cookies: response_cookies(&response_headers),
            redirect_url: first_header(&response_headers, "location")
                .unwrap_or_default()
                .to_owned(),
            headers: headers(&response_headers),
            content: response_content,
            headers_size: -1,
            body_size: response_body.stored_size,
        },
        cache: HarCache {},
        timings: HarTimings {
            blocked: -1,
            dns: -1,
            connect: -1,
            ssl: -1,
            send: -1,
            wait: -1,
            receive: -1,
        },
        proxy_crab: ProxyCrabMetadata {
            log_id: summary.id,
            session_id: summary.session_id,
            tags: summary.request.tags,
            stage: summary.stage,
            source_addr: summary.source,
            created_at: summary.created_at,
            updated_at: summary.updated_at,
            request_body_decoded: request_decode_failed,
            response_body_decoded: response_decode_failed,
        },
    })
}

async fn load_body(source: Option<BodySource>) -> ManagerResult<LoadedBody> {
    let Some(source) = source else {
        return Ok(LoadedBody {
            bytes: Vec::new(),
            stored_size: 0,
            mime_type: "application/octet-stream".into(),
            decode_failed: false,
        });
    };
    let stored_size = source.stored_size;
    let mime_type = source
        .content_type
        .clone()
        .unwrap_or_else(|| "application/octet-stream".into());
    let mut raw_reader = body_reader(&source, false).await?;
    let mut raw = Vec::new();
    raw_reader
        .read_to_end(&mut raw)
        .await
        .map_err(|error| crate::dto::ManagerError::new("body_read_failed", error.to_string()))?;
    if source.content_encodings.is_empty() {
        return Ok(LoadedBody {
            bytes: raw,
            stored_size,
            mime_type,
            decode_failed: false,
        });
    }

    let decoded_source = BodySource {
        data: BodySourceData::Bytes(raw.clone()),
        path: None,
        stored_size,
        content_type: source.content_type,
        content_encodings: source.content_encodings,
    };
    let decoded = async {
        let mut reader = body_reader(&decoded_source, true).await?;
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await.map_err(|error| {
            crate::dto::ManagerError::new("body_decode_failed", error.to_string())
        })?;
        Ok::<_, crate::dto::ManagerError>(bytes)
    }
    .await;
    match decoded {
        Ok(bytes) => Ok(LoadedBody {
            bytes,
            stored_size,
            mime_type,
            decode_failed: false,
        }),
        Err(_) => Ok(LoadedBody {
            bytes: raw,
            stored_size,
            mime_type,
            decode_failed: true,
        }),
    }
}

fn post_data(body: &LoadedBody) -> Option<HarPostData> {
    if body.stored_size == 0 {
        return None;
    }
    let (text, encoding) = encoded_text(body);
    Some(HarPostData {
        mime_type: body.mime_type.clone(),
        text,
        encoding,
    })
}

fn content(body: &LoadedBody) -> HarContent {
    let (text, encoding) = if body.stored_size == 0 {
        (None, None)
    } else {
        let (text, encoding) = encoded_text(body);
        (Some(text), encoding)
    };
    HarContent {
        size: body.bytes.len(),
        mime_type: body.mime_type.clone(),
        text,
        encoding,
    }
}

fn encoded_text(body: &LoadedBody) -> (String, Option<&'static str>) {
    if !body.decode_failed
        && is_textual(&body.mime_type)
        && let Ok(text) = std::str::from_utf8(&body.bytes)
    {
        return (text.to_owned(), None);
    }
    (STANDARD.encode(&body.bytes), Some("base64"))
}

fn is_textual(content_type: &str) -> bool {
    let content_type = content_type.to_ascii_lowercase();
    content_type.starts_with("text/")
        || ["json", "xml", "javascript", "x-www-form-urlencoded"]
            .iter()
            .any(|kind| content_type.contains(kind))
}

fn headers(values: &HeaderValues) -> Vec<HarNameValue> {
    values
        .iter()
        .flat_map(|(name, values)| {
            values.iter().map(|value| HarNameValue {
                name: name.clone(),
                value: value.clone(),
            })
        })
        .collect()
}

fn query_string(uri: &str) -> Vec<HarNameValue> {
    Url::parse(uri)
        .ok()
        .map(|url| {
            url.query_pairs()
                .map(|(name, value)| HarNameValue {
                    name: name.into_owned(),
                    value: value.into_owned(),
                })
                .collect()
        })
        .unwrap_or_default()
}

fn request_cookies(headers: &HeaderValues) -> Vec<HarCookie> {
    header_values(headers, "cookie")
        .flat_map(|header| header.split(';'))
        .filter_map(|pair| cookie_pair(pair.trim()))
        .map(|(name, value)| HarCookie {
            name,
            value,
            path: None,
            domain: None,
            http_only: None,
            secure: None,
        })
        .collect()
}

fn response_cookies(headers: &HeaderValues) -> Vec<HarCookie> {
    header_values(headers, "set-cookie")
        .filter_map(parse_set_cookie)
        .collect()
}

fn parse_set_cookie(header: &str) -> Option<HarCookie> {
    let mut parts = header.split(';');
    let (name, value) = cookie_pair(parts.next()?.trim())?;
    let mut cookie = HarCookie {
        name,
        value,
        path: None,
        domain: None,
        http_only: None,
        secure: None,
    };
    for attribute in parts.map(str::trim).filter(|part| !part.is_empty()) {
        let (name, value) = attribute
            .split_once('=')
            .map(|(name, value)| (name.trim(), Some(value.trim())))
            .unwrap_or((attribute, None));
        match name.to_ascii_lowercase().as_str() {
            "path" => cookie.path = value.map(str::to_owned),
            "domain" => cookie.domain = value.map(str::to_owned),
            "httponly" => cookie.http_only = Some(true),
            "secure" => cookie.secure = Some(true),
            _ => {}
        }
    }
    Some(cookie)
}

fn cookie_pair(pair: &str) -> Option<(String, String)> {
    let (name, value) = pair.split_once('=')?;
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    Some((name.to_owned(), value.trim().to_owned()))
}

fn header_values<'a>(headers: &'a HeaderValues, name: &'a str) -> impl Iterator<Item = &'a str> {
    headers
        .iter()
        .filter(move |(key, _)| key.eq_ignore_ascii_case(name))
        .flat_map(|(_, values)| values.iter().map(String::as_str))
}

fn first_header<'a>(headers: &'a HeaderValues, name: &'a str) -> Option<&'a str> {
    header_values(headers, name).next()
}

fn status_text(status: u16) -> String {
    axum::http::StatusCode::from_u16(status)
        .ok()
        .and_then(|status| status.canonical_reason())
        .unwrap_or_default()
        .to_owned()
}

fn timestamp(milliseconds: u64) -> ManagerResult<String> {
    let timestamp = OffsetDateTime::from_unix_timestamp_nanos(i128::from(milliseconds) * 1_000_000)
        .map_err(|error| crate::dto::ManagerError::internal(error.to_string()))?;
    timestamp
        .format(&Rfc3339)
        .map_err(|error| crate::dto::ManagerError::internal(error.to_string()))
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use flate2::{Compression, write::GzEncoder};
    use proxy_crab_mitm::{
        model::{
            BodyPayload, CaptureDetail, CaptureOutcome, CaptureSummary, HeaderValues, RequestData,
            ResponseData,
        },
        storage::{BodySource, BodySourceData},
    };

    use super::{HarCapture, serialize};

    fn capture(response_headers: HeaderValues) -> CaptureDetail {
        CaptureDetail {
            summary: CaptureSummary {
                id: 7,
                session_id: 3,
                source: "127.0.0.1:4321".into(),
                request: RequestData {
                    method: "POST".into(),
                    uri: "https://example.com/items?a=1&a=2".into(),
                    version: "HTTP/1.1".into(),
                    headers: HeaderValues::from([
                        (
                            "content-type".into(),
                            vec!["application/octet-stream".into()],
                        ),
                        ("cookie".into(), vec!["sid=secret; theme=dark".into()]),
                    ]),
                    tags: [("team".into(), "network".into())].into(),
                },
                response: Some(ResponseData {
                    status: 302,
                    version: "HTTP/1.1".into(),
                    headers: response_headers,
                }),
                outcome: CaptureOutcome::Success,
                stage: "completed".into(),
                error: None,
                created_at: 1_700_000_000_000,
                updated_at: 1_700_000_000_125,
            },
            request_body: BodyPayload::Empty,
            response_body: BodyPayload::Empty,
            request_interceptors: Vec::new(),
            response_interceptors: Vec::new(),
        }
    }

    #[tokio::test]
    async fn serializes_har_fields_and_decoded_response_body() {
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(b"decoded").unwrap();
        let compressed = encoder.finish().unwrap();
        let detail = capture(HeaderValues::from([
            (
                "content-type".into(),
                vec!["text/plain; charset=utf-8".into()],
            ),
            ("content-encoding".into(), vec!["gzip".into()]),
            ("location".into(), vec!["https://example.com/next".into()]),
            (
                "set-cookie".into(),
                vec!["token=value; Path=/; Domain=example.com; Secure; HttpOnly".into()],
            ),
        ]));

        let bytes = serialize(vec![HarCapture {
            detail,
            request_body: Some(BodySource {
                data: BodySourceData::Bytes(vec![0, 1]),
                path: None,
                stored_size: 2,
                content_type: Some("application/octet-stream".into()),
                content_encodings: Vec::new(),
            }),
            response_body: Some(BodySource {
                data: BodySourceData::Bytes(compressed.clone()),
                path: None,
                stored_size: compressed.len() as u64,
                content_type: Some("text/plain; charset=utf-8".into()),
                content_encodings: vec!["gzip".into()],
            }),
        }])
        .await
        .unwrap();
        let har: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let entry = &har["log"]["entries"][0];

        assert_eq!(har["log"]["version"], "1.2");
        assert_eq!(har["log"]["creator"]["name"], "ProxyCrab");
        assert_eq!(entry["startedDateTime"], "2023-11-14T22:13:20Z");
        assert_eq!(entry["time"], 125);
        assert_eq!(entry["request"]["queryString"][0]["name"], "a");
        assert_eq!(entry["request"]["queryString"][1]["value"], "2");
        assert_eq!(entry["request"]["cookies"][0]["name"], "sid");
        assert_eq!(entry["request"]["postData"]["text"], "AAE=");
        assert_eq!(entry["request"]["postData"]["_encoding"], "base64");
        assert_eq!(entry["request"]["bodySize"], 2);
        assert_eq!(entry["response"]["content"]["text"], "decoded");
        assert_eq!(entry["response"]["content"]["size"], 7);
        assert_eq!(entry["response"]["bodySize"], compressed.len());
        assert_eq!(entry["response"]["redirectURL"], "https://example.com/next");
        assert_eq!(entry["response"]["cookies"][0]["domain"], "example.com");
        assert_eq!(entry["response"]["cookies"][0]["httpOnly"], true);
        assert_eq!(entry["timings"]["ssl"], -1);
        assert_eq!(entry["_proxyCrab"]["logId"], 7);
        assert_eq!(entry["_proxyCrab"]["sessionId"], 3);
        assert_eq!(entry["_proxyCrab"]["tags"]["team"], "network");
        assert!(entry["_proxyCrab"].get("responseBodyDecoded").is_none());
    }

    #[tokio::test]
    async fn falls_back_to_raw_base64_for_unknown_content_encoding() {
        let detail = capture(HeaderValues::from([
            (
                "content-type".into(),
                vec!["application/octet-stream".into()],
            ),
            ("content-encoding".into(), vec!["snappy".into()]),
        ]));
        let bytes = serialize(vec![HarCapture {
            detail,
            request_body: None,
            response_body: Some(BodySource {
                data: BodySourceData::Bytes(vec![0, 255]),
                path: None,
                stored_size: 2,
                content_type: Some("application/octet-stream".into()),
                content_encodings: vec!["snappy".into()],
            }),
        }])
        .await
        .unwrap();
        let har: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let entry = &har["log"]["entries"][0];

        assert_eq!(entry["response"]["content"]["text"], "AP8=");
        assert_eq!(entry["response"]["content"]["encoding"], "base64");
        assert_eq!(entry["_proxyCrab"]["responseBodyDecoded"], false);
    }
}
