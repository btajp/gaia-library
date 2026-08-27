import { useEffect, useRef, useState } from "react";

type Props = {
  agentKey: string;
  onClose: () => void;
};

export default function IssuedKey({ agentKey, onClose }: Props) {
  const [copying, setCopying] = useState(false);
  const [copied, setCopied] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const mounted = useRef(false);
  const inFlight = useRef(false);

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);

  async function copyKey() {
    if (inFlight.current) return;
    setError(null);
    if (!navigator.clipboard?.writeText) {
      setError("コピー機能を利用できません。キーを選択してコピーしてください。");
      return;
    }
    inFlight.current = true;
    setCopying(true);
    setCopied(false);
    try {
      await navigator.clipboard.writeText(agentKey);
      if (mounted.current) setCopied(true);
    } catch {
      if (mounted.current) {
        setError("コピーできませんでした。キーを選択してコピーしてください。");
      }
    } finally {
      inFlight.current = false;
      if (mounted.current) setCopying(false);
    }
  }

  return (
    <section aria-labelledby="issued-key-title">
      <h2 id="issued-key-title" className="text-xl font-semibold">
        セットアップが完了しました
      </h2>
      <p id="issued-key-help" className="mt-3 text-sm leading-6 text-neutral-300">
        エージェント「claude-code」の接続キーです。この画面を閉じると再表示できません。
        必要ならコピーし、安全な場所に保管してください。キーの平文はアプリに保存しません。
      </p>
      <label htmlFor="issued-key" className="mt-5 block text-sm font-medium">
        発行したキー（秘密情報）
      </label>
      <textarea
        id="issued-key"
        value={agentKey}
        readOnly
        rows={3}
        spellCheck={false}
        autoComplete="off"
        aria-describedby="issued-key-help"
        onFocus={(event) => event.currentTarget.select()}
        className="mt-2 w-full resize-none rounded-md border border-neutral-700 bg-neutral-950 p-3 font-mono text-sm"
      />
      <p className="mt-2 text-xs leading-5 text-neutral-400">
        コピーしたキーはクリップボードに残ります。共有・送信先に注意してください。
      </p>
      {error && (
        <p role="alert" className="mt-3 text-sm text-red-300">
          {error}
        </p>
      )}
      <p role="status" className="mt-3 min-h-5 text-sm text-neutral-300">
        {copied ? "コピーしました。" : ""}
      </p>
      <div className="mt-4 flex flex-wrap gap-3">
        <button
          type="button"
          onClick={copyKey}
          disabled={copying}
          className="rounded-md border border-neutral-600 px-4 py-2 text-sm hover:bg-neutral-800 disabled:opacity-50"
        >
          {copying ? "コピー中…" : "キーをコピー"}
        </button>
        <button
          type="button"
          onClick={onClose}
          disabled={copying}
          className="rounded-md bg-neutral-100 px-4 py-2 text-sm font-medium text-neutral-950 hover:bg-white disabled:opacity-50"
        >
          閉じてメイン画面へ
        </button>
      </div>
    </section>
  );
}
