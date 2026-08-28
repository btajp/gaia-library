import { useEffect, useRef, useState } from "react";
import { errorMessage } from "../api";
import { copyReferenceUri } from "../contextApi";
import type { Reference } from "../types";
import Badge from "./Badges";

function ReferenceRow({ reference }: { reference: Reference }) {
  const [busy, setBusy] = useState(false);
  const [feedback, setFeedback] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const pending = useRef(false);
  const mounted = useRef(false);

  useEffect(() => {
    mounted.current = true;
    return () => { mounted.current = false; };
  }, []);

  async function copy() {
    if (pending.current) return;
    pending.current = true;
    setBusy(true);
    setFeedback(null);
    setError(null);
    try {
      await copyReferenceUri(reference.uri);
      if (mounted.current) setFeedback("URI をコピーしました。");
    } catch (cause) {
      if (mounted.current) setError(errorMessage(cause));
    } finally {
      pending.current = false;
      if (mounted.current) setBusy(false);
    }
  }

  return (
    <li className="rounded-md border border-neutral-800 p-3">
      <div className="flex flex-wrap gap-2">
        <Badge>{reference.system}</Badge>
        <Badge>scope: {reference.scope}</Badge>
        <Badge>{reference.target_type} #{reference.target_id}</Badge>
      </div>
      <p className="mt-2 break-words text-sm font-medium">{reference.title || reference.uri}</p>
      <p className="mt-1 whitespace-pre-wrap break-words text-sm text-neutral-300">{reference.note}</p>
      <p className="mt-2 select-text break-all font-mono text-xs text-neutral-400">{reference.uri}</p>
      <dl className="mt-2 flex flex-wrap gap-x-4 gap-y-1 text-xs text-neutral-400">
        <div><dt className="inline">最終確認: </dt><dd className="inline">{reference.last_verified ?? "未確認"}</dd></div>
        <div><dt className="inline">作成: </dt><dd className="inline">{reference.created_at}</dd></div>
      </dl>
      {reference.snapshot && (
        <details className="mt-3 text-sm">
          <summary className="cursor-pointer text-neutral-300">要点スナップショット</summary>
          <p className="mt-2 whitespace-pre-wrap break-words text-neutral-400">{reference.snapshot}</p>
        </details>
      )}
      <button type="button" onClick={copy} disabled={busy} className="mt-3 rounded border border-neutral-700 px-3 py-1 text-xs hover:bg-neutral-800 disabled:opacity-50">
        {busy ? "コピー中…" : "URI をコピー"}
      </button>
      {feedback && <p role="status" className="mt-2 text-xs text-neutral-300">{feedback}</p>}
      {error && <p role="alert" className="mt-2 text-xs text-red-300">{error}</p>}
    </li>
  );
}

export default function RefList({ refs }: { refs: Reference[] }) {
  if (refs.length === 0) return <p className="text-sm text-neutral-400">参照はありません。</p>;
  return (
    <ul className="space-y-3" aria-label="参照">
      {refs.map((reference) => (
        <ReferenceRow key={`${reference.scope}:${reference.id}:${reference.uri}`} reference={reference} />
      ))}
    </ul>
  );
}
