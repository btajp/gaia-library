import type { ManualTarget, Proposal } from "../proposalTypes";

export const proposal: Proposal = {
  id: 31,
  target_type: "person",
  action: "insert",
  patch: { name: "提案の人物" },
  kind: "fact",
  scope: "scope-a",
  proposed_by: "demo-agent",
  request_id: "request-0001",
  status: "pending",
  created_at: "2026-08-27T10:00:00Z",
};

export const manualCases: Array<{ target: ManualTarget; values: Record<string, string>; expected: Record<string, unknown>; kind?: "fact" | "inference" }> = [
  {
    target: "person",
    values: { name: " 人物サンプル ", org_id: " 2 ", role: " 担当 ", aliases: " 別名1\n\n別名2 ", first_met: "2026-01-01", last_seen: "2026-08-27" },
    expected: { name: "人物サンプル", org_id: 2, role: "担当", aliases: [{ alias: "別名1" }, { alias: "別名2" }], first_met: "2026-01-01", last_seen: "2026-08-27" },
  },
  { target: "organization", values: { name: " 組織サンプル ", kind: " partner " }, expected: { name: "組織サンプル", kind: "partner" } },
  {
    target: "engagement",
    values: { name: " 案件サンプル ", org_id: "2", status: " 検討中 ", started_at: "2026-01-01", ended_at: "2026-12-31", people: " 1, 窓口\n2 " },
    expected: { name: "案件サンプル", org_id: 2, status: "検討中", started_at: "2026-01-01", ended_at: "2026-12-31", people: [{ person_id: 1, role: "窓口" }, { person_id: 2 }] },
  },
  {
    target: "fact", kind: "inference",
    values: { entity_type: "person", entity_id: "1", statement: " 仮説です ", predicate: "interest", value: " アプリ ", valid_from: "2026-08-27" },
    expected: { entity_type: "person", entity_id: 1, statement: "仮説です", predicate: "interest", value: "アプリ", valid_from: "2026-08-27" },
  },
  {
    target: "ref",
    values: { target_type: "fact", target_id: "3", system: " minutes ", uri: " custom://record/3 ", title: " 表題 ", note: " 会議の決定事項 ", snapshot: " 要点 ", last_verified: "2026-08-27" },
    expected: { target_type: "fact", target_id: 3, system: "minutes", uri: "custom://record/3", title: "表題", note: "会議の決定事項", snapshot: "要点", last_verified: "2026-08-27" },
  },
  { target: "glossary", values: { term: " 用語 ", reading: " ようご ", definition: " 意味 ", engagement_id: "3" }, expected: { term: "用語", reading: "ようご", definition: "意味", engagement_id: 3 } },
  {
    target: "interaction",
    values: { kind: " call ", occurred_at: "2026-08-27T10:00:00+09:00", summary: " 要点 ", engagement_id: "3", person_ids: " 1, 2\n3 " },
    expected: { kind: "call", occurred_at: "2026-08-27T10:00:00+09:00", summary: "要点", engagement_id: 3, person_ids: [1, 2, 3] },
  },
];

export function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => { resolve = resolvePromise; reject = rejectPromise; });
  return { promise, resolve, reject };
}
