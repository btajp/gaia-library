import { isDetailType } from "../contextApi";
import type { ProposalDecision } from "../lib/proposalDecisions";
import type { OpenDetail } from "../types";
import { DetailLink } from "./ContextLists";
import OperationError from "./OperationError";

export default function ProposalFeedback({ operations, visibleIds, openDetail }: { operations: ProposalDecision[]; visibleIds: number[]; openDetail: OpenDetail }) {
  const hidden = operations.filter((operation) => !visibleIds.includes(operation.proposalId));
  if (hidden.length === 0) return null;
  return (
    <section className="space-y-3 rounded border border-neutral-700 p-3" aria-label="この scope の提案操作">
      <h3 className="text-sm font-medium">この scope の直近の操作（最大 5 件）</h3>
      {hidden.slice(-5).reverse().map((operation) => (
        <div key={operation.proposalId} className="space-y-1 text-sm">
          <p role="status" className={operation.output ? "text-emerald-300" : "text-neutral-300"}>提案 #{operation.proposalId}: {operation.busy ? "処理中…" : operation.output ? operation.output.status === "approved" ? "承認しました。" : "却下しました。" : "操作に失敗しました。一覧を未承認に戻すか再読込して確認してください。"}</p>
          <OperationError error={operation.error} />
          {operation.output && "result" in operation.output && isDetailType(operation.output.result.target_type) && <DetailLink type={operation.output.result.target_type} id={operation.output.result.id} openDetail={openDetail}>保存した {operation.output.result.target_type} #{operation.output.result.id}</DetailLink>}
        </div>
      ))}
    </section>
  );
}
