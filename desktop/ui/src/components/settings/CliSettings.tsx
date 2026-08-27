import { useCallback, useEffect, useState } from "react";
import { errorMessage } from "../../api";
import { useLatestRequest } from "../../hooks/useLatestRequest";
import { cliLinkCreate, cliLinkStatus, type CliLinkStatus } from "../../settingsApi";
import { buttonClass, primaryClass, ReloadButton, SettingsError, SettingsSection } from "./SettingsParts";
import { useSettingsAction } from "./useSettingsState";
import { beginCliLinkConfirmation, cliLinkIntent, type CliLinkConfirmation, type CliLinkIntent } from "./cliLinkIntent";

type ControlsProps = {
  status: CliLinkStatus;
  busy: boolean;
  blocked: boolean;
  intent: CliLinkIntent | null;
  begin: () => void;
  create: () => void;
  cancel: () => void;
};

export function CliLinkControls({ status, busy, blocked, intent, begin, create, cancel }: ControlsProps) {
  switch (status.status) {
    case "ok":
      return <p className="text-sm leading-6 text-emerald-300">同梱 CLI へのリンクが設定されています。アプリの更新で同梱 CLI も更新されます。</p>;
    case "not_symlink":
      return <p role="alert" className="text-sm leading-6 text-amber-200">配置先には通常ファイル等があります。この画面では上書きしません。既存の内容を確認し、必要に応じて別の場所へ移す対応が必要です。</p>;
    case "missing":
      return (
        <div className="space-y-3">
          <p className="text-sm text-neutral-400">CLI リンクは未作成です。</p>
          <button type="button" onClick={create} disabled={busy || blocked || !intent || intent.expectedTarget !== null} className={primaryClass}>{busy ? "作成中…" : "~/.local/bin/gaia にリンクを作成"}</button>
        </div>
      );
    case "wrong_target":
      return (
        <div className="space-y-3">
          <p className="text-sm text-amber-200">別のリンク先が設定されています。</p>
          <p className="break-all text-xs text-neutral-300">現在のリンク先: {status.current}</p>
          {intent?.expectedTarget === status.current ? (
            <section aria-labelledby="cli-confirmation-title" className="space-y-3 rounded-md border border-amber-700 p-4">
              <h4 id="cli-confirmation-title" className="font-medium">同梱 CLI へのリンクに変更しますか</h4>
              <p className="text-sm leading-6 text-amber-200">現在のリンクを置き換えます。以降、この場所の gaia は現在のアプリに同梱した CLI を起動します。リンク先のファイル自体は削除しません。</p>
              <div className="flex flex-wrap gap-3">
                <button type="button" onClick={create} disabled={busy || blocked} className={primaryClass}>{busy ? "変更中…" : "確認してリンクを変更"}</button>
                <button type="button" onClick={cancel} disabled={busy} className={buttonClass}>キャンセル</button>
              </div>
            </section>
          ) : <button type="button" onClick={begin} disabled={busy || blocked} className={buttonClass}>リンク先の変更を確認…</button>}
        </div>
      );
  }
}

export function CliLinkFailure({ error, needsRefresh }: { error: unknown; needsRefresh: boolean }) {
  return <SettingsError>前回のリンク設定を完了できませんでした。{errorMessage(error)} {needsRefresh ? "「再読込」で現在の状態を取得してから、リンク先を確認し直してください。" : "再取得した状態を確認してから操作してください。"}</SettingsError>;
}

export default function CliSettings() {
  const { request, snapshot: status } = useLatestRequest<CliLinkStatus>();
  const { action, snapshot, busy } = useSettingsAction<null>();
  const [confirmation, setConfirmation] = useState<CliLinkConfirmation | null>(null);
  const [needsRefresh, setNeedsRefresh] = useState(false);
  const loading = status.status === "idle" || status.status === "loading";
  const intent = needsRefresh ? null : cliLinkIntent(status, confirmation);

  const refresh = useCallback(() => {
    if (action.getSnapshot().status === "working") return;
    setConfirmation(null);
    setNeedsRefresh(false);
    void request.run("cli-link", cliLinkStatus);
  }, [action, request]);

  useEffect(() => {
    refresh();
    return request.reset;
  }, [refresh, request]);

  useEffect(() => setConfirmation(null), [status]);

  function beginConfirmation() {
    if (busy || needsRefresh) return;
    setConfirmation(beginCliLinkConfirmation(request.getSnapshot()));
  }

  async function create() {
    if (needsRefresh) return;
    const currentIntent = cliLinkIntent(request.getSnapshot(), confirmation);
    if (!currentIntent) return;
    const result = await action.run(() => cliLinkCreate(currentIntent.expectedTarget));
    if (result) {
      setConfirmation(null);
      refresh();
    } else if (action.getSnapshot().status === "error") {
      setConfirmation(null);
      setNeedsRefresh(true);
    }
  }

  return (
    <SettingsSection id="settings-cli" title="同梱 CLI">
      <p className="text-sm leading-6 text-neutral-400">ターミナルから同梱 CLI を使うため、~/.local/bin/gaia にシンボリックリンクを作成します。自動では作成しません。</p>
      <ReloadButton loading={loading || busy} refresh={refresh} />
      {status.status === "error" && <SettingsError>{snapshot.status === "success" ? "リンクの設定は完了しましたが、状態の再取得に失敗しました。" : "CLI リンクの状態を取得できませんでした。"} {errorMessage(status.error)}</SettingsError>}
      {status.status === "success" && status.data && <CliLinkControls status={status.data} busy={busy} blocked={needsRefresh} intent={intent} begin={beginConfirmation} create={() => void create()} cancel={() => setConfirmation(null)} />}
      {snapshot.status === "error" && <CliLinkFailure error={snapshot.error} needsRefresh={needsRefresh} />}
      {snapshot.status === "success" && <p role="status" className="text-sm text-emerald-300">同梱 CLI へのリンクを設定しました。</p>}
      <p className="text-xs leading-5 text-neutral-400">PATH の自動変更は行いません。~/.local/bin が PATH に含まれる環境で gaia を実行できます。</p>
    </SettingsSection>
  );
}
