import { decideProposal, decisionKey } from "../proposalApi";
import type { Decision, DecisionOutput, Proposal } from "../proposalTypes";
import { ObservableStore } from "./observableStore";

export type ProposalDecision = {
  proposalId: number;
  scope: string;
  decision: Decision;
  reason: string;
  busy: boolean;
  error: unknown;
  output?: DecisionOutput;
};

export type DecisionsSnapshot = {
  operations: ReadonlyMap<string, ProposalDecision>;
  scopeRevisions: ReadonlyMap<string, number>;
};

export function decisionsForScope(snapshot: DecisionsSnapshot, scope: string): ProposalDecision[] {
  return [...snapshot.operations.values()].filter((operation) => operation.scope === scope);
}

export class ProposalDecisions extends ObservableStore<DecisionsSnapshot> {
  constructor(private decide = decideProposal) {
    super({ operations: new Map(), scopeRevisions: new Map() });
  }

  private update(key: string, operation: ProposalDecision, finished: boolean) {
    const operations = new Map(this.snapshot.operations);
    operations.delete(key);
    operations.set(key, operation);
    const scopeRevisions = new Map(this.snapshot.scopeRevisions);
    if (finished) scopeRevisions.set(operation.scope, (scopeRevisions.get(operation.scope) ?? 0) + 1);
    this.publish({ operations, scopeRevisions });
  }

  async run(proposal: Proposal, decision: Decision, reason = ""): Promise<boolean> {
    if (proposal.status !== "pending") return false;
    const key = decisionKey(proposal.scope, proposal.id);
    const existing = this.snapshot.operations.get(key);
    if (existing?.busy || existing?.output) return false;
    const operation: ProposalDecision = {
      proposalId: proposal.id,
      scope: proposal.scope,
      decision,
      reason: reason.trim(),
      busy: true,
      error: null,
    };
    this.update(key, operation, false);
    try {
      const output = await this.decide(proposal, decision, operation.reason);
      if (output.proposal_id !== proposal.id || output.status !== (decision === "approve" ? "approved" : "rejected")) {
        throw new Error("承認・却下の応答が対象の提案と一致しません。一覧を再読込してください。");
      }
      this.update(key, { ...operation, busy: false, output }, true);
    } catch (error) {
      this.update(key, { ...operation, busy: false, error }, true);
    }
    return true;
  }
}
