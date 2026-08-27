import { describe, expect, it, mock } from "bun:test";
import { LatestRequest } from "../../lib/latestRequest";
import { SettingsAction } from "./settingsAction";

function deferred() {
  let resolve;
  let reject;
  const promise = new Promise((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}

describe("settings mutation state", () => {
  it("ignores duplicate submissions while the first call is in flight", async () => {
    const action = new SettingsAction();
    const wait = deferred();
    const operation = mock(() => wait.promise);
    const first = action.run(operation);
    expect(action.getSnapshot().status).toBe("working");
    expect(await action.run(operation)).toBeNull();
    expect(operation).toHaveBeenCalledTimes(1);
    wait.resolve("saved");
    expect(await first).toEqual({ data: "saved" });
  });

  it("does not republish an old secret after its screen is closed", async () => {
    const action = new SettingsAction();
    const wait = deferred();
    const result = action.run(() => wait.promise);
    action.reset();
    wait.resolve({ key: "test-only-secret" });
    expect(await result).toBeNull();
    expect(action.getSnapshot()).toEqual({ status: "idle", data: null, error: null });
  });

  it("does not publish a late failure after invalidation", async () => {
    const action = new SettingsAction();
    const wait = deferred();
    const result = action.run(() => wait.promise);
    action.reset();
    wait.reject(new Error("stale error"));
    await result;
    expect(action.getSnapshot()).toEqual({ status: "idle", data: null, error: null });
  });

  it("keeps the write lock until an invalidated operation settles", async () => {
    const action = new SettingsAction();
    const wait = deferred();
    const result = action.run(() => wait.promise);
    action.reset();
    const another = mock(async () => "other");
    expect(await action.run(another)).toBeNull();
    expect(another).not.toHaveBeenCalled();
    wait.resolve("old");
    await result;
    expect(await action.run(another)).toEqual({ data: "other" });
  });

  it("allows retry after a write failure", async () => {
    const action = new SettingsAction();
    const error = new Error("save failed");
    expect(await action.run(async () => { throw error; })).toBeNull();
    expect(action.getSnapshot()).toEqual({ status: "error", data: null, error });
    expect(await action.run(async () => "saved")).toEqual({ data: "saved" });
    expect(action.getSnapshot().status).toBe("success");
  });

  it("keeps a committed key visible when the following list refresh fails", async () => {
    const action = new SettingsAction();
    const list = new LatestRequest();
    const issued = { key: "test-only-issued-key", storage: { location: null, error: "storage failed" } };
    await action.run(async () => issued);
    await list.run("clients", async () => { throw new Error("list failed"); });
    expect(action.getSnapshot()).toEqual({ status: "success", data: issued, error: null });
    expect(list.getSnapshot().status).toBe("error");
  });

  it("clears the displayed key when explicitly closed", async () => {
    const action = new SettingsAction();
    await action.run(async () => ({ key: "test-only-issued-key" }));
    action.reset();
    expect(action.getSnapshot()).toEqual({ status: "idle", data: null, error: null });
  });
});
