export function validateScriptPath(name: string): string | null {
  if (!name) return "路径不能为空";
  if (new TextEncoder().encode(name).length > 128) return "路径不能超过 128 字节";
  if (
    name
      .split("/")
      .some((part) => !part || part === "." || part === ".." || !/^[\p{L}\p{N}._-]+$/u.test(part))
  ) {
    return "用 / 分隔目录；每段只能包含字母、数字、点、短横线和下划线，不能为 . 或 ..";
  }
  return null;
}
