import assert from "node:assert/strict";
import test from "node:test";
import { LruKeys } from "./lru.ts";

test("LRU touches keys and evicts the least recently used unprotected key", () => {
  const lru = new LruKeys<number>(2);
  lru.touch(1);
  lru.touch(2);
  lru.touch(1);
  lru.touch(3);

  assert.deepEqual(lru.evict(new Set()), [2]);
  assert.equal(lru.has(1), true);
  assert.equal(lru.has(3), true);
});

test("LRU keeps protected keys and shrinks after protection ends", () => {
  const lru = new LruKeys<number>(1);
  lru.touch(1);
  lru.touch(2);

  assert.deepEqual(lru.evict(new Set([1, 2])), []);
  assert.equal(lru.size, 2);
  assert.deepEqual(lru.evict(new Set([2])), [1]);
  assert.equal(lru.size, 1);
});

test("touching an existing key does not grow the cache", () => {
  const lru = new LruKeys<number>(2);
  lru.touch(1);
  lru.touch(1);

  assert.equal(lru.size, 1);
});
