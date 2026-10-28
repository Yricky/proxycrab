use super::*;
use crate::{
    lua::{ModificationJournal, SharedInterceptorState, execute_request_with_ws},
    model::script_content_hash,
    model::{InterceptorExecutionOrigin, InterceptorKind, InterceptorRun, RequestData},
};

fn setup(sources: &[&str]) -> (tempfile::TempDir, Chain) {
    let dir = tempfile::tempdir().unwrap();
    let store = CaptureStore::open(1, dir.path()).unwrap();
    let request = RequestData {
        method: "GET".into(),
        uri: "ws://example.test/".into(),
        version: "HTTP/1.1".into(),
        headers: Default::default(),
        tags: Default::default(),
    };
    let handshake = store.begin("test", &request, "request").unwrap();
    let id = store.begin("test", &request, "websocket").unwrap();
    store.link_websocket(id, handshake, "", None).unwrap();
    let callbacks = sources
        .iter()
        .enumerate()
        .map(|(position, source)| {
            let name = format!("callback-{position}");
            let execution_id = store
                .begin_interceptor_run(
                    handshake,
                    &InterceptorRun {
                        origin: InterceptorExecutionOrigin::Saved,
                        completed: true,
                        phase: InterceptorKind::Request,
                        position,
                        name: name.clone(),
                        script_hash: script_content_hash(source),
                        content: (*source).into(),
                        modifications: vec![],
                        error: None,
                    },
                )
                .unwrap();
            let (_, error, callback) = execute_request_with_ws(
                source,
                &request,
                SharedInterceptorState::new_request(
                    request.method.clone(),
                    request.uri.clone(),
                    request.headers.clone(),
                    request.tags.clone(),
                ),
                ModificationJournal::for_request(&request, None),
                &name,
                Some(handshake),
                None,
            )
            .unwrap();
            assert!(error.is_none(), "{error:?}");
            RegisteredCallback {
                source: WsInterceptor { execution_id, name },
                callback: callback.unwrap(),
            }
        })
        .collect::<Vec<_>>();
    store
        .set_websocket_interceptors(
            id,
            &callbacks
                .iter()
                .map(|c| c.source.clone())
                .collect::<Vec<_>>(),
        )
        .unwrap();
    (dir, Chain::new(callbacks, store, id).unwrap())
}
fn root(text: &str) -> Option<(BodySide, transport::Message)> {
    Some((
        BodySide::Request,
        transport::Message {
            opcode: 1,
            payload: text.as_bytes().to_vec(),
            started_at: now_millis(),
        },
    ))
}
fn finish(chain: &mut Chain, mut message: Processed) -> WebSocketMessage {
    if message.meta.status.is_none() {
        message.meta.status = Some("sent".into());
    }
    let meta = message.meta.clone();
    chain.persist(message).unwrap();
    meta
}
#[test]
fn breadth_first_cross_direction_provenance_and_finalized_history() {
    let (_dir, mut chain) = setup(&[
        r#"local n=0; req:on_ws_message(function(ws)
            n=n+1; assert(n==1); assert(#ws:history()==0)
            ws:send('response','X1'); ws:send('request','X2')
        end)"#,
        r#"req:on_ws_message(function(ws)
            local h=ws:history()
            if ws.message.body=='M' then assert(#h==0); ws:send('response','Y')
            elseif ws.message.body=='X1' then assert(#h==1); assert(h[1].body=='M'); ws:send('request','Z')
            elseif ws.message.body=='X2' then assert(#h==2); assert(h[2].side=='response'); assert(h[2].body=='X1')
            else error('earlier callback revisited') end
        end)"#,
        r#"req:on_ws_message(function(ws)
            for _, m in ipairs(ws:history()) do assert(m.sequence<ws.message.sequence) end
        end)"#,
    ]);
    let mut original = root("M");
    let mut values = vec![];
    let mut metadata = vec![];
    while let Some(message) = chain.process(original.take()).unwrap() {
        assert!(message.meta.errors.is_empty(), "{:?}", message.meta.errors);
        values.push(String::from_utf8(message.body.to_vec()).unwrap());
        metadata.push(finish(&mut chain, message));
    }
    assert_eq!(values, ["M", "X1", "X2", "Y", "Z"]);
    let z = &metadata[4];
    assert_eq!(
        z.root,
        Some(MessageRef {
            side: BodySide::Request,
            id: 1
        })
    );
    assert_eq!(
        z.parent,
        Some(MessageRef {
            side: BodySide::Response,
            id: 2
        })
    );
    assert_eq!(z.producer.as_ref().unwrap().name, "callback-1");
    assert_eq!(metadata[2].producer.as_ref().unwrap().send_index, 2);
    assert_ne!(
        z.producer.as_ref().unwrap().invocation_id,
        metadata[3].producer.as_ref().unwrap().invocation_id
    );
    let history = chain.history(6)(2, 2).unwrap();
    assert_eq!(
        history.iter().map(|m| m.metadata.id).collect::<Vec<_>>(),
        [3, 4]
    );
    let info = chain
        .store
        .websocket_info(chain.capture_id)
        .unwrap()
        .unwrap();
    assert_eq!(info.interceptors.len(), 3);
    let source = chain
        .store
        .interceptor_script_content(info.handshake_log_id, info.interceptors[0].execution_id)
        .unwrap()
        .unwrap();
    assert!(source.content.contains("local n=0"));
}
#[test]
fn drop_persists_and_stops_current_but_generated_continues() {
    let (_dir, mut chain) = setup(&[
        "req:on_ws_message(function(ws) ws:drop(); ws:send('response','replacement') end)",
        "req:on_ws_message(function(ws) assert(ws.message.body=='replacement'); local h=ws:history(); assert(#h==1); assert(h[1].status=='dropped'); assert(h[1].dropped_by.name=='callback-0') end)",
    ]);
    let m = chain.process(root("discard")).unwrap().unwrap();
    assert_eq!(m.meta.status.as_deref(), Some("dropped"));
    assert!(m.meta.errors.is_empty());
    finish(&mut chain, m);
    let m = chain.process(None).unwrap().unwrap();
    assert!(m.meta.errors.is_empty(), "{:?}", m.meta.errors);
    finish(&mut chain, m);
    assert!(chain.process(None).unwrap().is_none());
    assert_eq!(
        chain
            .store
            .websocket_message_body(chain.capture_id, BodySide::Request, 1)
            .unwrap()
            .unwrap(),
        b"discard"
    );
}
#[test]
fn callback_error_rolls_back_and_retries_next_network_message() {
    let (_dir, mut chain) = setup(&[
        "local n=0; req:on_ws_message(function(ws) n=n+1; ws:drop(); ws:send('response','generated'); if n==1 then error('rollback') end end)",
        "req:on_ws_message(function(ws) assert(ws.message.body=='first' or ws.message.body=='generated') end)",
    ]);
    let m = chain.process(root("first")).unwrap().unwrap();
    assert!(m.meta.status.is_none());
    assert_eq!(m.meta.errors.len(), 1);
    finish(&mut chain, m);
    assert!(chain.process(None).unwrap().is_none());
    let m = chain.process(root("second")).unwrap().unwrap();
    assert_eq!(m.meta.status.as_deref(), Some("dropped"));
    assert!(m.meta.errors.is_empty());
    finish(&mut chain, m);
    let m = chain.process(None).unwrap().unwrap();
    assert_eq!(&*m.body, b"generated");
    assert!(m.meta.errors.is_empty());
    finish(&mut chain, m);
}
#[test]
fn failed_delivery_preserves_queued_messages_without_callbacks() {
    let (_dir, mut chain) = setup(&[
        "req:on_ws_message(function(ws) ws:send('response','one'); ws:send('request','two') end)",
        "req:on_ws_message(function(ws) assert(ws.message.body=='original') end)",
    ]);
    let mut m = chain.process(root("original")).unwrap().unwrap();
    m.meta.status = Some("failed".into());
    m.meta.error = Some("closed".into());
    finish(&mut chain, m);
    chain.fail_pending("closed").unwrap();
    let messages = chain.history(4)(0, 10).unwrap();
    assert_eq!(messages.len(), 3);
    assert_eq!(messages[0].metadata.status.as_deref(), Some("failed"));
    for m in &messages[1..] {
        assert_eq!(m.metadata.status.as_deref(), Some("unsent"));
        assert_eq!(m.metadata.error.as_deref(), Some("closed"));
        assert!(m.metadata.errors.is_empty());
        assert_eq!(m.metadata.parent.as_ref().unwrap().id, 1);
    }
}

#[test]
fn large_callback_error_remains_readable_and_does_not_break_history() {
    let (_dir, mut chain) =
        setup(&["req:on_ws_message(function(ws) error(string.rep('x',70000)) end)"]);
    let m = chain.process(root("one")).unwrap().unwrap();
    assert!(m.meta.errors[0].error.len() > 65536);
    finish(&mut chain, m);
    let m = chain.process(root("two")).unwrap().unwrap();
    finish(&mut chain, m);
    let messages = chain.history(3)(0, 64).unwrap();
    assert_eq!(messages.len(), 2);
    assert!(messages[0].metadata.errors[0].error.len() > 65536);
}
