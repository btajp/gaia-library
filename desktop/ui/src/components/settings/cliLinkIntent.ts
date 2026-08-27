import type { RequestSnapshot } from "../../lib/latestRequest";
import type { CliLinkStatus } from "../../settingsApi";

export type CliLinkIntent = { expectedTarget: string | null };
export type CliLinkConfirmation = {
  observed: RequestSnapshot<CliLinkStatus>;
  expectedTarget: string;
};

export function beginCliLinkConfirmation(snapshot: RequestSnapshot<CliLinkStatus>): CliLinkConfirmation | null {
  if (snapshot.status !== "success" || snapshot.data?.status !== "wrong_target") return null;
  return { observed: snapshot, expectedTarget: snapshot.data.current };
}

export function cliLinkIntent(snapshot: RequestSnapshot<CliLinkStatus>, confirmation: CliLinkConfirmation | null): CliLinkIntent | null {
  if (snapshot.status !== "success") return null;
  if (snapshot.data?.status === "missing") return { expectedTarget: null };
  if (
    snapshot.data?.status === "wrong_target" &&
    confirmation?.observed === snapshot &&
    confirmation.expectedTarget === snapshot.data.current
  ) {
    return { expectedTarget: confirmation.expectedTarget };
  }
  return null;
}
