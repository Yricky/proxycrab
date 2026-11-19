import { afterEach, beforeEach, expect, test, vi } from "vitest";
import type { HttpApiChange } from "../api/generated/HttpApiChange";
import { watchBackendChanges } from "./api-changes";

vi.mock("../stores/app", () => ({ reportError: vi.fn() }));
const stops: (() => void)[] = [];
let changed: (change: HttpApiChange) => void;
const unsubscribe = vi.fn();
const backend = { subscribeChanges: vi.fn() };
beforeEach(() => {
  vi.resetAllMocks();
  vi.useFakeTimers();
  backend.subscribeChanges.mockImplementation(async (handler) => {
    changed = handler;
    return unsubscribe;
  });
});
afterEach(() => {
  for (const stop of stops.splice(0)) stop();
  vi.useRealTimers();
});
function watch(handler: (change: HttpApiChange) => void | Promise<void>) {
  const stop = watchBackendChanges(backend, handler);
  stops.push(stop);
  return stop;
}

test("each owner coalesces named resources while retaining global and Session scopes", async () => {
  const handler = vi.fn();
  watch(handler);
  changed({ resources: [{ kind: "column_scripts", names: ["first"] }], session_id: 1 });
  changed({
    resources: [{ kind: "column_scripts", names: ["second", "first"] }, { kind: "session_view" }],
    session_id: 1,
  });
  changed({ resources: [{ kind: "session_view" }], session_id: 2 });
  changed({ resources: [{ kind: "all" }] });
  await vi.advanceTimersByTimeAsync(40);
  expect(handler.mock.calls.map(([change]) => change)).toEqual([
    {
      resources: [{ kind: "column_scripts", names: ["first", "second"] }, { kind: "session_view" }],
      session_id: 1,
    },
    { resources: [{ kind: "session_view" }], session_id: 2 },
    { resources: [{ kind: "all" }] },
  ]);
});

test("disposal before async subscription resolves closes it and cancels queued callbacks", async () => {
  let resolve!: (stop: () => void) => void;
  backend.subscribeChanges.mockImplementation((handler) => {
    changed = handler;
    return new Promise((done) => {
      resolve = done;
    });
  });
  const handler = vi.fn();
  const stop = watch(handler);
  changed({ resources: [{ kind: "all" }] });
  stop();
  resolve(unsubscribe);
  await vi.advanceTimersByTimeAsync(40);
  expect(unsubscribe).toHaveBeenCalledTimes(1);
  changed({ resources: [{ kind: "all" }] });
  await vi.advanceTimersByTimeAsync(40);
  expect(handler).not.toHaveBeenCalled();
});

test("async refreshes do not overlap and changes received during a refresh are retained", async () => {
  let finish!: () => void;
  const handler = vi.fn().mockImplementationOnce(
    () =>
      new Promise<void>((resolve) => {
        finish = resolve;
      }),
  );
  watch(handler);
  changed({ resources: [{ kind: "all" }] });
  await vi.advanceTimersByTimeAsync(40);
  changed({ resources: [{ kind: "proxy" }] });
  changed({ resources: [{ kind: "proxy" }] });
  await vi.advanceTimersByTimeAsync(100);
  expect(handler).toHaveBeenCalledTimes(1);
  finish();
  await vi.advanceTimersByTimeAsync(40);
  expect(handler).toHaveBeenCalledTimes(2);
  expect(handler).toHaveBeenLastCalledWith({ resources: [{ kind: "proxy" }] });
});
