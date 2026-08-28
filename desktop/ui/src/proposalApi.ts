import { callTool, GaiaError } from "./api";
import type { ApproveOutput, Decision, DecisionOutput, ListProposalsOutput, ManualRequest, Proposal, ProposalStatus, ProposeOutput, RejectOutput } from "./proposalTypes";

export const PROPOSAL_LIMIT = 50;
export const MAX_PROPOSAL_LIMIT = 200;
const pendingMutations = new Set<string>();

async function withMutationLock<T>(scope: string, id: number, action: () => Promise<T>): Promise<T> {
  const key = decisionKey(scope, id);
  if (pendingMutations.has(key)) throw new GaiaError({ code: "busy", message: "この提案は別の保存・承認操作で処理中です。完了後に再試行してください。" });
  pendingMutations.add(key);
  try {
    return await action();
  } finally {
    pendingMutations.delete(key);
  }
}

export function explicitScope(scope: string): string {
  const value = scope.trim();
  if (!value) throw new Error("保存・提案操作の対象 scope を指定してください。");
  return value;
}

function checkId(id: number) {
  if (!Number.isSafeInteger(id) || id <= 0) throw new Error("提案 ID は正の整数で指定してください。");
}

export function proposalsKey(scope: string, status: ProposalStatus, limit: number): string {
  return JSON.stringify([scope.trim(), status, limit]);
}

export function decisionKey(scope: string, id: number): string {
  return JSON.stringify([scope, id]);
}

export async function listProposals(scope: string, status: ProposalStatus, limit = PROPOSAL_LIMIT): Promise<ListProposalsOutput> {
  if (!Number.isInteger(limit) || limit < 1 || limit > MAX_PROPOSAL_LIMIT) throw new Error("一覧の件数上限は 1〜200 で指定してください。");
  return callTool<ListProposalsOutput>("list_proposals", { scope: explicitScope(scope), status, limit });
}

export async function proposeManual(input: ManualRequest): Promise<ProposeOutput> {
  return callTool<ProposeOutput>("propose_update", { ...input, scope: explicitScope(input.scope) });
}

export async function approveProposal(proposalId: number, scope: string): Promise<ApproveOutput> {
  checkId(proposalId);
  const resolved = explicitScope(scope);
  return withMutationLock(resolved, proposalId, () => callTool<ApproveOutput>("approve_proposal", { proposal_id: proposalId, scope: resolved }));
}

export async function rejectProposal(proposalId: number, scope: string, reason: string): Promise<RejectOutput> {
  checkId(proposalId);
  const resolved = explicitScope(scope);
  const note = reason.trim();
  return withMutationLock(resolved, proposalId, () => callTool<RejectOutput>("reject_proposal", { proposal_id: proposalId, scope: resolved, ...(note ? { reason: note } : {}) }));
}

export async function decideProposal(proposal: Proposal, decision: Decision, reason = ""): Promise<DecisionOutput> {
  if (proposal.status !== "pending") throw new Error("未承認の提案だけを操作できます。一覧を再読込してください。");
  return decision === "approve" ? approveProposal(proposal.id, proposal.scope) : rejectProposal(proposal.id, proposal.scope, reason);
}

export async function findFinalizedProposal(proposalId: number, scope: string): Promise<Proposal | null> {
  checkId(proposalId);
  const resolved = explicitScope(scope);
  for (const status of ["approved", "rejected"] as const) {
    const result = await listProposals(resolved, status, MAX_PROPOSAL_LIMIT);
    const found = result.proposals.find((proposal) => proposal.id === proposalId && proposal.scope === resolved && proposal.status === status);
    if (found) return found;
  }
  return null;
}
