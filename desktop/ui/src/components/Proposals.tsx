import { useEffect, useState, useSyncExternalStore } from "react";
import { useLatestRequest } from "../hooks/useLatestRequest";
import { snapshotForKey } from "../lib/latestRequest";
import { decisionsForScope, type ProposalDecisions } from "../lib/proposalDecisions";
import { SingleFlight } from "../lib/singleFlight";
import { decisionKey, listProposals, proposalsKey, PROPOSAL_LIMIT } from "../proposalApi";
import { STATUS_LABELS, type ListProposalsOutput, type ProposalStatus } from "../proposalTypes";
import type { OpenDetail } from "../types";
import OperationError from "./OperationError";
import ProposalCard from "./ProposalCard";
import ProposalFeedback from "./ProposalFeedback";

type Props = { scope: string; decisions: ProposalDecisions; openDetail: OpenDetail };

export function ProposalCount({ count, status, limit }: { count: number; status: ProposalStatus; limit: number }) {
  return (
    <>
      <p role="status" className="text-sm text-neutral-400">{count === 0 ? `${STATUS_LABELS[status]}の提案はありません。` : `${count} 件の提案`}</p>
      {count >= limit && <p className="text-sm text-amber-300">取得上限に達しました。ほかにも該当する提案がある可能性があります。</p>}
    </>
  );
}

export default function Proposals({ scope, decisions, openDetail }: Props) {
  const [status, setStatus] = useState<ProposalStatus>("pending");
  const [limit, setLimit] = useState(PROPOSAL_LIMIT);
  const [attempt, setAttempt] = useState(0);
  const [flight] = useState(() => new SingleFlight<ListProposalsOutput>());
  const operations = useSyncExternalStore(decisions.subscribe, decisions.getSnapshot, decisions.getSnapshot);
  const revision = operations.scopeRevisions.get(scope) ?? 0;
  const { request, snapshot } = useLatestRequest<ListProposalsOutput>();
  const key = JSON.stringify([proposalsKey(scope, status, limit), attempt, revision]);
  const current = snapshotForKey(snapshot, key);
  const busy = !!scope && (!current || current.status === "idle" || current.status === "loading");
  const proposals = current?.status === "success" ? current.data?.proposals.filter((proposal) => proposal.scope === scope && proposal.status === status) ?? [] : [];

  useEffect(() => {
    if (!scope) request.reset();
    else void request.run(key, () => flight.run(key, () => listProposals(scope, status, limit)));
    return request.invalidate;
  }, [request, flight, key, scope, status, limit]);

  function refresh() {
    if (request.getSnapshot().status === "loading") return;
    request.reset();
    setAttempt((value) => value + 1);
  }

  return (
    <div className="space-y-5">
      <h2 className="text-xl font-semibold">提案キュー</h2>
      <p className="text-sm text-neutral-400">接続したエージェント（Claude Code / narumi など）が送ってきた書き込みの検品場所です。承認するまでデータ本体には入りません。</p>
      <p className="text-sm text-neutral-400">提案内容と出典を確認し、未承認の提案だけを承認・却下できます。操作は各提案の scope に限定します。</p>
      <p className="break-words text-xs text-neutral-400">対象 scope: {scope || "未確認"}</p>
      <div className="flex flex-wrap items-end gap-3">
        <div>
          <label htmlFor="proposal-status" className="block text-xs text-neutral-400">状態</label>
          <select id="proposal-status" value={status} onChange={(event) => { request.reset(); setStatus(event.target.value as ProposalStatus); }} className="mt-1 rounded-md border border-neutral-700 bg-neutral-950 px-3 py-2 text-sm">
            {Object.entries(STATUS_LABELS).map(([value, label]) => <option key={value} value={value}>{label}</option>)}
          </select>
        </div>
        <div>
          <label htmlFor="proposal-limit" className="block text-xs text-neutral-400">取得上限</label>
          <select id="proposal-limit" value={limit} onChange={(event) => { request.reset(); setLimit(Number(event.target.value)); }} className="mt-1 rounded-md border border-neutral-700 bg-neutral-950 px-3 py-2 text-sm">
            {[50, 100, 200].map((count) => <option key={count} value={count}>{count} 件</option>)}
          </select>
        </div>
        <button type="button" disabled={busy || !scope} onClick={refresh} className="rounded-md border border-neutral-600 px-3 py-2 text-sm hover:bg-neutral-800 disabled:opacity-50">{busy ? "読み込み中…" : current?.status === "error" ? "再試行" : "再読込"}</button>
      </div>
      <p className="text-xs text-neutral-400">新しい順に最大 {limit} 件を取得します。ページ送りはありません。上限より古い提案は表示されません。</p>
      {!scope && <p role="alert" className="text-sm text-amber-300">scope を入力するか、クライアントの既定 scope の確認を待ってください。</p>}
      {busy && <p role="status" className="text-sm text-neutral-400">提案を読み込んでいます…</p>}
      {current?.status === "error" && <OperationError error={current.error} />}
      <ProposalFeedback operations={decisionsForScope(operations, scope)} visibleIds={proposals.map((proposal) => proposal.id)} openDetail={openDetail} />
      {current?.status === "success" && (
        <>
          <ProposalCount count={proposals.length} status={status} limit={limit} />
          <div className="space-y-5">
            {proposals.map((proposal) => <ProposalCard key={proposal.id} proposal={proposal} operation={operations.operations.get(decisionKey(proposal.scope, proposal.id))} onDecide={(target, decision, reason) => { void decisions.run(target, decision, reason); }} openDetail={openDetail} />)}
          </div>
        </>
      )}
    </div>
  );
}
