import { beforeEach, describe, expect, it } from "bun:test";
import { invoke } from "./test/tauriMock";
import { deferred, proposal } from "./test/proposalFixtures";
import { GaiaError } from "./api";
import { approveProposal, decideProposal, decisionKey, findFinalizedProposal, listProposals, proposeManual, proposalsKey, rejectProposal } from "./proposalApi";

beforeEach(() => invoke.mockReset());

describe("proposal IPC and explicit scope", () => {
  it("lists the selected status with an explicit trimmed scope and bounded limit", async () => {
    const output = { proposals: [proposal] };
    invoke.mockResolvedValueOnce(output);
    expect(await listProposals(" scope-a ", "pending", 100)).toBe(output);
    expect(invoke).toHaveBeenCalledWith("call_tool", { name: "list_proposals", args: { scope: "scope-a", status: "pending", limit: 100 } });
  });

  it("does not fall back to a default scope for an empty write or queue request", async () => {
    await expect(listProposals(" ", "pending")).rejects.toThrow("scope");
    await expect(approveProposal(31, " ")).rejects.toThrow("scope");
    await expect(rejectProposal(31, " ", "")).rejects.toThrow("scope");
    await expect(proposeManual({ scope: " " })).rejects.toThrow("scope");
    for (const limit of [0, -1, 1.5, 201, NaN]) await expect(listProposals("scope-a", "pending", limit)).rejects.toThrow("1〜200");
    for (const id of [0, -1, 1.5, 9007199254740992]) await expect(approveProposal(id, "scope-a")).rejects.toThrow("正の整数");
    expect(invoke).not.toHaveBeenCalled();
  });

  it("always decides in the proposal's scope and omits unsupported approval reasons", async () => {
    invoke.mockResolvedValueOnce({ proposal_id: 31, status: "approved", result: { target_type: "person", id: 7 } });
    await decideProposal(proposal, "approve", "not a contract field");
    expect(invoke).toHaveBeenCalledWith("call_tool", { name: "approve_proposal", args: { proposal_id: 31, scope: "scope-a" } });
  });

  it("trims an optional rejection reason and leaves a blank reason out of the payload", async () => {
    invoke.mockResolvedValue({ proposal_id: 31, status: "rejected" });
    await decideProposal(proposal, "reject", " 修正が必要 ");
    expect(invoke).toHaveBeenLastCalledWith("call_tool", { name: "reject_proposal", args: { proposal_id: 31, scope: "scope-a", reason: "修正が必要" } });
    await decideProposal(proposal, "reject", " \n ");
    expect(invoke).toHaveBeenLastCalledWith("call_tool", { name: "reject_proposal", args: { proposal_id: 31, scope: "scope-a" } });
  });

  it("does not mutate already approved or rejected proposals", async () => {
    for (const status of ["approved", "rejected"]) {
      for (const action of ["approve", "reject"]) await expect(decideProposal({ ...proposal, status }, action)).rejects.toThrow("未承認");
    }
    expect(invoke).not.toHaveBeenCalled();
  });

  it("retains structured server errors rather than treating a failure as an empty list", async () => {
    invoke.mockRejectedValueOnce({ code: "scope_denied", message: "not allowed", details: { scope: "scope-a" } });
    try {
      await listProposals("scope-a", "pending");
      throw new Error("expected rejection");
    } catch (error) {
      expect(error).toBeInstanceOf(GaiaError);
      expect(error.code).toBe("scope_denied");
      expect(error.details).toEqual({ scope: "scope-a" });
    }
    expect(invoke).toHaveBeenCalledTimes(1);
  });

  it("uses distinct keys for scopes, statuses and limits", () => {
    const key = proposalsKey("scope-a", "pending", 50);
    expect(proposalsKey(" scope-a ", "pending", 50)).toBe(key);
    expect(proposalsKey("scope-b", "pending", 50)).not.toBe(key);
    expect(proposalsKey("scope-a", "approved", 50)).not.toBe(key);
    expect(proposalsKey("scope-a", "pending", 100)).not.toBe(key);
    expect(decisionKey("scope-a", 31)).not.toBe(decisionKey("scope-b", 31));
  });
});

describe("cross-view mutation guard", () => {
  it("prevents simultaneous approval and rejection for the same scoped proposal", async () => {
    const gate = deferred();
    invoke.mockImplementationOnce(() => gate.promise);
    const approving = approveProposal(31, "scope-a");
    await expect(rejectProposal(31, "scope-a", "reason")).rejects.toMatchObject({ code: "busy" });
    await expect(approveProposal(31, "scope-a")).rejects.toMatchObject({ code: "busy" });
    expect(invoke).toHaveBeenCalledTimes(1);
    gate.resolve({ proposal_id: 31, status: "approved", result: { target_type: "person", id: 7 } });
    await approving;
    invoke.mockResolvedValueOnce({ proposal_id: 31, status: "rejected" });
    await rejectProposal(31, "scope-a", "reason");
    expect(invoke).toHaveBeenCalledTimes(2);
  });

  it("releases the lock after a failure and keeps other proposals independent", async () => {
    const gate = deferred();
    invoke.mockImplementationOnce(() => gate.promise);
    invoke.mockResolvedValueOnce({ proposal_id: 32, status: "rejected" });
    const approving = approveProposal(31, "scope-a");
    await rejectProposal(32, "scope-a", "reason");
    gate.reject(new Error("transport failure"));
    await expect(approving).rejects.toThrow("transport failure");
    invoke.mockResolvedValueOnce({ proposal_id: 31, status: "rejected" });
    await rejectProposal(31, "scope-a", "reason");
    expect(invoke).toHaveBeenCalledTimes(3);
  });
});

describe("bounded read-only reconciliation", () => {
  it("checks finalized records in the original scope and never writes", async () => {
    const result = { ...proposal, status: "approved", result_id: 7 };
    invoke.mockResolvedValueOnce({ proposals: [{ ...result, scope: "scope-b" }, result] });
    expect(await findFinalizedProposal(31, " scope-a ")).toEqual(result);
    expect(invoke).toHaveBeenCalledTimes(1);
    expect(invoke).toHaveBeenCalledWith("call_tool", { name: "list_proposals", args: { scope: "scope-a", status: "approved", limit: 200 } });
  });

  it("checks rejected after approved and cannot mistake an unrelated ID or scope for the result", async () => {
    const result = { ...proposal, status: "rejected" };
    invoke.mockResolvedValueOnce({ proposals: [{ ...result, id: 99, status: "approved" }] });
    invoke.mockResolvedValueOnce({ proposals: [result] });
    expect(await findFinalizedProposal(31, "scope-a")).toEqual(result);
    expect(invoke.mock.calls.map((call) => call[1].args)).toEqual([{ scope: "scope-a", status: "approved", limit: 200 }, { scope: "scope-a", status: "rejected", limit: 200 }]);
  });

  it("returns unknown instead of assuming a proposal outside the returned lists is new", async () => {
    invoke.mockResolvedValue({ proposals: [] });
    expect(await findFinalizedProposal(31, "scope-a")).toBeNull();
    expect(invoke).toHaveBeenCalledTimes(2);
  });
});
