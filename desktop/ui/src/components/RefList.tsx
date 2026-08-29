import { useEffect, useRef, useState } from "react";
import { errorMessage } from "../api";
import { RESOLVE_PENDING_NOTE, copyReferenceUri, resolveKey, resolveReference } from "../contextApi";
import { useLatestRequest } from "../hooks/useLatestRequest";
import { snapshotForKey } from "../lib/latestRequest";
import type { Reference, ResolveSourceOutput } from "../types";
import Badge from "./Badges";

function ResolvedContent({ result }: { result: ResolveSourceOutput }) {
  if (result.resolved) {
    return (
      <div className="mt-3 rounded-md border border-neutral-700 bg-neutral-950 p-3 text-sm" aria-label="取得した内容">
        {result.reason && <p className="mb-2 text-xs text-neutral-400">注記: {result.reason}</p>}
        <pre className="max-h-96 overflow-auto whitespace-pre-wrap break-words font-mono text-xs text-neutral-200">{result.content ?? ""}</pre>
      </div>
    );
  }
  return (
    <div role="status" className="mt-3 rounded-md border border-amber-700 bg-amber-950/40 p-3 text-sm text-amber-200">
      <p>取得できませんでした: {result.reason ?? "理由は不明です"}</p>
      {result.reference.snapshot && (
        <details open className="mt-2 text-amber-100">
          <summary className="cursor-pointer">要点スナップショット（フォールバック）</summary>
          <p className="mt-2 whitespace-pre-wrap break-words">{result.reference.snapshot}</p>
        </details>
      )}
    </div>
  );
}

function ReferenceRow({ reference }: { reference: Reference }) {
  const [busy, setBusy] = useState(false);
  const [feedback, setFeedback] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  // 取得結果は LatestRequest に持つ。新しい取得の開始時と失敗時に前回の内容が消える（Search / Detail と同じ流儀）。
  const { request, snapshot } = useLatestRequest<ResolveSourceOutput>();
  const resolution = snapshotForKey(snapshot, resolveKey(reference));
  const resolving = resolution?.status === "loading";
  const resolved = resolution?.status === "success" ? resolution.data : null;
  const resolveError = resolution?.status === "error" ? errorMessage(resolution.error) : null;
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

  function resolve() {
    if (request.getSnapshot().status === "loading") return;
    // 前回の取得内容・コピー結果・エラーを消してから取得を始める（失敗時に新しいエラーと並ばない）。
    setFeedback(null);
    setError(null);
    // 結果は request の snapshot に持つだけで localStorage やログには保存しない。
    void request.run(resolveKey(reference), () => resolveReference(reference));
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
      <div className="mt-3 flex flex-wrap gap-2">
        <button type="button" onClick={copy} disabled={busy} className="rounded border border-neutral-700 px-3 py-1 text-xs hover:bg-neutral-800 disabled:opacity-50">
          {busy ? "コピー中…" : "URI をコピー"}
        </button>
        <button type="button" onClick={resolve} disabled={resolving} className="rounded border border-neutral-700 px-3 py-1 text-xs hover:bg-neutral-800 disabled:opacity-50">
          {resolving ? RESOLVE_PENDING_NOTE : "内容を取得"}
        </button>
      </div>
      {feedback && <p role="status" className="mt-2 text-xs text-neutral-300">{feedback}</p>}
      {error && <p role="alert" className="mt-2 text-xs text-red-300">{error}</p>}
      {resolveError && <p role="alert" className="mt-2 text-xs text-red-300">{resolveError}</p>}
      {resolved && <ResolvedContent result={resolved} />}
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

export { ResolvedContent };
