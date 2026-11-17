<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { openBundleImport, openSkillManager } from "./windows/launcher";
import { reportError } from "./stores/app";
import { t } from "./i18n";
import AppToolbar from "./components/AppToolbar.vue";
import SessionSidebar from "./components/SessionSidebar.vue";
import FilterBar from "./components/FilterBar.vue";
import InterceptorPipeline from "./components/InterceptorPipeline.vue";
import LogTable from "./components/LogTable.vue";
import FloatingWindow from "./components/FloatingWindow.vue";
import Toast from "./components/Toast.vue";
import ConfirmDialog from "./components/ConfirmDialog.vue";
import ContextMenu from "./components/ContextMenu.vue";
import { windowsStore } from "./stores/windows";
import { sessionsStore } from "./stores/sessions";
import { proxyStore } from "./stores/proxy";
import { createLogsStore } from "./stores/logs";
import { startHttpApiSync, stopHttpApiSync } from "./stores/http-api-sync";
import { breakpointsStore } from "./stores/breakpoints";
import { approvalsStore } from "./stores/approvals";
import { skillStore } from "./stores/skill";
import { useBackend } from "./api";
import { Io5Checkmark } from "vue-icons-plus/io5";

const backend = useBackend();

// 主内容区的日志视图：只在这里创建，由 AppMain 持有并交给子组件；
// "查看哪个会话" 就是 logs.sessionId（不再有全局 viewingSessionId）。
const logs = createLogsStore();

// 日志轮询由 store 根据当前 Session 的活跃/待复算状态动态降频或停止；
// 断点轮询只对正在运行的活跃 Session 有意义。
function syncCapturePolling(): void {
  const canPollLogs = proxyStore.running && logs.sessionId !== null;
  if (canPollLogs) {
    logs.startPolling();
  } else {
    logs.stopPolling();
  }
  const canPollBreakpoints = canPollLogs && sessionsStore.activeSessionId !== null;
  if (canPollBreakpoints) {
    breakpointsStore.startPolling({
      sessionId: () => logs.sessionId,
      hasActive: () => logs.hasActive,
    });
  } else {
    breakpointsStore.stopPolling();
  }
}

watch([() => proxyStore.running, () => sessionsStore.activeSessionId, () => logs.sessionId], () => {
  syncCapturePolling();
});

// 正在查看的会话失效（被归档/删除）时，回落到活跃会话，再不行取第一个。
watch(
  () => sessionsStore.sessions,
  (sessions) => {
    if (logs.sessionId !== null && sessions.some((s) => s.id === logs.sessionId)) return;
    const fallback = sessions.find((s) => s.id === sessionsStore.activeSessionId) ?? sessions[0];
    logs.view(fallback?.id ?? null);
  },
  { deep: true },
);

onMounted(async () => {
  window.addEventListener("proxycrab-open-session", onOpenSession);
  await startHttpApiSync();
  if (backend.listHttpApprovals) await approvalsStore.start();
  void skillStore.refresh();
  await proxyStore.init();
  await sessionsStore.init();
  const initial = sessionsStore.sessions.find(
    (session) => session.id === sessionsStore.activeSessionId,
  );
  logs.view(initial?.id ?? sessionsStore.sessions[0]?.id ?? null);
  // 确保启动时状态已刷新，避免 syncCapturePolling 拿到过期的 stopped 状态。
  await proxyStore.refresh();
  syncCapturePolling();
  ready.value = true;
});

onBeforeUnmount(() => {
  window.removeEventListener("proxycrab-open-session", onOpenSession);
  proxyStore.dispose();
  logs.dispose();
  breakpointsStore.stopPolling();
  stopHttpApiSync();
  if (backend.listHttpApprovals) approvalsStore.stop();
});
// 拖放导入：dragenter/dragleave 计数决定覆盖层，避免在子元素间移动时闪烁。
const draggingFiles = ref(false);
let dragDepth = 0;
let draggingInternally = false;
function fileDragStart(): void {
  draggingInternally = true;
}
function fileDragEnd(): void {
  draggingInternally = false;
  dragDepth = 0;
  draggingFiles.value = false;
}
function carriesFiles(event: DragEvent): boolean {
  return !draggingInternally && Array.from(event.dataTransfer?.types ?? []).includes("Files");
}
function fileDragEnter(event: DragEvent): void {
  if (!backend.importBundle || !carriesFiles(event)) return;
  dragDepth++;
  draggingFiles.value = true;
}
function fileDragLeave(): void {
  if (!draggingFiles.value) return;
  dragDepth = Math.max(0, dragDepth - 1);
  if (dragDepth === 0) draggingFiles.value = false;
}
function fileDrag(event: DragEvent): void {
  if (backend.importBundle && carriesFiles(event)) event.preventDefault();
}
async function fileDrop(event: DragEvent): Promise<void> {
  dragDepth = 0;
  draggingFiles.value = false;
  if (!backend.importBundle || !carriesFiles(event) || !event.dataTransfer?.files.length) return;
  event.preventDefault();
  event.stopPropagation();
  for (const file of Array.from(event.dataTransfer.files)) {
    try {
      await openBundleImport(file);
    } catch (e) {
      reportError(e, t("打开分享包失败"));
    }
  }
}
function onOpenSession(event: Event): void {
  const sessionId = (event as CustomEvent<number>).detail;
  if (typeof sessionId === "number") logs.view(sessionId);
}

/* ---- 未选择会话时的引导 ---- */
type GuideStep = "connect" | "start";

const ready = ref(false);
const showGuide = computed(() => ready.value && logs.sessionId === null);
const skillReady = computed(() => Boolean(skillStore.state && !skillStore.warning));
const canManageSkill = computed(() => Boolean(backend.host?.skillManager));

const guideHovered = ref<GuideStep | null>(null);
const guideCursor = ref({ x: 0, y: 0 });
const guideTargets = ref<{ x: number; y: number }[]>([]);

/** 悬停时指线的目标与颜色：连接设备指“连接”，第三步指启动代理与新建会话。 */
const GUIDE_STEPS: Record<GuideStep, { selectors: string[]; color: string }> = {
  connect: { selectors: ['[data-guide="connect"]'], color: "var(--success)" },
  start: {
    selectors: ['[data-guide="proxy-toggle"]', '[data-guide="new-session"]'],
    color: "var(--warning)",
  },
};

const guideColor = computed(() =>
  guideHovered.value ? GUIDE_STEPS[guideHovered.value].color : "",
);

function centerOf(selector: string): { x: number; y: number } | null {
  const el = document.querySelector(selector);
  if (!el) return null;
  const rect = el.getBoundingClientRect();
  return { x: rect.left + rect.width / 2, y: rect.top + rect.height / 2 };
}

/** 鼠标在步骤上移动时，持续更新光标位置与目标按钮中心。 */
function onGuideHover(event: MouseEvent, step: GuideStep): void {
  guideHovered.value = step;
  guideCursor.value = { x: event.clientX, y: event.clientY };
  guideTargets.value = GUIDE_STEPS[step].selectors
    .map(centerOf)
    .filter((point): point is { x: number; y: number } => point !== null);
}

const guideLines = computed(() =>
  guideTargets.value.map((point) => {
    const dx = point.x - guideCursor.value.x;
    const dy = point.y - guideCursor.value.y;
    return { point, length: Math.hypot(dx, dy), angle: (Math.atan2(dy, dx) * 180) / Math.PI };
  }),
);
</script>

<template>
  <div
    class="app-shell"
    @dragstart.capture="fileDragStart"
    @dragend.capture="fileDragEnd"
    @dragenter="fileDragEnter"
    @dragover="fileDrag"
    @dragleave="fileDragLeave"
    @drop="fileDrop"
  >
    <AppToolbar :session-id="logs.sessionId" />
    <div class="app-main">
      <SessionSidebar :logs="logs" />
      <section class="app-content">
        <div v-if="showGuide" class="app-guide">
          <section class="guide-card">
            <h2 class="guide-title">{{ t("开始使用 ProxyCrab") }}</h2>
            <p class="guide-sub">
              {{ t("三步就能看到真实流量：装好 Skill、让设备连上代理，再新建会话并启动代理。") }}
            </p>
            <ol class="guide-steps">
              <li class="guide-step">
                <span class="guide-index" :class="{ done: skillReady }">
                  <Io5Checkmark v-if="skillReady" :size="13" />
                  <template v-else>1</template>
                </span>
                <div class="guide-body">
                  <span class="guide-step-title">
                    {{ t("安装 Skill") }}
                    <span v-if="skillReady" class="guide-done">{{ t("已完成") }}</span>
                  </span>
                  <span class="guide-step-desc">
                    {{ t("安装 ProxyCrab Skill，即可让 agent 能够调用 ProxyCrab 的完整能力") }}
                  </span>
                </div>
                <button v-if="canManageSkill" class="btn" @click="openSkillManager">
                  {{ t("Skill 管理") }}
                </button>
              </li>
              <li
                class="guide-step"
                @mouseenter="onGuideHover($event, 'connect')"
                @mousemove="onGuideHover($event, 'connect')"
                @mouseleave="guideHovered = null"
              >
                <span class="guide-index">2</span>
                <div class="guide-body">
                  <span class="guide-step-title">{{ t("连接设备") }}</span>
                  <span class="guide-step-desc">
                    {{ t("按说明配置代理地址与证书，让手机或其他设备的流量进入 ProxyCrab。") }}
                  </span>
                </div>
                <span class="guide-hint">{{ t("悬停指出按钮位置") }}</span>
              </li>
              <li
                class="guide-step"
                @mouseenter="onGuideHover($event, 'start')"
                @mousemove="onGuideHover($event, 'start')"
                @mouseleave="guideHovered = null"
              >
                <span class="guide-index">3</span>
                <div class="guide-body">
                  <span class="guide-step-title">{{ t("新建会话并启动代理") }}</span>
                  <span class="guide-step-desc">
                    {{ t("新建一个会话并把代理启动起来，抓包内容就会展示在这块区域。") }}
                  </span>
                </div>
                <span class="guide-hint">{{ t("悬停指出按钮位置") }}</span>
              </li>
            </ol>
          </section>
        </div>
        <template v-else>
          <div class="traffic-controls">
            <FilterBar
              :filter="logs.appliedFilter"
              :session-id="logs.sessionId"
              :syncing="Boolean(backend.replaceSessionFilter)"
              :apply-filter="(filter) => logs.applyFilter(filter)"
            />
            <InterceptorPipeline :logs="logs" />
          </div>
          <LogTable :logs="logs" />
        </template>
      </section>
    </div>

    <Teleport to="body">
      <div
        v-if="showGuide && guideHovered && guideLines.length > 0"
        class="guide-overlay"
        :style="{ '--guide-color': guideColor }"
      >
        <template v-for="(line, index) in guideLines" :key="index">
          <div
            class="guide-line"
            :style="{
              left: guideCursor.x + 'px',
              top: guideCursor.y + 'px',
              width: line.length + 'px',
              transform: `rotate(${line.angle}deg)`,
            }"
          ></div>
          <div
            class="guide-dot"
            :style="{ left: line.point.x + 'px', top: line.point.y + 'px' }"
          ></div>
        </template>
      </div>
    </Teleport>

    <div v-if="draggingFiles" class="drop-overlay">
      <span class="drop-hint">{{ t("松开以导入分享包") }}</span>
    </div>

    <FloatingWindow v-for="win in windowsStore.windows" :key="win.id" :win="win" />
    <Toast />
    <ConfirmDialog />
    <ContextMenu />
  </div>
</template>

<style scoped>
.app-shell {
  height: 100%;
  display: flex;
  flex-direction: column;
}
.app-main {
  flex: 1;
  min-height: 0;
  display: flex;
}
.app-content {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  background: var(--bg-app);
}
.app-guide {
  flex: 1;
  min-height: 0;
  display: flex;
  padding: 24px;
  overflow: auto;
}
.guide-card {
  width: min(560px, 100%);
  /* 用 margin:auto 居中：窗口过矮时不会被 overflow 裁掉顶部。 */
  margin: auto;
  padding: 20px;
  border: 1px solid var(--border);
  border-radius: var(--radius-lg);
  background: var(--bg-panel);
}
.guide-title {
  margin: 0 0 6px;
  font-size: 15px;
  font-weight: 600;
  color: var(--text);
}
.guide-sub {
  margin: 0 0 14px;
  color: var(--text-secondary);
  font-size: 12px;
  line-height: 1.5;
}
.guide-steps {
  margin: 0;
  padding: 0;
  list-style: none;
  display: flex;
  flex-direction: column;
  gap: 10px;
}
.guide-step {
  display: grid;
  grid-template-columns: auto 1fr auto;
  align-items: center;
  gap: 10px;
}
.guide-index {
  width: 22px;
  height: 22px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border: 1px solid var(--border-strong);
  border-radius: 50%;
  color: var(--text-secondary);
  font-size: 11px;
}
.guide-index.done {
  border-color: transparent;
  background: var(--success);
  color: #fff;
}
.guide-body {
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
}
.guide-step-title {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  color: var(--text);
  font-size: 12px;
}
.guide-done {
  color: var(--success);
  font-size: 11px;
}
.guide-step-desc {
  color: var(--text-faint);
  font-size: 11px;
  line-height: 1.5;
}
.guide-hint {
  color: var(--text-faint);
  font-size: 11px;
}
/* 第三步悬停时的指线：从光标出发连到两个按钮，末端圆点呼吸。 */
.guide-overlay {
  position: fixed;
  inset: 0;
  z-index: 5;
  pointer-events: none;
}
.guide-line {
  position: absolute;
  height: 1.5px;
  border-radius: 1px;
  background: var(--guide-color);
  transform-origin: 0 50%;
}
.guide-dot {
  position: absolute;
  width: 8px;
  height: 8px;
  margin: -4px 0 0 -4px;
  border-radius: 50%;
  background: var(--guide-color);
  animation: guide-pulse 1s ease-in-out infinite alternate;
}
@keyframes guide-pulse {
  from {
    transform: scale(0.6);
    opacity: 0.6;
  }
  to {
    transform: scale(1.6);
    opacity: 1;
  }
}
.traffic-controls {
  flex: none;
  display: flex;
  flex-direction: column;
  gap: 6px;
  padding: 6px 8px;
  border-bottom: 1px solid var(--border);
  background: color-mix(in srgb, var(--bg-panel) 72%, var(--bg-app));
}
.drop-overlay {
  position: fixed;
  inset: 0;
  z-index: 1000;
  display: flex;
  align-items: center;
  justify-content: center;
  background: color-mix(in srgb, var(--bg-app) 65%, transparent);
  pointer-events: none;
}
.drop-overlay::before {
  content: "";
  position: absolute;
  inset: 10px;
  border: 2px dashed var(--accent);
  border-radius: var(--radius-lg);
  background: color-mix(in srgb, var(--accent) 7%, transparent);
}
.drop-hint {
  position: relative;
  padding: 8px 16px;
  border: 1px solid var(--border-strong);
  border-radius: var(--radius-md);
  background: var(--bg-elevated);
  box-shadow: var(--shadow-popup);
  font-size: 13px;
}
</style>
