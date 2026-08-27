import { useState } from "react";
import { isDetailType } from "../contextApi";
import type { ProposalDecision } from "../lib/proposalDecisions";
import { STATUS_LABELS, type Decision, type Proposal } from "../proposalTypes";
import type { OpenDetail } from "../types";
import Badge from "./Badges";
import { DetailLink } from "./ContextLists";
import OperationError from "./OperationError";

type Props = {
  proposal: Proposal;
  operation?: ProposalDecision;
  onDecide: (proposal: Proposal, decision: Decision, reason?: string) => void;
  openDetail: OpenDetail;
};

export default function ProposalCard({ proposal, operation, onDecide, openDetail }: Props) {
  const [reason, setReason] = useState(operation?.reason ?? "");
  const status = operation?.output?.status ?? proposal.status;
  const disabled = operation?.busy === true || operation?.output !== undefined;
  const kindLabel = proposal.kind === "inference" ? "inference（推測）" : "fact（事実）";
  const resultId = operation?.output && "result" in operation.output ? operation.output.result.id : proposal.result_id;

  return (
    <article className="space-y-4 rounded-lg border border-neutral-700 p-4" aria-labelledby={`proposal-${proposal.id}`} aria-busy={operation?.busy === true}>
      <header className="flex flex-wrap items-center gap-2">
        <h3 id={`proposal-${proposal.id}`} className="mr-1 font-semibold">提案 #{proposal.id}</h3>
        <Badge tone={status === "approved" ? "green" : status === "pending" ? "amber" : "neutral"}>{STATUS_LABELS[status]}</Badge>
        <Badge>{proposal.target_type} / {proposal.action}</Badge>
        <Badge tone={proposal.kind === "inference" ? "amber" : "neutral"}>{kindLabel}</Badge>
      </header>
      <dl className="space-y-1 break-words text-xs text-neutral-400">
        <div><dt className="inline">scope: </dt><dd className="inline">{proposal.scope}</dd></div>
        <div><dt className="inline">クライアント (proposed_by): </dt><dd className="inline">{proposal.proposed_by}</dd></div>
        <div><dt className="inline">request_id: </dt><dd className="inline break-all font-mono">{proposal.request_id}</dd></div>
        <div><dt className="inline">作成日時: </dt><dd className="inline">{proposal.created_at}</dd></div>
        {proposal.target_id !== undefined && <div><dt className="inline">対象 ID: </dt><dd className="inline">{isDetailType(proposal.target_type) ? <DetailLink type={proposal.target_type} id={proposal.target_id} openDetail={openDetail}>{proposal.target_type} #{proposal.target_id}</DetailLink> : `${proposal.target_type} #${proposal.target_id}`}</dd></div>}
        {resultId !== undefined && <div><dt className="inline">結果 ID: </dt><dd className="inline">{isDetailType(proposal.target_type) ? <DetailLink type={proposal.target_type} id={resultId} openDetail={openDetail}>{proposal.target_type} #{resultId}</DetailLink> : `${proposal.target_type} #${resultId}`}</dd></div>}
        {proposal.decided_at && <div><dt className="inline">判断日時: </dt><dd className="inline">{proposal.decided_at}</dd></div>}
        {proposal.decided_by && <div><dt className="inline">判断したクライアント: </dt><dd className="inline">{proposal.decided_by}</dd></div>}
        {proposal.decision_note && <div><dt className="inline">判断理由: </dt><dd className="inline whitespace-pre-wrap">{proposal.decision_note}</dd></div>}
      </dl>
      <details className="text-sm" open>
        <summary className="cursor-pointer font-medium">変更内容 (patch)</summary>
        <pre className="mt-2 overflow-auto whitespace-pre-wrap break-words rounded bg-neutral-950 p-3 text-xs text-neutral-300">{JSON.stringify(proposal.patch, null, 2)}</pre>
      </details>
      <details className="text-sm">
        <summary className="cursor-pointer font-medium">出典 (provenance)</summary>
        {proposal.provenance_id !== undefined && <p className="mt-2 text-xs text-neutral-400">参照 ID: {proposal.provenance_id}</p>}
        {proposal.provenance ? <pre className="mt-2 overflow-auto whitespace-pre-wrap break-words rounded bg-neutral-950 p-3 text-xs text-neutral-300">{JSON.stringify(proposal.provenance, null, 2)}</pre> : proposal.provenance_id === undefined && <p className="mt-2 text-xs text-neutral-400">出典は未指定です。</p>}
      </details>
      <OperationError error={operation?.error} />
      {operation?.busy && <p role="status" className="text-sm text-neutral-300">{operation.decision === "approve" ? "承認中…" : "却下中…"}</p>}
      {status === "pending" && (
        <div className="space-y-3 border-t border-neutral-800 pt-4">
          <div>
            <label htmlFor={`reason-${proposal.id}`} className="block text-xs text-neutral-400">却下理由（任意）</label>
            <textarea id={`reason-${proposal.id}`} value={reason} onChange={(event) => setReason(event.target.value)} disabled={disabled} rows={2} aria-describedby={`reason-help-${proposal.id}`} className="mt-2 w-full rounded border border-neutral-700 bg-neutral-950 px-3 py-2 text-sm disabled:opacity-50" />
            <p id={`reason-help-${proposal.id}`} className="mt-1 text-xs text-neutral-500">却下時だけ送信します。空欄なら省略します。</p>
          </div>
          <div className="flex flex-wrap gap-3">
            <button type="button" disabled={disabled} onClick={() => onDecide(proposal, "approve")} className="rounded bg-emerald-800 px-3 py-2 text-sm font-medium hover:bg-emerald-700 disabled:opacity-50">承認する</button>
            <button type="button" disabled={disabled} onClick={() => onDecide(proposal, "reject", reason)} className="rounded border border-red-900 px-3 py-2 text-sm text-red-300 hover:bg-red-950 disabled:opacity-50">却下する</button>
            {operation?.error != null && <button type="button" disabled={disabled} onClick={() => onDecide(proposal, operation.decision, operation.reason)} className="rounded border border-neutral-600 px-3 py-2 text-sm disabled:opacity-50">同じ{operation.decision === "approve" ? "承認" : "却下"}操作を再試行</button>}
          </div>
        </div>
      )}
    </article>
  );
}
