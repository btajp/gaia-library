import { useEffect, useState } from "react";
import { errorMessage } from "../api";
import { detailKey, loadDetail } from "../contextApi";
import { useLatestRequest } from "../hooks/useLatestRequest";
import { snapshotForKey } from "../lib/latestRequest";
import type { DetailResult, DetailTarget, OpenDetail } from "../types";
import DetailContent from "./DetailContent";

type Props = { target: DetailTarget; scope: string; onBack: () => void; openDetail: OpenDetail };

export default function Detail({ target, scope, onBack, openDetail }: Props) {
  const [attempt, setAttempt] = useState(0);
  const { request, snapshot } = useLatestRequest<DetailResult>();
  const key = detailKey(target, scope);
  const current = snapshotForKey(snapshot, key);
  const busy = !current || current.status === "idle" || current.status === "loading";

  useEffect(() => {
    void request.run(key, () => loadDetail({ type: target.type, id: target.id }, scope));
    return request.invalidate;
  }, [request, key, target.type, target.id, scope, attempt]);

  return (
    <div className="space-y-5">
      <button type="button" onClick={onBack} className="rounded-md border border-neutral-700 px-3 py-2 text-sm hover:bg-neutral-800">検索へ戻る</button>
      <p className="text-xs text-neutral-400">対象 scope: {scope || "クライアントの既定値"}</p>
      {busy && <p role="status" className="text-sm text-neutral-400">読み込み中…</p>}
      {current?.status === "error" && (
        <div className="space-y-3">
          <p role="alert" className="break-words text-sm text-red-300">{errorMessage(current.error)}</p>
          <button type="button" onClick={() => setAttempt((value) => value + 1)} className="rounded-md border border-neutral-700 px-3 py-2 text-sm hover:bg-neutral-800">再試行</button>
        </div>
      )}
      {current?.status === "success" && current.data && <DetailContent result={current.data} openDetail={openDetail} />}
    </div>
  );
}
