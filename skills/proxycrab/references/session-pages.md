# Session pages

Build a normal webpage. At runtime, use this native module import:

```html
<!doctype html>
<html>
<head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"></head>
<body>
  <h1>Session</h1><pre id="results"></pre>
  <script type="module">
    import { backend } from "./init.js";
    const { sessions: [session] } = await backend.listSessions();
    document.querySelector("h1").textContent = session.name;
    const { matched_ids } = await backend.getLogIds({ session_id: session.id, limit: 50 });
    const logs = await Promise.all(matched_ids.map(id => backend.getLog(session.id, id)));
    document.querySelector("#results").textContent = JSON.stringify(logs, null, 2);
  </script>
</body>
</html>
```

`init.js` belongs to ProxyCrab: do not include it in the ZIP. Its export is ready after normal ES module initialization. No API URLs, token handling, npm SDK, global lookup, callback, or initialization function is needed. For an external module under `assets/`, import `../init.js`. Preserve that runtime import when using a bundler; the simplest package is plain HTML and native modules. Use relative asset URLs and hash routing. Content is rendered through DOM text APIs; treat captured traffic as untrusted data.

The package root must also contain `manifest.json`:

```json
{"version":1,"id":"request-summary","name":"Request summary","description":"Recent requests","backend_version":1}
```

ID: 1–100 lowercase ASCII letters, digits, `-`, or `_`. Name: nonblank, up to 256 UTF-8 bytes. Description is optional. ZIP the **contents**, not their parent directory. Limits: 128 MiB compressed and expanded, 4096 entries. Unsafe paths, symlinks, duplicates, unsupported versions, non-UTF-8 index HTML, and a packaged root `init.js` are rejected. Same-ID replacement must be explicitly accepted.

The exported [Backend declaration](session-page-backend.d.ts) is generated from application types. It uses the application's method names and signatures. Shared read methods are present; other capabilities are optional and generally absent. Test method presence before displaying an optional feature. `listSessions()` contains only the selected Session. `getLogIds` / `getLogViews` are stateless; pass the selected Session ID for consistency. The server enforces scope regardless of supplied IDs. Script lists contain names/display labels with empty source. Reads apply the effective redaction policy.

Common methods:

- `getLogIds({session_id, filter?, limit?, max_id?})`: newest matching IDs. Use the smallest ID as exclusive `max_id` for the next page.
- `getLog(sessionId, id)`: full request/response metadata.
- `fetchBody({kind:"log",sessionId,id}, "request" | "response", maxSize?)`: `{bytes, decodedSize, contentType}`; default limit 16 MiB. Snapshot bodies use `kind:"interceptor"` and `executionId`.
- `getLogViews({session_id,logs:[{id}],view?})`: up to 200 rows per request.
- `getWebSocketMessages?.(...)`, `fetchWebSocketMessageBody?.(...)`: captured WebSocket data.
- `getSessionView(sessionId)`, `getSessionInterceptors(sessionId)`: current read-only configuration.
- `subscribeChanges(handler)`: resolves to an unsubscribe function. It reports view/config/proxy changes; it is **not** a captured-log stream. Poll log IDs/details separately when needed, avoid overlapping polls, and clear timers/unsubscribe when unmounting.
- `saveFile({filename})`: append bytes to the returned writer, then `markAsFinish()`; use `markAsError()` on failure. Downloads remain browser managed.

Management:

```bash
node <skill-dir>/scripts/session-page-import.mjs --file page.zip
# Only after acceptance of replacing the same package ID:
node <skill-dir>/scripts/session-page-import.mjs --file page.zip --replace true
```

Read `/api/agents.md` first, as for other management workflows. `GET /api/session-pages` lists manifests and sizes. `GET /api/session-pages/{id}` exports the original ZIP to the owner; `DELETE` deletes it. Upload is a raw `application/zip` body to `POST /api/session-pages?replace=false`. Import/delete default to approval. These packages are separate from Session configuration bundles.

To include pages in a Session share, set `session_share_config.session_pages` to an array of IDs through `POST /api/session-shares`. Omission preserves the selection; `[]` removes it. Missing packages are cleaned automatically. Keep other settings, including persistence, as requested. Every selected page uses `/session-pages/<id>/?token=...` with the **same** Session token. Files below that prefix map to ZIP paths; resources use package-scoped HTTP-only cookies and recheck the original authorization. Revoking a page or share rejects later resource requests. Sharing does not offer the original ZIP download to visitors.

The desktop and CLI owner UI expose Assets → Session pages for package details and import/export/delete. To run a page, include it in a Session share and open its sharing link. Never infer the Session from resource cookies: Backend stays bound to the entry's own sharing token.
