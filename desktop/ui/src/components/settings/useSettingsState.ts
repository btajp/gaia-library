import { useCallback, useEffect, useState, useSyncExternalStore } from "react";
import { useLatestRequest } from "../../hooks/useLatestRequest";
import { SettingsAction } from "./settingsAction";

export function useSettingsAction<T>() {
  const [action] = useState(() => new SettingsAction<T>());
  const snapshot = useSyncExternalStore(action.subscribe, action.getSnapshot, action.getSnapshot);
  useEffect(() => action.reset, [action]);
  return { action, snapshot, busy: snapshot.status === "working" };
}

export function useSettingsResource<T>(load: () => Promise<T>) {
  const { request, snapshot } = useLatestRequest<T>();
  const refresh = useCallback(() => request.run("settings", load), [load, request]);
  useEffect(() => {
    void refresh();
    return request.reset;
  }, [refresh, request]);
  return { snapshot, refresh, loading: snapshot.status === "idle" || snapshot.status === "loading" };
}
