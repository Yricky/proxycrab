export interface FileTreeEntry<T> {
  path: string;
  label?: string;
  title?: string;
  data: T;
}

export interface FileTreeRow<T> {
  path: string;
  label: string;
  depth: number;
  entry?: FileTreeEntry<T>;
}

/** Directories are derived from relative paths; file identity is independent of its label. */
export function fileTreeRows<T>(
  entries: readonly FileTreeEntry<T>[],
  collapsed: ReadonlySet<string>,
): FileTreeRow<T>[] {
  const children = new Map<string, Map<string, FileTreeRow<T>>>();
  for (const entry of entries) {
    const parts = entry.path.split("/");
    let parent = "";
    parts.forEach((part, depth) => {
      const path = parent ? `${parent}/${part}` : part;
      const file = depth === parts.length - 1;
      if (!children.has(parent)) children.set(parent, new Map());
      children.get(parent)!.set(`${file ? "file" : "dir"}:${path}`, {
        path,
        label: file ? (entry.label ?? part) : part,
        depth,
        ...(file ? { entry } : {}),
      });
      parent = path;
    });
  }
  const rows: FileTreeRow<T>[] = [];
  function visit(parent: string): void {
    const entries = [...(children.get(parent)?.values() ?? [])].sort(
      (a, b) => Number(!!a.entry) - Number(!!b.entry) || a.label.localeCompare(b.label),
    );
    for (const entry of entries) {
      rows.push(entry);
      if (!entry.entry && !collapsed.has(entry.path)) visit(entry.path);
    }
  }
  visit("");
  return rows;
}

export function revealFilePath(collapsed: Set<string>, path: string): void {
  const parts = path.split("/");
  for (let i = 1; i < parts.length; i++) collapsed.delete(parts.slice(0, i).join("/"));
}
