export class SingleFlight<T> {
  private pending = new Map<string, Promise<T>>();

  run(key: string, operation: () => Promise<T>): Promise<T> {
    const existing = this.pending.get(key);
    if (existing) return existing;
    const promise = Promise.resolve().then(operation);
    this.pending.set(key, promise);
    const clear = () => {
      if (this.pending.get(key) === promise) this.pending.delete(key);
    };
    void promise.then(clear, clear);
    return promise;
  }
}
