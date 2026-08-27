export class ObservableStore<T> {
  protected snapshot: T;
  private listeners = new Set<() => void>();

  constructor(initial: T) {
    this.snapshot = initial;
  }

  readonly getSnapshot = () => this.snapshot;
  readonly subscribe = (listener: () => void) => {
    this.listeners.add(listener);
    return () => { this.listeners.delete(listener); };
  };

  protected publish(snapshot: T) {
    this.snapshot = snapshot;
    this.listeners.forEach((listener) => listener());
  }
}
