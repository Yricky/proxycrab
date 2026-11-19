import { afterEach, expect, test, vi } from "vitest";
import { shareChanges } from "./changes";

afterEach(() => vi.restoreAllMocks());

test("subscribers share one connection until the last unsubscribe, then can reconnect", async () => {
  let emit!: (value: number) => void;
  const close = vi.fn();
  const connect = vi.fn(async (handler: (value: number) => void) => {
    emit = handler;
    return close;
  });
  const subscribe = shareChanges(connect);
  const first = vi.fn();
  const second = vi.fn();
  const [stopFirst, stopSecond] = await Promise.all([subscribe(first), subscribe(second)]);
  expect(connect).toHaveBeenCalledTimes(1);
  emit(1);
  expect(first).toHaveBeenCalledWith(1);
  expect(second).toHaveBeenCalledWith(1);
  stopFirst();
  emit(2);
  expect(first).toHaveBeenCalledTimes(1);
  expect(second).toHaveBeenCalledTimes(2);
  expect(close).not.toHaveBeenCalled();
  stopSecond();
  stopSecond();
  expect(close).toHaveBeenCalledTimes(1);
  const stopThird = await subscribe(first);
  expect(connect).toHaveBeenCalledTimes(2);
  stopThird();
  expect(close).toHaveBeenCalledTimes(2);
});

test("a failed connection can be retried and old handlers are removed", async () => {
  let emit!: (value: number) => void;
  const connect = vi
    .fn()
    .mockRejectedValueOnce(new Error("failed"))
    .mockImplementation(async (handler) => {
      emit = handler;
      return () => {};
    });
  const subscribe = shareChanges<number>(connect);
  const first = vi.fn();
  await expect(subscribe(first)).rejects.toThrow("failed");
  const second = vi.fn();
  const stop = await subscribe(second);
  emit(1);
  expect(first).not.toHaveBeenCalled();
  expect(second).toHaveBeenCalledWith(1);
  stop();
});

test("one failing consumer does not interrupt other consumers", async () => {
  vi.spyOn(console, "error").mockImplementation(() => {});
  let emit!: (value: number) => void;
  const subscribe = shareChanges<number>(async (handler) => {
    emit = handler;
    return () => {};
  });
  const stopFirst = await subscribe(() => {
    throw new Error("consumer failed");
  });
  const second = vi.fn();
  const stopSecond = await subscribe(second);
  emit(1);
  expect(second).toHaveBeenCalledWith(1);
  stopFirst();
  stopSecond();
});
