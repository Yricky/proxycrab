# Clash Subscription Implementation Plan

> **For agentic workers:** Execute inline, following the existing manager, HTTP permission, and frontend Backend contracts.

**Goal:** Add a second tab to certificate management for a temporary, token-protected Clash subscription, selectable local IPv4, QR code, and client import.

**Architecture:** Keep subscription state in the shared manager's memory, initially disabled. Management routes use normal API permissions; `/clash.yaml?token=…` independently authorizes download. The YAML always uses the selected non-loopback IPv4 and current configured proxy port. Desktop and CLI expose an optional Backend capability; the existing certificate content remains the default tab.

**Tech Stack:** Rust/Axum/Tauri, Vue/TypeScript, existing qrcode package.

## Contract

```text
GET    /api/clash-subscription              -> {enabled, server?, token?}
PUT    /api/clash-subscription {server: IP}  -> enable/update, preserve token while enabled
DELETE /api/clash-subscription              -> {enabled:false}, invalidate old token
GET    /clash.yaml?token=TOKEN              -> YAML or 404; no-store
```

```yaml
mode: rule
proxies:
  - name: ProxyCrab
    type: http
    server: SELECTED_IPV4
    port: CURRENT_PROXY_PORT
rules:
  - MATCH,ProxyCrab
```

## Steps

- [x] Add `crates/proxy-crab-mgr/src/clash_subscription.rs`: ephemeral state, IPv4 validation, random token, constant-time token verification, download response headers. Extend `manager.rs` and register routes/permissions/change resource in `http.rs`, `permission.rs`, and `dto.rs`.
- [x] Verify disabled/enable/update/revoke/re-enable/restart behavior; reject unusable IPs and malformed/missing tokens; verify proxy stopped support and fresh port rendering. Verify management authorization and change events.
- [x] Add Tauri commands and optional `Backend.clashSubscription` implementations for desktop/CLI. Permit only the Clash import URL scheme in the desktop opener capability.
- [x] Add top tabs to `CertManagerWindow.vue`, with `1.证书配置` selected initially. Put connection controls in a focused component, reusing the common switch/button styles and qrcode. Filter local IPv4 candidates, update state through the backend, and clear stale links/QR images while updates are pending.
- [x] Show copyable HTTP subscription URL and QR, plus `clash://install-config?url=ENCODED_URL` import button. Show stopped-proxy, unavailable-HTTP-service, and empty-IP states. Explain that disabling only revokes subscription retrieval and that restart resets it.
- [x] Update README, backend API reference, bundled Skill documentation/script/eval. Preserve all unrelated staged work.
- [x] Run `cargo test -p proxy-crab-mgr`, `cargo check --workspace`, `pnpm test:unit`, `pnpm build`, and Skill script tests. Check the generated YAML with the installed Mihomo binary and inspect the UI with an isolated CLI workspace.

## Accepted behavior

No configuration editor or multiple routing schemes in this iteration. Enabling the subscription does not start or stop the proxy. State and selected IP are not persisted across application restarts. QR encodes the HTTP download URL; import invokes the OS handler for Clash. Existing imported configurations remain usable after disabling the subscription.

## Verification results

- `cargo test -p proxy-crab-mgr`: 78 unit tests and 3 permission contract tests passed.
- `cargo test -p proxy-crab-t http_permissions --lib`: 10 tests passed.
- `cargo check --workspace`: passed.
- `pnpm test:unit`: 47 tests passed; `pnpm build`: passed.
- `node --test skills/proxycrab/scripts/lib/common.test.mjs`: 6 tests passed.
- Isolated CLI browser check: certificate default tab, LAN IPv4 selection, subscription enable while proxy stopped, HTTP YAML download without Bearer, QR rendering, copy, encoded Clash import URL, tab state retention, revoke/re-enable, and no browser errors passed. The import URL was intercepted to avoid modifying the user's installed Clash profiles.
- Installed Mihomo `-t` validation accepted the actual downloaded YAML.
- Restarting the isolated CLI reset the switch and invalidated the old subscription token. Test processes stopped afterward.

## Follow-up: direct CA download

The user requested removing the old proxy virtual-host implementation entirely.

- [x] Remove special CA handling from the HTTP entry, MITM TLS entry, and post-interceptor response path.
- [x] Serve the current public CA at `GET /ca.crt` on the management server without credentials, using certificate MIME type, an attachment filename, and `no-store`. Retain the permission-controlled JSON management read.
- [x] Lift IP/service/subscription state into the certificate window so both tabs share one selector, refresh, and listener. Certificate links and QR codes become direct HTTP URLs.
- [x] Replace old special-case integration tests with ordinary interceptor/capture regression tests. Add a public-download test covering stopped proxy, no private key, and regeneration.
- [x] Update project/Skill references, add the certificate download script, and add an evaluation scenario.
- [x] Run manager tests, proxy integration tests, frontend checks, workspace compile, and isolated browser checks for both tabs.

Follow-up verification: 79 manager unit tests, 3 permission contract tests, 45 proxy integration tests, 47 frontend tests, and 6 Skill helper tests passed. Workspace compile and frontend production build passed. An isolated CLI browser run verified anonymous LAN certificate download with the proxy stopped, certificate headers and public PEM, clipboard, the download script, shared IP updates across both tabs and subscription YAML, QR rendering after tab changes, and subscription revocation without affecting certificate downloads. Both tabs were visually inspected; no browser errors occurred. The isolated test server was stopped afterward.
