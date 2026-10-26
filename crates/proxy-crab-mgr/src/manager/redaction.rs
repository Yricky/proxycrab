use super::*;
use proxy_crab_mitm::{
    lua::redaction::{RedactedCapture, RedactionEvaluator},
    model::{BodyPayload, BodySourceType},
    storage::BodySourceData,
};

#[derive(Clone)]
pub(crate) struct ReadPolicy {
    pub script: Script,
    pub api: String,
}
pub(crate) fn failed() -> ManagerError {
    ManagerError::new(
        ErrorCode::RedactionFailed,
        "redaction failed; content was not returned",
    )
}
#[derive(Clone)]
pub(crate) struct ReadRedactor {
    evaluator: RedactionEvaluator,
    api: String,
}
impl ReadRedactor {
    pub fn prepare(policy: &Option<ReadPolicy>) -> ManagerResult<Option<Self>> {
        policy
            .as_ref()
            .map(|p| {
                RedactionEvaluator::new(&p.script.content, &p.script.name)
                    .map(|evaluator| Self {
                        evaluator,
                        api: p.api.clone(),
                    })
                    .map_err(|_| failed())
            })
            .transpose()
    }
    pub fn uri(&self, uri: &str) -> ManagerResult<String> {
        RedactedCapture::uri_only(self.evaluator.clone(), &self.api, uri)
            .map(|v| v.summary().request.uri)
            .map_err(|_| failed())
    }
    pub fn view(
        &self,
        runtime: &Arc<ProxyCrab>,
        summary: &CaptureSummary,
    ) -> ManagerResult<RedactedCapture> {
        let runtime = runtime.clone();
        let (sid, id) = (summary.session_id, summary.id);
        self.with_sources(summary.clone(), move |side| {
            runtime.capture_body_source(sid, id, side)
        })
    }
    pub fn with_sources<F>(
        &self,
        summary: CaptureSummary,
        resolver: F,
    ) -> ManagerResult<RedactedCapture>
    where
        F: Fn(BodySide) -> anyhow::Result<Option<BodySource>> + Send + Sync + 'static,
    {
        RedactedCapture::new(self.evaluator.clone(), &self.api, summary, resolver)
            .map_err(|_| failed())
    }
    pub fn detail(
        &self,
        runtime: &Arc<ProxyCrab>,
        mut detail: CaptureDetail,
    ) -> ManagerResult<CaptureDetail> {
        let view = self.view(runtime, &detail.summary)?;
        detail.request_body = payload(view.body_source(BodySide::Request).map_err(|_| failed())?)?;
        detail.response_body =
            payload(view.body_source(BodySide::Response).map_err(|_| failed())?)?;
        detail.summary = view.summary();
        Ok(detail)
    }
    pub fn snapshot(
        &self,
        runtime: &Arc<ProxyCrab>,
        sid: u64,
        id: u64,
        execution: u64,
    ) -> ManagerResult<(InterceptorSnapshot, RedactedCapture)> {
        let snapshot = runtime
            .interceptor_snapshot(sid, id, execution)
            .map_err(map_error)?
            .ok_or_else(|| ManagerError::new(ErrorCode::SnapshotNotFound, "snapshot not found"))?;
        let mut summary = runtime
            .captures(sid, &[id])
            .map_err(map_error)?
            .pop()
            .ok_or_else(|| ManagerError::not_found("log not found"))?;
        // Snapshot fields must never be borrowed from the final request/response.
        if let Some(request) = &snapshot.request {
            summary.request = RequestData {
                method: request.method.clone(),
                uri: request.uri.clone(),
                version: request.version.clone(),
                headers: request.headers.clone(),
                tags: Default::default(),
            };
        } else {
            summary.request = RequestData {
                method: String::new(),
                uri: String::new(),
                version: String::new(),
                headers: Default::default(),
                tags: Default::default(),
            };
        }
        summary.response = snapshot.response.as_ref().map(|r| ResponseData {
            status: r.status,
            version: r.version.clone(),
            headers: r.headers.clone(),
        });
        let runtime = runtime.clone();
        let view = RedactedCapture::snapshot(
            self.evaluator.clone(),
            &self.api,
            summary,
            snapshot.request.is_some(),
            move |side| runtime.interceptor_snapshot_body_source(sid, id, execution, side),
        )
        .map_err(|_| failed())?;
        Ok((snapshot, view))
    }
}
use proxy_crab_mitm::model::{RequestData, ResponseData};
pub(crate) fn payload(source: Option<BodySource>) -> ManagerResult<BodyPayload> {
    let Some(source) = source else {
        return Ok(BodyPayload::Empty);
    };
    if source.stored_size == 0 {
        return Ok(BodyPayload::Empty);
    }
    if source.stored_size > 65536 {
        return Ok(BodyPayload::Large {
            size: source.stored_size,
        });
    }
    // Reuse the existing bounded historical decoder and payload classification.
    proxy_crab_mitm::storage::read_body_payload(source).map_err(|_| failed())
}
pub(crate) fn snapshot_output(
    mut snapshot: InterceptorSnapshot,
    view: &RedactedCapture,
) -> ManagerResult<InterceptorSnapshot> {
    for side in [BodySide::Request, BodySide::Response] {
        let body = view.body_source(side).map_err(|_| failed())?;
        let replacement = body.and_then(|s| match s.data {
            BodySourceData::Bytes(b) => String::from_utf8(b)
                .ok()
                .map(|content| BodySourceType::String { content }),
            _ => None,
        });
        let output = view.summary();
        match side {
            BodySide::Request => {
                if let Some(req) = &mut snapshot.request {
                    req.uri = output.request.uri;
                    req.headers = output.request.headers;
                    if let Some(body) = replacement {
                        req.body = body;
                    }
                }
            }
            BodySide::Response => {
                if let (Some(resp), Some(output)) = (&mut snapshot.response, output.response) {
                    resp.headers = output.headers;
                    if let Some(body) = replacement {
                        resp.body = body;
                    }
                }
            }
        }
    }
    Ok(snapshot)
}
