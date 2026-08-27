import type { ManualField as Field } from "../forms/manualFields";

type Props = { field: Field; value: string; disabled: boolean; onChange: (value: string) => void };

export default function ManualField({ field, value, disabled, onChange }: Props) {
  const id = `manual-${field.key}`;
  const className = "mt-2 w-full rounded-md border border-neutral-700 bg-neutral-950 px-3 py-2 text-sm disabled:opacity-50";
  const common = { id, value, disabled, required: field.required, "aria-describedby": `${id}-help`, className };
  return (
    <div>
      <label htmlFor={id} className="block text-sm font-medium">{field.label}{field.required ? "（必須）" : "（任意）"}</label>
      {field.kind === "select" ? (
        <select {...common} onChange={(event) => onChange(event.target.value)}>
          <option value="">{field.required ? "選択してください" : "未指定"}</option>
          {field.options?.map((option) => <option key={option} value={option}>{option}</option>)}
        </select>
      ) : ["textarea", "aliases", "ids", "members"].includes(field.kind ?? "") ? (
        <textarea {...common} rows={3} placeholder={field.placeholder} onChange={(event) => onChange(event.target.value)} />
      ) : (
        <input {...common} type="text" inputMode={field.kind === "id" ? "numeric" : undefined} autoComplete="off" placeholder={field.placeholder} onChange={(event) => onChange(event.target.value)} />
      )}
      <p id={`${id}-help`} className="mt-1 text-xs leading-5 text-neutral-400">
        {field.help ?? (field.kind === "id" ? "登録済みの対象を正の整数 ID で指定します。" : field.required ? "空白だけの値は使えません。" : "空欄の場合は送信しません。")}
      </p>
    </div>
  );
}
