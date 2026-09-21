#!/usr/bin/env bash
# 使用 AppImage 内嵌的 WebKit 运行时进行本地调试，保证 dev 环境与发布产物一致。
#
# 用法:
#   scripts/dev-appimage-webkit.sh import <AppImage路径>   # 导入/更新运行时到 .webkit-runtime/
#   scripts/dev-appimage-webkit.sh                         # 构建 debug 二进制并以该运行时启动
#
# 原理: 将 cargo 构建出的 debug 二进制替换进解包的 AppImage 目录，
# 再通过官方 AppRun 启动。窗口加载的仍是 vite dev server (http://localhost:1420)，
# 但 GTK/WebKit 运行时与发布的 AppImage 完全相同。
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
RUNTIME="$ROOT/.webkit-runtime"

if [[ "${1:-}" == "import" ]]; then
  APPIMAGE="${2:?用法: $0 import <AppImage路径>}"
  TMP="$RUNTIME.tmp"
  rm -rf "$TMP"
  mkdir -p "$TMP"
  (cd "$TMP" && "$APPIMAGE" --appimage-extract >/dev/null)
  rm -rf "$RUNTIME"
  mv "$TMP/squashfs-root" "$RUNTIME"
  rmdir "$TMP"
  echo "运行时已导入 $RUNTIME"
  exit 0
fi

if [[ ! -x "$RUNTIME/AppRun" ]]; then
  echo "未找到 $RUNTIME，请先运行: $0 import <AppImage路径>" >&2
  exit 1
fi

cd "$ROOT"
cargo build -p proxy-crab-t
cp target/debug/proxy-crab-t "$RUNTIME/usr/bin/proxy-crab-t"

pnpm dev &
VITE_PID=$!
trap 'kill $VITE_PID 2>/dev/null || true' EXIT

# 等待 vite dev server 就绪
for _ in $(seq 1 60); do
  curl -sf -o /dev/null http://localhost:1420 && break
  sleep 1
done

exec "$RUNTIME/AppRun"
