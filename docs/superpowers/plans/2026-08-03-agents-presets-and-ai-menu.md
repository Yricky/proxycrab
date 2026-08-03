# AGENTS.md Presets and AI Menu Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add workspace-scoped AGENTS.md behavior presets, expose the active preset as raw text, make capture filtering optionally stateless, and provide a restrained AI menu and preset editor.

**Architecture:** Store preset metadata in `agents/config.json` and Markdown bodies in `agents/presets/<id>.md`, initialized idempotently by the management layer when it opens the current workspace. Expose CRUD to the Tauri UI through the manager abstraction, expose only the active Markdown through `GET /api/agents.md`, and keep the existing `/api/logs/ids` behavior unless callers explicitly pass `persist_filter: false`. AGENTS.md behavior remains outside the MITM runtime crate.

**Tech Stack:** Rust 2024, Axum, Tauri 2, Vue 3, TypeScript, Monaco Editor, Node.js Skill scripts.

---

### Task 1: Management-layer workspace preset store

**Files:**
- Create: `crates/proxy-crab-mgr/src/agents.rs`
- Modify: `crates/proxy-crab-mgr/src/lib.rs`
- Modify: `crates/proxy-crab-mgr/src/dto.rs`
- Modify: `crates/proxy-crab-mgr/src/manager.rs`

- [x] **Step 1: Add failing storage and migration tests**

```rust
#[test]
fn initializes_two_default_agent_presets_and_activates_full_access() { /* assert files/config */ }

#[test]
fn agent_presets_support_create_update_activate_delete_and_reimport() { /* assert invariants */ }
```

- [x] **Step 2: Run the focused tests and confirm they fail**

```bash
cargo test -p proxy-crab-mgr agents
```

- [x] **Step 3: Implement the preset store and v3 migration**

```text
agents/config.json
agents/presets/full-capability.md
agents/presets/quiet-investigation.md
```

The store must initialize once when `agents/config.json` is absent, keep names unique, prevent deleting the final preset, select the next preset after deleting the active one, read Markdown from disk for every active-content request, and preserve the active selection when defaults are reimported.

- [x] **Step 4: Run the focused tests**

```bash
cargo test -p proxy-crab-mgr agents
```

### Task 2: Manager and Tauri preset operations

**Files:**
- Modify: `crates/proxy-crab-mgr/src/manager.rs`
- Modify: `crates/proxy-crab-mgr/src/dto.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src/api/types.ts`
- Modify: `src/api/backend.ts`
- Modify: `src/api/tauri-backend.ts`

- [x] **Step 1: Add manager tests for validation and state transitions**

```rust
let created = manager.create_agents_preset(CreateAgentsPresetRequest { name: "自定义".into() }).await?;
manager.update_agents_preset(created.id.clone(), UpdateAgentsPresetRequest { name: None, content: Some("# Rules".into()) }).await?;
manager.activate_agents_preset(created.id.clone()).await?;
assert_eq!(manager.agents_markdown().await?, "# Rules");
```

- [x] **Step 2: Implement manager trait methods and Tauri commands**

Expose list/create/update/delete/activate/reimport operations to the frontend while keeping HTTP preset management read-only.

- [x] **Step 3: Verify Rust compilation and focused tests**

```bash
cargo test -p proxy-crab-mgr agents
cargo check -p proxy-crab-tauri
```

### Task 3: Raw AGENTS.md endpoint and stateless filtering

**Files:**
- Modify: `crates/proxy-crab-mgr/src/http.rs`
- Modify: `crates/proxy-crab-mgr/src/dto.rs`
- Modify: `crates/proxy-crab-mgr/src/manager.rs`
- Modify: `src/api/types.ts`

- [x] **Step 1: Add failing HTTP and manager tests**

```rust
// GET /api/agents.md returns Content-Type text/plain and the unwrapped active Markdown.
// POST /api/logs/ids with persist_filter=false returns filtered IDs without changing SessionView.filter.
// Omitting persist_filter retains the existing persistent behavior.
```

- [x] **Step 2: Implement `GET /api/agents.md`**

Return the active file as a raw body with `Content-Type: text/plain; charset=utf-8`, outside the standard JSON envelope.

- [x] **Step 3: Implement `persist_filter`**

```rust
pub struct LogIdsRequest {
    // existing fields
    #[serde(default = "default_true")]
    pub persist_filter: bool,
}
```

Preserve compatibility by treating an omitted field as `true`; skip `replace_session_view` and the UI change event when false.

- [x] **Step 4: Run focused tests**

```bash
cargo test -p proxy-crab-mgr http
cargo test -p proxy-crab-mgr log_ids
```

### Task 4: AI menu and AGENTS.md editor

**Files:**
- Create: `src/windows/AgentsPresetsWindow.vue`
- Modify: `src/windows/launcher.ts`
- Modify: `src/components/AppToolbar.vue`
- Modify: `src/api/types.ts`
- Modify: `src/api/backend.ts`
- Modify: `src/api/tauri-backend.ts`

- [x] **Step 1: Add frontend types and backend calls**

```ts
export interface AgentsPreset { id: string; name: string; content: string }
export interface AgentsPresetState { active_id: string; presets: AgentsPreset[] }
```

- [x] **Step 2: Build the split preset editor**

Use a compact sidebar for create/select/activate/delete and a Markdown Monaco editor for rename/content/save. Register a close guard, support Cmd/Ctrl+S, confirm destructive default reimport, and preserve dirty content across selection/close prompts.

- [x] **Step 3: Add and restyle the AI menu**

```text
AI
  AGENTS.md 预设…
  安装 ProxyCrab Skill…
脚本
小工具
系统
```

Render all top-level menu triggers as borderless, compact menu-bar labels with restrained hover/open feedback; keep proxy lifecycle buttons unchanged.

- [x] **Step 4: Verify frontend type checking and build**

```bash
npm run build
```

### Task 5: Skill, scripts, evaluations, and documentation

**Files:**
- Modify: `skills/proxycrab/SKILL.md`
- Modify: `skills/proxycrab/references/http-api.md`
- Modify: `skills/proxycrab/references/best-practices.md`
- Modify: `skills/proxycrab/scripts/log-query.mjs`
- Modify: `skills/proxycrab/scripts/log-wait.mjs`
- Create: `skills/proxycrab/scripts/agents-get.mjs`
- Modify: `skills/proxycrab/evals/evals.json`
- Modify: `docs/backend-api.md`
- Modify: `README.md`

- [x] **Step 1: Make Skill bootstrap behavior explicit**

Require `GET /api/agents.md` immediately after reading the Skill. State precedence as current user request, then active AGENTS.md preset, then conservative built-in fallback when the endpoint is unavailable.

- [x] **Step 2: Make Skill queries stateless by default**

Have bundled log query/wait scripts send `persist_filter: false` whenever they supply a filter, and document how callers intentionally opt into persistence through the raw API.

- [x] **Step 3: Document endpoint and UI behavior**

Document the raw text exception, workspace preset layout, default policies, import semantics, and the fact that reading `/api/agents.md` has no UI side effect.

- [x] **Step 4: Add evaluation coverage**

```json
{
  "prompt": "Use ProxyCrab to investigate a failed request while respecting the active AGENTS.md preset.",
  "expected_output": "The agent reads /api/agents.md first and uses stateless filtering under the quiet preset."
}
```

### Task 6: Full verification

**Files:**
- Verify all modified files

- [x] **Step 1: Format and test Rust**

```bash
cargo fmt --all --check
cargo test --workspace
```

- [x] **Step 2: Validate frontend and Skill scripts**

```bash
npm run build
node --check skills/proxycrab/scripts/agents-get.mjs
node --check skills/proxycrab/scripts/log-query.mjs
node --check skills/proxycrab/scripts/log-wait.mjs
```

- [x] **Step 3: Inspect the final diff**

Confirm every change traces to the requested feature and that pre-existing uncommitted work remains intact.
