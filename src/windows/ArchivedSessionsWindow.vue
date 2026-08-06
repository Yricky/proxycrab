<script setup lang="ts">
import { onMounted } from "vue";
import { Io5ArrowUndo, Io5Refresh, Io5Trash } from "vue-icons-plus/io5";
import type { SessionMetadata } from "../api/types";
import { appStore } from "../stores/app";
import { confirmDialog } from "../stores/dialog";
import { sessionsStore } from "../stores/sessions";
import { formatDateTime } from "../utils/format";

async function restore(session: SessionMetadata): Promise<void> {
  if (await sessionsStore.restore(session.id)) {
    appStore.toast(`「${session.name}」已取消归档`, "success");
  }
}

async function remove(session: SessionMetadata): Promise<void> {
  const ok = await confirmDialog({
    title: "永久删除已归档会话",
    message: `确定永久删除「${session.name}」及其全部抓包记录吗？此操作不可恢复。`,
    confirmText: "永久删除",
    danger: true,
  });
  if (ok && (await sessionsStore.deleteArchived(session.id))) {
    appStore.toast("已归档会话已永久删除", "success");
  }
}

onMounted(() => void sessionsStore.refresh());
</script>

<template>
  <div class="as-root">
    <div class="as-toolbar">
      <span class="text-secondary">已归档 Session</span>
      <span class="text-faint">{{ sessionsStore.archivedSessions.length }} 个</span>
      <span class="as-spacer" />
      <button class="btn icon" title="刷新" @click="sessionsStore.refresh">
        <Io5Refresh :size="14" />
      </button>
    </div>

    <div v-if="sessionsStore.archivedSessions.length === 0" class="empty-hint">
      暂无已归档 Session
    </div>
    <div v-else class="as-list">
      <div
        v-for="session in sessionsStore.archivedSessions"
        :key="session.id"
        class="as-row"
      >
        <div class="as-info">
          <div class="as-name" :title="session.name">{{ session.name }}</div>
          <div class="as-meta">
            <span>{{ formatDateTime(session.created_at) }}</span>
            <span class="mono">#{{ session.id }}</span>
          </div>
          <div v-if="session.description" class="as-description" :title="session.description">
            {{ session.description }}
          </div>
        </div>
        <div class="as-actions">
          <button class="btn" @click="restore(session)">
            <Io5ArrowUndo :size="14" />取消归档
          </button>
          <button class="btn danger" @click="remove(session)">
            <Io5Trash :size="14" />永久删除
          </button>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.as-root {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
}
.as-toolbar {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  padding: var(--space-2) var(--space-3);
  border-bottom: 1px solid var(--border);
}
.as-spacer {
  flex: 1;
}
.as-list {
  min-height: 0;
  overflow-y: auto;
}
.as-row {
  display: flex;
  align-items: center;
  min-height: 64px;
  padding: var(--space-3);
  border-bottom: 1px solid var(--border);
}
.as-row:hover {
  background: var(--bg-hover);
}
.as-info {
  flex: 1;
  min-width: 0;
}
.as-name {
  overflow: hidden;
  color: var(--text);
  font-weight: 600;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.as-meta {
  display: flex;
  gap: var(--space-3);
  margin-top: 4px;
  color: var(--text-faint);
  font-size: 11px;
}
.as-description {
  margin-top: 4px;
  overflow: hidden;
  color: var(--text-secondary);
  font-size: 12px;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.as-actions {
  display: flex;
  flex: none;
  gap: var(--space-2);
}
</style>
