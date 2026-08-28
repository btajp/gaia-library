import { describe, expect, it } from "bun:test";
import "../test/tauriMock";
import { manualCases } from "../test/proposalFixtures";
import { ENTITY_TYPES, MANUAL_FORMS, PREDICATES, REF_TARGET_TYPES } from "./manualFields";
import { buildManualIntent, positiveId, valuesFromIntent } from "./manualPayload";

const common = (await Bun.file(new URL("../../../../contracts/defs/common.json", import.meta.url)).json()).$defs;

describe("manual payloads from the real contracts", () => {
  for (const sample of manualCases) {
    it(`builds ${sample.target} with only actual fields and preserves explicit values`, () => {
      const intent = buildManualIntent(sample.target, sample.values, sample.kind ?? "", " scope-a ");
      expect(intent).toEqual({ target_type: sample.target, action: "insert", patch: sample.expected, kind: sample.kind ?? "fact", scope: "scope-a" });
      expect(buildManualIntent(sample.target, valuesFromIntent(intent), intent.kind, intent.scope)).toEqual(intent);
    });
  }

  it("keeps the seven forms and selectable types aligned with the contract", () => {
    expect(Object.keys(MANUAL_FORMS).sort()).toEqual(["person", "organization", "engagement", "fact", "ref", "glossary", "interaction"].sort());
    expect([...ENTITY_TYPES]).toEqual(common.EntityType.enum);
    expect([...REF_TARGET_TYPES]).toEqual(common.RefTargetType.enum);
    expect(PREDICATES).toEqual(["role", "status", "interest", "decision"]);
    const patchNames = { person: "PersonPatch", organization: "OrganizationPatch", engagement: "EngagementPatch", fact: "FactPatch", ref: "RefPatch", glossary: "GlossaryPatch", interaction: "InteractionPatch" };
    for (const [target, name] of Object.entries(patchNames)) {
      expect(MANUAL_FORMS[target].fields.map((field) => field.key).sort()).toEqual(Object.keys(common[name].properties).sort());
    }
  });

  it("omits every blank optional value instead of inventing dates, arrays, or state", () => {
    for (const sample of manualCases) {
      const form = MANUAL_FORMS[sample.target];
      const values = Object.fromEntries(form.fields.map((field) => [field.key, field.required ? sample.values[field.key] : " \n "]));
      const { patch } = buildManualIntent(sample.target, values, "fact", "scope-a");
      expect(Object.keys(patch).sort()).toEqual(form.fields.filter((field) => field.required).map((field) => field.key).sort());
    }
  });

  it("rejects whitespace in every mandatory field before any proposal exists", () => {
    for (const sample of manualCases) {
      for (const field of MANUAL_FORMS[sample.target].fields.filter((field) => field.required)) {
        expect(() => buildManualIntent(sample.target, { ...sample.values, [field.key]: " \n\t " }, "fact", "scope-a")).toThrow(field.label);
      }
    }
  });

  it("requires an explicit fact kind and a value for structured predicates", () => {
    const values = { entity_type: "person", entity_id: "1", statement: "内容" };
    for (const kind of ["", " ", "guess"]) expect(() => buildManualIntent("fact", values, kind, "scope-a")).toThrow("kind");
    expect(() => buildManualIntent("fact", { ...values, predicate: "role", value: " " }, "fact", "scope-a")).toThrow("value");
    expect(() => buildManualIntent("fact", { ...values, predicate: "unsupported", value: "value" }, "fact", "scope-a")).toThrow("選択肢");
    expect(buildManualIntent("fact", { ...values, value: "自由値" }, "inference", "scope-a").patch).toEqual({ ...values, entity_id: 1, value: "自由値" });
  });

  it("rejects URI-only references and accepts non-HTTP reference schemes", () => {
    const values = { target_type: "person", target_id: "1", system: "file", uri: "file:///records/notes.md" };
    expect(() => buildManualIntent("ref", values, "", "scope-a")).toThrow("注記");
    expect(buildManualIntent("ref", { ...values, note: "会議記録の要点" }, "", "scope-a").patch.uri).toBe(values.uri);
  });

  it("allows only the contract's entity and reference target types", () => {
    const fact = { entity_type: "fact", entity_id: "1", statement: "内容" };
    expect(() => buildManualIntent("fact", fact, "fact", "scope-a")).toThrow("選択肢");
    const ref = { target_type: "glossary", target_id: "1", system: "file", uri: "file:record", note: "文脈" };
    expect(() => buildManualIntent("ref", ref, "", "scope-a")).toThrow("選択肢");
  });

  it("rejects blank scope and unknown targets without guessing values", () => {
    expect(() => buildManualIntent("person", { name: "人物" }, "", " ")).toThrow("scope");
    for (const target of ["entity", "invalid", "constructor", "__proto__"]) {
      expect(() => buildManualIntent(target, {}, "", "scope-a")).toThrow("種別");
    }
    expect(buildManualIntent("person", { name: "人物", unexpected: "omit" }, "", "scope-a").patch).toEqual({ name: "人物" });
  });
});

describe("positive IDs and list conversion", () => {
  it("accepts only safe positive integer notation", () => {
    expect(positiveId(" 42 ", "ID")).toBe(42);
    for (const value of ["", " ", "0", "-1", "+1", "1.5", "1e3", "0x10", "NaN", "Infinity", "9007199254740992", "1 2"]) {
      expect(() => positiveId(value, "ID")).toThrow("正の整数");
    }
  });

  it("validates IDs in all nested lists and rejects repeated engagement members", () => {
    expect(() => buildManualIntent("engagement", { name: "案件", people: "1\n1,role" }, "", "scope-a")).toThrow("重複");
    for (const people of ["0,role", "-1", "word", "1.1"]) {
      expect(() => buildManualIntent("engagement", { name: "案件", people }, "", "scope-a")).toThrow("正の整数");
    }
    for (const person_ids of ["1,0", "1,,2", "1,", "1,2.5"]) {
      expect(() => buildManualIntent("interaction", { kind: "chat", occurred_at: "2026-08-27", summary: "要点", person_ids }, "", "scope-a")).toThrow("正の整数");
    }
  });
});
