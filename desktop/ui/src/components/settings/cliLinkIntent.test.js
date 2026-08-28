import { describe, expect, it } from "bun:test";
import { beginCliLinkConfirmation, cliLinkIntent } from "./cliLinkIntent";

const success = (data) => ({ key: "cli-link", status: "success", data, error: null });

describe("CLI link operation intent", () => {
  it("allows only creation when the displayed state is missing", () => {
    const snapshot = success({ status: "missing" });
    expect(cliLinkIntent(snapshot, null)).toEqual({ expectedTarget: null });
    expect(beginCliLinkConfirmation(snapshot)).toBeNull();
  });

  it("does not authorize replacement before confirmation", () => {
    const snapshot = success({ status: "wrong_target", current: "/test/old-cli" });
    expect(cliLinkIntent(snapshot, null)).toBeNull();
  });

  it("binds replacement to the exact displayed target without normalization", () => {
    const current = " ../old cli ";
    const snapshot = success({ status: "wrong_target", current });
    const confirmation = beginCliLinkConfirmation(snapshot);
    expect(confirmation.expectedTarget).toBe(current);
    expect(cliLinkIntent(snapshot, confirmation)).toEqual({ expectedTarget: current });
  });

  it("invalidates the old confirmation when the observed target changes", () => {
    const previous = success({ status: "wrong_target", current: "/test/old-cli" });
    const confirmation = beginCliLinkConfirmation(previous);
    const current = success({ status: "wrong_target", current: "/test/new-cli" });
    expect(cliLinkIntent(current, confirmation)).toBeNull();
  });

  it("requires confirmation again after reloading even when the target is unchanged", () => {
    const previous = success({ status: "wrong_target", current: "/test/old-cli" });
    const confirmation = beginCliLinkConfirmation(previous);
    const current = success({ status: "wrong_target", current: "/test/old-cli" });
    expect(cliLinkIntent(current, confirmation)).toBeNull();
    expect(cliLinkIntent(current, beginCliLinkConfirmation(current))).toEqual({ expectedTarget: "/test/old-cli" });
  });

  for (const status of ["idle", "loading", "error"]) {
    it(`blocks operations while status is ${status}`, () => {
      const previous = success({ status: "wrong_target", current: "/test/old-cli" });
      const confirmation = beginCliLinkConfirmation(previous);
      const uncertain = { ...previous, status };
      expect(cliLinkIntent(uncertain, confirmation)).toBeNull();
      expect(beginCliLinkConfirmation(uncertain)).toBeNull();
    });
  }

  for (const status of ["ok", "not_symlink"]) {
    it(`does not offer mutation for ${status}`, () => {
      const snapshot = success({ status });
      expect(cliLinkIntent(snapshot, null)).toBeNull();
      expect(beginCliLinkConfirmation(snapshot)).toBeNull();
    });
  }

  it("does not reuse an earlier missing state as permission to replace a newly found link", () => {
    const earlier = success({ status: "missing" });
    const current = success({ status: "wrong_target", current: "/test/external-cli" });
    expect(cliLinkIntent(earlier, null)).toEqual({ expectedTarget: null });
    expect(cliLinkIntent(current, null)).toBeNull();
  });

  it("does not retain replacement authority when the UI cancels confirmation", () => {
    const snapshot = success({ status: "wrong_target", current: "/test/old-cli" });
    expect(cliLinkIntent(snapshot, beginCliLinkConfirmation(snapshot))).not.toBeNull();
    expect(cliLinkIntent(snapshot, null)).toBeNull();
  });

  it("rejects a target that changed even if an invalid caller reused the snapshot object", () => {
    const snapshot = success({ status: "wrong_target", current: "/test/old-cli" });
    const confirmation = beginCliLinkConfirmation(snapshot);
    snapshot.data.current = "/test/new-cli";
    expect(cliLinkIntent(snapshot, confirmation)).toBeNull();
  });
});
