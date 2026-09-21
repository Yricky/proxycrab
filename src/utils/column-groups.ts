import type { FilterColumn } from "../api/types";

export type ColumnGroupSort = "count" | "value";

export interface ColumnGroupEntry {
  key: string;
  kind: "value" | "empty" | "error";
  value: string;
  label: string;
  count: number;
  exactPattern: string | null;
}

export function columnGroupKey(column: FilterColumn): string {
  return column.kind === "script"
    ? `script:${column.script_name}`
    : column.kind;
}

export function copyFilterColumn(column: FilterColumn): FilterColumn {
  return column.kind === "script"
    ? { kind: "script", script_name: column.script_name }
    : { kind: column.kind };
}

export function newestId(ids: number[]): number | undefined {
  return ids.reduce<number | undefined>(
    (newest, id) => (newest === undefined || id > newest ? id : newest),
    undefined,
  );
}

export function oldestId(ids: number[]): number | undefined {
  return ids.reduce<number | undefined>(
    (oldest, id) => (oldest === undefined || id < oldest ? id : oldest),
    undefined,
  );
}

function escapeRegExpLiteral(value: string): string {
  return value.replace(/[\\^$.*+?()[\]{}|]/g, "\\$&");
}

export function exactColumnRegex(value: string): string {
  return `^${escapeRegExpLiteral(value)}$`;
}

export function prefixColumnRegex(prefix: string): string {
  return `^${escapeRegExpLiteral(prefix)}`;
}

/**
 * Splits a value into tree segments by a literal delimiter. Delimiter runs are
 * kept as the prefix of the following segment (so `http://host/p` becomes
 * `http:` / `//host` / `/p`), and a leading empty segment is dropped.
 */
export function splitGroupSegments(value: string, delimiter: string): string[] {
  if (delimiter.length === 0 || value.length === 0) {
    return value.length > 0 ? [value] : [];
  }
  const runs: number[] = [];
  let index = 0;
  while (index < value.length) {
    if (value.startsWith(delimiter, index)) {
      runs.push(index);
      do {
        index += delimiter.length;
      } while (value.startsWith(delimiter, index));
    } else {
      index += 1;
    }
  }
  if (runs.length === 0) return [value];
  const segments: string[] = [];
  const first = value.slice(0, runs[0]);
  if (first.length > 0) segments.push(first);
  for (let i = 0; i < runs.length; i++) {
    const start = runs[i];
    const end = i + 1 < runs.length ? runs[i + 1] : value.length;
    segments.push(value.slice(start, end));
  }
  return segments;
}

export function filterAndSortGroups(
  values: Map<string, number>,
  emptyCount: number,
  errorCount: number,
  query: string,
  sort: ColumnGroupSort,
  descending: boolean,
): ColumnGroupEntry[] {
  const entries: ColumnGroupEntry[] = [...values.entries()].map(([value, count]) => ({
    key: `value:${value}`,
    kind: "value",
    value,
    label: value,
    count,
    exactPattern: exactColumnRegex(value),
  }));
  if (emptyCount > 0) {
    entries.push({
      key: "empty",
      kind: "empty",
      value: "",
      label: "（空值）",
      count: emptyCount,
      exactPattern: "^$",
    });
  }
  if (errorCount > 0) {
    entries.push({
      key: "error",
      kind: "error",
      value: "",
      label: "（计算失败）",
      count: errorCount,
      exactPattern: null,
    });
  }

  const needle = query.toLocaleLowerCase();
  const filtered = needle
    ? entries.filter((entry) => entry.label.toLocaleLowerCase().includes(needle))
    : entries;
  const direction = descending ? -1 : 1;
  return filtered.sort((left, right) => {
    const primary =
      sort === "count"
        ? left.count - right.count
        : left.label.localeCompare(right.label);
    if (primary !== 0) return primary * direction;
    return left.label.localeCompare(right.label) * direction;
  });
}

export interface ColumnGroupRow {
  key: string;
  kind: "group" | "value" | "empty" | "error";
  depth: number;
  label: string;
  tooltip: string;
  count: number;
  pattern: string | null;
  hasChildren: boolean;
  expanded: boolean;
  /** Keys of ancestor group rows, root first. Empty for flat/special rows. */
  ancestors: string[];
}

interface GroupTreeNode {
  segment: string;
  prefix: string;
  count: number;
  children: Map<string, GroupTreeNode>;
}

function compareTreeNodes(
  sort: ColumnGroupSort,
  descending: boolean,
): (left: GroupTreeNode, right: GroupTreeNode) => number {
  const direction = descending ? -1 : 1;
  return (left, right) => {
    const primary =
      sort === "count"
        ? left.count - right.count
        : left.segment.localeCompare(right.segment);
    if (primary !== 0) return primary * direction;
    return left.segment.localeCompare(right.segment) * direction;
  };
}

/**
 * Builds the rows rendered by the group popup. Search always matches against
 * the full original value in both modes. Without a delimiter this is the
 * classic flat list. With a delimiter the filtered values form a tree split by
 * the delimiter; branches are emitted only when expanded, except that an
 * active search force-expands every surviving branch so hits stay visible.
 */
export function buildGroupRows(
  values: Map<string, number>,
  emptyCount: number,
  errorCount: number,
  query: string,
  sort: ColumnGroupSort,
  descending: boolean,
  delimiter: string,
  expanded: ReadonlySet<string>,
): ColumnGroupRow[] {
  if (delimiter.length === 0) {
    return filterAndSortGroups(
      values,
      emptyCount,
      errorCount,
      query,
      sort,
      descending,
    ).map((entry) => ({
      key: entry.key,
      kind: entry.kind,
      depth: 0,
      label: entry.label,
      tooltip: entry.label,
      count: entry.count,
      pattern: entry.exactPattern,
      hasChildren: false,
      expanded: false,
      ancestors: [],
    }));
  }

  const needle = query.toLocaleLowerCase();
  const roots = new Map<string, GroupTreeNode>();
  for (const [value, count] of values) {
    if (needle.length > 0 && !value.toLocaleLowerCase().includes(needle)) {
      continue;
    }
    let level = roots;
    let prefix = "";
    for (const segment of splitGroupSegments(value, delimiter)) {
      prefix += segment;
      let node = level.get(segment);
      if (!node) {
        node = { segment, prefix, count: 0, children: new Map() };
        level.set(segment, node);
      }
      node.count += count;
      level = node.children;
    }
  }

  const rows: ColumnGroupRow[] = [];
  const specialVisible = (label: string): boolean =>
    needle.length === 0 || label.toLocaleLowerCase().includes(needle);
  if (emptyCount > 0 && specialVisible("（空值）")) {
    rows.push({
      key: "empty",
      kind: "empty",
      depth: 0,
      label: "（空值）",
      tooltip: "（空值）",
      count: emptyCount,
      pattern: "^$",
      hasChildren: false,
      expanded: false,
      ancestors: [],
    });
  }
  if (errorCount > 0 && specialVisible("（计算失败）")) {
    rows.push({
      key: "error",
      kind: "error",
      depth: 0,
      label: "（计算失败）",
      tooltip: "（计算失败）",
      count: errorCount,
      pattern: null,
      hasChildren: false,
      expanded: false,
      ancestors: [],
    });
  }

  const compare = compareTreeNodes(sort, descending);
  const visit = (
    nodes: Map<string, GroupTreeNode>,
    depth: number,
    ancestors: string[],
  ): void => {
    for (const node of [...nodes.values()].sort(compare)) {
      const isGroup = node.children.size > 0;
      const key = isGroup ? `group:${node.prefix}` : `value:${node.prefix}`;
      const isExpanded =
        isGroup && (needle.length > 0 || expanded.has(key));
      rows.push({
        key,
        kind: isGroup ? "group" : "value",
        depth,
        label: node.segment,
        tooltip: node.prefix,
        count: node.count,
        pattern: isGroup
          ? prefixColumnRegex(node.prefix)
          : exactColumnRegex(node.prefix),
        hasChildren: isGroup,
        expanded: isExpanded,
        ancestors,
      });
      if (isExpanded) visit(node.children, depth + 1, [...ancestors, key]);
    }
  };
  visit(roots, 0, []);
  return rows;
}
