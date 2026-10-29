export interface ScriptTreeRow<T> {
  path: string;
  label: string;
  depth: number;
  script?: T;
}

/** Derive directories from script identities; empty directories are not managed. */
export function scriptTreeRows<T extends { name: string }>(
  scripts: readonly T[],
  collapsed: ReadonlySet<string>,
): ScriptTreeRow<T>[] {
  const children = new Map<string, Map<string, ScriptTreeRow<T>>>();
  for (const script of scripts) {
    const parts = script.name.split("/");
    let parent = "";
    parts.forEach((label, depth) => {
      const path = parent ? `${parent}/${label}` : label;
      const file = depth === parts.length - 1;
      if (!children.has(parent)) children.set(parent, new Map());
      children.get(parent)!.set(`${file ? "file" : "dir"}:${path}`, {
        path,
        label: file ? `${label}.lua` : label,
        depth,
        ...(file ? { script } : {}),
      });
      parent = path;
    });
  }
  const rows: ScriptTreeRow<T>[] = [];
  function visit(parent: string): void {
    const entries = [...(children.get(parent)?.values() ?? [])].sort(
      (a, b) => Number(!!a.script) - Number(!!b.script) || a.label.localeCompare(b.label),
    );
    for (const entry of entries) {
      rows.push(entry);
      if (!entry.script && !collapsed.has(entry.path)) visit(entry.path);
    }
  }
  visit("");
  return rows;
}

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
