import { describe, expect, it, mock } from "bun:test";
import "../test/tauriMock";
import { GaiaError } from "../api";
import { deferred, proposal } from "../test/proposalFixtures";
import { ManualSave, canCorrectSave, isSaveBusy } from "./manualSave";

const intent = { target_type: "person", action: "insert", patch: { name: "人物" }, kind: "fact", scope: "scope-a" };
const proposed = { proposal_id: 31, status: "pending", duplicate: false };
const approved = { proposal_id: 31, status: "approved", result: { target_type: "person", id: 7 } };
const conflict = () => new GaiaError({ code: "conflict", message: "not pending", details: { proposal_id: 31 } });
const approvedProposal = { ...proposal, status: "approved", result_id: 7 };

function setup(overrides = {}) {
  const api = {
    propose: mock(async () => proposed),
    approve: mock(async () => approved),
    findFinalized: mock(async () => null),
    ...overrides,
  };
  const requestId = mock(() => "ui-fixed-request-id");
  return { api, requestId, save: new ManualSave(api, requestId) };
}

describe("one manual-save operation", () => {
  it("proposes once and approves that proposal in the original scope", async () => {
    const { save, api, requestId } = setup();
    const phases = [];
    save.subscribe(() => phases.push(save.getSnapshot().phase));
    expect(await save.start(intent)).toBe(true);
    expect(phases).toEqual(["paused", "proposing", "approving", "complete"]);
    expect(api.propose).toHaveBeenCalledWith({ ...intent, request_id: "ui-fixed-request-id" });
    expect(api.approve).toHaveBeenCalledWith(31, "scope-a");
    expect(requestId).toHaveBeenCalledTimes(1);
    expect(save.getSnapshot()).toMatchObject({ phase: "complete", proposalId: 31, proposalStatus: "approved", result: approved.result });
    expect(await save.start(intent)).toBe(false);
    await save.retry();
    expect(api.propose).toHaveBeenCalledTimes(1);
    expect(api.approve).toHaveBeenCalledTimes(1);
  });

  it("reuses the exact request ID and payload after an ambiguous proposal failure", async () => {
    const propose = mock().mockRejectedValueOnce(new Error("lost reply")).mockResolvedValueOnce({ ...proposed, duplicate: true });
    const { save, api, requestId } = setup({ propose });
    await save.start(intent);
    const failed = save.getSnapshot();
    expect(failed.phase).toBe("error");
    expect(failed.proposalId).toBeUndefined();
    expect(save.clearAfterReview()).toBe(false);
    expect(await save.start({ ...intent, patch: { name: "new" } })).toBe(false);
    await save.retry();
    expect(propose.mock.calls[0][0]).toEqual(propose.mock.calls[1][0]);
    expect(requestId).toHaveBeenCalledTimes(1);
    expect(api.approve).toHaveBeenCalledTimes(1);
    expect(save.getSnapshot().phase).toBe("complete");
  });

  it("retries only approval after the proposal ID is known", async () => {
    const approve = mock().mockRejectedValueOnce(new Error("approval failed")).mockResolvedValueOnce(approved);
    const { save, api } = setup({ approve });
    await save.start(intent);
    expect(save.getSnapshot()).toMatchObject({ phase: "error", proposalId: 31, input: { scope: "scope-a", request_id: "ui-fixed-request-id" } });
    expect(save.clearAfterReview()).toBe(false);
    await save.retry();
    expect(api.propose).toHaveBeenCalledTimes(1);
    expect(approve.mock.calls).toEqual([[31, "scope-a"], [31, "scope-a"]]);
    expect(save.getSnapshot().phase).toBe("complete");
  });

  it("prevents rapid double submit/retry and freezes the original input while pending", async () => {
    const gate = deferred();
    const { save, api } = setup({ propose: mock(() => gate.promise) });
    const mutable = structuredClone(intent);
    const running = save.start(mutable);
    mutable.patch.name = "changed after submit";
    mutable.scope = "scope-b";
    expect(isSaveBusy(save.getSnapshot())).toBe(true);
    expect(await save.start(mutable)).toBe(false);
    await save.retry();
    expect(save.clearAfterReview()).toBe(false);
    expect(api.propose).toHaveBeenCalledTimes(1);
    expect(api.propose.mock.calls[0][0]).toMatchObject({ patch: { name: "人物" }, scope: "scope-a" });
    gate.resolve(proposed);
    await running;
    expect(api.approve).toHaveBeenCalledWith(31, "scope-a");
  });

  it("keeps a pending approval across listeners and does not switch it to a new scope", async () => {
    const gate = deferred();
    const { save, api } = setup({ approve: mock(() => gate.promise) });
    const oldListener = mock();
    const unsubscribe = save.subscribe(oldListener);
    const running = save.start(intent);
    await Promise.resolve();
    expect(save.getSnapshot().phase).toBe("approving");
    unsubscribe();
    const newListener = mock();
    save.subscribe(newListener);
    expect(await save.start({ ...intent, scope: "scope-b" })).toBe(false);
    await save.retry();
    gate.resolve(approved);
    await running;
    expect(api.approve.mock.calls).toEqual([[31, "scope-a"]]);
    expect(save.getSnapshot().input.scope).toBe("scope-a");
    expect(newListener).toHaveBeenCalledTimes(1);
  });

  it("reconciles a lost approval reply without creating a second proposal", async () => {
    const approve = mock().mockRejectedValueOnce(new Error("lost reply")).mockRejectedValueOnce(conflict());
    const { save, api } = setup({ approve, findFinalized: mock(async () => approvedProposal) });
    await save.start(intent);
    await save.retry();
    expect(save.getSnapshot()).toMatchObject({ phase: "complete", result: approved.result, proposalId: 31 });
    expect(api.propose).toHaveBeenCalledTimes(1);
    expect(api.findFinalized).toHaveBeenCalledWith(31, "scope-a");
  });

  it("does not approve a duplicate proposal that is already approved or rejected", async () => {
    for (const status of ["approved", "rejected"]) {
      const { save, api } = setup({ propose: mock(async () => ({ ...proposed, duplicate: true, status })), findFinalized: mock(async () => approvedProposal) });
      await save.start(intent);
      expect(api.approve).not.toHaveBeenCalled();
      expect(save.getSnapshot().phase).toBe(status === "approved" ? "complete" : "rejected");
      expect(save.getSnapshot().input.request_id).toBe("ui-fixed-request-id");
    }
  });

  it("reports a known approval even when its result is outside the bounded lookup", async () => {
    const { save, api } = setup({ propose: mock(async () => ({ ...proposed, status: "approved", duplicate: true })) });
    await save.start(intent);
    expect(save.getSnapshot()).toMatchObject({ phase: "complete", proposalStatus: "approved" });
    expect(save.getSnapshot().notice).toContain("取得上限");
    expect(api.approve).not.toHaveBeenCalled();
  });

  it("retains an uncertain conflict and cannot start another operation", async () => {
    const { save, api } = setup({ approve: mock(async () => { throw conflict(); }) });
    await save.start(intent);
    expect(save.getSnapshot()).toMatchObject({ phase: "error", proposalId: 31 });
    expect(save.getSnapshot().notice).toContain("200 件");
    expect(save.clearAfterReview()).toBe(false);
    expect(await save.start({ ...intent, scope: "scope-b" })).toBe(false);
    expect(api.propose).toHaveBeenCalledTimes(1);
  });

  it("never takes a different scope or ID from a reconciliation response", async () => {
    for (const foreign of [{ ...approvedProposal, scope: "scope-b" }, { ...approvedProposal, id: 99 }]) {
      const { save } = setup({ approve: mock(async () => { throw conflict(); }), findFinalized: mock(async () => foreign) });
      await save.start(intent);
      expect(save.getSnapshot()).toMatchObject({ phase: "error", proposalId: 31, input: { scope: "scope-a" } });
      expect(save.getSnapshot().result).toBeUndefined();
      expect(save.clearAfterReview()).toBe(false);
    }
  });

  it("does not accept an approval response for another proposal", async () => {
    const { save } = setup({ approve: mock(async () => ({ ...approved, proposal_id: 99 })) });
    await save.start(intent);
    expect(save.getSnapshot()).toMatchObject({ phase: "error", proposalId: 31 });
    expect(save.getSnapshot().result).toBeUndefined();
  });
});

describe("explicit review and correction", () => {
  it("allows correction only after a definitive pre-proposal rejection", async () => {
    const rejected = new GaiaError({ code: "invalid_params", message: "invalid input", details: { field: "name" } });
    const { save, api } = setup({ propose: mock().mockRejectedValueOnce(rejected).mockResolvedValueOnce(proposed) });
    await save.start(intent);
    expect(canCorrectSave(save.getSnapshot())).toBe(true);
    expect(await save.start(intent)).toBe(false);
    expect(save.clearAfterReview()).toBe(true);
    expect(save.getSnapshot()).toBeNull();
    await save.start({ ...intent, patch: { name: "corrected" } });
    expect(api.propose).toHaveBeenCalledTimes(2);
    expect(save.getSnapshot().phase).toBe("complete");
  });

  it("does not clear a proposal just because approval validation failed", async () => {
    const { save } = setup({ approve: mock(async () => { throw new GaiaError({ code: "invalid_params", message: "target not found" }); }) });
    await save.start(intent);
    expect(canCorrectSave(save.getSnapshot())).toBe(false);
    expect(save.clearAfterReview()).toBe(false);
  });

  it("checks a queue rejection read-only before allowing a new input", async () => {
    const { save, api } = setup({ approve: mock(async () => { throw new Error("apply failed"); }), findFinalized: mock(async () => ({ ...proposal, status: "rejected" })) });
    await save.start(intent);
    await save.checkStatus();
    expect(api.propose).toHaveBeenCalledTimes(1);
    expect(api.approve).toHaveBeenCalledTimes(1);
    expect(save.getSnapshot().phase).toBe("rejected");
    expect(save.clearAfterReview()).toBe(true);
  });

  it("blocks repeated status checks and retains the ID when no finalized record is found", async () => {
    const gate = deferred();
    const { save, api } = setup({ approve: mock(async () => { throw new Error("apply failed"); }), findFinalized: mock(() => gate.promise) });
    await save.start(intent);
    const running = save.checkStatus();
    await save.checkStatus();
    await save.retry();
    expect(api.findFinalized).toHaveBeenCalledTimes(1);
    gate.resolve(null);
    await running;
    expect(save.getSnapshot()).toMatchObject({ phase: "error", proposalId: 31 });
    expect(save.getSnapshot().notice).toContain("新しい提案は作成していません");
    expect(save.clearAfterReview()).toBe(false);
  });
});
