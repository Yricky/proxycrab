# Canvas Log List Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the DOM log table with a single Canvas-rendered virtual table while preserving its appearance and interactions, adding stable scroll anchoring and new-log following.

**Architecture:** `LogTable.vue` owns fixed-size Canvas drawing, hit testing, dual-axis scroll offsets, and custom scrollbars. It derives only the visible row range from the complete ID list and keeps the existing store viewport hydration buffer. Small pure helpers cover geometry, unseen-ID reconciliation, and scroll anchoring so the behavior is unit tested without a browser.

**Tech Stack:** Vue 3, TypeScript, Canvas 2D, Node test runner, Vite.

---

### Task 1: Add tested virtual-list geometry and unseen-ID helpers

**Files:**
- Create: `src/utils/log-canvas.ts`
- Create: `src/utils/log-canvas.test.mts`

- [ ] **Step 1: Write failing tests**

  Cover fully visible row bounds with partial top/bottom rows, clamped scroll offsets, row-anchor compensation after insertion/removal, and unseen-ID reconciliation for additions, removals, and re-entry.

- [ ] **Step 2: Run the focused tests and confirm failure**

  Run: `node --experimental-strip-types --test src/utils/log-canvas.test.mts`
  Expected: FAIL because `src/utils/log-canvas.ts` does not exist.

- [ ] **Step 3: Implement the minimal helpers**

  Add `clamp`, `fullyVisibleRowRange`, `anchoredScrollTop`, and `reconcileUnseenIds`. The visible range uses the body viewport only; anchoring applies `(newIndex - oldIndex) * rowHeight`; reconciliation retains unseen IDs still present and adds IDs absent from the previous list.

- [ ] **Step 4: Run the focused tests**

  Run: `node --experimental-strip-types --test src/utils/log-canvas.test.mts`
  Expected: PASS.

### Task 2: Replace DOM rows and scrolling with one Canvas

**Files:**
- Modify: `src/components/LogTable.vue`

- [ ] **Step 1: Introduce Canvas viewport state**

  Replace the DOM scroller/body/window with a fixed Canvas and a `ResizeObserver`. Track `scrollTop`, `scrollLeft`, viewport dimensions, device-pixel ratio, content dimensions, and visible/buffered row ranges. Continue calling `logsStore.setViewportIds` with a 500-row buffer.

- [ ] **Step 2: Draw the existing table appearance**

  Draw the fixed header, alternating rows, hover state, cell borders, clipped ellipsized mono text, code colors, error backgrounds, outcome dots, add-column area, empty state, and theme colors. Observe theme changes and reactive visible-row data to schedule one animation-frame redraw.

- [ ] **Step 3: Preserve table interactions through hit testing**

  Hit test header cells, resize gutters, body rows/cells, the new-log capsule, and both scrollbars. Preserve ID sorting, column menus, add column, live column resizing and persistence, row opening, cell context-copy, row hover, wheel/trackpad scrolling, scrollbar thumb dragging outside the Canvas, and track-click direct jumps.

### Task 3: Add stable following and unseen-new-log behavior

**Files:**
- Modify: `src/components/LogTable.vue`
- Test: `src/utils/log-canvas.test.mts`

- [ ] **Step 1: Track follow state and unseen IDs**

  Reset state on Session/filter reload. When the maximum displayed ID is fully visible, keep following new IDs and clear unseen state. Otherwise reconcile an ID set so updates to existing rows do not increment the count and removed IDs disappear from it.

- [ ] **Step 2: Keep reading position stable**

  Before a list change, capture the first fully visible row ID and its index. When not following, find that ID in the new sorted list and adjust `scrollTop` by its index delta so the same row remains at the same Canvas position.

- [ ] **Step 3: Draw and activate the directional capsule**

  Draw a centered top capsule with an up arrow for descending order and a bottom capsule with a down arrow for ascending order. Display only arrow plus unseen count. Fully visible unseen rows decrement the count; clicking jumps to the maximum-ID edge, clears it, and resumes following.

### Task 4: Verify behavior and project health

**Files:**
- Review: `src/components/LogTable.vue`
- Review: `src/utils/log-canvas.ts`
- Review: `src/utils/log-canvas.test.mts`

- [ ] **Step 1: Run frontend tests and build**

  Run: `npm run test:unit`
  Run: `npm run build`
  Expected: all tests pass and Vue/TypeScript/Vite build succeeds.

- [ ] **Step 2: Run static diff checks**

  Run: `git diff --check`
  Expected: no whitespace errors.

- [ ] **Step 3: Visually exercise the local app**

  Verify light/dark rendering, wheel and trackpad scrolling, both custom scrollbars, sorting, menus, resizing, row/cell actions, stable anchored content, directional unseen counts, capsule jumping, loading, and empty states.
