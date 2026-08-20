<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import { Io5Add, Io5Globe, Io5Settings, Io5Trash, Io5Warning } from "vue-icons-plus/io5";
import { useBackend } from "../api";
import type {
  ApiActionView,
  HttpApiChange,
  HttpServiceStatus,
  PermissionIdentitySummary,
  PermissionMode,
} from "../api/types";
import ApiKeySecretDialog from "../components/ApiKeySecretDialog.vue";
import CustomSelect, { type CustomSelectOption } from "../components/CustomSelect.vue";
import PermissionEditor from "../components/PermissionEditor.vue";
import { appStore, reportError } from "../stores/app";
import { confirmDialog } from "../stores/dialog";
import { HTTP_API_CHANGE_EVENT } from "../stores/http-api-sync";
import { windowsStore } from "../stores/windows";
import { formatDateTime } from "../utils/format";
import { DIMENSION_OPTIONS, type PermissionGroupDimension } from "../utils/permission-meta";

type SettingsPage = "general" | "management";
const WINDOW_ID = "settings";
const backend = useBackend();
const page = ref<SettingsPage>("general");

const currentPath = ref("");
const configuredPath = ref("");
const loadedConfiguredPath = ref("");
const externalChanged = ref(false);
const serviceStatus = ref<HttpServiceStatus | null>(null);
const permissionConfigError = ref<string | null>(null);

const catalog = ref<ApiActionView[]>([]);
const dimension = ref<PermissionGroupDimension>("function");
const identities = ref<PermissionIdentitySummary[]>([]);
const selectedIdentityId = ref<string | null>(null);
const permissions = ref<Record<string, PermissionMode>>({});
const loadedPermissions = ref<Record<string, PermissionMode>>({});
const loadingPermissions = ref(false);
const savingPermissions = ref(false);
const showCreateDialog = ref(false);

const workspaceDirty = computed(() => configuredPath.value !== loadedConfiguredPath.value);
const permissionDirty = computed(
  () => JSON.stringify(permissions.value) !== JSON.stringify(loadedPermissions.value),
);
const selectedIdentity = computed(() =>
  identities.value.find((identity) => identity.id === selectedIdentityId.value),
);
const identityOptions = computed<CustomSelectOption[]>(() =>
  identities.value.map((identity) => ({
    value: identity.id,
    label: identity.kind === "local" ? "本机无 API Key" : identity.name,
    description:
      identity.kind === "api_key" && identity.prefix ? `前缀 ${identity.prefix}` : undefined,
  })),
);
const managementDisabled = computed(() => permissionConfigError.value !== null);
const canSwitchWorkspace = backend.capabilities.workspaceSwitch;
const serviceUrl = computed(() => {
  const status = serviceStatus.value;
  return status ? `http://${status.host}:${status.port}` : "—";
});

async function refreshWorkspace(): Promise<void> {
  try {
    const workspace = await backend.getWorkspace();
    currentPath.value = workspace.current_path;
    configuredPath.value = workspace.configured_path;
    loadedConfiguredPath.value = workspace.configured_path;
  } catch (error) {
    reportError(error, "获取工作区信息失败");
  }
}

async function refreshManagement(preferredId?: string): Promise<void> {
  try {
    serviceStatus.value = await backend.getHttpServiceStatus();
  } catch (error) {
    reportError(error, "获取 HTTP 管理接口状态失败");
    return;
  }
  try {
    const [nextCatalog, nextIdentities] = await Promise.all([
      backend.getHttpPermissionCatalog(),
      backend.listHttpPermissionIdentities(),
    ]);
    catalog.value = nextCatalog;
    identities.value = nextIdentities;
    permissionConfigError.value = null;
    const nextId =
      preferredId && nextIdentities.some((identity) => identity.id === preferredId)
        ? preferredId
        : selectedIdentityId.value &&
            nextIdentities.some((identity) => identity.id === selectedIdentityId.value)
          ? selectedIdentityId.value
          : nextIdentities[0]?.id;
    if (nextId) await loadIdentity(nextId);
  } catch (error) {
    permissionConfigError.value =
      error instanceof Error ? error.message : typeof error === "string" ? error : String(error);
    identities.value = [];
    catalog.value = [];
    selectedIdentityId.value = null;
    permissions.value = {};
    loadedPermissions.value = {};
  }
}

async function loadIdentity(id: string): Promise<void> {
  loadingPermissions.value = true;
  try {
    const result = await backend.getHttpIdentityPermissions(id);
    const next = Object.fromEntries(
      result.permissions.map((entry) => [
        entry.action_id,
        backend.capabilities.permissionModes.includes(entry.mode) ? entry.mode : "deny",
      ]),
    ) as Record<string, PermissionMode>;
    selectedIdentityId.value = id;
    permissions.value = next;
    loadedPermissions.value = { ...next };
  } catch (error) {
    reportError(error, "加载身份权限失败");
  } finally {
    loadingPermissions.value = false;
  }
}

async function confirmDiscardPermissions(): Promise<boolean> {
  if (!permissionDirty.value) return true;
  return confirmDialog({
    title: "放弃权限修改？",
    message: "当前身份的权限尚未保存，继续将丢弃这些修改。",
    confirmText: "放弃修改",
    danger: true,
  });
}

async function confirmDiscardCurrentPage(): Promise<boolean> {
  if (showCreateDialog.value) return false;
  if (page.value === "management") return confirmDiscardPermissions();
  if (!workspaceDirty.value) return true;
  return confirmDialog({
    title: "放弃工作区修改？",
    message: "下次启动工作区尚未保存，继续将丢弃这项修改。",
    confirmText: "放弃修改",
    danger: true,
  });
}

async function selectPage(next: SettingsPage): Promise<void> {
  if (next === page.value) return;
  if (!(await confirmDiscardCurrentPage())) return;
  if (page.value === "management") permissions.value = { ...loadedPermissions.value };
  else configuredPath.value = loadedConfiguredPath.value;
  page.value = next;
}

async function selectIdentity(id: string): Promise<void> {
  if (id === selectedIdentityId.value) return;
  if (!(await confirmDiscardPermissions())) return;
  await loadIdentity(id);
}

async function saveWorkspace(): Promise<void> {
  const path = configuredPath.value.trim();
  if (!path) {
    reportError("工作区路径不能为空");
    return;
  }
  try {
    const workspace = await backend.setWorkspaceForNextStart(path);
    configuredPath.value = workspace.configured_path;
    loadedConfiguredPath.value = workspace.configured_path;
    externalChanged.value = false;
    appStore.toast("已保存，重启后生效", "success");
  } catch (error) {
    reportError(error, "保存工作区失败");
  }
}

async function savePermissions(): Promise<void> {
  if (!selectedIdentityId.value || !permissionDirty.value || savingPermissions.value) return;
  savingPermissions.value = true;
  try {
    const entries = catalog.value.map((action) => ({
      action_id: action.id,
      mode: permissions.value[action.id] ?? action.default_mode,
    }));
    const result = await backend.replaceHttpIdentityPermissions(
      selectedIdentityId.value,
      entries,
    );
    const next = Object.fromEntries(
      result.permissions.map((entry) => [entry.action_id, entry.mode]),
    ) as Record<string, PermissionMode>;
    permissions.value = next;
    loadedPermissions.value = { ...next };
    appStore.toast("权限已保存", "success");
  } catch (error) {
    reportError(error, "保存权限失败");
  } finally {
    savingPermissions.value = false;
  }
}

async function openCreate(): Promise<void> {
  if (!(await confirmDiscardPermissions())) return;
  permissions.value = { ...loadedPermissions.value };
  showCreateDialog.value = true;
}

async function onCreateClosed(createdId?: string): Promise<void> {
  showCreateDialog.value = false;
  if (createdId) await refreshManagement(createdId);
}

async function deleteSelectedKey(): Promise<void> {
  const identity = selectedIdentity.value;
  if (!identity || identity.kind !== "api_key") return;
  if (!(await confirmDiscardPermissions())) return;
  const ok = await confirmDialog({
    title: "删除 API Key",
    message: `删除“${identity.name}”后，使用该密钥的新请求将立即无法鉴权。此操作不可恢复。`,
    confirmText: "删除",
    danger: true,
  });
  if (!ok) return;
  try {
    await backend.deleteHttpApiKey(identity.id);
    appStore.toast("API Key 已删除", "success");
    await refreshManagement("local");
  } catch (error) {
    reportError(error, "删除 API Key 失败");
  }
}

function reloadExternal(): void {
  externalChanged.value = false;
  void refreshWorkspace();
}

function onHttpApiChange(event: Event): void {
  const { resources } = (event as CustomEvent<HttpApiChange>).detail;
  if (!resources.includes("all") && !resources.includes("workspace") && !resources.includes("config")) return;
  if (workspaceDirty.value) {
    externalChanged.value = true;
    return;
  }
  externalChanged.value = false;
  void refreshWorkspace();
}

function onKeydown(event: KeyboardEvent): void {
  if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "s") {
    event.preventDefault();
    if (page.value === "general") void saveWorkspace();
    else void savePermissions();
  }
}

onMounted(() => {
  windowsStore.registerCloseGuard(WINDOW_ID, confirmDiscardCurrentPage);
  window.addEventListener(HTTP_API_CHANGE_EVENT, onHttpApiChange);
  window.addEventListener("keydown", onKeydown);
  void Promise.all([refreshWorkspace(), refreshManagement()]);
});

onBeforeUnmount(() => {
  windowsStore.unregisterCloseGuard(WINDOW_ID);
  window.removeEventListener(HTTP_API_CHANGE_EVENT, onHttpApiChange);
  window.removeEventListener("keydown", onKeydown);
});
</script>

<template>
  <div class="settings-root">
    <aside class="settings-sidebar">
      <button :class="{ active: page === 'general' }" @click="selectPage('general')">
        <Io5Settings :size="15" />
        通用
      </button>
      <button :class="{ active: page === 'management' }" @click="selectPage('management')">
        <Io5Globe :size="15" />
        管理接口
      </button>
    </aside>

    <main class="settings-content">
      <template v-if="page === 'general'">
        <header class="page-header">
          <div>
            <h2>通用</h2>
            <p>设置工作区和应用启动行为</p>
          </div>
        </header>
        <div v-if="externalChanged" class="external-warning">
          <span>设置已被外部修改，当前未保存输入仍保留。</span>
          <button class="btn compact" @click="reloadExternal">重新加载</button>
        </div>
        <section class="settings-section">
          <h3>工作区</h3>
          <label class="settings-field">
            <span>当前工作区</span>
            <code>{{ currentPath || "—" }}</code>
          </label>
          <label class="settings-field">
            <span>下次启动工作区</span>
            <div class="field-row">
              <input v-model="configuredPath" class="input" placeholder="工作区目录路径" :disabled="!canSwitchWorkspace" />
              <button v-if="canSwitchWorkspace" class="btn primary" :disabled="!workspaceDirty" @click="saveWorkspace">
                保存
              </button>
            </div>
          </label>
        </section>
      </template>

      <template v-else>
        <header class="page-header management-header">
          <div>
            <h2>管理接口</h2>
            <p>配置本机调用和每个 API Key 可访问的接口</p>
          </div>
          <div class="service-state" :class="{ failed: serviceStatus?.error }">
            <span class="status-dot" />
            <span>{{ serviceStatus?.running ? "运行中" : serviceStatus?.error ? "启动失败" : "未运行" }}</span>
            <code>{{ serviceUrl }}</code>
          </div>
        </header>

        <div v-if="serviceStatus?.error" class="service-error">
          <Io5Warning :size="17" />
          <div>
            <strong>管理接口未启动</strong>
            <span>{{ serviceStatus.error }}</span>
            <small v-if="!permissionConfigError">权限仍可配置，服务将在问题修复并重启应用后使用新设置。</small>
          </div>
        </div>
        <div v-if="permissionConfigError" class="service-error permission-error">
          <Io5Warning :size="17" />
          <div>
            <strong>权限配置不可用</strong>
            <span>{{ permissionConfigError }}</span>
            <small>请修复工作区中的 http_api_permissions.json 后重启应用。ProxyCrab 不会自动覆盖损坏的配置。</small>
          </div>
        </div>

        <template v-if="!managementDisabled">
          <section class="identity-section">
            <div class="identity-picker">
              <CustomSelect
                class="identity-select"
                :model-value="selectedIdentityId"
                :options="identityOptions"
                placeholder="选择身份"
                aria-label="权限身份"
                :disabled="loadingPermissions"
                @update:model-value="selectIdentity"
              />
              <button class="btn" @click="openCreate">
                <Io5Add :size="14" />
                新建
              </button>
            </div>
            <div v-if="selectedIdentity?.kind === 'api_key'" class="identity-detail">
              <span>创建于 {{ formatDateTime(selectedIdentity.created_at ?? 0) }}</span>
              <span>最近使用 {{ selectedIdentity.last_used_at ? formatDateTime(selectedIdentity.last_used_at) : "从未" }}</span>
              <span class="mono">前缀 {{ selectedIdentity.prefix }}</span>
              <button class="btn danger compact" @click="deleteSelectedKey">
                <Io5Trash :size="13" />
                删除
              </button>
            </div>
            <p v-else class="identity-hint">未携带 Authorization 的本机请求使用此身份。</p>
          </section>

          <section class="permission-section">
            <div class="permission-heading">
              <div>
                <h3>接口权限</h3>
                <span v-if="permissionDirty" class="dirty-label">有未保存修改</span>
              </div>
              <div class="permission-heading-actions">
                <div class="dimension-switch" role="group" aria-label="权限分组维度">
                  <button
                    v-for="option in DIMENSION_OPTIONS"
                    :key="option.value"
                    type="button"
                    class="dimension-btn"
                    :class="{ active: dimension === option.value }"
                    :aria-pressed="dimension === option.value"
                    @click="dimension = option.value"
                  >
                    {{ option.label }}
                  </button>
                </div>
                <button
                  class="btn primary"
                  :disabled="!permissionDirty || savingPermissions || loadingPermissions"
                  @click="savePermissions"
                >
                  {{ savingPermissions ? "保存中…" : "保存" }}
                </button>
              </div>
            </div>
            <div v-if="loadingPermissions" class="permission-loading">加载权限…</div>
            <PermissionEditor
              v-else-if="selectedIdentityId"
              v-model="permissions"
              :catalog="catalog"
              :dimension="dimension"
              :allowed-modes="backend.capabilities.permissionModes"
              :disabled="savingPermissions"
            />
          </section>
        </template>
      </template>
    </main>

    <ApiKeySecretDialog
      v-if="showCreateDialog"
      :identities="identities"
      @close="onCreateClosed"
    />
  </div>
</template>

<style scoped>
.settings-root { flex: 1; min-height: 0; display: flex; background: var(--bg-app); }
.settings-sidebar { width: 154px; flex: none; padding: 10px 8px; border-right: 1px solid var(--border); background: var(--bg-panel); }
.settings-sidebar button { width: 100%; height: 34px; display: flex; align-items: center; gap: 8px; padding: 0 10px; border: 0; border-radius: var(--radius-md); background: transparent; color: var(--text-secondary); font: inherit; font-size: 12px; cursor: pointer; }
.settings-sidebar button:hover { background: var(--bg-hover); color: var(--text); }
.settings-sidebar button.active { background: var(--bg-selected); color: var(--accent); font-weight: 600; }
.settings-content { flex: 1; min-width: 0; min-height: 0; overflow-y: auto; padding: 0 22px 24px; }
.page-header { min-height: 76px; display: flex; align-items: center; justify-content: space-between; gap: 16px; border-bottom: 1px solid var(--border); }
.page-header h2 { margin: 0; font-size: 16px; }
.page-header p { margin: 2px 0 0; color: var(--text-secondary); font-size: 11px; }
.settings-section { max-width: 680px; padding-top: 20px; }
.settings-section h3, .permission-heading h3 { margin: 0; font-size: 12px; }
.settings-field { display: flex; flex-direction: column; gap: 5px; margin-top: 14px; color: var(--text-secondary); font-size: 11px; }
.settings-field code { overflow-wrap: anywhere; color: var(--text); font: 12px/1.5 var(--font-mono); user-select: text; }
.field-row { display: flex; gap: 8px; }
.field-row .input { flex: 1; min-width: 0; color: var(--text); }
.external-warning { display: flex; align-items: center; gap: 10px; margin-top: 14px; padding: 8px 10px; border-left: 3px solid var(--warning); background: color-mix(in srgb, var(--warning) 9%, transparent); color: var(--warning); font-size: 11px; }
.external-warning span { flex: 1; }
.service-state { display: flex; align-items: center; gap: 7px; color: var(--success); font-size: 11px; }
.service-state code { color: var(--text-secondary); font: 10px var(--font-mono); user-select: text; }
.status-dot { width: 7px; height: 7px; border-radius: 50%; background: currentColor; }
.service-state.failed { color: var(--danger); }
.service-error { display: flex; align-items: flex-start; gap: 10px; margin-top: 18px; padding: 12px; border-left: 3px solid var(--danger); background: color-mix(in srgb, var(--danger) 8%, transparent); color: var(--danger); }
.service-error > svg { flex: none; margin-top: 1px; }
.service-error div { display: flex; flex-direction: column; }
.service-error span { margin-top: 3px; overflow-wrap: anywhere; font: 11px/1.5 var(--font-mono); user-select: text; }
.service-error small { margin-top: 5px; color: var(--text-secondary); }
.permission-error { margin-top: 10px; }
.identity-section { padding: 16px 0; border-bottom: 1px solid var(--border); }
.identity-picker { display: flex; align-items: stretch; gap: 8px; }
.identity-select { width: min(420px, calc(100% - 82px)); }
.identity-detail { display: flex; align-items: center; flex-wrap: wrap; gap: 4px 14px; margin-top: 9px; color: var(--text-secondary); font-size: 10px; }
.identity-detail .danger { margin-left: auto; }
.identity-hint { margin: 7px 0 0; color: var(--text-faint); font-size: 10px; }
.permission-section { padding-top: 14px; }
.permission-heading { position: sticky; z-index: 2; top: 0; display: flex; align-items: center; justify-content: space-between; gap: 12px; min-height: 42px; margin: -14px 0 0; padding: 10px 0 7px; background: var(--bg-app); }
.permission-heading > div { display: flex; align-items: baseline; gap: 8px; }
.permission-heading .permission-heading-actions { align-items: center; }
.dimension-switch { display: inline-flex; padding: 2px; border: 1px solid var(--border); border-radius: var(--radius-md); background: var(--bg-panel); }
.dimension-btn { min-width: 44px; height: 24px; padding: 0 8px; border: 0; border-radius: calc(var(--radius-md) - 2px); background: transparent; color: var(--text-secondary); font: inherit; font-size: 11px; cursor: pointer; }
.dimension-btn:hover { color: var(--text); }
.dimension-btn.active { background: var(--bg-selected); color: var(--accent); font-weight: 600; }
.dirty-label { color: var(--warning); font-size: 10px; }
.permission-loading { padding: 28px; color: var(--text-faint); text-align: center; }
.compact { min-height: 25px; padding: 2px 8px; font-size: 11px; }
@media (max-width: 660px) {
  .settings-sidebar { width: 122px; }
  .settings-content { padding-right: 14px; padding-left: 14px; }
  .management-header { align-items: flex-start; flex-direction: column; justify-content: center; gap: 5px; }
}
</style>
