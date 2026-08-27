export type ActionSnapshot<T> = {
  status: "idle" | "working" | "success" | "error";
  data: T | null;
  error: unknown;
};

const emptySnapshot = <T>(): ActionSnapshot<T> => ({ status: "idle", data: null, error: null });

export class SettingsAction<T> {
  private version = 0;
  private pending = false;
  private snapshot = emptySnapshot<T>();
  private listeners = new Set<() => void>();

  readonly getSnapshot = () => this.snapshot;
  readonly subscribe = (listener: () => void) => {
    this.listeners.add(listener);
    return () => { this.listeners.delete(listener); };
  };

  private publish(snapshot: ActionSnapshot<T>) {
    this.snapshot = snapshot;
    this.listeners.forEach((listener) => listener());
  }

  readonly reset = () => {
    this.version += 1;
    this.publish(emptySnapshot<T>());
  };

  readonly run = async (operation: () => Promise<T>): Promise<{ data: T } | null> => {
    if (this.pending) return null;
    this.pending = true;
    const version = ++this.version;
    this.publish({ status: "working", data: null, error: null });
    try {
      const data = await operation();
      if (version !== this.version) return null;
      this.publish({ status: "success", data, error: null });
      return { data };
    } catch (error) {
      if (version === this.version) this.publish({ status: "error", data: null, error });
      return null;
    } finally {
      this.pending = false;
    }
  };
}
