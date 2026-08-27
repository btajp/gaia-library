import { errorMessage, type ServerStatus } from "../../api";
import { useServerStatus } from "../../hooks/useServerStatus";
import { appVersion, checkUpdates } from "../../settingsApi";
import { buttonClass, ReloadButton, SettingsError, SettingsSection } from "./SettingsParts";
import { useSettingsAction, useSettingsResource } from "./useSettingsState";

export function ServerStatusContent({ status, error, loading }: { status: ServerStatus | null; error: string | null; loading: boolean }) {
  if (error) return <SettingsError>現在のサーバー状態を取得できませんでした。{error}</SettingsError>;
  if (!status) return <p role="status" className="text-sm text-neutral-400">{loading ? "サーバー状態を確認しています…" : "サーバー状態は未取得です。"}</p>;
  return (
    <div className="space-y-2 text-sm">
      {status.url ? <p className="break-all text-emerald-300">HTTP URL: {status.url}</p> : (
        <p className="text-neutral-300">HTTP サーバーは起動していません。{status.error ? "" : "停止理由は取得できていません。"}</p>
      )}
      {status.error && <SettingsError>{status.error}</SettingsError>}
      <p className="break-words text-xs text-neutral-400">アプリのクライアント: {status.client ?? "未取得"} / 既定 scope: {status.default_scope ?? "なし"}</p>
    </div>
  );
}

export function ServerSettings() {
  const { status, error, loading, refresh } = useServerStatus();
  return (
    <SettingsSection id="settings-server" title="HTTP サーバー">
      <ServerStatusContent status={status} error={error} loading={loading} />
      <p className="text-xs leading-5 text-neutral-400">アプリが実際に使用している URL を表示します。15 秒ごとに状態を確認します。</p>
      <ReloadButton loading={loading} refresh={refresh} />
    </SettingsSection>
  );
}

export function VersionSettings() {
  const { snapshot, refresh, loading } = useSettingsResource(appVersion);
  const update = useSettingsAction<void>();
  return (
    <SettingsSection id="settings-version" title="バージョン">
      {snapshot.status === "success" && <p className="text-sm text-neutral-300">gaia-library {snapshot.data}</p>}
      {snapshot.status === "error" && <SettingsError>バージョンを取得できませんでした。{errorMessage(snapshot.error)}</SettingsError>}
      <ReloadButton loading={loading} refresh={() => void refresh()} />
      <div className="space-y-3 border-t border-neutral-800 pt-4">
        <button type="button" disabled={update.busy} onClick={() => void update.action.run(checkUpdates)} className={buttonClass}>{update.busy ? "確認を開始中…" : "アップデートを確認…"}</button>
        <p className="text-xs leading-5 text-neutral-400">確認結果はアプリのダイアログに表示します。更新の適用と再起動は、その画面で選択できます。進行状況はアプリメニューにも表示します。</p>
        {update.snapshot.status === "error" && <SettingsError>更新の確認を開始できませんでした。{errorMessage(update.snapshot.error)}</SettingsError>}
      </div>
    </SettingsSection>
  );
}
