<script setup lang="ts">
import { onMounted, ref } from "vue";
import { sessionsStore } from "../stores/sessions";
import { routingStore } from "../stores/routing";
import { proxyStore } from "../stores/proxy";
import { confirmDialog, openContextMenu } from "../stores/dialog";
import { formatRelativeTime } from "../utils/format";
import { appStore } from "../stores/app";
import {
  openArchivedSessions,
  openBypass,
  openRoutingManager,
  openSessionExportShare,
} from "../windows/launcher";
import type { SessionMetadata } from "../api/types";
import {
  Io5Add,
  Io5Archive,
  Io5Create,
  Io5Eye,
  Io5Link,
  Io5RadioButtonOn,
} from "vue-icons-plus/io5";
import { IoArrowForwardCircle } from "vue-icons-plus/io";

const creating = ref(false);
const editingId = ref<number | null>(null);
const editName = ref("");
const editDesc = ref("");

function formatActivityCount(count: number): string {
  return count > 99 ? "99+" : String(count);
}

/* ---- 侧边栏宽度拖拽调整 ---- */
const SIDEBAR_MIN = 160;
const SIDEBAR_MAX = 480;
const SIDEBAR_DEFAULT = 240;
const SIDEBAR_STORAGE_KEY = "proxycrab.sidebarWidth";

function sidebarMaxWidth(): number {
  // 不超过窗口的 60%，防止窗口过窄时侧边栏吃掉全部空间
  return Math.min(SIDEBAR_MAX, Math.max(SIDEBAR_MIN, Math.floor(window.innerWidth * 0.6)));
}

function initialSidebarWidth(): number {
  const saved = Number(localStorage.getItem(SIDEBAR_STORAGE_KEY));
  if (!Number.isFinite(saved)) return SIDEBAR_DEFAULT;
  return Math.min(sidebarMaxWidth(), Math.max(SIDEBAR_MIN, saved));
}

const sidebarWidth = ref(initialSidebarWidth());
const dragging = ref(false);
let dragStartX = 0;
let dragStartWidth = 0;

function onResizePointerDown(e: PointerEvent): void {
  if (e.button !== 0) return;
  dragging.value = true;
  dragStartX = e.clientX;
  dragStartWidth = sidebarWidth.value;
  (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
  e.preventDefault();
  document.body.style.userSelect = "none";
}

function onResizePointerMove(e: PointerEvent): void {
  if (!dragging.value) return;
  const width = Math.round(dragStartWidth + (e.clientX - dragStartX));
  sidebarWidth.value = Math.min(sidebarMaxWidth(), Math.max(SIDEBAR_MIN, width));
}

function endResize(e: PointerEvent): void {
  if (!dragging.value) return;
  dragging.value = false;
  (e.currentTarget as HTMLElement).releasePointerCapture?.(e.pointerId);
  document.body.style.userSelect = "";
  localStorage.setItem(SIDEBAR_STORAGE_KEY, String(sidebarWidth.value));
}

async function createSession(): Promise<void> {
  if (creating.value) return;
  creating.value = true;
  const session = await sessionsStore.create("", null);
  creating.value = false;
  if (!session) return;
  sessionsStore.view(session.id);
  appStore.toast("会话已创建", "success");
}

function startEdit(session: SessionMetadata): void {
  editingId.value = session.id;
  editName.value = session.name;
  editDesc.value = session.description ?? "";
}

async function submitEdit(): Promise<void> {
  if (editingId.value === null) return;
  const name = editName.value.trim();
  if (!name) return;
  const updated = await sessionsStore.update(
    editingId.value,
    name,
    editDesc.value.trim() || null,
  );
  if (updated) {
    editingId.value = null;
    appStore.toast("会话信息已更新", "success");
  }
}

async function archiveSession(id: number, name: string): Promise<void> {
  const ok = await confirmDialog({
    title: "归档会话",
    message: `确定归档会话「${name}」吗？抓包记录会完整保留，可在已归档 Session 中恢复。`,
    confirmText: "归档",
  });
  if (ok && (await sessionsStore.archive(id))) {
    appStore.toast(`「${name}」已归档`, "success");
  }
}

async function toggleActive(session: SessionMetadata): Promise<void> {
  const active = sessionsStore.activeSessionId === session.id;
  if (await sessionsStore.replaceActive(active ? null : session.id)) {
    appStore.toast(active ? "已取消活跃会话" : `「${session.name}」已设为活跃会话`, "success");
  }
}

function sessionMenu(event: MouseEvent, session: SessionMetadata): void {
  openContextMenu(event, [
    { label: "查看", icon: Io5Eye, action: () => sessionsStore.view(session.id) },
    {
      label: "导出和分享",
      icon: Io5Link,
      action: () => openSessionExportShare(session.id, session.name),
    },
    { label: "编辑会话", icon: Io5Create, action: () => startEdit(session) },
    {
      label: sessionsStore.activeSessionId === session.id ? "取消活跃" : "设为活跃",
      icon: Io5RadioButtonOn,
      action: () => void toggleActive(session),
    },
    {
      label:
        sessionsStore.activeSessionId === session.id
          ? "归档（请先取消活跃）"
          : "归档",
      icon: Io5Archive,
      dividerBefore: true,
      disabled: sessionsStore.activeSessionId === session.id,
      action: () => void archiveSession(session.id, session.name),
    },
  ]);
}

onMounted(() => {
  void routingStore.refresh();
});
</script>

<template>
  <aside class="sidebar" :style="{ width: sidebarWidth + 'px' }">
    <div
      class="sb-resize"
      :class="{ dragging }"
      title="拖动调整侧边栏宽度"
      @pointerdown="onResizePointerDown"
      @pointermove="onResizePointerMove"
      @pointerup="endResize"
      @pointercancel="endResize"
    ></div>
    <div class="sb-top">
      <div class="sb-header">
        <span class="sb-title">会话</span>
        <div class="sb-header-actions">
          <button
            class="btn icon"
            :title="
              proxyStore.activeBypassCount > 0
                ? `透明转发记录（${proxyStore.activeBypassCount} 条活跃）`
                : '透明转发记录'
            "
            :aria-label="
              proxyStore.activeBypassCount > 0
                ? `透明转发记录，${proxyStore.activeBypassCount} 条活跃`
                : '透明转发记录'
            "
            @click="openBypass"
          >
            <span v-if="proxyStore.activeBypassCount > 0" class="activity-badge">
              {{ formatActivityCount(proxyStore.activeBypassCount) }}
            </span>
            <IoArrowForwardCircle v-else :size="15" />
          </button>
          <button
            class="btn icon"
            title="新建会话"
            :disabled="creating"
            @click="createSession"
          >
            <Io5Add :size="16" />
          </button>
        </div>
      </div>
      <button
        class="sb-routing"
        :class="{ empty: !routingStore.selectedName }"
        title="打开分流规则管理器"
        @click="openRoutingManager"
      >
        <Io5RadioButtonOn :size="13" />
        <span v-if="routingStore.selectedName" class="sb-routing-name">
          {{ routingStore.selectedName }}
        </span>
        <span v-else>未设置分流规则</span>
      </button>
    </div>

    <div class="sb-list">
      <div
        v-for="session in sessionsStore.sessions"
        :key="session.id"
        class="sb-item"
        :class="{
          viewing: sessionsStore.viewingSessionId === session.id,
          active: sessionsStore.activeSessionId === session.id,
        }"
        @click="sessionsStore.view(session.id)"
        @contextmenu="sessionMenu($event, session)"
        @dblclick="startEdit(session)"
      >
        <template v-if="editingId === session.id">
          <div class="sb-edit" @click.stop @dblclick.stop>
            <input
              v-model="editName"
              class="input sb-edit-name"
              placeholder="会话名称"
              autofocus
              @keyup.enter="submitEdit"
              @keyup.esc="editingId = null"
            />
            <input
              v-model="editDesc"
              class="input sb-edit-desc"
              placeholder="描述（可选）"
              @keyup.enter="submitEdit"
              @keyup.esc="editingId = null"
            />
            <div class="sb-edit-actions">
              <button class="btn" @click="editingId = null">取消</button>
              <button class="btn primary" :disabled="!editName.trim()" @click="submitEdit">
                保存
              </button>
            </div>
          </div>
        </template>
        <template v-else>
          <div class="sb-item-top">
            <span class="sb-name" :title="session.name">{{ session.name }}</span>
            <span
              v-if="sessionsStore.activeSessionId === session.id"
              class="sb-active"
              :title="
                proxyStore.activeNetlogCount(session.id) > 0
                  ? `活跃会话，${proxyStore.activeNetlogCount(session.id)} 条活跃连接`
                  : '活跃会话'
              "
            >
              <span
                v-if="proxyStore.activeNetlogCount(session.id) > 0"
                class="activity-badge session-activity-badge"
              >
                {{ formatActivityCount(proxyStore.activeNetlogCount(session.id)) }}
              </span>
              <Io5RadioButtonOn v-else :size="11" />活跃
            </span>
          </div>
          <div class="sb-item-meta">
            <span>{{ formatRelativeTime(session.created_at) }}</span>
            <span v-if="session.description" class="sb-desc" :title="session.description">
              {{ session.description }}
            </span>
          </div>
        </template>
      </div>
      <div v-if="!sessionsStore.loading && sessionsStore.sessions.length === 0" class="empty-hint">
        暂无会话，点击右上角 + 新建
      </div>
    </div>
    <button class="sb-archived" @click="openArchivedSessions">
      <Io5Archive :size="14" />
      <span>已归档 Session</span>
    </button>
  </aside>
</template>

<style scoped>
.sidebar {
  width: 240px;
  flex: none;
  position: relative;
  display: flex;
  flex-direction: column;
  background: var(--bg-panel);
  border-right: 1px solid var(--border);
}
.sb-resize {
  position: absolute;
  top: 0;
  right: 0;
  width: 5px;
  height: 100%;
  z-index: 10;
  cursor: col-resize;
  touch-action: none;
}
.sb-resize::after {
  content: "";
  position: absolute;
  top: 0;
  right: 0;
  width: 2px;
  height: 100%;
  background: transparent;
  transition: background 0.12s;
}
.sb-resize:hover::after,
.sb-resize.dragging::after {
  background: var(--accent);
}
.sb-top {
  border-bottom: 1px solid var(--border);
}
.sb-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 8px 12px;
}
.sb-title {
  font-size: 12px;
  font-weight: 600;
  color: var(--text-secondary);
}
.sb-header-actions {
  display: flex;
  align-items: center;
  gap: 2px;
}
.sb-routing {
  width: calc(100% - 16px);
  box-sizing: border-box;
  min-height: 30px;
  margin: 0 8px 7px;
  padding: 5px 9px;
  display: flex;
  align-items: center;
  gap: 6px;
  border: 1px solid var(--border);
  border-radius: var(--radius-md);
  background: var(--bg-input);
  color: var(--text-secondary);
  font: inherit;
  font-size: 11px;
  text-align: left;
  cursor: pointer;
}
.sb-routing:hover {
  border-color: var(--border-strong);
  background: var(--bg-hover);
  color: var(--text);
}
.sb-routing.empty {
  border-style: dashed;
  background: transparent;
  color: var(--text-faint);
}
.sb-routing-name {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-family: var(--font-mono);
}
.sb-edit {
  display: flex;
  flex-direction: column;
  gap: 2px;
  padding: 1px 0 2px;
}
.sb-edit-actions {
  display: flex;
  justify-content: flex-end;
  gap: 2px;
  margin-top: 1px;
}
.sb-edit-actions .btn {
  padding: 1px 6px;
  border-color: transparent;
  background: transparent;
  font-size: 11px;
}
.sb-edit-actions .btn.primary {
  border-color: transparent;
  background: transparent;
  color: var(--accent);
}
.sb-edit-actions .btn.primary:hover:not(:disabled) {
  border-color: transparent;
  background: var(--bg-hover);
  color: var(--accent-hover);
}
.sb-list {
  flex: 1;
  overflow-y: auto;
  padding: 4px;
}
.sb-archived {
  flex: none;
  width: 100%;
  min-height: 38px;
  padding: 8px 12px;
  display: flex;
  align-items: center;
  gap: 7px;
  border: 0;
  border-top: 1px solid var(--border);
  background: var(--bg-panel);
  color: var(--text-secondary);
  font: inherit;
  font-size: 12px;
  text-align: left;
  cursor: pointer;
}
.sb-archived:hover {
  background: var(--bg-hover);
  color: var(--text);
}
.sb-item {
  padding: 7px 10px;
  border-radius: var(--radius-md);
  cursor: pointer;
  margin-bottom: 2px;
}
.sb-item:hover {
  background: var(--bg-hover);
}
.sb-item.viewing {
  background: var(--bg-selected);
}
.sb-item.active .sb-name {
  color: var(--accent);
}
.sb-item-top {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 6px;
}
.sb-name {
  font-weight: 500;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.sb-active {
  flex: none;
  display: inline-flex;
  align-items: center;
  gap: 3px;
  color: var(--accent);
  font-size: 10px;
}
.activity-badge {
  min-width: 15px;
  height: 15px;
  padding: 0 3px;
  border-radius: 8px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  background: var(--accent);
  color: white;
  font-size: 9px;
  font-weight: 700;
  line-height: 1;
}
.session-activity-badge {
  min-width: 13px;
  height: 13px;
  padding: 0 2px;
  font-size: 8px;
}
.sb-item-meta {
  display: flex;
  align-items: center;
  gap: 8px;
  min-width: 0;
  font-size: 11px;
  color: var(--text-faint);
  margin-top: 2px;
}
.sb-desc {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.sb-edit .input {
  width: 100%;
  border-color: transparent;
  border-radius: var(--radius-sm);
  background: transparent;
}
.sb-edit .input:hover {
  border-color: var(--border);
  background: var(--bg-panel);
}
.sb-edit .input:focus {
  border-color: var(--accent);
  background: var(--bg-panel);
}
.sb-edit-name {
  height: 24px;
  padding: 1px 3px;
  font-weight: 500;
}
.sb-edit-desc {
  height: 22px;
  padding: 1px 3px;
  color: var(--text-faint);
  font-size: 11px;
}
</style>
