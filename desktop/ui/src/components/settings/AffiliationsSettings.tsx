import { useState, type FormEvent } from "react";
import { errorMessage } from "../../api";
import { adminAffiliationAdd, adminAffiliationList } from "../../settingsApi";
import { inputClass, primaryClass, ReloadButton, SettingsError, SettingsSection } from "./SettingsParts";
import { useSettingsAction, useSettingsResource } from "./useSettingsState";

export default function AffiliationsSettings() {
  const { snapshot: list, refresh, loading } = useSettingsResource(adminAffiliationList);
  const { action, snapshot, busy } = useSettingsAction<number>();
  const [name, setName] = useState("");
  const [identity, setIdentity] = useState("");

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!name.trim()) return;
    const result = await action.run(() => adminAffiliationAdd(name, identity));
    if (result) {
      setName("");
      setIdentity("");
      void refresh();
    }
  }

  return (
    <SettingsSection id="settings-affiliations" title="所属元">
      <p className="text-sm leading-6 text-neutral-400">情報を区切る scope の名前を管理します。追加操作は監査ログに記録されます。</p>
      <ReloadButton loading={loading} refresh={() => void refresh()} />
      {list.status === "error" && (
        <SettingsError>{snapshot.status === "success" ? "追加は完了しましたが、一覧の再取得に失敗しました。" : "所属元一覧を取得できませんでした。"} {errorMessage(list.error)}</SettingsError>
      )}
      {list.status === "success" && list.data && (
        list.data.length === 0 ? <p className="text-sm text-neutral-400">所属元は登録されていません。</p> : (
          <ul className="divide-y divide-neutral-800 rounded-md border border-neutral-800">
            {list.data.map((item) => (
              <li key={item.id} className="p-3">
                <p className="break-words text-sm font-medium">{item.name}</p>
                <p className="mt-1 break-words text-xs text-neutral-400">識別情報: {item.identity ?? "未設定"}</p>
              </li>
            ))}
          </ul>
        )
      )}
      <form onSubmit={submit} aria-busy={busy} className="space-y-4 border-t border-neutral-800 pt-4">
        <div>
          <label htmlFor="new-affiliation-name" className="block text-sm font-medium">追加する所属元名（必須）</label>
          <p id="new-affiliation-name-help" className="mt-1 text-xs leading-5 text-neutral-400">scope として使う名前を入力します。例: 会社名、個人用。空欄では追加できません。</p>
          <input id="new-affiliation-name" value={name} onChange={(event) => setName(event.target.value)} required disabled={busy} autoComplete="off" aria-describedby="new-affiliation-name-help" className={inputClass} />
        </div>
        <div>
          <label htmlFor="new-affiliation-identity" className="block text-sm font-medium">識別情報（任意）</label>
          <p id="new-affiliation-identity-help" className="mt-1 text-xs leading-5 text-neutral-400">所属元を区別する補足情報を入力します。不要なら空欄で構いません。</p>
          <input id="new-affiliation-identity" value={identity} onChange={(event) => setIdentity(event.target.value)} disabled={busy} autoComplete="off" aria-describedby="new-affiliation-identity-help" className={inputClass} />
        </div>
        {snapshot.status === "error" && <SettingsError>所属元を追加できませんでした。{errorMessage(snapshot.error)}</SettingsError>}
        {snapshot.status === "success" && <p role="status" className="text-sm text-emerald-300">所属元を追加しました。</p>}
        <button type="submit" disabled={busy || !name.trim()} className={primaryClass}>{busy ? "追加中…" : "所属元を追加"}</button>
      </form>
    </SettingsSection>
  );
}
