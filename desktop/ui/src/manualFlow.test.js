import { beforeEach, describe, expect, it } from "bun:test";
import { invoke } from "./test/tauriMock";
import { deferred, manualCases, proposal } from "./test/proposalFixtures";
import { buildManualIntent } from "./forms/manualPayload";
import { listProposals, proposalsKey } from "./proposalApi";
import { LatestRequest, snapshotForKey } from "./lib/latestRequest";
import { ManualSave } from "./lib/manualSave";
import { ProposalDecisions, decisionsForScope } from "./lib/proposalDecisions";
import { SingleFlight } from "./lib/singleFlight";

beforeEach(() => invoke.mockReset());

describe("manual entry through mocked Tauri IPC", () => {
  for (const sample of manualCases) {
    it(`saves ${sample.target} only through propose_update then approve_proposal`, async () => {
      invoke.mockResolvedValueOnce({ proposal_id: 31, status: "pending", duplicate: false });
      invoke.mockResolvedValueOnce({ proposal_id: 31, status: "approved", result: { target_type: sample.target, id: 7 } });
      const save = new ManualSave();
      await save.start(buildManualIntent(sample.target, sample.values, sample.kind ?? "", "scope-a"));
      expect(invoke.mock.calls.map((call) => call[0])).toEqual(["call_tool", "call_tool"]);
      const request = invoke.mock.calls[0][1];
      expect(request.name).toBe("propose_update");
      expect(request.args).toMatchObject({ target_type: sample.target, action: "insert", patch: sample.expected, kind: sample.kind ?? "fact", scope: "scope-a" });
      expect(request.args.request_id).toMatch(/^ui-[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/);
      expect(invoke.mock.calls[1][1]).toEqual({ name: "approve_proposal", args: { proposal_id: 31, scope: "scope-a" } });
      expect(save.getSnapshot().phase).toBe("complete");
    });
  }

  it("reuses a request after a lost proposal response and does not repropose after approval failure", async () => {
    invoke.mockRejectedValueOnce("lost response");
    invoke.mockResolvedValueOnce({ proposal_id: 31, status: "pending", duplicate: true });
    invoke.mockRejectedValueOnce({ code: "busy", message: "locked", details: { retry: true } });
    invoke.mockResolvedValueOnce({ proposal_id: 31, status: "approved", result: { target_type: "person", id: 7 } });
    const save = new ManualSave();
    await save.start(buildManualIntent("person", { name: "人物" }, "", "scope-a"));
    await save.retry();
    expect(save.getSnapshot().error.code).toBe("busy");
    expect(save.getSnapshot().error.details).toEqual({ retry: true });
    const input = save.getSnapshot().input;
    expect(await save.start({ ...input, scope: "scope-b" })).toBe(false);
    await save.retry();
    expect(invoke.mock.calls.map((call) => call[1].name)).toEqual(["propose_update", "propose_update", "approve_proposal", "approve_proposal"]);
    expect(invoke.mock.calls[0][1].args).toEqual(invoke.mock.calls[1][1].args);
    expect(invoke.mock.calls[2][1].args).toEqual(invoke.mock.calls[3][1].args);
    expect(save.getSnapshot().input).toEqual(input);
  });

  it("prevents the proposal queue from sending another mutation during manual approval", async () => {
    const gate = deferred();
    invoke.mockResolvedValueOnce({ proposal_id: 31, status: "pending", duplicate: false });
    invoke.mockImplementationOnce(() => gate.promise);
    const save = new ManualSave();
    const approving = save.start(buildManualIntent("person", { name: "人物" }, "", "scope-a"));
    for (let tick = 0; tick < 8 && invoke.mock.calls.length < 2; tick += 1) await Promise.resolve();
    expect(invoke).toHaveBeenCalledTimes(2);
    const decisions = new ProposalDecisions();
    await decisions.run(proposal, "reject", "reason");
    expect(invoke).toHaveBeenCalledTimes(2);
    expect(decisionsForScope(decisions.getSnapshot(), "scope-a")[0].error.code).toBe("busy");
    gate.resolve({ proposal_id: 31, status: "approved", result: { target_type: "person", id: 7 } });
    await approving;
    expect(save.getSnapshot().phase).toBe("complete");
  });
});

describe("queue reads, mutations, and stale-response guards", () => {
  it("reloads the pending list after the current scope's decision revision changes", async () => {
    invoke.mockResolvedValueOnce({ proposals: [proposal] });
    invoke.mockResolvedValueOnce({ proposal_id: 31, status: "approved", result: { target_type: "person", id: 7 } });
    invoke.mockResolvedValueOnce({ proposals: [] });
    const decisions = new ProposalDecisions();
    const request = new LatestRequest();
    const key = () => JSON.stringify([proposalsKey("scope-a", "pending", 50), 0, decisions.getSnapshot().scopeRevisions.get("scope-a") ?? 0]);
    await request.run(key(), () => listProposals("scope-a", "pending", 50));
    const initial = key();
    await decisions.run(request.getSnapshot().data.proposals[0], "approve");
    expect(key()).not.toBe(initial);
    expect(snapshotForKey(request.getSnapshot(), key())).toBeNull();
    await request.run(key(), () => listProposals("scope-a", "pending", 50));
    expect(request.getSnapshot().data.proposals).toEqual([]);
    expect(invoke.mock.calls.map((call) => call[1].name)).toEqual(["list_proposals", "approve_proposal", "list_proposals"]);
    expect(invoke.mock.calls.every((call) => call[1].args.scope === "scope-a")).toBe(true);
  });

  it("clears old results immediately and ignores a delayed response after scope changes", async () => {
    const old = deferred();
    const next = deferred();
    invoke.mockResolvedValueOnce({ proposals: [proposal] });
    invoke.mockImplementationOnce(() => old.promise);
    invoke.mockImplementationOnce(() => next.promise);
    const request = new LatestRequest();
    const oldKey = proposalsKey("scope-a", "pending", 50);
    const newKey = proposalsKey("scope-b", "pending", 50);
    await request.run(oldKey, () => listProposals("scope-a", "pending", 50));
    expect(snapshotForKey(request.getSnapshot(), newKey)).toBeNull();
    const oldRun = request.run(oldKey, () => listProposals("scope-a", "pending", 50));
    request.reset();
    expect(request.getSnapshot().data).toBeNull();
    const newRun = request.run(newKey, () => listProposals("scope-b", "pending", 50));
    next.resolve({ proposals: [{ ...proposal, id: 32, scope: "scope-b" }] });
    await newRun;
    old.resolve({ proposals: [proposal] });
    await oldRun;
    expect(snapshotForKey(request.getSnapshot(), newKey).data.proposals.map((item) => item.scope)).toEqual(["scope-b"]);
    expect(snapshotForKey(request.getSnapshot(), oldKey)).toBeNull();
  });

  it("ignores an old filter response and does not publish a response after unmount", async () => {
    const old = deferred();
    invoke.mockImplementationOnce(() => old.promise);
    invoke.mockResolvedValueOnce({ proposals: [] });
    const request = new LatestRequest();
    const running = request.run("pending", () => listProposals("scope-a", "pending"));
    request.reset();
    await request.run("approved", () => listProposals("scope-a", "approved"));
    request.invalidate();
    const last = request.getSnapshot();
    old.reject(new Error("late old failure"));
    await running;
    expect(request.getSnapshot()).toBe(last);
    expect(request.getSnapshot().key).toBe("approved");
  });

  it("shares a read across StrictMode-like repeated effects without losing the latest subscriber", async () => {
    const gate = deferred();
    invoke.mockImplementationOnce(() => gate.promise);
    const request = new LatestRequest();
    const flight = new SingleFlight();
    const key = proposalsKey("scope-a", "pending", 50);
    const run = () => request.run(key, () => flight.run(key, () => listProposals("scope-a", "pending", 50)));
    const first = run();
    request.invalidate();
    const second = run();
    await Promise.resolve();
    expect(invoke).toHaveBeenCalledTimes(1);
    gate.resolve({ proposals: [proposal] });
    await Promise.all([first, second]);
    expect(request.getSnapshot().status).toBe("success");
    expect(request.getSnapshot().data.proposals).toEqual([proposal]);
  });
});
