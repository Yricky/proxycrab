<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from "vue";
import { useBackend } from "../api";
import { resourceKinds, useBackendChanges } from "../utils/api-changes";
import type { LogsStore } from "../stores/logs";
import { sessionsStore } from "../stores/sessions";
import { routingStore } from "../stores/routing";
import { scriptLabelFor } from "../utils/script-label";
import { t } from "../i18n";
import { proxyStore } from "../stores/proxy";
import { confirmDialog, openContextMenu } from "../stores/dialog";
import { formatActivityCount } from "../utils/format";
import { appStore } from "../stores/app";
import SessionListItem from "./SessionListItem.vue";
import {
  openArchivedSessions,
  openBypass,
  openRoutingManager,
  openSessionExportShare,
} from "../windows/launcher";
import type { SessionMetadata } from "../api/generated/SessionMetadata";
import {
  Io5Add,
  Io5Archive,
  Io5Create,
  Io5Eye,
  Io5Link,
  Io5RadioButtonOn,
} from "vue-icons-plus/io5";
import { IoArrowForwardCircle } from "vue-icons-plus/io";

const props = defineProps<{ logs: LogsStore }>();
const logs = props.logs;

const creating = ref(false);
const editingId = ref<number | null>(null);
const editName = ref("");
const editDesc = ref("");

/* ---- 侧边栏气泡 popup 定位（编辑 / 新建引导共用） ---- */
const POPUP_WIDTH = 260;
const POPUP_EDGE_MARGIN = 8;
const POPUP_ARROW_GAP = 9;
const rootEl = ref<HTMLElement | null>(null);
const editAnchor = ref<HTMLElement | null>(null);
const popupEl = ref<HTMLElement | null>(null);
const popupStyle = ref({ left: "0px", top: "0px" });
const arrowTop = ref(0);

/** 侧边栏顶部（即工具栏底边）：气泡不得越过它，否则会盖住工具栏按钮。 */
function popupTopLimit(): number {
  return rootEl.value?.getBoundingClientRect().top ?? POPUP_EDGE_MARGIN;
}

/** 贴在锚点右侧、垂直居中，箭头指向锚点中心；顶部不低于工具栏底边。 */
function placePopup(
  anchor: HTMLElement | null,
  popup: HTMLElement | null,
): { style: { left: string; top: string }; arrowTop: number } | null {
  if (!anchor || !popup) return null;
  const rect = anchor.getBoundingClientRect();
  const centerY = rect.top + rect.height / 2;
  const height = popup.offsetHeight;
  const minTop = popupTopLimit() + POPUP_EDGE_MARGIN;
  const maxTop = Math.max(window.innerHeight - POPUP_EDGE_MARGIN - height, POPUP_EDGE_MARGIN);
  const top = Math.min(Math.max(centerY - height / 2, minTop), maxTop);
  const maxLeft = Math.max(window.innerWidth - POPUP_EDGE_MARGIN - POPUP_WIDTH, POPUP_EDGE_MARGIN);
  const left = Math.min(rect.right + POPUP_ARROW_GAP, maxLeft);
  return {
    style: { left: `${Math.round(left)}px`, top: `${Math.round(top)}px` },
    arrowTop: Math.min(Math.max(centerY - top, 14), height - 14),
  };
}

function updatePopupPos(): void {
  if (editingId.value === null) return;
  const placement = placePopup(editAnchor.value, popupEl.value);
  if (!placement) return;
  popupStyle.value = placement.style;
  arrowTop.value = placement.arrowTop;
}

function closeEdit(): void {
  editingId.value = null;
  editAnchor.value = null;
}

function onDocMouseDown(e: MouseEvent): void {
  if (editingId.value === null) return;
  const popup = popupEl.value;
  if (popup && popup.contains(e.target as Node)) return;
  closeEdit();
}

function onWindowResize(): void {
  updatePopupPos();
  updateNewHintPos();
  updateActiveHintPos();
}

/* ---- 代理运行但没有活跃会话时的提示 ---- */
const activeHintDismissed = ref(false);
const activeHintEl = ref<HTMLElement | null>(null);
const routingEl = ref<HTMLElement | null>(null);
const activeHintStyle = ref({ left: "0px", top: "0px" });
const activeHintArrowTop = ref(0);

const showActiveHint = computed(
  () =>
    proxyStore.running &&
    !activeHintDismissed.value &&
    !sessionsStore.loading &&
    sessionsStore.sessions.length > 0 &&
    sessionsStore.activeSessionId === null,
);

function updateActiveHintPos(): void {
  const placement = placePopup(routingEl.value, activeHintEl.value);
  if (!placement) return;
  activeHintStyle.value = placement.style;
  activeHintArrowTop.value = placement.arrowTop;
}

watch(showActiveHint, (visible) => {
  if (visible) void nextTick(updateActiveHintPos);
});

/* ---- 代理运行但无会话时的新建引导 ---- */
const newHintDismissed = ref(false);
const newHintEl = ref<HTMLElement | null>(null);
const newBtnEl = ref<HTMLElement | null>(null);
const newHintStyle = ref({ left: "0px", top: "0px" });
const newHintArrowTop = ref(0);

const showNewHint = computed(
  () =>
    proxyStore.running &&
    !newHintDismissed.value &&
    !sessionsStore.loading &&
    sessionsStore.sessions.length === 0,
);

function updateNewHintPos(): void {
  const placement = placePopup(newBtnEl.value, newHintEl.value);
  if (!placement) return;
  newHintStyle.value = placement.style;
  newHintArrowTop.value = placement.arrowTop;
}

// 不持久化：“我知道了”只在本次代理运行内生效，代理启停一次就重新计。
watch(
  () => proxyStore.running,
  () => {
    newHintDismissed.value = false;
    activeHintDismissed.value = false;
  },
);

watch(showNewHint, (visible) => {
  if (visible) void nextTick(updateNewHintPos);
});

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
  logs.view(session.id);
  appStore.toast(t("会话已创建"), "success");
}

function startEdit(session: SessionMetadata, anchor: HTMLElement | null): void {
  editAnchor.value = anchor;
  editingId.value = session.id;
  editName.value = session.name;
  editDesc.value = session.description ?? "";
  void nextTick(updatePopupPos);
}

function onItemDblclick(event: MouseEvent, session: SessionMetadata): void {
  startEdit(session, event.currentTarget as HTMLElement);
}

async function submitEdit(): Promise<void> {
  if (editingId.value === null) return;
  const name = editName.value.trim();
  if (!name) return;
  const updated = await sessionsStore.update(editingId.value, name, editDesc.value.trim() || null);
  if (updated) {
    closeEdit();
    appStore.toast(t("会话信息已更新"), "success");
  }
}

async function archiveSession(id: number, name: string): Promise<void> {
  const ok = await confirmDialog({
    title: t("归档会话"),
    message: t("确定归档会话「{name}」吗？抓包记录会完整保留，可在已归档 Session 中恢复。", {
      name,
    }),
    confirmText: t("归档"),
  });
  if (ok && (await sessionsStore.archive(id))) {
    appStore.toast(t("「{name}」已归档", { name }), "success");
  }
}

async function toggleActive(session: SessionMetadata): Promise<void> {
  const active = sessionsStore.activeSessionId === session.id;
  if (await sessionsStore.replaceActive(active ? null : session.id)) {
    appStore.toast(
      active ? t("已取消活跃会话") : t("「{name}」已设为活跃会话", { name: session.name }),
      "success",
    );
  }
}

function sessionMenu(event: MouseEvent, session: SessionMetadata): void {
  // currentTarget 仅在事件分发期间有效，需在打开菜单前捕获
  const anchor = event.currentTarget as HTMLElement;
  openContextMenu(event, [
    { label: t("查看"), icon: Io5Eye, action: () => logs.view(session.id) },
    {
      label: t("链接分享"),
      icon: Io5Link,
      action: () => openSessionExportShare(session.id, session.name),
    },
    { label: t("编辑会话"), icon: Io5Create, action: () => startEdit(session, anchor) },
    {
      label: sessionsStore.activeSessionId === session.id ? t("取消活跃") : t("设为活跃"),
      icon: Io5RadioButtonOn,
      action: () => void toggleActive(session),
    },
    {
      label: sessionsStore.activeSessionId === session.id ? t("归档（请先取消活跃）") : t("归档"),
      icon: Io5Archive,
      dividerBefore: true,
      disabled: sessionsStore.activeSessionId === session.id,
      action: () => void archiveSession(session.id, session.name),
    },
  ]);
}

const backend = useBackend();
const canReadRouting =
  typeof backend.listRoutingScripts === "function" &&
  typeof backend.getRoutingSelection === "function";
if (canReadRouting)
  useBackendChanges(backend, (change) => {
    if (
      resourceKinds(change).some((kind) =>
        ["all", "routing_scripts", "routing_selection"].includes(kind),
      )
    ) {
      return routingStore.refresh();
    }
  });

onMounted(() => {
  if (canReadRouting) void routingStore.refresh();
  document.addEventListener("mousedown", onDocMouseDown);
  window.addEventListener("resize", onWindowResize);
});

onUnmounted(() => {
  document.removeEventListener("mousedown", onDocMouseDown);
  window.removeEventListener("resize", onWindowResize);
});

watch(sidebarWidth, () => {
  updatePopupPos();
  updateNewHintPos();
  updateActiveHintPos();
});
</script>

<template>
  <aside ref="rootEl" class="sidebar" :style="{ width: sidebarWidth + 'px' }">
    <div
      class="sb-resize"
      :class="{ dragging }"
      :title="t('拖动调整侧边栏宽度')"
      @pointerdown="onResizePointerDown"
      @pointermove="onResizePointerMove"
      @pointerup="endResize"
      @pointercancel="endResize"
    ></div>
    <div class="sb-top">
      <div class="sb-header">
        <span class="sb-title">{{ t("会话") }}</span>
      </div>
      <div
        v-if="canReadRouting"
        class="sb-routing"
        ref="routingEl"
        :class="{ empty: !routingStore.selectedName }"
      >
        <button
          class="sb-bypass"
          :title="
            proxyStore.activeBypassCount > 0
              ? t('透明转发记录（{n} 条活跃）', proxyStore.activeBypassCount)
              : t('透明转发记录')
          "
          :aria-label="
            proxyStore.activeBypassCount > 0
              ? t('透明转发记录，{n} 条活跃', proxyStore.activeBypassCount)
              : t('透明转发记录')
          "
          @click="openBypass"
        >
          <span v-if="proxyStore.activeBypassCount > 0" class="activity-badge">
            {{ formatActivityCount(proxyStore.activeBypassCount) }}
          </span>
          <IoArrowForwardCircle v-else :size="18" />
        </button>
        <button
          class="sb-routing-main"
          :title="t('打开分流规则管理器')"
          @click="openRoutingManager"
        >
          <span
            v-if="routingStore.selectedName"
            class="sb-routing-name"
            :title="routingStore.selectedName"
          >
            {{ scriptLabelFor(routingStore.scripts, routingStore.selectedName) }}
          </span>
          <span v-else>{{ t("未设置分流规则") }}</span>
        </button>
      </div>
    </div>

    <div class="sb-body">
      <div class="sb-list" @scroll.passive="updatePopupPos">
        <SessionListItem
          v-for="session in sessionsStore.sessions"
          :key="session.id"
          :session="session"
          :viewing="logs.sessionId === session.id"
          :active="sessionsStore.activeSessionId === session.id"
          :active-connections="proxyStore.activeNetlogCount(session.id)"
          @click="logs.view(session.id)"
          @contextmenu="sessionMenu($event, session)"
          @dblclick="onItemDblclick($event, session)"
        />
        <div
          v-if="!sessionsStore.loading && sessionsStore.sessions.length === 0"
          class="empty-hint"
        >
          {{ t("暂无会话，点击下方新建") }}
        </div>
      </div>
      <div class="sb-new-bar">
        <button
          ref="newBtnEl"
          class="sb-new"
          data-guide="new-session"
          :disabled="creating"
          @click="createSession"
        >
          <Io5Add :size="14" />
          <span>{{ t("新建会话") }}</span>
        </button>
      </div>
    </div>
    <button class="sb-archived" @click="openArchivedSessions">
      <Io5Archive :size="14" />
      <span>{{ t("已归档 Session") }}</span>
    </button>

    <Teleport to="body">
      <div
        v-if="editingId !== null"
        ref="popupEl"
        class="sb-edit-popup"
        :style="popupStyle"
        @dblclick.stop
      >
        <span class="sb-edit-arrow" :style="{ top: arrowTop + 'px' }">
          <span class="sb-edit-arrow-tip"></span>
        </span>
        <input
          v-model="editName"
          class="input"
          :placeholder="t('会话名称')"
          autofocus
          @keyup.enter="submitEdit"
          @keyup.esc="closeEdit"
        />
        <textarea
          v-model="editDesc"
          class="input sb-edit-desc"
          rows="3"
          :placeholder="t('描述（可选，支持多行）')"
          @keydown.esc.stop.prevent="closeEdit"
          @keydown.enter.meta.prevent="submitEdit"
          @keydown.enter.ctrl.prevent="submitEdit"
        />
        <div class="sb-edit-actions">
          <span class="sb-edit-hint">{{ t("⌘/Ctrl + Enter 保存") }}</span>
          <button class="btn primary" :disabled="!editName.trim()" @click="submitEdit">
            {{ t("保存") }}
          </button>
        </div>
      </div>
    </Teleport>

    <Teleport to="body">
      <div v-if="showNewHint" ref="newHintEl" class="sb-edit-popup" :style="newHintStyle">
        <span class="sb-edit-arrow" :style="{ top: newHintArrowTop + 'px' }">
          <span class="sb-edit-arrow-tip"></span>
        </span>
        <span class="sb-popup-text">
          {{ t("如果想捕获请求内容，需要新建会话并设为活跃") }}
        </span>
        <div class="sb-edit-actions">
          <button class="btn primary" @click="newHintDismissed = true">
            {{ t("我知道了") }}
          </button>
        </div>
      </div>
    </Teleport>

    <Teleport to="body">
      <div v-if="showActiveHint" ref="activeHintEl" class="sb-edit-popup" :style="activeHintStyle">
        <span class="sb-edit-arrow" :style="{ top: activeHintArrowTop + 'px' }">
          <span class="sb-edit-arrow-tip"></span>
        </span>
        <span class="sb-popup-text">
          {{
            t(
              routingStore.selectedName !== null
                ? "当前没有活跃会话，所有没有被分流脚本明确分流目标的请求都会被透明转发。如果您想捕获请求内容，请将下方的某个会话设置为活跃。"
                : "当前没有活跃会话，所有请求都会被透明转发。如果您想捕获请求内容，请将下方的某个会话设置为活跃。您可以在会话的右键菜单中设置",
            )
          }}
        </span>
        <div class="sb-edit-actions">
          <button class="btn primary" @click="activeHintDismissed = true">
            {{ t("我知道了") }}
          </button>
        </div>
      </div>
    </Teleport>
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
  font-size: 13px;
  /* 与右侧 .traffic-controls 的总高（74.5px）对齐，避免两侧分隔线错位 */
  line-height: 20.5px;
  font-weight: 600;
  color: var(--text-secondary);
}
.sb-routing {
  width: calc(100% - 16px);
  box-sizing: border-box;
  min-height: 30px;
  margin: 0 8px 7px;
  padding: 2px 8px 2px 3px;
  display: flex;
  align-items: center;
  gap: 5px;
  border: 1px solid var(--border);
  border-radius: var(--radius-md);
  background: var(--bg-input);
  color: var(--text-secondary);
  font-size: 11px;
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
/* 透明转发记录入口：固定尺寸，不随名称长度压缩。 */
.sb-bypass {
  flex: none;
  width: 24px;
  height: 24px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border: 0;
  border-radius: var(--radius-sm);
  background: transparent;
  color: inherit;
  cursor: pointer;
}
.sb-bypass:hover {
  background: var(--bg-hover);
  color: var(--text);
}
.sb-routing-main {
  flex: 1;
  min-width: 0;
  height: 100%;
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 0;
  border: 0;
  background: transparent;
  color: inherit;
  font: inherit;
  cursor: pointer;
}
.sb-routing-name {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-family: var(--font-mono);
}
.sb-edit-popup {
  position: fixed;
  width: 260px;
  box-sizing: border-box;
  display: flex;
  flex-direction: column;
  gap: 6px;
  padding: 10px;
  background: var(--bg-elevated);
  border: 1px solid var(--border-strong);
  border-radius: var(--radius-md);
  box-shadow: var(--shadow-popup);
  z-index: 100000;
}
.sb-edit-arrow {
  position: absolute;
  right: 100%;
  width: 0;
  height: 0;
  margin-top: -9px;
  border-top: 9px solid transparent;
  border-right: 9px solid var(--border-strong);
  border-bottom: 9px solid transparent;
}
.sb-edit-arrow-tip {
  position: absolute;
  top: -9px;
  left: 1px;
  width: 0;
  height: 0;
  border-top: 9px solid transparent;
  border-right: 9px solid var(--bg-elevated);
  border-bottom: 9px solid transparent;
}
.sb-edit-actions {
  display: flex;
  align-items: center;
  justify-content: flex-end;
  gap: 8px;
}
.sb-edit-hint {
  margin-right: auto;
  color: var(--text-faint);
  font-size: 11px;
}
/* 只让会话列表本身滚动：列表随内容收缩，新建按钮在滚动区之外。 */
.sb-body {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
}
.sb-list {
  min-height: 0;
  overflow-y: auto;
  padding: 4px;
}
.sb-new-bar {
  flex: none;
  padding: 0 4px 4px;
}
.sb-new {
  width: 100%;
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 6px;
  height: 30px;
  border: 1px dashed var(--border);
  border-radius: var(--radius-md);
  background: transparent;
  color: var(--text-secondary);
  font: inherit;
  font-size: 12px;
  cursor: pointer;
}
.sb-new:hover:not(:disabled) {
  border-color: var(--border-strong);
  background: var(--bg-hover);
  color: var(--text);
}
.sb-new:disabled {
  cursor: default;
  opacity: 0.5;
}
.sb-popup-text {
  color: var(--text-secondary);
  font-size: 12px;
  line-height: 1.5;
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
.sb-edit-popup .input {
  width: 100%;
  box-sizing: border-box;
}
.sb-edit-desc {
  min-height: 56px;
  max-height: 180px;
  resize: vertical;
  font-family: inherit;
  line-height: 1.45;
}
</style>
