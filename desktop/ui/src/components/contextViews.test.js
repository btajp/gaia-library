import { describe, expect, it } from "bun:test";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import "../test/tauriMock";
import { engagementOutput, fact, organizationOutput, personOutput, reference, searchOutput } from "../test/contextFixtures";

const { default: FactList } = await import("./FactList");
const { default: RefList, ResolvedContent } = await import("./RefList");
const { default: SearchResults } = await import("./SearchResults");
const { default: DetailContent } = await import("./DetailContent");
const { default: Search } = await import("./Search");
const { default: Detail } = await import("./Detail");

const openDetail = () => {};
const render = (component, props) => renderToStaticMarkup(createElement(component, props));

describe("facts and references", () => {
  it("distinguishes facts and inferences and preserves validity and replacement metadata", () => {
    const html = render(FactList, { facts: [fact, { ...fact, id: 9, kind: "inference", statement: "推測の内容", superseded_by: 10 }] });
    expect(html).toContain("fact（事実）");
    expect(html).toContain("inference（推測）");
    expect(html).toContain("推測の内容");
    expect(html).toContain("2026-08-20");
    expect(html).toContain("2026-08-27T10:00:00Z");
    expect(html).toContain("status = 検討中");
    expect(html).toContain("scope: personal");
    expect(html).toContain("置換済み → fact #10");
  });

  it("accepts omitted optional fact fields without displaying undefined", () => {
    const { predicate, value, valid_from, ...minimal } = fact;
    const html = render(FactList, { facts: [minimal] });
    expect(html).toContain("有効開始: ");
    expect(html).toContain("未設定");
    expect(html).not.toContain("undefined");
    expect(html).not.toContain("置換済み");
  });

  it("shows reference context, snapshot and verification time with a copy-only action", () => {
    const html = render(RefList, { refs: [reference] });
    expect(html).toContain(reference.system);
    expect(html).toContain(reference.title);
    expect(html).toContain(reference.note);
    expect(html).toContain(reference.snapshot);
    expect(html).toContain(reference.last_verified);
    expect(html).toContain(reference.uri);
    expect(html).toContain("URI をコピー");
    expect(html).not.toContain("href=");
    expect(html).not.toContain("src=");
  });

  it("escapes remote-looking content and never creates an executable URI link", () => {
    const html = render(RefList, { refs: [{ ...reference, uri: "javascript:alert(1)", title: "<script>alert(1)</script>", note: "<img src=x>" }] });
    expect(html).toContain("javascript:alert(1)");
    expect(html).toContain("&lt;script&gt;");
    expect(html).toContain("&lt;img src=x&gt;");
    expect(html).not.toContain("<script>");
    expect(html).not.toContain("<img ");
    expect(html).not.toContain("href=");
  });

  it("renders empty lists explicitly", () => {
    expect(render(FactList, { facts: [] })).toContain("facts はありません");
    expect(render(RefList, { refs: [] })).toContain("参照はありません");
  });
});

describe("search results", () => {
  it("shows all response categories, hints, actual scopes and retrieval limits", () => {
    const html = render(SearchResults, { result: searchOutput, limit: 10, openDetail });
    expect(html).toContain("人物サンプル");
    expect(html).toContain("導入（どうにゅう）");
    expect(html).toContain("運用を開始すること");
    expect(html).toContain("導入条件を確認した");
    expect(html).toContain("検索した scope: personal");
    expect(html).toContain("各カテゴリ最大 10 件");
    expect(html).toContain("最大 20 件");
    expect(html).toContain(searchOutput.hints[0]);
    expect(html).toContain("検索スコア 2.0");
  });

  it("reports no results only when every category is empty", () => {
    const empty = { ...searchOutput, entities: [], glossary: [], interactions: [], hints: [] };
    expect(render(SearchResults, { result: empty, limit: 10, openDetail })).toContain("該当する結果はありません");
    expect(render(SearchResults, { result: { ...empty, glossary: searchOutput.glossary }, limit: 10, openDetail })).not.toContain("該当する結果はありません");
    expect(render(SearchResults, { result: { ...empty, interactions: searchOutput.interactions }, limit: 10, openDetail })).not.toContain("該当する結果はありません");
  });

  it("warns about truncation without claiming there must be more results", () => {
    const html = render(SearchResults, { result: searchOutput, limit: 1, openDetail });
    expect(html).toContain("件数上限に達した");
    expect(html).toContain("ほかにも結果がある可能性");
  });

  it("marks explicit cross-scope results and unsupported detail types", () => {
    const result = { ...searchOutput, cross_scope: true, scopes: ["a", "b"], entities: [{ ...searchOutput.entities[0], type: "entity", name: "汎用項目", facts: [], refs: [] }] };
    const html = render(SearchResults, { result, limit: 10, openDetail });
    expect(html).toContain("複数 scope を横断した結果");
    expect(html).toContain("a / b");
    expect(html).toContain("詳細画面は未対応");
    expect(html).not.toMatch(/<button[^>]*>汎用項目<\/button>/);
  });

  it("labels the search input and blocks empty submission", () => {
    const html = render(Search, { scope: "personal", openDetail });
    expect(html).toContain("登録した記憶の索引を検索します");
    expect(html).toContain("選択中の scope（機密境界）の中だけから返ります");
    expect(html).toContain('for="search-query"');
    expect(html).toContain('for="search-limit"');
    expect(html).toContain('type="submit" disabled=""');
  });
});

describe("typed detail views", () => {
  it("renders person aliases, organization, engagements, facts and interactions", () => {
    const html = render(DetailContent, { result: { type: "person", data: personOutput }, openDetail });
    for (const text of ["人物サンプル", "責任者", "別名サンプル", "nickname", "組織サンプル", "案件サンプル", fact.statement, reference.note, "導入条件を確認した"]) {
      expect(html).toContain(text);
    }
    expect(html).toContain("最大 50 件");
    expect(html).toContain("履歴をまとめて取得する機能は未対応");
    expect(html).toContain("直近 20 件");
  });

  it("renders the organization response directly rather than treating it as a person", () => {
    const html = render(DetailContent, { result: { type: "organization", data: organizationOutput }, openDetail });
    expect(html).toContain("組織サンプル");
    expect(html).toContain("partner");
    expect(html).toContain("所属する人物");
    expect(html).toContain("人物サンプル");
    expect(html).toContain("案件サンプル");
    expect(html).not.toContain("直近のやり取り");
  });

  it("keeps engagement-specific member roles distinct from a person's job role", () => {
    const html = render(DetailContent, { result: { type: "engagement", data: engagementOutput }, openDetail });
    for (const text of ["案件サンプル", "active", "2026-01-01", "2026-12-31", "組織サンプル", "役職: 責任者", "案件での役割: 窓口", "別名サンプル", "導入（どうにゅう）"]) {
      expect(html).toContain(text);
    }
  });

  it("handles detail output with all optional fields omitted", () => {
    const result = { type: "person", data: { person: { id: 1, name: "最小の人物", aliases: [] }, engagements: [], facts: [], refs: [], interactions: [] } };
    const html = render(DetailContent, { result, openDetail });
    expect(html).toContain("最小の人物");
    expect(html).toContain("組織: 未登録");
    expect(html).toContain("別名はありません");
    expect(html).not.toContain("undefined");
  });

  it("starts detail loading without rendering a previous entity", () => {
    const html = render(Detail, { target: { type: "person", id: 1 }, scope: "personal", onBack: () => {}, openDetail });
    expect(html).toContain("読み込み中");
    expect(html).toContain("検索へ戻る");
    expect(html).toContain("対象 scope: personal");
    expect(html).not.toContain("人物サンプル");
  });
});

describe("reference resolution view", () => {

  it("offers a resolve action next to the copy action without opening the URI", () => {
    const html = render(RefList, { refs: [reference] });
    expect(html).toContain("内容を取得");
    expect(html).toContain("URI をコピー");
    expect(html).not.toContain("href=");
  });

  it("renders resolved content as escaped text with the note", () => {
    const html = render(ResolvedContent, { result: { reference, resolved: true, content: "<script>alert(1)</script>\n# 見出し", reason: "content truncated to 10 of 20 chars" } });
    expect(html).toContain("&lt;script&gt;alert(1)&lt;/script&gt;");
    expect(html).not.toContain("<script>");
    expect(html).toContain("注記: content truncated to 10 of 20 chars");
    expect(html).toContain("# 見出し");
  });

  it("shows the reason and opens the snapshot when the source could not be resolved", () => {
    const html = render(ResolvedContent, { result: { reference, resolved: false, reason: "resolver `narumi` is not configured (set [sources.narumi].command); fallback: see reference.snapshot" } });
    expect(html).toContain("取得できませんでした");
    expect(html).toContain("resolver `narumi` is not configured");
    expect(html).toContain("<details open");
    expect(html).toContain(reference.snapshot);
    expect(html).not.toContain("<pre");
  });

  it("handles missing snapshot and reason without displaying undefined", () => {
    const { snapshot, ...noSnapshot } = reference;
    const html = render(ResolvedContent, { result: { reference: noSnapshot, resolved: false } });
    expect(html).toContain("理由は不明です");
    expect(html).not.toContain("undefined");
    expect(html).not.toContain("<details");
  });
});
