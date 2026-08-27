import { useEffect, useId, useRef, type ReactNode } from "react";
import { errorMessage } from "../../api";
import type { RequestSnapshot } from "../../lib/latestRequest";
import type { ConnectionSnippet, IssuedClientKey, Transport } from "../../settingsApi";
import KeyStorageNotice from "../KeyStorageNotice";
import { buttonClass, SettingsError } from "./SettingsParts";
import { useSettingsAction } from "./useSettingsState";

function SecretText({ value, label, onClose }: { value: string; label: string; onClose: () => void }) {
  const id = useId();
  const { action, snapshot, busy } = useSettingsAction<void>();

  function copy() {
    void action.run(async () => {
      if (!navigator.clipboard?.writeText) throw new Error("コピー機能を利用できません。内容を選択してコピーしてください。");
      try {
        await navigator.clipboard.writeText(value);
      } catch {
        throw new Error("コピーできませんでした。内容を選択してコピーしてください。");
      }
    });
  }

  return (
    <div className="space-y-3">
      <label htmlFor={id} className="block text-sm font-medium">{label}</label>
      <textarea id={id} value={value} readOnly rows={Math.min(12, Math.max(3, value.split("\n").length))} autoComplete="off" spellCheck={false} onFocus={(event) => event.currentTarget.select()} className="w-full resize-y rounded-md border border-neutral-600 bg-neutral-950 p-3 font-mono text-xs" />
      <p className="text-xs leading-5 text-neutral-400">コピーした内容はクリップボードに残ります。共有・送信先に注意してください。閉じると表示中の内容を画面の状態から破棄します。</p>
      {snapshot.status === "error" && <SettingsError>{errorMessage(snapshot.error)}</SettingsError>}
      {snapshot.status === "success" && <p role="status" className="text-sm text-emerald-300">コピーしました。</p>}
      <div className="flex flex-wrap gap-3">
        <button type="button" onClick={copy} disabled={busy} className={buttonClass}>{busy ? "コピー中…" : "コピー"}</button>
        <button type="button" onClick={onClose} disabled={busy} className={buttonClass}>閉じる（表示内容を破棄）</button>
      </div>
    </div>
  );
}

function SecretCard({ title, children }: { title: string; children: ReactNode }) {
  const id = useId();
  const section = useRef<HTMLElement>(null);
  useEffect(() => section.current?.focus(), [title]);
  return <section ref={section} tabIndex={-1} aria-labelledby={id} className="space-y-4 rounded-md border border-amber-700 bg-neutral-950 p-4"><h4 id={id} className="break-words font-medium">{title}</h4>{children}</section>;
}

export function IssuedClientSecret({ name, issued, onClose }: { name: string; issued: IssuedClientKey; onClose: () => void }) {
  return (
    <SecretCard title={`${name} のキーを発行しました`}>
      <KeyStorageNotice storage={issued.storage} />
      <SecretText value={issued.key} label="発行したキー（秘密情報）" onClose={onClose} />
    </SecretCard>
  );
}

type SnippetProps = {
  name: string;
  transport: Transport;
  snapshot: RequestSnapshot<ConnectionSnippet> | null;
  refresh: () => void;
  onClose: () => void;
};

export function SnippetPanel({ name, transport, snapshot, refresh, onClose }: SnippetProps) {
  const loading = !snapshot || snapshot.status === "idle" || snapshot.status === "loading";
  return (
    <SecretCard title={`${name} の ${transport === "http" ? "HTTP" : "stdio"} 接続設定`}>
      {loading && <p role="status" className="text-sm text-neutral-400">接続設定を取得しています…</p>}
      {snapshot?.status === "error" && <SettingsError>{errorMessage(snapshot.error)}</SettingsError>}
      {snapshot?.status === "success" && snapshot.data ? (
        <>
          {transport === "http" ? (
            <p className="text-xs leading-5 text-amber-200">この接続設定には秘密のキーが含まれます。保管場所: {snapshot.data.key_storage === "keychain" ? "macOS Keychain" : snapshot.data.key_storage === "file" ? "権限 0600 のローカルファイル" : "不明"}。</p>
          ) : <p className="text-xs leading-5 text-neutral-400">stdio は接続キーを使いません。設定には、このアプリに同梱した CLI と設定・DB のパスが含まれます。</p>}
          <SecretText value={snapshot.data.text} label={transport === "http" ? "MCP 接続設定（秘密情報）" : "MCP 接続設定"} onClose={onClose} />
        </>
      ) : (
        <div className="flex flex-wrap gap-3">
          {snapshot?.status === "error" && <button type="button" onClick={refresh} className={buttonClass}>再試行</button>}
          <button type="button" onClick={onClose} className={buttonClass}>閉じる</button>
        </div>
      )}
    </SecretCard>
  );
}
