use super::{
    WsInterceptor,
    interception::{self, RegisteredCallback},
    transport,
};
use crate::{
    lua::{ModificationJournal, SharedInterceptorState, execute_request_with_ws},
    model::RequestData,
    storage::{BodySide, CaptureStore},
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_util::sync::CancellationToken;

fn setup(
    script: &str,
) -> (
    tempfile::TempDir,
    CaptureStore,
    u64,
    Vec<RegisteredCallback>,
) {
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
    let mut ws = request.clone();
    ws.version = "WebSocket/13".into();
    let id = store.begin("test", &ws, "websocket").unwrap();
    store
        .link_websocket(id, handshake, "permessage-deflate", None)
        .unwrap();
    let state = SharedInterceptorState::new_request(
        request.method.clone(),
        request.uri.clone(),
        request.headers.clone(),
        request.tags.clone(),
    );
    let (_, error, callback) = execute_request_with_ws(
        script,
        &request,
        state,
        ModificationJournal::for_request(&request, None),
        "test",
        Some(handshake),
        None,
    )
    .unwrap();
    assert!(error.is_none(), "{error:?}");
    let callbacks = callback
        .into_iter()
        .map(|callback| RegisteredCallback {
            source: WsInterceptor {
                execution_id: 1,
                name: "test".into(),
            },
            callback,
        })
        .collect();
    (dir, store, id, callbacks)
}

async fn finish(task: tokio::task::JoinHandle<anyhow::Result<()>>) -> anyhow::Result<()> {
    tokio::time::timeout(std::time::Duration::from_secs(3), task)
        .await
        .unwrap()
        .unwrap()
}

#[tokio::test]
async fn close_handshake_persists_intervening_data_and_relays_controls() {
    let (_dir, store, id, callbacks) =
        setup("req:on_ws_message(function(ws) error('must not run after close') end)");
    let (client, downstream) = tokio::io::duplex(4096);
    let (upstream, server) = tokio::io::duplex(4096);
    let captured = store.clone();
    let task = tokio::spawn(interception::forward(
        downstream,
        upstream,
        captured,
        id,
        "",
        CancellationToken::new(),
        callbacks,
    ));
    let (cr, mut cw) = tokio::io::split(client);
    let (sr, mut sw) = tokio::io::split(server);
    let mut client = transport::Reader::new(cr, "", BodySide::Response).unwrap();
    let mut server = transport::Reader::new(sr, "", BodySide::Request).unwrap();
    transport::write(&mut cw, 8, &1000u16.to_be_bytes(), true)
        .await
        .unwrap();
    assert_eq!(server.next().await.unwrap().unwrap().opcode, 8);
    let mut packet = Vec::new();
    transport::write(&mut packet, 1, b"unsent one", false)
        .await
        .unwrap();
    transport::write(&mut packet, 1, b"unsent two", false)
        .await
        .unwrap();
    transport::write(&mut packet, 9, b"control", false)
        .await
        .unwrap();
    transport::write(&mut packet, 8, &1000u16.to_be_bytes(), false)
        .await
        .unwrap();
    sw.write_all(&packet).await.unwrap();
    assert_eq!(client.next().await.unwrap().unwrap().opcode, 9);
    assert_eq!(client.next().await.unwrap().unwrap().opcode, 8);
    finish(task).await.unwrap();
    let page = store
        .websocket_messages(id, BodySide::Response, 0, 10)
        .unwrap()
        .unwrap();
    assert_eq!(page.messages.len(), 2);
    for message in &page.messages {
        assert_eq!(message.status.as_deref(), Some("unsent"));
        assert!(
            message
                .error
                .as_deref()
                .unwrap()
                .contains("close handshake")
        );
        assert!(message.errors.is_empty());
    }
    assert_eq!(
        store
            .websocket_message_body(id, BodySide::Response, page.messages[1].id)
            .unwrap()
            .unwrap(),
        b"unsent two"
    );
    assert_eq!(
        store.websocket_info(id).unwrap().unwrap().close_code,
        Some(1000)
    );
}

fn compressed_frame(compressor: &mut flate2::Compress, text: &[u8]) -> Vec<u8> {
    let mut output = Vec::with_capacity(text.len() + 256);
    compressor
        .compress_vec(text, &mut output, flate2::FlushCompress::Sync)
        .unwrap();
    assert_eq!(&output[output.len() - 4..], &[0, 0, 255, 255]);
    output.truncate(output.len() - 4);
    let mut bytes = vec![193];
    if output.len() < 126 {
        bytes.push(128 | output.len() as u8);
    } else {
        bytes.push(254);
        bytes.extend_from_slice(&(output.len() as u16).to_be_bytes());
    }
    let mask = [3, 7, 9, 11];
    bytes.extend(mask);
    bytes.extend(output.iter().enumerate().map(|(i, b)| b ^ mask[i % 4]));
    bytes
}

#[tokio::test]
async fn compressed_drop_and_bidirectional_send_preserve_next_dictionary_message() {
    let (_dir, store, id, callbacks) = setup(
        r#"
        local count = 0
        req:on_ws_message(function(ws)
            count = count + 1
            if count == 1 then
                ws:drop()
                ws:send('request', 'replacement')
                ws:send('response', 'ack')
            else
                assert(#ws:history() == 3)
            end
        end)
    "#,
    );
    let (client, downstream) = tokio::io::duplex(4096);
    let (upstream, server) = tokio::io::duplex(4096);
    let captured = store.clone();
    let task = tokio::spawn(interception::forward(
        downstream,
        upstream,
        captured,
        id,
        "permessage-deflate",
        CancellationToken::new(),
        callbacks,
    ));
    let (cr, mut cw) = tokio::io::split(client);
    let (sr, mut sw) = tokio::io::split(server);
    let mut client = transport::Reader::new(cr, "permessage-deflate", BodySide::Response).unwrap();
    let mut server = transport::Reader::new(sr, "permessage-deflate", BodySide::Request).unwrap();
    let text = b"dictionary repeated dictionary repeated dictionary repeated";
    let mut compressor = flate2::Compress::new(flate2::Compression::default(), false);
    let first = compressed_frame(&mut compressor, text);
    let second = compressed_frame(&mut compressor, text);
    cw.write_all(&[first, second].concat()).await.unwrap();
    assert_eq!(
        server.next().await.unwrap().unwrap().payload,
        b"replacement"
    );
    assert_eq!(client.next().await.unwrap().unwrap().payload, b"ack");
    assert_eq!(server.next().await.unwrap().unwrap().payload, text);
    transport::write(&mut cw, 8, &1000u16.to_be_bytes(), true)
        .await
        .unwrap();
    assert_eq!(server.next().await.unwrap().unwrap().opcode, 8);
    transport::write(&mut sw, 8, &1000u16.to_be_bytes(), false)
        .await
        .unwrap();
    assert_eq!(client.next().await.unwrap().unwrap().opcode, 8);
    finish(task).await.unwrap();
    let messages = store
        .websocket_messages(id, BodySide::Request, 0, 10)
        .unwrap()
        .unwrap()
        .messages;
    assert_eq!(messages.len(), 3);
    assert_eq!(messages[0].status.as_deref(), Some("dropped"));
    assert_eq!(messages[1].status.as_deref(), Some("sent"));
    assert_eq!(messages[1].parent.as_ref().unwrap().id, messages[0].id);
    assert_eq!(messages[2].status.as_deref(), Some("sent"));
    assert!(messages[2].errors.is_empty(), "{:?}", messages[2].errors);
}

#[tokio::test]
async fn cancellation_persists_pending_sends_and_buffered_originals() {
    let (_dir, store, id, callbacks) =
        setup("req:on_ws_message(function(ws) ws:send('response','derived') end)");
    let (mut client, downstream) = tokio::io::duplex(4096);
    let (upstream, mut server) = tokio::io::duplex(64);
    let cancellation = CancellationToken::new();
    let captured = store.clone();
    let token = cancellation.clone();
    let task = tokio::spawn(interception::forward(
        downstream, upstream, captured, id, "", token, callbacks,
    ));
    let mut packet = Vec::new();
    transport::write(&mut packet, 1, &vec![b'a'; 256], true)
        .await
        .unwrap();
    transport::write(&mut packet, 1, b"buffered", true)
        .await
        .unwrap();
    client.write_all(&packet).await.unwrap();
    // Reading only the frame prefix proves the first write is in progress, while
    // the tiny server buffer keeps its payload blocked until cancellation.
    let mut header = [0; 2];
    server.read_exact(&mut header).await.unwrap();
    cancellation.cancel();
    assert!(finish(task).await.is_err());
    let request = store
        .websocket_messages(id, BodySide::Request, 0, 10)
        .unwrap()
        .unwrap()
        .messages;
    let response = store
        .websocket_messages(id, BodySide::Response, 0, 10)
        .unwrap()
        .unwrap()
        .messages;
    assert_eq!(request.len(), 2);
    assert_eq!(response.len(), 1);
    for message in request.iter().chain(response.iter()) {
        assert_eq!(message.status.as_deref(), Some("unsent"));
        assert!(message.error.as_deref().unwrap().contains("cancelled"));
    }
    assert_eq!(
        store
            .websocket_message_body(id, BodySide::Request, request[1].id)
            .unwrap()
            .unwrap(),
        b"buffered"
    );
}
