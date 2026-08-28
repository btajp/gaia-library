import { describe, expect, it, mock } from "bun:test";
import { deferred } from "../test/proposalFixtures";
import { SingleFlight } from "./singleFlight";

describe("deduplicated pending reads", () => {
  it("shares the same in-flight read across repeated effects with the same key", async () => {
    const gate = deferred();
    const load = mock(() => gate.promise);
    const flight = new SingleFlight();
    const first = flight.run("scope-a:pending:0", load);
    const repeated = flight.run("scope-a:pending:0", load);
    expect(repeated).toBe(first);
    await Promise.resolve();
    expect(load).toHaveBeenCalledTimes(1);
    gate.resolve({ proposals: [] });
    await first;
    await flight.run("scope-a:pending:0", load);
    expect(load).toHaveBeenCalledTimes(2);
  });

  it("does not reuse a pre-mutation read for a different scope or revision", async () => {
    const flight = new SingleFlight();
    const load = mock(async () => ({}));
    await Promise.all([flight.run("scope-a:0", load), flight.run("scope-b:0", load), flight.run("scope-a:1", load)]);
    expect(load).toHaveBeenCalledTimes(3);
  });

  it("releases rejected reads so the user can retry", async () => {
    const flight = new SingleFlight();
    const load = mock().mockRejectedValueOnce(new Error("failed")).mockResolvedValueOnce("result");
    await expect(flight.run("scope-a", load)).rejects.toThrow("failed");
    expect(await flight.run("scope-a", load)).toBe("result");
    expect(load).toHaveBeenCalledTimes(2);
  });
});
