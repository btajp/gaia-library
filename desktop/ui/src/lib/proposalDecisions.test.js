import { describe, expect, it, mock } from "bun:test";
import "../test/tauriMock";
import { deferred, proposal } from "../test/proposalFixtures";
import { decisionKey } from "../proposalApi";
import { ProposalDecisions, decisionsForScope } from "./proposalDecisions";

const approved = { proposal_id: 31, status: "approved", result: { target_type: "person", id: 7 } };

describe("proposal decision lifetime", () => {
  it("does not permit finalized proposals to reach the mutation helper", async () => {
    const api = mock();
    const decisions = new ProposalDecisions(api);
    expect(await decisions.run({ ...proposal, status: "approved" }, "approve")).toBe(false);
    expect(await decisions.run({ ...proposal, status: "rejected" }, "reject")).toBe(false);
    expect(api).not.toHaveBeenCalled();
    expect(decisions.getSnapshot().operations.size).toBe(0);
  });

  it("blocks duplicate actions during a mutation and after its confirmed success", async () => {
    const gate = deferred();
    const api = mock(() => gate.promise);
    const decisions = new ProposalDecisions(api);
    const running = decisions.run(proposal, "approve");
    expect(await decisions.run(proposal, "approve")).toBe(false);
    expect(await decisions.run(proposal, "reject")).toBe(false);
    gate.resolve(approved);
    await running;
    expect(await decisions.run(proposal, "approve")).toBe(false);
    expect(api).toHaveBeenCalledTimes(1);
    expect(decisions.getSnapshot().scopeRevisions.get("scope-a")).toBe(1);
  });

  it("keeps pending actions after unsubscription and increments only the original scope", async () => {
    const gate = deferred();
    const decisions = new ProposalDecisions(() => gate.promise);
    const stop = decisions.subscribe(mock());
    const running = decisions.run(proposal, "approve");
    stop();
    expect(decisionsForScope(decisions.getSnapshot(), "scope-b")).toEqual([]);
    expect(await decisions.run(proposal, "reject")).toBe(false);
    gate.resolve(approved);
    await running;
    expect(decisions.getSnapshot().scopeRevisions.get("scope-b")).toBeUndefined();
    expect(decisionsForScope(decisions.getSnapshot(), "scope-a")[0]).toMatchObject({ proposalId: 31, scope: "scope-a", busy: false, output: approved });
  });

  it("retries the same scoped proposal after failure and retains the original reason", async () => {
    const failed = new Error("retry later");
    const api = mock().mockRejectedValueOnce(failed).mockResolvedValueOnce({ proposal_id: 31, status: "rejected" });
    const decisions = new ProposalDecisions(api);
    await decisions.run(proposal, "reject", " 説明不足 ");
    const operation = decisions.getSnapshot().operations.get(decisionKey("scope-a", 31));
    expect(operation).toMatchObject({ error: failed, reason: "説明不足", busy: false });
    expect(decisions.getSnapshot().scopeRevisions.get("scope-a")).toBe(1);
    await decisions.run(proposal, operation.decision, operation.reason);
    expect(api.mock.calls).toEqual([[proposal, "reject", "説明不足"], [proposal, "reject", "説明不足"]]);
    expect(decisions.getSnapshot().scopeRevisions.get("scope-a")).toBe(2);
    expect(decisions.getSnapshot().operations.get(decisionKey("scope-a", 31)).error).toBeNull();
  });

  it("does not record a response for a different proposal as a successful mutation", async () => {
    const decisions = new ProposalDecisions(async () => ({ ...approved, proposal_id: 99 }));
    await decisions.run(proposal, "approve");
    const operation = decisionsForScope(decisions.getSnapshot(), "scope-a")[0];
    expect(operation.output).toBeUndefined();
    expect(operation.error.message).toContain("一致しません");
  });

  it("keeps independent scope histories and refresh revisions separate", async () => {
    const first = deferred();
    const second = deferred();
    const api = mock().mockImplementationOnce(() => first.promise).mockImplementationOnce(() => second.promise);
    const decisions = new ProposalDecisions(api);
    const oldRun = decisions.run(proposal, "approve");
    const newRun = decisions.run({ ...proposal, id: 32, scope: "scope-b" }, "reject", "reason");
    second.resolve({ proposal_id: 32, status: "rejected" });
    await newRun;
    const revision = decisions.getSnapshot().scopeRevisions.get("scope-b");
    first.resolve(approved);
    await oldRun;
    expect(decisions.getSnapshot().scopeRevisions.get("scope-b")).toBe(revision);
    expect(decisionsForScope(decisions.getSnapshot(), "scope-b").map((item) => item.proposalId)).toEqual([32]);
  });
});
