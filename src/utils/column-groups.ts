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

export function exactColumnRegex(value: string): string {
  return `^${value.replace(/[\\^$.*+?()[\]{}|]/g, "\\$&")}$`;
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
