import { useEffect, useRef, useState } from "react";
import { errorMessage } from "../../api";
import { useLatestRequest } from "../../hooks/useLatestRequest";
import { snapshotForKey } from "../../lib/latestRequest";
import { adminClientAdd, adminClientKeygen, adminClientList, type ClientInput, type ClientSummary, type ConnectionSnippet, type IssuedClientKey, type Transport } from "../../settingsApi";
import ClientForm from "./ClientForm";
import { IssuedClientSecret, SnippetPanel } from "./ClientSecrets";
import { buttonClass, primaryClass, ReloadButton, SettingsError, SettingsSection } from "./SettingsParts";
import { useSettingsAction, useSettingsResource } from "./useSettingsState";
import { requestSnippet, snippetKey } from "./snippetRequest";

type ClientChange = { name: string; issued: IssuedClientKey | null };
type SnippetTarget = { name: string; transport: Transport };

export function KeyConfirmation({ client, busy, confirm, cancel }: { client: ClientSummary; busy: boolean; confirm: () => void; cancel: () => void }) {
  const section = useRef<HTMLElement>(null);
  useEffect(() => section.current?.focus(), [client.name]);
  return (
    <section ref={section} tabIndex={-1} aria-labelledby="key-confirmation-title" className="space-y-3 rounded-md border border-amber-700 p-4">
      <h4 id="key-confirmation-title" className="break-words font-medium">「{client.name}」のキーを{client.has_key ? "再発行" : "発行"}しますか</h4>
      <p className="text-sm leading-6 text-amber-200">既存のキーがある場合は失効し、HTTP の次のリクエストから使えなくなります。接続中のクライアントには新しい接続設定が必要です。</p>
      {client.role === "human" && <p className="text-sm leading-6 text-amber-200">human のキーには承認・却下の権限があります。エージェントへ渡さないでください。</p>}
      <p className="text-xs leading-5 text-neutral-400">新しいキーは Keychain に保管し、利用できない場合は権限 0600 のファイルへ保管します。</p>
      <div className="flex flex-wrap gap-3">
        <button type="button" onClick={confirm} disabled={busy} className={primaryClass}>{busy ? "発行中…" : "確認してキーを発行"}</button>
        <button type="button" onClick={cancel} disabled={busy} className={buttonClass}>キャンセル</button>
      </div>
    </section>
  );
}

export default function ClientsSettings() {
  const { snapshot: list, refresh, loading } = useSettingsResource(adminClientList);
  const { action, snapshot, busy } = useSettingsAction<ClientChange>();
  const { request: snippetRequest, snapshot: snippetSnapshot } = useLatestRequest<ConnectionSnippet>();
  const [confirmation, setConfirmation] = useState<ClientSummary | null>(null);
  const [snippet, setSnippet] = useState<SnippetTarget | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  useEffect(() => snippetRequest.reset, [snippetRequest]);

  function closeSnippet() {
    snippetRequest.reset();
    setSnippet(null);
  }

  async function addClient(input: ClientInput) {
    closeSnippet();
    setConfirmation(null);
    setNotice(null);
    const result = await action.run(async () => ({ name: input.name.trim(), issued: await adminClientAdd(input) }));
    if (!result) return false;
    setNotice(`クライアント「${result.data.name}」を追加しました。`);
    void refresh();
    return true;
  }

  async function issueKey() {
    if (!confirmation) return;
    const name = confirmation.name;
    closeSnippet();
    setNotice(null);
    const result = await action.run(async () => ({ name, issued: await adminClientKeygen(name) }));
    if (result) {
      setConfirmation(null);
      setNotice(`「${name}」のキーを発行しました。`);
      void refresh();
    }
  }

  function beginConfirmation(client: ClientSummary) {
    if (busy) return;
    if (snapshot.status !== "success") action.reset();
    setNotice(null);
    closeSnippet();
    setConfirmation(client);
  }

  function showSnippet(name: string, transport: Transport) {
    if (busy) return;
    if (snapshot.status !== "success") action.reset();
    setConfirmation(null);
    setSnippet({ name, transport });
    void requestSnippet(snippetRequest, name, transport);
  }

  return (
    <SettingsSection id="settings-clients" title="クライアントと接続キー">
      <p className="text-sm leading-6 text-neutral-400">一覧にはキーの有無だけを表示します。Keychain の読み出しや秘密情報の表示は、キー発行・接続設定の表示操作時に行います。</p>
      <ReloadButton loading={loading} refresh={() => void refresh()} />
      {list.status === "error" && <SettingsError>{notice ? "変更は完了しましたが、一覧の再取得に失敗しました。" : "クライアント一覧を取得できませんでした。"} {errorMessage(list.error)}</SettingsError>}
      {list.status === "success" && list.data && (
        list.data.length === 0 ? <p className="text-sm text-neutral-400">クライアントは登録されていません。</p> : (
          <ul className="space-y-3">
            {list.data.map((client) => (
              <li key={client.name} className="space-y-3 rounded-md border border-neutral-800 p-4">
                <div>
                  <p className="break-words text-sm font-semibold">{client.name}</p>
                  <p className="mt-1 break-words text-xs leading-5 text-neutral-400">役割: {client.role} / 既定 scope: {client.default_scope ?? "なし（呼び出し時に明示）"} / キー: {client.has_key ? "発行済み" : "未発行"}</p>
                </div>
                <div className="flex flex-wrap gap-2">
                  <button type="button" disabled={busy} onClick={() => beginConfirmation(client)} className={buttonClass}>{client.has_key ? "キーを再発行…" : "キーを発行…"}</button>
                  <button type="button" disabled={busy} onClick={() => showSnippet(client.name, "http")} className={buttonClass}>HTTP 接続設定を表示</button>
                  <button type="button" disabled={busy} onClick={() => showSnippet(client.name, "stdio")} className={buttonClass}>stdio 接続設定を表示</button>
                </div>
              </li>
            ))}
          </ul>
        )
      )}
      {notice && <p role="status" className="break-words text-sm text-emerald-300">{notice}</p>}
      {snapshot.status === "error" && <SettingsError>変更を完了できませんでした。{errorMessage(snapshot.error)}</SettingsError>}
      {confirmation && <KeyConfirmation client={confirmation} busy={busy} confirm={() => void issueKey()} cancel={() => setConfirmation(null)} />}
      {snapshot.status === "success" && snapshot.data?.issued && <IssuedClientSecret name={snapshot.data.name} issued={snapshot.data.issued} onClose={action.reset} />}
      {snippet && <SnippetPanel key={snippetKey(snippet.name, snippet.transport)} {...snippet} snapshot={snapshotForKey(snippetSnapshot, snippetKey(snippet.name, snippet.transport))} refresh={() => void requestSnippet(snippetRequest, snippet.name, snippet.transport)} onClose={closeSnippet} />}
      <ClientForm busy={busy} onSubmit={addClient} />
    </SettingsSection>
  );
}
