import type { EntityType, Kind } from "./types";

export type ProposalStatus = "pending" | "approved" | "rejected";
export type ProposalAction = "insert" | "update" | "supersede";
export type ProposalTargetType = EntityType | "fact" | "ref" | "glossary";
export type ManualTarget = Exclude<ProposalTargetType, "entity">;

export type Provenance = {
  ref_id?: number;
  system?: string;
  uri?: string;
  title?: string;
  note?: string;
  snapshot?: string;
};

export type Proposal = {
  id: number;
  action: ProposalAction;
  target_type: ProposalTargetType;
  patch: Record<string, unknown>;
  kind: Kind;
  scope: string;
  proposed_by: string;
  request_id: string;
  status: ProposalStatus;
  created_at: string;
  target_id?: number;
  provenance?: Provenance;
  provenance_id?: number;
  result_id?: number;
  decision_note?: string;
  decided_at?: string;
  decided_by?: string;
};

export type ApplyResult = { target_type: ProposalTargetType; id: number };
export type ProposeOutput = { proposal_id: number; status: ProposalStatus; duplicate: boolean };
export type ApproveOutput = { proposal_id: number; status: ProposalStatus; result: ApplyResult };
export type RejectOutput = { proposal_id: number; status: ProposalStatus };
export type ListProposalsOutput = { proposals: Proposal[] };
export type Decision = "approve" | "reject";
export type DecisionOutput = ApproveOutput | RejectOutput;

export type ManualIntent = {
  target_type: ManualTarget;
  action: "insert";
  patch: Record<string, unknown>;
  kind: Kind;
  scope: string;
};
export type ManualRequest = ManualIntent & { request_id: string };

export const STATUS_LABELS: Record<ProposalStatus, string> = {
  pending: "未承認",
  approved: "承認済み",
  rejected: "却下済み",
};
