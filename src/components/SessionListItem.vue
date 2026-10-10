<script setup lang="ts">
/**
 * 会话行的唯一实现：侧边栏、发送请求的会话选择器、已归档 Session 窗口共用，
 * 保证各处会话列表样式一致。事件（click/contextmenu/dblclick）通过属性继承
 * 落到根元素，由调用方自行绑定。
 */
import { Io5RadioButtonOn } from "vue-icons-plus/io5";
import type { SessionMetadata } from "../api/types";
import { formatActivityCount, formatRelativeTime } from "../utils/format";

withDefaults(
  defineProps<{
    session: SessionMetadata;
    /** 主表格正在查看的会话（侧边栏高亮项）。 */
    viewing?: boolean;
    /** 是否为活跃会话。 */
    active?: boolean;
    /** 活跃连接数，大于 0 时以徽标替代「活跃」文字。 */
    activeConnections?: number;
  }>(),
  { viewing: false, active: false, activeConnections: 0 },
);
</script>

<template>
  <div class="session-item" :class="{ viewing, active }">
    <div class="session-item-body">
      <div class="session-item-top">
        <span class="session-item-name" :title="session.name">{{ session.name }}</span>
        <span
          v-if="active"
          class="session-item-active"
          :title="
            activeConnections > 0
              ? `活跃会话，${activeConnections} 条活跃连接`
              : '活跃会话'
          "
        >
          <span v-if="activeConnections > 0" class="activity-badge session-item-badge">
            {{ formatActivityCount(activeConnections) }}
          </span>
          <Io5RadioButtonOn v-else :size="11" />活跃
        </span>
      </div>
      <div class="session-item-meta">
        <span>{{ formatRelativeTime(session.created_at) }}</span>
        <span
          v-if="session.description"
          class="session-item-desc"
          :title="session.description"
        >
          {{ session.description }}
        </span>
      </div>
    </div>
    <div v-if="$slots.actions" class="session-item-actions">
      <slot name="actions" />
    </div>
  </div>
</template>

<style scoped>
.session-item {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 7px 10px;
  border-radius: var(--radius-md);
  cursor: pointer;
  margin-bottom: 2px;
}
.session-item-body {
  flex: 1;
  min-width: 0;
}
.session-item:hover {
  background: var(--bg-hover);
}
.session-item.viewing {
  background: var(--bg-selected);
}
.session-item.active .session-item-name {
  color: var(--accent);
}
.session-item-top {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 6px;
}
.session-item-name {
  font-weight: 500;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.session-item-active {
  flex: none;
  display: inline-flex;
  align-items: center;
  gap: 3px;
  color: var(--accent);
  font-size: 10px;
}
.session-item-badge {
  min-width: 13px;
  height: 13px;
  padding: 0 2px;
  font-size: 8px;
}
.session-item-meta {
  display: flex;
  align-items: center;
  gap: 8px;
  min-width: 0;
  font-size: 11px;
  color: var(--text-faint);
  margin-top: 2px;
}
.session-item-desc {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
/* 行内操作（已归档窗口）：占位排版，悬停或聚焦时才显形，不遮挡行文本。 */
.session-item-actions {
  flex: none;
  display: flex;
  align-items: center;
  gap: 2px;
  opacity: 0;
  pointer-events: none;
}
.session-item:hover .session-item-actions,
.session-item:focus-within .session-item-actions {
  opacity: 1;
  pointer-events: auto;
}
</style>
