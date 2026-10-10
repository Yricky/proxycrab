<script setup lang="ts">
import { onMounted } from "vue";
import { Io5ArrowUndo, Io5Refresh, Io5Trash } from "vue-icons-plus/io5";
import type { SessionMetadata } from "../api/types";
import { appStore } from "../stores/app";
import { confirmDialog } from "../stores/dialog";
import { sessionsStore } from "../stores/sessions";
import SessionListItem from "../components/SessionListItem.vue";

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
      <SessionListItem
        v-for="session in sessionsStore.archivedSessions"
        :key="session.id"
        :session="session"
      >
        <template #actions>
          <button class="btn icon" title="取消归档" @click="restore(session)">
            <Io5ArrowUndo :size="14" />
          </button>
          <button class="btn icon" title="永久删除" @click="remove(session)">
            <Io5Trash :size="14" />
          </button>
        </template>
      </SessionListItem>
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
  padding: 4px;
}
</style>
