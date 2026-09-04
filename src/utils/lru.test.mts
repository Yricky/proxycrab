import { expect, test } from "vitest";
import { LruKeys } from "./lru.ts";

test("LRU touches keys and evicts the least recently used unprotected key", () => {
  const lru = new LruKeys<number>(2);
  lru.touch(1);
  lru.touch(2);
  lru.touch(1);
  lru.touch(3);

  expect(lru.evict(new Set())).toEqual([2]);
  expect(lru.has(1)).toBe(true);
  expect(lru.has(3)).toBe(true);
});

test("LRU keeps protected keys and shrinks after protection ends", () => {
  const lru = new LruKeys<number>(1);
  lru.touch(1);
  lru.touch(2);

  expect(lru.evict(new Set([1, 2]))).toEqual([]);
  expect(lru.size).toBe(2);
  expect(lru.evict(new Set([2]))).toEqual([1]);
  expect(lru.size).toBe(1);
});

test("touching an existing key does not grow the cache", () => {
  const lru = new LruKeys<number>(2);
  lru.touch(1);
  lru.touch(1);

  expect(lru.size).toBe(1);
});
