import { useCallback, useEffect, useState } from "react";
import { errorMessage, serverStatus, type ServerStatus } from "../api";

const POLL_INTERVAL_MS = 15_000;

export function useServerStatus() {
  const [status, setStatus] = useState<ServerStatus | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [revision, setRevision] = useState(0);

  useEffect(() => {
    let active = true;
    let pending = false;

    async function refreshStatus() {
      if (!active || pending) return;
      pending = true;
      setLoading(true);
      try {
        const result = await serverStatus();
        if (active) {
          setStatus(result);
          setError(null);
        }
      } catch (cause) {
        if (active) setError(errorMessage(cause));
      } finally {
        pending = false;
        if (active) setLoading(false);
      }
    }

    void refreshStatus();
    const timer = window.setInterval(() => void refreshStatus(), POLL_INTERVAL_MS);
    return () => {
      active = false;
      window.clearInterval(timer);
    };
  }, [revision]);

  const refresh = useCallback(() => setRevision((value) => value + 1), []);
  return { status, error, loading, refresh };
}
