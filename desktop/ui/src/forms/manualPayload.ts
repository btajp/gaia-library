import { explicitScope } from "../proposalApi";
import type { ManualIntent, ManualTarget } from "../proposalTypes";
import type { Kind } from "../types";
import { MANUAL_FORMS, type ManualField } from "./manualFields";

export type FormValues = Record<string, string>;

export function positiveId(value: string, label: string): number {
  const normalized = value.trim();
  const id = Number(normalized);
  if (!/^[0-9]+$/.test(normalized) || !Number.isSafeInteger(id) || id <= 0) {
    throw new Error(`${label} は正の整数で入力してください。`);
  }
  return id;
}

function parseValue(field: ManualField, value: string): unknown {
  switch (field.kind) {
    case "id": return positiveId(value, field.label);
    case "select":
      if (!field.options?.includes(value)) throw new Error(`${field.label} を選択肢から選んでください。`);
      return value;
    case "aliases": return value.split(/\r?\n/).map((alias) => alias.trim()).filter(Boolean).map((alias) => ({ alias }));
    case "ids": return value.split(/[,\n]/).map((item) => positiveId(item, field.label));
    case "members": {
      const seen = new Set<number>();
      return value.split(/\r?\n/).filter((line) => line.trim()).map((line) => {
        const [id, ...rest] = line.split(",");
        const personId = positiveId(id, field.label);
        if (seen.has(personId)) throw new Error("関係者の人物 ID が重複しています。");
        seen.add(personId);
        const role = rest.join(",").trim();
        return { person_id: personId, ...(role ? { role } : {}) };
      });
    }
    default: return value;
  }
}

export function buildManualIntent(target: ManualTarget, values: FormValues, factKind: string, scope: string): ManualIntent {
  if (!Object.hasOwn(MANUAL_FORMS, target)) throw new Error("追加する種別を選んでください。");
  const form = MANUAL_FORMS[target];
  if (!form) throw new Error("追加する種別を選んでください。");
  const patch: Record<string, unknown> = {};
  for (const field of form.fields) {
    const value = (values[field.key] ?? "").trim();
    if (!value) {
      if (field.required) throw new Error(`${field.label} を入力してください。`);
      continue;
    }
    patch[field.key] = parseValue(field, value);
  }
  let kind: Kind = "fact";
  if (target === "fact") {
    if (factKind !== "fact" && factKind !== "inference") throw new Error("fact の kind（事実 / 推測）を選んでください。");
    kind = factKind;
    if (patch.predicate && !patch.value) throw new Error("predicate を指定した場合は value を入力してください。");
  }
  return { target_type: target, action: "insert", patch, kind, scope: explicitScope(scope) };
}

export function valuesFromIntent(intent: ManualIntent): FormValues {
  const result: FormValues = {};
  for (const field of MANUAL_FORMS[intent.target_type].fields) {
    const value = intent.patch[field.key];
    if (value === undefined) continue;
    if (field.kind === "aliases" && Array.isArray(value)) result[field.key] = value.map((alias) => alias.alias).join("\n");
    else if (field.kind === "ids" && Array.isArray(value)) result[field.key] = value.join(", ");
    else if (field.kind === "members" && Array.isArray(value)) result[field.key] = value.map((member) => `${member.person_id}${member.role ? `,${member.role}` : ""}`).join("\n");
    else result[field.key] = String(value);
  }
  return result;
}
