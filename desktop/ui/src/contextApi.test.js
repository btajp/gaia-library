import { beforeEach, describe, expect, it, mock } from "bun:test";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { invoke } from "./test/tauriMock";
import { LatestRequest, snapshotForKey } from "./lib/latestRequest";
import { engagementOutput, organizationOutput, personOutput, searchOutput } from "./test/contextFixtures";

const context = await import("./contextApi");
const { GaiaError } = await import("./api");
const { DetailLink } = await import("./components/ContextLists");
const { default: DetailContent } = await import("./components/DetailContent");

beforeEach(() => invoke.mockReset());

describe("scoped context IPC", () => {
  it("sends trimmed query, explicit single scope and the selected limit", async () => {
    invoke.mockResolvedValueOnce(searchOutput);
    expect(await context.searchContext(" 導入 ", " personal ", 20)).toBe(searchOutput);
    expect(invoke).toHaveBeenCalledWith("call_tool", { name: "search_context", args: { query: "導入", scope: "personal", limit: 20 } });
  });

  it("omits an empty scope so the server applies the client default", async () => {
    invoke.mockResolvedValueOnce(searchOutput);
    await context.searchContext("導入", " ", 10);
    expect(invoke).toHaveBeenCalledWith("call_tool", { name: "search_context", args: { query: "導入", limit: 10 } });
    expect(context.scopeArgs("a,b")).toEqual({ scope: "a,b" });
  });

  it("does not issue invalid blank searches or invalid limits", async () => {
    await expect(context.searchContext("  ", "personal", 10)).rejects.toThrow("検索語");
    for (const limit of [0, 51, 1.5, NaN]) {
      await expect(context.searchContext("導入", "personal", limit)).rejects.toThrow("1〜50");
    }
    expect(invoke).not.toHaveBeenCalled();
  });

  it("uses the correct id argument and keeps the scope for every detail type", async () => {
    const cases = [
      ["person", 1, "person_id", personOutput],
      ["organization", 2, "organization_id", organizationOutput],
      ["engagement", 3, "engagement_id", engagementOutput],
    ];
    for (const [type, id, field, output] of cases) {
      invoke.mockResolvedValueOnce(output);
      expect(await context.loadDetail({ type, id }, " personal ")).toEqual({ type, data: output });
      expect(invoke).toHaveBeenLastCalledWith("call_tool", { name: `get_${type}`, args: { [field]: id, scope: "personal" } });
    }
  });

  it("keeps structured failures and does not fall back to another scope", async () => {
    invoke.mockRejectedValueOnce({ code: "scope_denied", message: "scope denied", details: { scope: "private" } });
    await expect(context.loadDetail({ type: "person", id: 1 }, "private")).rejects.toBeInstanceOf(GaiaError);
    expect(invoke).toHaveBeenCalledTimes(1);
  });

  it("distinguishes query, scope, limit and detail target request keys", () => {
    const baseline = context.searchKey("導入", "personal", 10);
    expect(context.searchKey(" 導入 ", "personal", 10)).toBe(baseline);
    expect(context.searchKey("別の語", "personal", 10)).not.toBe(baseline);
    expect(context.searchKey("導入", "other", 10)).not.toBe(baseline);
    expect(context.searchKey("導入", "personal", 20)).not.toBe(baseline);
    expect(context.detailKey({ type: "person", id: 1 }, "personal")).not.toBe(context.detailKey({ type: "organization", id: 1 }, "personal"));
    expect(context.isDetailType("entity")).toBe(false);
    expect(context.isDetailType("interaction")).toBe(false);
  });
});

describe("mocked navigation and request integration", () => {
  it("loads the linked entity through the same scoped tool path", async () => {
    const navigate = mock(() => {});
    const link = DetailLink({ type: "organization", id: 2, children: "組織サンプル", openDetail: navigate });
    link.props.onClick();
    expect(navigate).toHaveBeenCalledWith("organization", 2);
    invoke.mockResolvedValueOnce(organizationOutput);
    const request = new LatestRequest();
    const target = { type: "organization", id: 2 };
    await request.run(context.detailKey(target, "personal"), () => context.loadDetail(target, "personal"));
    const html = renderToStaticMarkup(createElement(DetailContent, { result: request.getSnapshot().data, openDetail: navigate }));
    expect(html).toContain("組織サンプル");
    expect(html).toContain("人物サンプル");
    expect(html).toContain("案件サンプル");
  });

  it("cannot apply a delayed old-scope search response to the new scope", async () => {
    let resolveOld;
    let resolveNew;
    invoke.mockImplementationOnce(() => new Promise((resolve) => { resolveOld = resolve; }));
    invoke.mockImplementationOnce(() => new Promise((resolve) => { resolveNew = resolve; }));
    const request = new LatestRequest();
    const oldKey = context.searchKey("導入", "old", 10);
    const newKey = context.searchKey("導入", "new", 10);
    const oldRun = request.run(oldKey, () => context.searchContext("導入", "old", 10));
    request.reset();
    const newRun = request.run(newKey, () => context.searchContext("導入", "new", 10));
    resolveNew({ ...searchOutput, scopes: ["new"] });
    await newRun;
    resolveOld({ ...searchOutput, scopes: ["old"] });
    await oldRun;
    expect(snapshotForKey(request.getSnapshot(), newKey).data.scopes).toEqual(["new"]);
    expect(snapshotForKey(request.getSnapshot(), oldKey)).toBeNull();
    expect(invoke.mock.calls.map((call) => call[1].args.scope)).toEqual(["old", "new"]);
  });

  it("does not mix responses while moving rapidly between detail types", async () => {
    let resolveOld;
    invoke.mockImplementationOnce(() => new Promise((resolve) => { resolveOld = resolve; }));
    invoke.mockResolvedValueOnce(engagementOutput);
    const request = new LatestRequest();
    const first = { type: "person", id: 1 };
    const second = { type: "engagement", id: 3 };
    const oldRun = request.run(context.detailKey(first, "personal"), () => context.loadDetail(first, "personal"));
    await request.run(context.detailKey(second, "personal"), () => context.loadDetail(second, "personal"));
    resolveOld(personOutput);
    await oldRun;
    const current = snapshotForKey(request.getSnapshot(), context.detailKey(second, "personal"));
    expect(current.data.type).toBe("engagement");
    expect(current.data.data.engagement.name).toBe("案件サンプル");
  });
});

describe("reference copying", () => {
  it("copies the URI only, including non-HTTP references, without opening it", async () => {
    const writeText = mock(() => Promise.resolve());
    await context.copyReferenceUri("custom://record/1", { writeText });
    expect(writeText).toHaveBeenCalledWith("custom://record/1");
    expect(writeText).toHaveBeenCalledTimes(1);
    expect(invoke).not.toHaveBeenCalled();
  });

  it("reports missing clipboard support and permission failures", async () => {
    await expect(context.copyReferenceUri("custom://record/1", {})).rejects.toThrow("コピー機能");
    await expect(context.copyReferenceUri("custom://record/1", { writeText: async () => { throw new Error("denied"); } })).rejects.toThrow("URI をコピーできませんでした");
    expect(invoke).not.toHaveBeenCalled();
  });
});

describe("reference resolution", () => {
  it("calls resolve_source with the reference id and its own scope only", async () => {
    const { reference } = await import("./test/contextFixtures");
    const output = { reference, resolved: true, content: "本文" };
    invoke.mockResolvedValueOnce(output);
    expect(await context.resolveReference({ ...reference, scope: " personal " })).toBe(output);
    expect(invoke).toHaveBeenCalledTimes(1);
    expect(invoke).toHaveBeenCalledWith("call_tool", { name: "resolve_source", args: { ref_id: reference.id, scope: "personal" } });
  });

  it("rejects references without an id or scope before calling the server", async () => {
    const { reference } = await import("./test/contextFixtures");
    await expect(context.resolveReference({ ...reference, id: 0 })).rejects.toThrow("参照 ID");
    await expect(context.resolveReference({ ...reference, scope: " " })).rejects.toThrow("scope");
    expect(invoke).not.toHaveBeenCalled();
  });

  it("keeps structured failures from resolve_source", async () => {
    const { reference } = await import("./test/contextFixtures");
    invoke.mockRejectedValueOnce({ code: "busy", message: "resolver `narumi` is busy; retry later" });
    await expect(context.resolveReference(reference)).rejects.toBeInstanceOf(GaiaError);
  });

  it("shows a pending note that does not promise a fixed timeout", () => {
    // 上限は [sources] の設定次第なので、UI の文言に秒数を含めない
    expect(context.RESOLVE_PENDING_NOTE).toBe("取得中…（時間がかかることがあります）");
    expect(context.RESOLVE_PENDING_NOTE).not.toMatch(/\d+ ?秒/);
  });
});
