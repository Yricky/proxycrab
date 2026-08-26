export class LruKeys<K> {
  private readonly order = new Map<K, true>();
  readonly capacity: number;

  constructor(capacity: number) {
    this.capacity = capacity;
  }

  get size(): number {
    return this.order.size;
  }

  has(key: K): boolean {
    return this.order.has(key);
  }

  touch(key: K): void {
    this.order.delete(key);
    this.order.set(key, true);
  }

  delete(key: K): void {
    this.order.delete(key);
  }

  clear(): void {
    this.order.clear();
  }

  evict(protectedKeys: ReadonlySet<K>): K[] {
    const evicted: K[] = [];
    for (const key of this.order.keys()) {
      if (this.order.size <= this.capacity) break;
      if (protectedKeys.has(key)) continue;
      this.order.delete(key);
      evicted.push(key);
    }
    return evicted;
  }
}
