import { useEffect, useState, useSyncExternalStore } from "react";
import { LatestRequest } from "../lib/latestRequest";

export function useLatestRequest<T>() {
  const [request] = useState(() => new LatestRequest<T>());
  const snapshot = useSyncExternalStore(request.subscribe, request.getSnapshot, request.getSnapshot);
  useEffect(() => request.invalidate, [request]);
  return { request, snapshot };
}
