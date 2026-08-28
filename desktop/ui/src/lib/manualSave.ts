import { GaiaError, errorMessage } from "../api";
import { approveProposal, explicitScope, findFinalizedProposal, proposeManual } from "../proposalApi";
import type { ApplyResult, ApproveOutput, ManualIntent, ManualRequest, Proposal, ProposalStatus, ProposeOutput } from "../proposalTypes";
import { ObservableStore } from "./observableStore";

export type SavePhase = "paused" | "proposing" | "approving" | "checking" | "error" | "complete" | "rejected";
export type ManualSaveOperation = {
  input: ManualRequest;
  phase: SavePhase;
  proposalId?: number;
  proposalStatus?: ProposalStatus;
  result?: ApplyResult;
  error: unknown;
  notice?: string;
};

type SaveApi = {
  propose: (input: ManualRequest) => Promise<ProposeOutput>;
  approve: (id: number, scope: string) => Promise<ApproveOutput>;
  findFinalized: (id: number, scope: string) => Promise<Proposal | null>;
};

export function isSaveBusy(operation: ManualSaveOperation | null): boolean {
  return operation !== null && ["proposing", "approving", "checking"].includes(operation.phase);
}

export function canCorrectSave(operation: ManualSaveOperation): boolean {
  return operation.phase === "error" && operation.proposalId === undefined && operation.error instanceof GaiaError
    && ["invalid_params", "scope_denied", "unauthorized", "not_found", "contract_mismatch"].includes(operation.error.code);
}

export class ManualSave extends ObservableStore<ManualSaveOperation | null> {
  constructor(
    private api: SaveApi = { propose: proposeManual, approve: approveProposal, findFinalized: findFinalizedProposal },
    private newRequestId: () => string = () => `ui-${crypto.randomUUID()}`,
  ) {
    super(null);
  }

  async start(intent: ManualIntent): Promise<boolean> {
    if (this.snapshot !== null) return false;
    const input = structuredClone({ ...intent, scope: explicitScope(intent.scope), request_id: this.newRequestId() });
    this.publish({ input, phase: "paused", error: null });
    await this.retry();
    return true;
  }

  private fromFinalized(operation: ManualSaveOperation, proposal: Proposal): ManualSaveOperation {
    if (proposal.id !== operation.proposalId || proposal.scope !== operation.input.scope || proposal.status === "pending") {
      throw new Error("提案の確認結果が保存操作と一致しません。新しい提案は作成していません。");
    }
    const result = proposal.status === "approved" && proposal.result_id !== undefined
      ? { target_type: proposal.target_type, id: proposal.result_id } : undefined;
    return {
      ...operation,
      proposalId: proposal.id,
      proposalStatus: proposal.status,
      phase: proposal.status === "approved" ? "complete" : "rejected",
      result,
      error: null,
      notice: proposal.status === "approved" && !result ? "承認済みです。結果 ID は応答に含まれていません。" : undefined,
    };
  }

  private async alreadyApproved(operation: ManualSaveOperation) {
    this.publish({ ...operation, phase: "checking" });
    let notice = "同じ request_id の提案は承認済みです。結果 ID は一覧の取得上限内で確認できませんでした。";
    try {
      const proposal = await this.api.findFinalized(operation.proposalId!, operation.input.scope);
      if (proposal?.status === "approved") {
        this.publish(this.fromFinalized(operation, proposal));
        return;
      }
    } catch (cause) {
      notice = `承認済みですが結果 ID の確認に失敗しました: ${errorMessage(cause)}`;
    }
    this.publish({ ...operation, phase: "complete", proposalStatus: "approved", error: null, notice });
  }

  async retry(): Promise<void> {
    if (!this.snapshot || isSaveBusy(this.snapshot) || ["complete", "rejected"].includes(this.snapshot.phase)) return;
    let operation: ManualSaveOperation = { ...this.snapshot, error: null, notice: undefined };
    try {
      if (operation.proposalId === undefined) {
        operation = { ...operation, phase: "proposing" };
        this.publish(operation);
        const proposed = await this.api.propose(operation.input);
        operation = { ...operation, proposalId: proposed.proposal_id, proposalStatus: proposed.status };
        if (proposed.status === "rejected") {
          this.publish({ ...operation, phase: "rejected" });
          return;
        }
        if (proposed.status === "approved") {
          await this.alreadyApproved(operation);
          return;
        }
      }
      operation = { ...operation, phase: "approving" };
      this.publish(operation);
      const approved = await this.api.approve(operation.proposalId!, operation.input.scope);
      if (approved.proposal_id !== operation.proposalId || approved.status !== "approved") {
        throw new Error("承認の応答が保存操作と一致しません。新しい提案は作成していません。");
      }
      this.publish({ ...operation, phase: "complete", proposalStatus: "approved", result: approved.result, error: null });
    } catch (cause) {
      if (operation.proposalId !== undefined && cause instanceof GaiaError && cause.code === "conflict") {
        try {
          const finalized = await this.api.findFinalized(operation.proposalId, operation.input.scope);
          if (finalized) {
            this.publish(this.fromFinalized(operation, finalized));
            return;
          }
          operation = { ...operation, notice: "承認・却下済みか確認できませんでした。各状態の最新 200 件が確認対象です。" };
        } catch (confirmationError) {
          operation = { ...operation, notice: `状態の再確認にも失敗しました: ${errorMessage(confirmationError)}` };
        }
      }
      this.publish({ ...operation, phase: "error", error: cause });
    }
  }

  async checkStatus(): Promise<void> {
    const operation = this.snapshot;
    if (!operation || operation.proposalId === undefined || isSaveBusy(operation) || ["complete", "rejected"].includes(operation.phase)) return;
    this.publish({ ...operation, phase: "checking", notice: undefined });
    try {
      const finalized = await this.api.findFinalized(operation.proposalId, operation.input.scope);
      if (finalized) this.publish(this.fromFinalized(operation, finalized));
      else this.publish({ ...operation, phase: "error", notice: "未承認、または承認・却下一覧の取得上限外です。新しい提案は作成していません。" });
    } catch (cause) {
      this.publish({ ...operation, phase: "error", notice: `状態を確認できませんでした: ${errorMessage(cause)}` });
    }
  }

  clearAfterReview(): boolean {
    const operation = this.snapshot;
    if (!operation || isSaveBusy(operation)) return false;
    if (!["complete", "rejected"].includes(operation.phase) && !canCorrectSave(operation)) return false;
    this.publish(null);
    return true;
  }
}
