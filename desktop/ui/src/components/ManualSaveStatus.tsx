import { isDetailType } from "../contextApi";
import { canCorrectSave, isSaveBusy, type ManualSave, type ManualSaveOperation } from "../lib/manualSave";
import type { OpenDetail } from "../types";
import { DetailLink } from "./ContextLists";
import OperationError from "./OperationError";

type Props = {
  operation: ManualSaveOperation;
  controller: ManualSave;
  openDetail: OpenDetail;
  showProposals: () => void;
  onClear: () => void;
};

export default function ManualSaveStatus({ operation, controller, openDetail, showProposals, onClear }: Props) {
  const busy = isSaveBusy(operation);
  const terminal = operation.phase === "complete" || operation.phase === "rejected";
  const correctable = canCorrectSave(operation);
  const result = operation.result;
  return (
    <section className="space-y-4 rounded-md border border-neutral-700 p-4" aria-labelledby="save-operation-title">
      <h3 id="save-operation-title" className="font-semibold">保存操作</h3>
      <dl className="space-y-1 break-all text-xs text-neutral-400">
        <div><dt className="inline">request_id: </dt><dd className="inline font-mono">{operation.input.request_id}</dd></div>
        <div><dt className="inline">保存先 scope: </dt><dd className="inline">{operation.input.scope}</dd></div>
        <div><dt className="inline">種別: </dt><dd className="inline">{operation.input.target_type} / {operation.input.kind}</dd></div>
        <div><dt className="inline">提案 ID: </dt><dd className="inline">{operation.proposalId ?? "未確認"}</dd></div>
      </dl>
      {busy && <p role="status" className="text-sm text-neutral-300">{operation.phase === "proposing" ? "提案を作成しています…" : operation.phase === "approving" ? "作成した提案を承認しています…" : "提案の状態を確認しています…"}</p>}
      {operation.phase === "complete" && (
        <div role="status" className="text-sm text-emerald-300">
          <p>保存済みです。{result ? ` ${result.target_type} #${result.id}` : ` 提案 #${operation.proposalId} は承認済みです。`}</p>
          {result && isDetailType(result.target_type) && <div className="mt-2"><DetailLink type={result.target_type} id={result.id} openDetail={openDetail}>保存した結果を見る</DetailLink></div>}
        </div>
      )}
      {operation.phase === "rejected" && <p role="status" className="text-sm text-amber-300">この提案は却下済みです。承認や新規提案の再送はしていません。</p>}
      <OperationError error={operation.error} />
      {operation.notice && <p className="text-sm text-amber-300">{operation.notice}</p>}
      {!terminal && !busy && (
        <p className="text-sm leading-6 text-neutral-300">
          {correctable ? "提案の作成が拒否されました。入力を修正できます。" : operation.proposalId === undefined
            ? "提案が作成されたか未確認です。入力を固定し、同じ request_id で再試行します。"
            : "提案は作成済みです。承認の結果を確認するまで入力を固定し、再試行では承認だけを送ります。"}
        </p>
      )}
      <details className="text-xs text-neutral-400">
        <summary className="cursor-pointer">固定した送信内容</summary>
        <pre className="mt-2 overflow-auto whitespace-pre-wrap break-words rounded bg-neutral-950 p-3">{JSON.stringify(operation.input.patch, null, 2)}</pre>
      </details>
      <div className="flex flex-wrap gap-3">
        {!terminal && <button type="button" disabled={busy} onClick={() => void controller.retry()} className="rounded border border-neutral-600 px-3 py-2 text-sm disabled:opacity-50">{operation.proposalId === undefined ? "同じ操作を再試行" : "承認だけ再試行"}</button>}
        {!terminal && operation.proposalId !== undefined && <button type="button" disabled={busy} onClick={() => void controller.checkStatus()} className="rounded border border-neutral-700 px-3 py-2 text-sm disabled:opacity-50">承認・却下の状態を確認</button>}
        {operation.proposalId !== undefined && <button type="button" onClick={showProposals} className="rounded border border-neutral-700 px-3 py-2 text-sm">提案キューで確認</button>}
        {(terminal || correctable) && <button type="button" onClick={onClear} className="rounded bg-neutral-100 px-3 py-2 text-sm font-medium text-neutral-950">{correctable ? "入力を修正する" : "新しい入力へ"}</button>}
      </div>
      {!terminal && <p className="text-xs leading-5 text-neutral-400">タブを切り替えてもこの操作を保持します。アプリ終了時には途中状態が失われます。作成済み提案は提案キューで確認できます。</p>}
    </section>
  );
}
