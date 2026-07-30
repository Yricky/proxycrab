<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import {
  Io5ArrowForward,
  Io5Create,
  Io5PhonePortraitOutline,
  Io5Refresh,
  Io5ServerOutline,
  Io5Trash,
} from "vue-icons-plus/io5";
import { useBackend } from "../api";
import type { InterceptorKind, SessionInterceptorItem } from "../api/types";
import { reportError } from "../stores/app";
import { openContextMenu } from "../stores/dialog";
import { interceptorsStore } from "../stores/interceptors";
import { sessionsStore } from "../stores/sessions";
import { openScriptEditor } from "../windows/launcher";
import AppTooltip from "./AppTooltip.vue";
import InterceptorPicker from "./InterceptorPicker.vue";

const backend = useBackend();

const picker = ref({
  visible: false,
  kind: "request" as InterceptorKind,
  index: 0,
  x: 0,
  y: 0,
});
const dragging = ref<{ kind: InterceptorKind; index: number } | null>(null);
const dragTarget = ref<{ kind: InterceptorKind; index: number } | null>(null);

const requestItems = computed(() => interceptorsStore.request);
const responseItems = computed(() => interceptorsStore.response);

function entries(kind: InterceptorKind): SessionInterceptorItem[] {
  return kind === "request" ? requestItems.value : responseItems.value;
}

function kindLabel(kind: InterceptorKind): string {
  return kind === "request" ? "请求拦截器" : "响应拦截器";
}

function statusLabel(item: SessionInterceptorItem): string {
  if (!item.valid) return "脚本文件不存在 · 右键可重新创建";
  return item.enabled ? "已开启 · 点击关闭" : "已关闭 · 点击开启";
}

function openPicker(
  kind: InterceptorKind,
  index: number,
  x: number,
  y: number,
): void {
  picker.value = { visible: true, kind, index, x, y };
}

function openPickerAtElement(
  kind: InterceptorKind,
  index: number,
  event: MouseEvent,
): void {
  const rect = (event.currentTarget as HTMLElement).getBoundingClientRect();
  openPicker(kind, index, rect.left, rect.bottom + 5);
}

async function selectScript(name: string): Promise<void> {
  const { kind, index } = picker.value;
  picker.value.visible = false;
  await interceptorsStore.insert(kind, index, name);
}

async function recreate(kind: InterceptorKind, item: SessionInterceptorItem): Promise<void> {
  try {
    await backend.createInterceptor({ kind, name: item.name, content: "" });
    window.dispatchEvent(new CustomEvent("interceptors-changed"));
    await interceptorsStore.refresh(sessionsStore.viewingSessionId);
    openScriptEditor(kind, item.name);
  } catch (error) {
    reportError(error, "重新创建拦截器失败");
  }
}

function nodeMenu(
  event: MouseEvent,
  kind: InterceptorKind,
  item: SessionInterceptorItem,
  index: number,
): void {
  openContextMenu(event, [
    {
      label: "编辑脚本",
      icon: Io5Create,
      disabled: !item.valid,
      action: () => openScriptEditor(kind, item.name),
    },
    ...(item.valid
      ? []
      : [
          {
            label: "重新创建同名脚本",
            icon: Io5Refresh,
            dividerBefore: true,
            action: () => void recreate(kind, item),
          },
        ]),
    {
      label: "从当前会话移除",
      icon: Io5Trash,
      danger: true,
      dividerBefore: true,
      action: () => void interceptorsStore.remove(kind, index),
    },
  ]);
}

function dragStart(event: DragEvent, kind: InterceptorKind, index: number): void {
  dragging.value = { kind, index };
  dragTarget.value = { kind, index };
  event.dataTransfer?.setData("text/plain", `${kind}:${index}`);
  if (event.dataTransfer) event.dataTransfer.effectAllowed = "move";
}

function dragOver(event: DragEvent, kind: InterceptorKind, index: number): void {
  if (dragging.value?.kind !== kind) return;
  event.preventDefault();
  dragTarget.value = { kind, index };
  if (event.dataTransfer) event.dataTransfer.dropEffect = "move";
}

async function drop(event: DragEvent, kind: InterceptorKind, index: number): Promise<void> {
  event.preventDefault();
  const source = dragging.value;
  dragging.value = null;
  dragTarget.value = null;
  if (!source || source.kind !== kind) return;
  await interceptorsStore.move(kind, source.index, index);
}

function dragEnd(): void {
  dragging.value = null;
  dragTarget.value = null;
}

function refresh(): void {
  void interceptorsStore.refresh(sessionsStore.viewingSessionId);
}

watch(
  () => sessionsStore.viewingSessionId,
  () => refresh(),
  { immediate: true },
);

onMounted(() => {
  window.addEventListener("focus", refresh);
  window.addEventListener("interceptors-changed", refresh);
});

onBeforeUnmount(() => {
  window.removeEventListener("focus", refresh);
  window.removeEventListener("interceptors-changed", refresh);
});
</script>

<template>
  <div class="pipeline-shell">
    <div v-if="sessionsStore.viewingSessionId === null" class="pipeline-empty">
      请选择或创建会话
    </div>
    <div v-else class="pipeline-scroll">
      <div class="pipeline-track" :class="{ saving: interceptorsStore.saving }">
        <AppTooltip title="原设备" detail="请求来源">
          <span class="pipeline-endpoint" role="img" aria-label="原设备">
            <Io5PhonePortraitOutline :size="17" />
          </span>
        </AppTooltip>

        <AppTooltip
          title="添加请求拦截器"
          detail="点击在此位置插入"
          :status="requestItems.length >= 12 ? '已达到 12 个上限' : undefined"
        >
          <button
            class="pipeline-arrow-action"
            :disabled="requestItems.length >= 12"
            aria-label="在此添加请求拦截器"
            @click="openPickerAtElement('request', 0, $event)"
          >
            <Io5ArrowForward class="pipeline-arrow" :size="13" />
          </button>
        </AppTooltip>

        <template v-if="requestItems.length">
          <template v-for="(item, index) in requestItems" :key="`request-${item.name}`">
            <AppTooltip
              :title="item.name"
              :detail="`${kindLabel('request')} · #${index + 1}`"
              :status="statusLabel(item)"
            >
              <button
                class="pipeline-node"
                :class="{
                  enabled: item.valid && item.enabled,
                  disabled: item.valid && !item.enabled,
                  invalid: !item.valid,
                  dragging: dragging?.kind === 'request' && dragging.index === index,
                  'drop-target':
                    dragTarget?.kind === 'request' &&
                    dragTarget.index === index &&
                    dragging?.index !== index,
                }"
                :draggable="true"
                :aria-label="item.name"
                @click="interceptorsStore.toggle('request', index)"
                @contextmenu="nodeMenu($event, 'request', item, index)"
                @dragstart="dragStart($event, 'request', index)"
                @dragover="dragOver($event, 'request', index)"
                @drop="drop($event, 'request', index)"
                @dragend="dragEnd"
              >
                <span v-if="!item.valid">!</span>
              </button>
            </AppTooltip>
            <AppTooltip
              title="添加请求拦截器"
              detail="点击在此位置插入"
              :status="requestItems.length >= 12 ? '已达到 12 个上限' : undefined"
            >
              <button
                class="pipeline-arrow-action"
                :disabled="requestItems.length >= 12"
                aria-label="在此添加请求拦截器"
                @click="openPickerAtElement('request', index + 1, $event)"
              >
                <Io5ArrowForward class="pipeline-arrow" :size="13" />
              </button>
            </AppTooltip>
          </template>
        </template>

        <AppTooltip title="服务端" detail="请求实际发送目标">
          <span class="pipeline-endpoint server" role="img" aria-label="服务端">
            <Io5ServerOutline :size="17" />
          </span>
        </AppTooltip>

        <AppTooltip
          title="添加响应拦截器"
          detail="点击在此位置插入"
          :status="responseItems.length >= 12 ? '已达到 12 个上限' : undefined"
        >
          <button
            class="pipeline-arrow-action"
            :disabled="responseItems.length >= 12"
            aria-label="在此添加响应拦截器"
            @click="openPickerAtElement('response', 0, $event)"
          >
            <Io5ArrowForward class="pipeline-arrow" :size="13" />
          </button>
        </AppTooltip>

        <template v-if="responseItems.length">
          <template v-for="(item, index) in responseItems" :key="`response-${item.name}`">
            <AppTooltip
              :title="item.name"
              :detail="`${kindLabel('response')} · #${index + 1}`"
              :status="statusLabel(item)"
            >
              <button
                class="pipeline-node"
                :class="{
                  enabled: item.valid && item.enabled,
                  disabled: item.valid && !item.enabled,
                  invalid: !item.valid,
                  dragging: dragging?.kind === 'response' && dragging.index === index,
                  'drop-target':
                    dragTarget?.kind === 'response' &&
                    dragTarget.index === index &&
                    dragging?.index !== index,
                }"
                :draggable="true"
                :aria-label="item.name"
                @click="interceptorsStore.toggle('response', index)"
                @contextmenu="nodeMenu($event, 'response', item, index)"
                @dragstart="dragStart($event, 'response', index)"
                @dragover="dragOver($event, 'response', index)"
                @drop="drop($event, 'response', index)"
                @dragend="dragEnd"
              >
                <span v-if="!item.valid">!</span>
              </button>
            </AppTooltip>
            <AppTooltip
              title="添加响应拦截器"
              detail="点击在此位置插入"
              :status="responseItems.length >= 12 ? '已达到 12 个上限' : undefined"
            >
              <button
                class="pipeline-arrow-action"
                :disabled="responseItems.length >= 12"
                aria-label="在此添加响应拦截器"
                @click="openPickerAtElement('response', index + 1, $event)"
              >
                <Io5ArrowForward class="pipeline-arrow" :size="13" />
              </button>
            </AppTooltip>
          </template>
        </template>

        <AppTooltip title="原设备" detail="响应返回目标">
          <span class="pipeline-endpoint" role="img" aria-label="响应返回原设备">
            <Io5PhonePortraitOutline :size="17" />
          </span>
        </AppTooltip>
      </div>
    </div>

    <InterceptorPicker
      :visible="picker.visible"
      :kind="picker.kind"
      :used-names="entries(picker.kind).map((item) => item.name)"
      :at-capacity="entries(picker.kind).length >= 12"
      :x="picker.x"
      :y="picker.y"
      @close="picker.visible = false"
      @select="selectScript"
    />
  </div>
</template>

<style scoped>
.pipeline-shell {
  height: 34px;
  flex: none;
  display: flex;
  align-items: stretch;
  border-bottom: 1px solid var(--border);
  background: color-mix(in srgb, var(--bg-panel) 72%, var(--bg-app));
}
.pipeline-empty {
  display: flex;
  align-items: center;
  padding: 0 11px;
  color: var(--text-faint);
  font-size: 11px;
}
.pipeline-scroll {
  flex: 1;
  min-width: 0;
  overflow-x: auto;
  overflow-y: hidden;
  scrollbar-width: thin;
}
.pipeline-track {
  min-width: max-content;
  height: 33px;
  display: flex;
  align-items: center;
  padding: 0 10px;
  transition: opacity 0.14s ease;
}
.pipeline-track.saving {
  opacity: 0.72;
}
.pipeline-endpoint,
.pipeline-node,
.pipeline-arrow-action {
  flex: none;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border: 0;
  cursor: pointer;
}
.pipeline-endpoint {
  width: 24px;
  height: 24px;
  padding: 0;
  border-radius: 7px;
  background: transparent;
  color: var(--text-secondary);
  cursor: default;
}
.pipeline-endpoint:hover {
  color: var(--accent);
  background: var(--bg-hover);
}
.pipeline-endpoint.server {
  color: var(--text);
}
.pipeline-arrow {
  flex: none;
  color: var(--text-faint);
  transition:
    color 0.14s ease,
    transform 0.14s ease;
}
.pipeline-arrow-action {
  width: 21px;
  height: 24px;
  padding: 0;
  border-radius: 5px;
  background: transparent;
  color: var(--text-faint);
}
.pipeline-arrow-action:hover:not(:disabled) {
  background: var(--bg-hover);
}
.pipeline-arrow-action:hover:not(:disabled) .pipeline-arrow {
  color: var(--accent);
  transform: scale(1.15);
}
.pipeline-arrow-action:disabled {
  cursor: not-allowed;
  opacity: 0.55;
}
.pipeline-node {
  position: relative;
  width: 20px;
  height: 20px;
  padding: 0;
  border-radius: 50%;
  transition:
    transform 0.14s ease,
    background 0.14s ease,
    box-shadow 0.14s ease,
    opacity 0.14s ease;
}
.pipeline-node:hover {
  transform: scale(1.1);
}
.pipeline-node.enabled {
  background: var(--success);
  box-shadow: inset 0 0 0 1px color-mix(in srgb, #000 12%, transparent);
}
.pipeline-node.disabled {
  background: var(--border-strong);
}
.pipeline-node.invalid {
  border: 1.5px solid var(--danger);
  background: transparent;
  color: var(--danger);
  font-size: 12px;
  font-weight: 700;
}
.pipeline-node.dragging {
  opacity: 0.38;
  transform: scale(0.92);
}
.pipeline-node.drop-target::before {
  content: "";
  position: absolute;
  left: -7px;
  top: 1px;
  width: 2px;
  height: 18px;
  border-radius: 1px;
  background: var(--accent);
}
@media (prefers-reduced-motion: reduce) {
  .pipeline-track,
  .pipeline-node,
  .pipeline-arrow {
    transition: none;
  }
}
</style>
