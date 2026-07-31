<script setup lang="ts">
import { onMounted, ref } from "vue";
import { sessionsStore } from "../stores/sessions";
import { routingStore } from "../stores/routing";
import { confirmDialog, openContextMenu } from "../stores/dialog";
import { formatRelativeTime } from "../utils/format";
import { appStore } from "../stores/app";
import { proxyStore } from "../stores/proxy";
import { openBypass, openRoutingManager } from "../windows/launcher";
import type { SessionMetadata } from "../api/types";
import {
  Io5Add,
  Io5Create,
  Io5Eye,
  Io5RadioButtonOn,
  Io5Trash,
} from "vue-icons-plus/io5";

const creating = ref(false);
const editingId = ref<number | null>(null);
const editName = ref("");
const editDesc = ref("");
const editTags = ref<string[]>([]);
const tagDraft = ref("");
const tagError = ref("");
const TAG_PATTERN = /^[a-z0-9_]{1,20}$/;

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
  editTags.value = [...session.tags];
  tagDraft.value = "";
  tagError.value = "";
}

function addTag(): void {
  const tag = tagDraft.value;
  if (!TAG_PATTERN.test(tag)) {
    tagError.value = "仅支持 1–20 位小写字母、数字和下划线";
    return;
  }
  if (!editTags.value.includes(tag)) {
    editTags.value = [...editTags.value, tag].sort();
  }
  tagDraft.value = "";
  tagError.value = "";
}

function removeTag(tag: string): void {
  editTags.value = editTags.value.filter((item) => item !== tag);
}

async function submitEdit(): Promise<void> {
  if (editingId.value === null) return;
  const name = editName.value.trim();
  if (!name) return;
  const updated = await sessionsStore.update(
    editingId.value,
    name,
    editDesc.value.trim() || null,
    editTags.value,
  );
  if (updated) {
    editingId.value = null;
    appStore.toast("会话信息已更新", "success");
  }
}

async function removeSession(id: number, name: string): Promise<void> {
  const ok = await confirmDialog({
    title: "删除会话",
    message: `确定删除会话「${name}」及其全部抓包记录吗？此操作不可恢复。`,
    confirmText: "删除",
    danger: true,
  });
  if (ok) await sessionsStore.remove(id);
}

async function setDefault(session: SessionMetadata): Promise<void> {
  if (session.tags.includes("default")) return;
  const updated = await sessionsStore.update(
    session.id,
    session.name,
    session.description,
    [...session.tags, "default"],
  );
  if (updated) appStore.toast(`「${session.name}」已设为默认会话`, "success");
}

function sessionMenu(event: MouseEvent, session: SessionMetadata): void {
  openContextMenu(event, [
    { label: "查看", icon: Io5Eye, action: () => sessionsStore.view(session.id) },
    { label: "编辑会话", icon: Io5Create, action: () => startEdit(session) },
    {
      label: "设为默认",
      icon: Io5RadioButtonOn,
      disabled: session.tags.includes("default"),
      action: () => void setDefault(session),
    },
    {
      label: "删除",
      icon: Io5Trash,
      danger: true,
      dividerBefore: true,
      disabled: proxyStore.status.status === "starting" ||
        proxyStore.status.status === "running" ||
        proxyStore.status.status === "stopping",
      action: () => void removeSession(session.id, session.name),
    },
  ]);
}

onMounted(() => {
  void routingStore.refresh();
});
</script>

<template>
  <aside class="sidebar">
    <div class="sb-top">
      <div class="sb-header">
        <span class="sb-title">会话</span>
        <div class="sb-header-actions">
          <button class="btn icon" title="透明转发记录" @click="openBypass">
            <Io5Eye :size="15" />
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
            <div class="sb-tag-editor">
              <div v-if="editTags.length" class="sb-tags">
                <button
                  v-for="tag in editTags"
                  :key="tag"
                  type="button"
                  class="sb-tag removable"
                  :class="{ default: tag === 'default' }"
                  :title="`移除 ${tag}`"
                  @click="removeTag(tag)"
                >
                  {{ tag }} ×
                </button>
              </div>
              <input
                v-model="tagDraft"
                class="input mono sb-tag-input"
                placeholder="添加 tag"
                @keyup.enter="addTag"
                @keyup.esc="tagDraft = ''"
              />
              <span v-if="tagError" class="sb-tag-error">{{ tagError }}</span>
            </div>
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
          </div>
          <div class="sb-item-meta">
            <span>{{ formatRelativeTime(session.created_at) }}</span>
            <span v-if="session.description" class="sb-desc" :title="session.description">
              {{ session.description }}
            </span>
            <div v-if="session.tags.length" class="sb-tags">
              <span
                v-for="(tag, index) in session.tags"
                :key="tag"
                class="sb-tag"
                :class="{ default: tag === 'default' }"
                :title="tag"
              >
                {{ index === 0 ? tag : tag.charAt(0) }}
              </span>
            </div>
          </div>
        </template>
      </div>
      <div v-if="!sessionsStore.loading && sessionsStore.sessions.length === 0" class="empty-hint">
        暂无会话，点击右上角 + 新建
      </div>
    </div>
  </aside>
</template>

<style scoped>
.sidebar {
  width: 240px;
  flex: none;
  display: flex;
  flex-direction: column;
  background: var(--bg-panel);
  border-right: 1px solid var(--border);
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
.sb-tags {
  display: flex;
  flex-wrap: wrap;
  gap: 4px;
  margin-top: 5px;
}
.sb-tag {
  max-width: 100%;
  padding: 0 5px;
  border-radius: 4px;
  background: #6b7280;
  color: #fff;
  font: inherit;
  font-family: var(--font-mono);
  font-size: 10px;
  line-height: 16px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.sb-tag.default {
  background: var(--accent);
}
.sb-tag.removable {
  border: 0;
  cursor: pointer;
  text-align: left;
  white-space: normal;
  overflow-wrap: anywhere;
}
.sb-tag-editor {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 4px;
  min-width: 0;
  padding: 1px 3px;
}
.sb-tag-editor .sb-tags {
  display: contents;
}
.sb-tag-editor .sb-tag {
  flex: none;
  max-width: 100%;
  margin: 0;
}
.sb-tag-editor .sb-tag-input {
  flex: 1 1 64px;
  width: auto;
  min-width: 64px;
  height: 20px;
  margin: 0;
  padding: 0 4px;
  font-size: 10px;
}
.sb-tag-error {
  display: block;
  flex-basis: 100%;
  margin-top: 3px;
  color: var(--danger);
  font-size: 10px;
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
.sb-item-meta .sb-tags {
  flex: none;
  min-width: 0;
  max-width: 60%;
  display: flex;
  align-items: center;
  gap: 3px;
  flex-wrap: nowrap;
  margin-top: 0;
  margin-left: auto;
}
.sb-item-meta .sb-tag {
  flex-shrink: 1;
  min-width: 0;
}
.sb-item-meta .sb-tag:not(:first-child) {
  flex: none;
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
.sb-edit .sb-tag-input {
  width: auto;
}
</style>
