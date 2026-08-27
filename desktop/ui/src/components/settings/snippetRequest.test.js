import { beforeEach, describe, expect, it } from "bun:test";
import { LatestRequest } from "../../lib/latestRequest";
import { invoke } from "../../test/tauriMock";
import { requestSnippet, snippetKey } from "./snippetRequest";

function deferred() {
  let resolve;
  const promise = new Promise((yes) => { resolve = yes; });
  return { promise, resolve };
}

beforeEach(() => invoke.mockReset());

describe("explicit snippet requests", () => {
  it("does not call Tauri merely by creating the request state", () => {
    const request = new LatestRequest();
    expect(request.getSnapshot().status).toBe("idle");
    expect(invoke).not.toHaveBeenCalled();
  });

  it("suppresses duplicate in-flight reads for the same client and transport", async () => {
    const request = new LatestRequest();
    const wait = deferred();
    invoke.mockImplementationOnce(() => wait.promise);
    const first = requestSnippet(request, "reader", "http");
    await requestSnippet(request, "reader", "http");
    expect(invoke).toHaveBeenCalledTimes(1);
    wait.resolve({ text: "test-only-snippet", key_storage: "keychain" });
    await first;
    expect(request.getSnapshot().status).toBe("success");
  });

  it("never replaces the selected client with a late previous response", async () => {
    const request = new LatestRequest();
    const wait = deferred();
    invoke.mockImplementationOnce(() => wait.promise);
    const first = requestSnippet(request, "first", "http");
    const current = { text: "current-test-snippet", key_storage: null };
    invoke.mockResolvedValueOnce(current);
    await requestSnippet(request, "second", "stdio");
    wait.resolve({ text: "old-test-secret", key_storage: "keychain" });
    await first;
    expect(request.getSnapshot().data).toBe(current);
    expect(request.getSnapshot().key).toBe(snippetKey("second", "stdio"));
  });

  it("purges the display on close and suppresses a late secret response", async () => {
    const request = new LatestRequest();
    const wait = deferred();
    invoke.mockImplementationOnce(() => wait.promise);
    const pending = requestSnippet(request, "reader", "http");
    request.reset();
    wait.resolve({ text: "test-only-secret", key_storage: "keychain" });
    await pending;
    expect(request.getSnapshot()).toEqual({ key: null, status: "idle", data: null, error: null });
  });
});
