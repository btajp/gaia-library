import type { ManualTarget } from "../proposalTypes";

export type FieldKind = "text" | "textarea" | "id" | "aliases" | "ids" | "members" | "select";
export type ManualField = {
  key: string;
  label: string;
  kind?: FieldKind;
  required?: boolean;
  options?: readonly string[];
  help?: string;
  placeholder?: string;
};
export type ManualForm = { label: string; fields: readonly ManualField[] };

export const ENTITY_TYPES = ["person", "organization", "engagement", "interaction", "entity"] as const;
export const REF_TARGET_TYPES = [...ENTITY_TYPES, "fact"] as const;
export const PREDICATES = ["role", "status", "interest", "decision"] as const;

const optionalDate = (key: string, label: string): ManualField => ({ key, label, help: "任意。日付・日時は入力した文字列のまま保存します。", placeholder: "例: 2026-08-27" });

export const MANUAL_FORMS: Record<ManualTarget, ManualForm> = {
  person: {
    label: "人物",
    fields: [
      { key: "name", label: "氏名", required: true },
      { key: "org_id", label: "所属組織 ID", kind: "id", help: "任意。登録済みの組織を ID で指定します。" },
      { key: "role", label: "役職" },
      { key: "aliases", label: "別名", kind: "aliases", help: "任意。1 行に 1 つ入力します。空行は除外し、別名種別は指定しません。" },
      optionalDate("first_met", "初対面の日付"),
      optionalDate("last_seen", "最終接点の日付"),
    ],
  },
  organization: {
    label: "組織",
    fields: [
      { key: "name", label: "組織名", required: true },
      { key: "kind", label: "組織の種別", help: "任意の名前です。例: customer / partner / affiliation" },
    ],
  },
  engagement: {
    label: "案件",
    fields: [
      { key: "name", label: "案件名", required: true },
      { key: "org_id", label: "相手組織 ID", kind: "id" },
      { key: "status", label: "状態", help: "任意の名前です。既存の運用に合わせて入力します。" },
      optionalDate("started_at", "開始日"),
      optionalDate("ended_at", "終了日"),
      { key: "people", label: "関係者", kind: "members", help: "任意。1 行に「人物 ID,案件での役割」を入力します。役割は省略できます。", placeholder: "1,contact\n2" },
    ],
  },
  fact: {
    label: "fact（事実・推測）",
    fields: [
      { key: "entity_type", label: "対象種別", kind: "select", options: ENTITY_TYPES, required: true },
      { key: "entity_id", label: "対象 ID", kind: "id", required: true },
      { key: "statement", label: "内容", kind: "textarea", required: true },
      { key: "predicate", label: "構造化項目（predicate）", kind: "select", options: PREDICATES, help: "任意。登録済みの 4 種類から選びます。指定時は value が必須です。" },
      { key: "value", label: "構造化値（value）" },
      optionalDate("valid_from", "有効開始日"),
    ],
  },
  ref: {
    label: "参照",
    fields: [
      { key: "target_type", label: "紐付け先の種別", kind: "select", options: REF_TARGET_TYPES, required: true },
      { key: "target_id", label: "紐付け先 ID", kind: "id", required: true },
      { key: "system", label: "システム", required: true, help: "例: notion / box / minutes / file。URI は自動では開きません。" },
      { key: "uri", label: "URI", required: true },
      { key: "title", label: "タイトル" },
      { key: "note", label: "注記", kind: "textarea", required: true, help: "何が・どの粒度で・いつ時点の情報なのかを入力します。URI だけでは登録できません。" },
      { key: "snapshot", label: "要点スナップショット", kind: "textarea", help: "任意。参照先に到達できない場合にも使える要点です。" },
      optionalDate("last_verified", "最終確認日"),
    ],
  },
  glossary: {
    label: "用語",
    fields: [
      { key: "term", label: "用語", required: true },
      { key: "reading", label: "読み" },
      { key: "definition", label: "定義", kind: "textarea" },
      { key: "engagement_id", label: "案件 ID", kind: "id" },
    ],
  },
  interaction: {
    label: "やり取り",
    fields: [
      { key: "kind", label: "やり取りの種別", required: true, help: "任意の名前です。例: meeting / call / chat / mail" },
      { key: "occurred_at", label: "発生日時（ISO 8601）", required: true, placeholder: "例: 2026-08-27T10:00:00+09:00" },
      { key: "summary", label: "要点", kind: "textarea", required: true },
      { key: "engagement_id", label: "案件 ID", kind: "id" },
      { key: "person_ids", label: "関係する人物 ID", kind: "ids", help: "任意。正の整数をカンマまたは改行で区切ります。" },
    ],
  },
};
