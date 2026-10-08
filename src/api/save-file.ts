import type { SaveFileRequest } from "./types";

/** base64 字符串转字节数组。 */
export function base64ToBytes(data: string): Uint8Array<ArrayBuffer> {
  const binary = atob(data);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i += 1) {
    bytes[i] = binary.charCodeAt(i);
  }
  return bytes;
}

/** Webview 环境的文件保存实现：Blob + a[download] 触发浏览器下载，始终返回成功。 */
export function saveFileInWebview(request: SaveFileRequest): Promise<boolean> {
  const blob = new Blob([request.data]);
  const url = URL.createObjectURL(blob);
  const link = document.createElement("a");
  link.href = url;
  link.download = request.filename;
  link.click();
  URL.revokeObjectURL(url);
  return Promise.resolve(true);
}
