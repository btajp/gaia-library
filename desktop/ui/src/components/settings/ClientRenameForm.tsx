import { useEffect, useRef, useState, type FormEvent } from "react";
import type { ClientSummary } from "../../settingsApi";
import { buttonClass, inputClass, primaryClass } from "./SettingsParts";

type Props = { client: ClientSummary; busy: boolean; onSubmit: (name: string) => Promise<boolean>; cancel: () => void };

export default function ClientRenameForm({ client, busy, onSubmit, cancel }: Props) {
  const [name, setName] = useState(client.name);
  const section = useRef<HTMLElement>(null);
  const submitting = useRef(false);
  useEffect(() => section.current?.focus(), [client.name]);
  const trimmed = name.trim();
  const disabled = busy || !trimmed || trimmed === client.name;

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (disabled || submitting.current) return;
    submitting.current = true;
    try {
      await onSubmit(trimmed);
    } finally {
      submitting.current = false;
    }
  }

  return (
    <section ref={section} tabIndex={-1} aria-labelledby="client-rename-title" className="space-y-3 rounded-md border border-amber-700 p-4">
      <form onSubmit={submit} aria-busy={busy} className="space-y-3">
        <h4 id="client-rename-title" className="break-words font-medium">「{client.name}」の名前を変更</h4>
        <p className="text-sm leading-6 text-neutral-300">役割・既定 scope・接続キーはそのまま引き継ぎます。DB の履歴（提案者・承認者）は旧名のまま残ります。</p>
        <p className="text-sm leading-6 text-amber-200">stdio 接続設定には新しい名前が入るため、配布済みの stdio 接続設定は変更後に出し直してください。HTTP のキーは有効なままです。</p>
        <div>
          <label htmlFor="client-rename-name" className="block text-sm font-medium">新しい名前</label>
          <input id="client-rename-name" value={name} onChange={(event) => setName(event.target.value)} required disabled={busy} autoComplete="off" className={inputClass} />
        </div>
        <div className="flex flex-wrap gap-3">
          <button type="submit" disabled={disabled} className={primaryClass}>{busy ? "変更中…" : "名前を変更"}</button>
          <button type="button" onClick={cancel} disabled={busy} className={buttonClass}>キャンセル</button>
        </div>
      </form>
    </section>
  );
}
