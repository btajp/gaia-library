import { describe, expect, it, mock } from "bun:test";
import { LatestRequest, snapshotForKey } from "./latestRequest";

function deferred() {
  let resolve;
  let reject;
  const promise = new Promise((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}

describe("latest request state", () => {
  it("clears previous data immediately when a new request starts", async () => {
    const request = new LatestRequest();
    await request.run("old", async () => "old data");
    const response = deferred();
    const running = request.run("new", () => response.promise);
    expect(request.getSnapshot()).toEqual({ key: "new", status: "loading", data: null, error: null });
    response.resolve("new data");
    await running;
    expect(request.getSnapshot().data).toBe("new data");
  });

  it("ignores a success that arrives after a newer response", async () => {
    const request = new LatestRequest();
    const first = deferred();
    const second = deferred();
    const oldRun = request.run("old", () => first.promise);
    const newRun = request.run("new", () => second.promise);
    second.resolve("new data");
    await newRun;
    first.resolve("old data");
    await oldRun;
    expect(request.getSnapshot().key).toBe("new");
    expect(request.getSnapshot().data).toBe("new data");
  });

  it("ignores an old failure after a newer successful request", async () => {
    const request = new LatestRequest();
    const first = deferred();
    const oldRun = request.run("old", () => first.promise);
    await request.run("new", async () => "new data");
    first.reject("old failure");
    await oldRun;
    expect(request.getSnapshot().status).toBe("success");
    expect(request.getSnapshot().error).toBeNull();
  });

  it("resets visible results and ignores responses after query or scope changes", async () => {
    const request = new LatestRequest();
    await request.run("old", async () => "old data");
    const response = deferred();
    const running = request.run("pending", () => response.promise);
    request.reset();
    expect(request.getSnapshot()).toEqual({ key: null, status: "idle", data: null, error: null });
    response.resolve("must stay hidden");
    await running;
    expect(request.getSnapshot().data).toBeNull();
    expect(request.getSnapshot().status).toBe("idle");
  });

  it("does not notify or retain a response invalidated by unmount", async () => {
    const request = new LatestRequest();
    const response = deferred();
    const listener = mock(() => {});
    const unsubscribe = request.subscribe(listener);
    const running = request.run("pending", () => response.promise);
    expect(listener).toHaveBeenCalledTimes(1);
    request.invalidate();
    unsubscribe();
    response.resolve("must stay hidden");
    await running;
    expect(listener).toHaveBeenCalledTimes(1);
    expect(request.getSnapshot().data).toBeNull();
  });

  it("hides a mismatched request key even before effects invalidate the response", async () => {
    const request = new LatestRequest();
    await request.run("scope-a:person-1", async () => "scope-a data");
    expect(snapshotForKey(request.getSnapshot(), "scope-b:person-1")).toBeNull();
    expect(snapshotForKey(request.getSnapshot(), "scope-a:person-2")).toBeNull();
    expect(snapshotForKey(request.getSnapshot(), "scope-a:person-1").data).toBe("scope-a data");
  });

  it("preserves errors for display and allows a clean retry", async () => {
    const request = new LatestRequest();
    const error = new Error("failed");
    await request.run("same", async () => { throw error; });
    expect(request.getSnapshot().status).toBe("error");
    expect(request.getSnapshot().error).toBe(error);
    await request.run("same", async () => "retried");
    expect(request.getSnapshot().status).toBe("success");
    expect(request.getSnapshot().error).toBeNull();
  });

  it("also catches synchronous failures from the operation", async () => {
    const request = new LatestRequest();
    await request.run("failed", () => { throw new Error("sync failure"); });
    expect(request.getSnapshot().error.message).toBe("sync failure");
  });
});
