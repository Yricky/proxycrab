import { ref, useTemplateRef, watch } from "vue";
import type HexViewer from "../components/HexViewer.vue";
import { t } from "../i18n";
import { appStore, reportError } from "../stores/app";
import { openContextMenu } from "../stores/dialog";
import type { HexRightPane, HexSelection, HexViewerCopyFormat } from "./hex-viewer";

/** Shared application controls for Body and WebSocket Hex views. */
export function useHexViewerMenu() {
  const viewer = useTemplateRef<InstanceType<typeof HexViewer>>("hexViewer");
  const selection = ref<HexSelection | null>(null);
  const rightPane = ref<HexRightPane>("ascii");
  watch(viewer, () => {
    selection.value = null;
  });

  async function copy(format: HexViewerCopyFormat): Promise<void> {
    try {
      await viewer.value?.copy(format);
    } catch (error) {
      reportError(error, t("复制选中内容失败"));
    }
  }

  function contextMenu(event: MouseEvent): void {
    if (!selection.value) return;
    openContextMenu(event, [
      { label: t("按组分割复制"), action: () => void copy("group") },
      { label: t("按字节分割复制"), action: () => void copy("byte") },
      { label: t("不分割复制"), action: () => void copy("compact") },
      { label: t("复制 ASCII 字符串"), dividerBefore: true, action: () => void copy("ascii") },
    ]);
  }

  function copied(byteCount: number): void {
    appStore.toast(t("已复制 {n} 字节", { n: byteCount }), "success");
  }

  return { selection, rightPane, contextMenu, copied };
}
