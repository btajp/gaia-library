import { useRef, useState, type FormEvent } from "react";
import type { ClientInput, ClientRole } from "../../settingsApi";
import { inputClass, primaryClass } from "./SettingsParts";

type Props = { busy: boolean; onSubmit: (input: ClientInput) => Promise<boolean> };

export default function ClientForm({ busy, onSubmit }: Props) {
  const [name, setName] = useState("");
  const [role, setRole] = useState<ClientRole>("agent");
  const [defaultScope, setDefaultScope] = useState("");
  const [generateKey, setGenerateKey] = useState(false);
  const submitting = useRef(false);

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (busy || submitting.current || !name.trim()) return;
    submitting.current = true;
    try {
      if (await onSubmit({ name, role, defaultScope, generateKey })) {
        setName("");
        setDefaultScope("");
        setRole("agent");
        setGenerateKey(false);
      }
    } finally {
      submitting.current = false;
    }
  }

  return (
    <form onSubmit={submit} aria-busy={busy} className="space-y-4 border-t border-neutral-800 pt-4">
      <h4 className="font-medium">クライアントを追加</h4>
      <div>
        <label htmlFor="new-client-name" className="block text-sm font-medium">クライアント名（必須）</label>
        <p id="new-client-name-help" className="mt-1 text-xs leading-5 text-neutral-400">接続元を識別する一意の名前を入力します。例: codex。空欄では追加できません。</p>
        <input id="new-client-name" value={name} onChange={(event) => setName(event.target.value)} required disabled={busy} autoComplete="off" aria-describedby="new-client-name-help" className={inputClass} />
      </div>
      <div>
        <label htmlFor="new-client-role" className="block text-sm font-medium">役割</label>
        <p id="new-client-role-help" className="mt-1 text-xs leading-5 text-neutral-400">agent は検索と提案、human は承認・却下も行えます。エージェントには agent を選びます。</p>
        <select id="new-client-role" value={role} disabled={busy} onChange={(event) => { setRole(event.target.value as ClientRole); setGenerateKey(false); }} aria-describedby="new-client-role-help" className={inputClass}>
          <option value="agent">agent（エージェント）</option>
          <option value="human">human（人間）</option>
        </select>
      </div>
      <div>
        <label htmlFor="new-client-scope" className="block text-sm font-medium">既定 scope（任意）</label>
        <p id="new-client-scope-help" className="mt-1 text-xs leading-5 text-neutral-400">通常使う所属元名を入力します。空欄なら既定値を持たず、各呼び出しで scope の明示が必要です。</p>
        <input id="new-client-scope" value={defaultScope} onChange={(event) => setDefaultScope(event.target.value)} disabled={busy} autoComplete="off" aria-describedby="new-client-scope-help" className={inputClass} />
      </div>
      <div>
        <label htmlFor="new-client-generate-key" className="flex items-center gap-2 text-sm font-medium">
          <input id="new-client-generate-key" type="checkbox" checked={generateKey} onChange={(event) => setGenerateKey(event.target.checked)} disabled={busy} aria-describedby="new-client-key-help" />
          追加と同時に接続キーを発行する
        </label>
        <p id="new-client-key-help" className="mt-1 text-xs leading-5 text-neutral-400">HTTP 接続にはキーが必要です。未選択ならキーは発行せず、後から個別に発行できます。stdio 接続には不要です。</p>
        {role === "human" && <p className="mt-2 text-sm leading-6 text-amber-200">human のキーは承認・却下の権限も持ちます。エージェントへ渡さないでください。</p>}
      </div>
      <button type="submit" disabled={busy || !name.trim()} className={primaryClass}>{busy ? "処理中…" : generateKey ? "クライアントを追加してキーを発行" : "クライアントを追加"}</button>
    </form>
  );
}
