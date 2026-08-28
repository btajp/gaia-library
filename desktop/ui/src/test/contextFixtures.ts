import type { Fact, GetEngagementOutput, GetOrganizationOutput, GetPersonOutput, Reference, SearchContextOutput } from "../types";

export const fact: Fact = {
  id: 7,
  entity_type: "person",
  entity_id: 1,
  statement: "導入を検討している",
  kind: "fact",
  scope: "personal",
  created_at: "2026-08-27T10:00:00Z",
  predicate: "status",
  value: "検討中",
  valid_from: "2026-08-20",
};

export const reference: Reference = {
  id: 8,
  target_type: "fact",
  target_id: 7,
  system: "minutes",
  uri: "https://example.invalid/meeting/1",
  title: "導入相談の記録",
  note: "担当者の発言と次回の確認事項",
  snapshot: "来月の打ち合わせで判断する",
  scope: "personal",
  created_at: "2026-08-27T10:00:00Z",
  last_verified: "2026-08-27",
};

const person = {
  id: 1,
  name: "人物サンプル",
  aliases: [{ alias: "別名サンプル", kind: "nickname" }],
  role: "責任者",
  org_id: 2,
  org_name: "組織サンプル",
  first_met: "2026-01-01",
  last_seen: "2026-08-27",
};

const organization = { id: 2, name: "組織サンプル", kind: "partner" };
const engagement = {
  id: 3,
  name: "案件サンプル",
  scope: "personal",
  org_id: 2,
  org_name: "組織サンプル",
  status: "active",
  started_at: "2026-01-01",
  ended_at: "2026-12-31",
};
const glossary = [{ id: 4, term: "導入", reading: "どうにゅう", definition: "運用を開始すること", engagement_id: 3, scope: "personal" }];
const interactions = [{ id: 5, kind: "meeting", occurred_at: "2026-08-27T09:00:00Z", summary: "導入条件を確認した", engagement_id: 3, scope: "personal", person_ids: [1] }];

export const personOutput: GetPersonOutput = {
  person,
  organization,
  engagements: [engagement],
  facts: [fact],
  refs: [reference],
  interactions,
};

export const organizationOutput: GetOrganizationOutput = {
  organization,
  people: [person],
  engagements: [engagement],
  facts: [],
  refs: [],
};

export const engagementOutput: GetEngagementOutput = {
  engagement,
  organization,
  people: [{ person, role: "窓口" }],
  facts: [],
  refs: [],
  glossary,
  interactions,
};

export const searchOutput: SearchContextOutput = {
  query: "導入",
  scopes: ["personal"],
  cross_scope: false,
  entities: [{ type: "person", id: 1, name: person.name, summary: "責任者", score: 2, matched_on: ["fact:7"], facts: [fact], refs: [reference] }],
  glossary,
  interactions,
  hints: ["short query used substring matching"],
};
