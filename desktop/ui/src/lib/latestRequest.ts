export type RequestSnapshot<T> = {
  key: string | null;
  status: "idle" | "loading" | "success" | "error";
  data: T | null;
  error: unknown;
};

function emptySnapshot<T>(): RequestSnapshot<T> {
  return { key: null, status: "idle", data: null, error: null };
}

export function snapshotForKey<T>(snapshot: RequestSnapshot<T>, key: string): RequestSnapshot<T> | null {
  return snapshot.key === key ? snapshot : null;
}

export class LatestRequest<T> {
  private version = 0;
  private snapshot = emptySnapshot<T>();
  private listeners = new Set<() => void>();

  readonly getSnapshot = () => this.snapshot;

  readonly subscribe = (listener: () => void) => {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  };

  private publish(snapshot: RequestSnapshot<T>) {
    this.snapshot = snapshot;
    this.listeners.forEach((listener) => listener());
  }

  readonly invalidate = () => {
    this.version += 1;
  };

  readonly reset = () => {
    this.invalidate();
    this.publish(emptySnapshot<T>());
  };

  readonly run = async (key: string, operation: () => Promise<T>): Promise<void> => {
    const version = ++this.version;
    this.publish({ key, status: "loading", data: null, error: null });
    try {
      const data = await operation();
      if (version === this.version) {
        this.publish({ key, status: "success", data, error: null });
      }
    } catch (error) {
      if (version === this.version) {
        this.publish({ key, status: "error", data: null, error });
      }
    }
  };
}
