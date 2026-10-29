# Session configuration bundles

A sharing link can optionally allow **Download Session configuration**, alongside Export HAR. Enable it with a switch in the link settings and pick assets from the workspace resource tree rendered inline below it (collapsed behind a summary row); an empty selection is allowed. A checked file adds an exact asset ID, a checked directory adds a prefix ending in `/`; rows covered by a directory prefix are shown checked but read-only, and the summary row totals rules, matched resources and size. Every change is saved immediately. Each selector is an exact asset ID or a prefix ending in `/`. Prefixes include nested resources and future additions. Export reads saved session configuration, referenced scripts (including disabled interceptors, custom columns and filters), and matching resources at download time. Missing scripts, missing exact IDs, or empty prefix matches fail explicitly. Lua and assets are original files; no Lua dependency analysis is performed.

The ZIP mirrors workspace-relative paths. It contains `workspace_schema.json`, `sessions/<source-id>/{metadata,view,interceptors}.json`, required `scripts/{request,response,column,filter}/...lua`, selected `assets/...` and matching `assets/.metadata/...json`. It excludes capture history, global proxy settings and credentials. Source session IDs identify package directories; import assigns new local IDs and creation times.

`intents.json` is descriptive:

```json
{"version":1,"intents":[{"id":"session-123","type":"session","path":"sessions/123","assets":["mock/login/","certs/demo.pem"]}]}
```

IDs must be nonempty and unique within the package. An intent's asset selectors apply only to package contents during import. Only explicitly accepted intents, their directly referenced scripts and selected assets are imported; shared dependencies are deduplicated. Unsupported intent types are visible but cannot be selected in the UI.

## Import

Drag a ZIP into the desktop or management Web application; a full-window drop zone appears while files are dragged. The frontend previews intents locally, initially unselected, with the package name/size and an intent count in the window header. Conflicts are resolved in a table with per-row handling plus bulk reuse/overwrite, and a successful import offers to open the new sessions. Share pages expose no import capability. The backend independently validates the uploaded ZIP and accepted IDs.

- `POST /api/imports`: multipart fields `file` (ZIP) and `accepted_intent_ids` (nonempty JSON string array). The backend keeps the original ZIP in a temporary directory, validates dependencies, and reports each script/resource as `new`, `same`, or `conflict`. If no conflicts remain, it executes immediately.
- `POST /api/imports/{id}/resolve`: JSON array of `{path, action, fingerprint, name?}`. Supply the exact `fingerprint` returned for every conflicting target. `keep` reuses local content; `overwrite` replaces it. Scripts also support `rename`, with `name` excluding `.lua`; the imported session references are updated. Resources always retain their asset IDs. Rename targets must not exist or collide with other selected files.

Both operations require independent write permissions, each defaulting to approval, and return `{import_id, files, sessions}`. A non-null opaque `import_id` means conflicts remain and no files or sessions were imported. Pending imports are bound to the manager/workspace, expire after 30 minutes and are bounded to 16 packages; expired packages are purged on import operations or manager shutdown. Resolution uses the stored package and accepted IDs, requiring no re-upload. Changed conflicts return a new inspection result for confirmation.

After resolution, required files are placed before session intents execute in manifest order. New sessions are published only when the entire batch succeeds. Placement/intent failure restores overwritten files and removes newly created files and sessions; rollback errors are reported explicitly. Import does not activate sessions. External modification of the workspace during import is unsupported. Packages are limited to 128 MiB compressed/uncompressed and 4096 entries; unsafe paths, symlinks, duplicate files and unsupported workspace schemas are rejected.

## Sharing API

`POST /api/session-shares` accepts `session_share_config.session_config_share`: `{ "assets": ["mock/login/"] }` enables it, `null` disables it, omission preserves the current setting. Existing `persistent` behavior is unchanged; persistent links save their configuration-sharing settings too.

`GET /share-api/bootstrap` and `GET /share-api/capabilities` report `session_config_download`. The share backend exposes its download method only when authorized; existing pages poll for changes. `GET /share-api/session-config?token=...` checks the token and capability each time and returns a live ZIP. The client cannot expand the authorized resource selection by passing asset selectors to this endpoint. Disabling configuration downloads preserves the viewing link and rejects subsequent downloads.
