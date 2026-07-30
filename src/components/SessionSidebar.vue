<script setup lang="ts">
import { ref } from "vue";
import { sessionsStore } from "../stores/sessions";
import { confirmDialog, openContextMenu } from "../stores/dialog";
import { formatRelativeTime } from "../utils/format";
import { appStore } from "../stores/app";
import type { SessionMetadata } from "../api/types";
import {
  Io5Add,
  Io5Checkmark,
  Io5Create,
  Io5Eye,
  Io5RadioButtonOn,
  Io5Trash,
} from "vue-icons-plus/io5";

const creating = ref(false);
const editingId = ref<number | null>(null);
const editName = ref("");
const editDesc = ref("");

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

async function removeSession(id: number, name: string): Promise<void> {
  const ok = await confirmDialog({
    title: "删除会话",
    message: `确定删除会话「${name}」及其全部抓包记录吗？此操作不可恢复。`,
    confirmText: "删除",
    danger: true,
  });
  if (ok) await sessionsStore.remove(id);
}

function sessionMenu(event: MouseEvent, session: SessionMetadata): void {
  openContextMenu(event, [
    { label: "查看", icon: Io5Eye, action: () => sessionsStore.view(session.id) },
    {
      label: "设为活跃会话",
      icon: Io5RadioButtonOn,
      disabled: sessionsStore.activeSessionId === session.id,
      action: () => void sessionsStore.activate(session.id),
    },
    { label: "编辑会话", icon: Io5Create, action: () => startEdit(session) },
    {
      label: "删除",
      icon: Io5Trash,
      danger: true,
      disabled: sessionsStore.activeSessionId === session.id,
      action: () => void removeSession(session.id, session.name),
    },
  ]);
}
</script>

<template>
  <aside class="sidebar">
    <div class="sb-header">
      <span class="sb-title">会话</span>
      <button
        class="btn icon"
        title="新建会话"
        :disabled="creating"
        @click="createSession"
      >
        <Io5Add :size="16" />
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
              class="input"
              placeholder="会话名称"
              autofocus
              @keyup.enter="submitEdit"
              @keyup.esc="editingId = null"
            />
            <input
              v-model="editDesc"
              class="input"
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
            <Io5Checkmark
              v-if="sessionsStore.activeSessionId === session.id"
              :size="14"
              class="sb-active"
              title="活跃会话"
            />
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
.sb-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 8px 12px;
  border-bottom: 1px solid var(--border);
}
.sb-title {
  font-size: 12px;
  font-weight: 600;
  color: var(--text-secondary);
}
.sb-edit {
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.sb-edit-actions {
  display: flex;
  justify-content: flex-end;
  gap: 6px;
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
.sb-active {
  color: var(--success);
  flex: none;
}
.sb-item-meta {
  display: flex;
  gap: 8px;
  font-size: 11px;
  color: var(--text-faint);
  margin-top: 2px;
}
.sb-desc {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.sb-edit .input {
  width: 100%;
}
</style>
