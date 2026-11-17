# Session pages

A Session page is a reusable static website stored as one ZIP under `<workspace>/session-pages/<id>.zip`. Assets → 会话页面 opens a manager with a page list, package details and import/export/delete. Replacing an existing ID requires confirmation and replaces the entire ZIP. Delete warns that shared page links will stop working. Open a page through its Session sharing link to view Session contents.

[Page authoring guide and example](../skills/proxycrab/references/session-pages.md) covers the manifest, native `import { backend } from './init.js'`, relative paths, Backend methods and package import. [Standalone Backend types](../skills/proxycrab/references/session-page-backend.d.ts) are generated with `node scripts/export-session-page-types.mjs`; `--check` verifies that they match the current application contract.

A runnable example lives in `examples/session-pages/request-summary/`. From that directory, run `zip -r ../request-summary.zip manifest.json index.html`, then import the resulting ZIP through the manager.

## Storage and runtime

`session_page::PageStore` validates all ZIP entries before an atomic installation. No package files are permanently extracted. Import is limited to 128 MiB compressed/expanded and 4096 entries, rejects unsafe paths/duplicates/symlinks/unsupported versions and reserves root `init.js`. Resource reads open the ZIP and read the requested entry on the management blocking-task limiter. `/session-pages/<id>/` serves root `index.html`; other suffixes are ZIP paths. A missing trailing slash redirects preserving the query, missing files return 404, and no SPA fallback is applied. Use hash routes for page navigation.

The application owns `init.js`, which re-exports the initialized Backend from the stable built runtime entry. Native module initialization awaits share bootstrap. Page authors do not implement a transport or authorization. The runtime shares the existing read-only Backend implementation and effective redaction policy. The public page type preserves common methods as required and host capabilities as optional, reflecting their absence in the share implementation. The internal full application interface remains unchanged.

## Sharing

SessionShareConfig adds optional `session_pages: string[]`. Omission preserves selection, `[]` removes it, and all selected page links share the Session's existing token. Selection changes do not rotate tokens; persistence includes page IDs. Share reads/updates automatically remove missing packages, persisting cleanup for durable shares. No reference counting or eager deletion cascade is used. Page updates keep paths and tokens; a refreshed page uses the new package.

The entry requires a valid token whose selected packages include the requested ID. It sets an HTTP-only SameSite=Lax Cookie scoped to the package prefix. Cookies use token-specific names to support multiple Sessions in one browser. Resources check the token's current package authorization and Session availability on every request, carry no-store/no-referrer headers and correct MIME types, and reject traversal after URL decoding. Invalid or duplicate token query parameters cannot fall back to a Cookie. Resource cookies do not choose the Backend Session. Revocation prevents future reads; previously delivered content cannot be withdrawn.

## Management API

| Route | Data | Default |
| --- | --- | --- |
| `GET /api/session-pages` | Manifest fields and compressed `size` for each package | Allow |
| `POST /api/session-pages?replace=false` | Raw ZIP body; returns installed package metadata; conflict unless replacement accepted | Approval |
| `GET /api/session-pages/{id}` | Original ZIP as attachment | Allow |
| `DELETE /api/session-pages/{id}` | Delete the package | Approval |

Management APIs use existing authentication and permission handling; package/share routes accept their scoped tokens. Owner Backends expose page management methods; share Backends omit them. Package import/delete emit `session_pages` changes so open managers and sharing settings refresh. Visitor ZIP downloads and in-page page-switcher navigation are outside this version.
