export function clamp(value: number, min: number, max: number): number {
  return Math.max(min, Math.min(value, max));
}

export function fullyVisibleRowRange(
  scrollTop: number,
  viewportHeight: number,
  rowHeight: number,
  rowCount: number,
): { start: number; end: number } {
  const start = clamp(Math.ceil(scrollTop / rowHeight), 0, rowCount);
  const end = clamp(
    Math.floor((scrollTop + Math.max(0, viewportHeight)) / rowHeight),
    start,
    rowCount,
  );
  return { start, end };
}

export function anchoredScrollTop(
  scrollTop: number,
  previousIndex: number,
  nextIndex: number,
  rowHeight: number,
  maxScrollTop: number,
): number {
  return clamp(
    scrollTop + (nextIndex - previousIndex) * rowHeight,
    0,
    maxScrollTop,
  );
}

export function reconcileUnseenIds(
  unseenIds: ReadonlySet<number>,
  previousIds: ReadonlySet<number>,
  currentIds: readonly number[],
): Set<number> {
  const current = new Set(currentIds);
  const next = new Set([...unseenIds].filter((id) => current.has(id)));
  for (const id of currentIds) {
    if (!previousIds.has(id)) next.add(id);
  }
  return next;
}
