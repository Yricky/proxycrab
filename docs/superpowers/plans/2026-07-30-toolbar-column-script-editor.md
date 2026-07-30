# Toolbar Menus and Column Script Editor Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the toolbar utility-icon cluster with three categorized menus and turn the custom-column manager into a split script-management and Lua-editing workspace with saved-script debugging.

**Architecture:** Keep window launching and the existing backend contracts intact. The toolbar owns one small dropdown state, while `ColumnManagerWindow.vue` owns script selection, editing, rename/delete/create operations, and calls the existing `getLogViews` endpoint with a one-column temporary view after saving. Register Lua tokenization once in the Monaco bootstrap and add a close guard to the existing floating-window store for dirty editor state.

**Tech Stack:** Vue 3 Composition API, TypeScript, Monaco Editor, Tauri backend abstraction, existing Rust management API.

---

### Task 1: Categorize the toolbar launchers

**Files:**
- Modify: `src/components/AppToolbar.vue`

- [ ] **Step 1: Replace the six icon launchers**

Render three text-and-chevron buttons named `脚本`, `小工具`, and `系统`. Map their actions exactly as follows:

```text
脚本: 拦截器, 自定义列
小工具: Base64 编解码
系统: 证书管理, 系统日志, 设置, 浅色, 深色, 跟随系统
```

- [ ] **Step 2: Add dropdown interaction**

Track one active menu, position each panel below its trigger, close it after an action, and close it on outside pointer interaction or Escape. Mark the current theme entry as selected and place a divider before the theme choices.

- [ ] **Step 3: Verify the frontend types**

Run:

```bash
pnpm exec vue-tsc --noEmit
```

Expected: exit code 0.

### Task 2: Add the split custom-column script workspace

**Files:**
- Modify: `src/windows/ColumnManagerWindow.vue`
- Modify: `src/windows/launcher.ts`

- [ ] **Step 1: Remove session-view editing**

Delete the `getSessionView`/`replaceSessionView` calls and all UI for the `可见列` list. The manager must only operate on global column scripts.

- [ ] **Step 2: Build list and editor panes**

Render a fixed-width left script list and a flexible right workspace. Select the first script on initial load, select and focus a newly created script, and show an empty state when no scripts exist.

- [ ] **Step 3: Support save, rename, and delete**

Keep explicit save plus `Ctrl/Cmd + S`. Rename in place with Enter to confirm and Escape to cancel, calling:

```ts
await backend.updateColumnScript(oldName, { name: nextName });
```

The backend already updates references in every session view. Keep current delete semantics, including allowing stale view references after confirmation.

- [ ] **Step 4: Add saved-script debugging**

Keep `保存`, the Log ID input, `保存并运行`, and the single-line output in the editor's top toolbar. Save first, then evaluate only the selected script against the current viewed session:

```ts
await backend.getLogViews({
  session_id: sessionsStore.viewingSessionId,
  logs: [{ id: logId }],
  view: {
    columns: [{ kind: "script", script_name: selectedName, width: 160 }],
  },
});
```

Display `rows[0].cells[0]` on success. Display the returned exception or request error on failure. Never persist this temporary one-column view.

- [ ] **Step 5: Increase the manager's default size**

Open the split manager at a size suitable for a script list and code editor, while preserving floating-window resize behavior.

### Task 3: Add Lua highlighting and dirty-state safeguards

**Files:**
- Modify: `src/monaco.ts`
- Modify: `src/components/MonacoEditor.vue`
- Modify: `src/stores/windows.ts`
- Modify: `src/components/FloatingWindow.vue`
- Modify: `src/windows/ColumnManagerWindow.vue`

- [ ] **Step 1: Register Lua with Monaco**

Register a `lua` language and Monarch tokenizer once from the Monaco bootstrap. Cover comments, strings, numbers, operators, Lua keywords, and standard built-ins so every component using `language="lua"` receives highlighting.

- [ ] **Step 2: Expose editor focus**

Expose a minimal `focus()` method from `MonacoEditor.vue` so selecting or creating a script can focus the embedded editor without coupling the manager to Monaco internals.

- [ ] **Step 3: Add floating-window close guards**

Allow a window component to register an async close guard by window ID. The column manager registers a guard that asks whether to discard dirty content; unregister it on unmount.

- [ ] **Step 4: Guard destructive editor transitions**

Before switching scripts, deleting the selected script, or closing the manager, show a discard confirmation when the current content is dirty. Keep the current selection/content when the user cancels.

### Task 4: Verify the complete change

**Files:**
- Check all modified files

- [ ] **Step 1: Run formatting checks**

Run:

```bash
cargo fmt --all -- --check
git diff --check
```

Expected: both commands exit 0.

- [ ] **Step 2: Run automated tests and build**

Run:

```bash
cargo test --workspace
pnpm run build
```

Expected: all Rust tests pass, Vue type checking passes, and Vite completes a production build.

- [ ] **Step 3: Review requirement coverage**

Confirm the diff contains the three toolbar menus, the split column-script manager, create/edit/rename/delete, save and save-run controls in one top toolbar, current-session Log ID debugging through `getLogViews`, Lua highlighting for all Monaco script editors, and dirty-state safeguards.
