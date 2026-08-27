import { useEffect, useRef, useState, type FormEvent } from "react";
import { errorMessage, firstRunSetup } from "../api";

type Props = {
  onComplete: (agentKey: string) => void;
};

export default function FirstRun({ onComplete }: Props) {
  const [affiliation, setAffiliation] = useState("");
  const [userName, setUserName] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const submitting = useRef(false);
  const mounted = useRef(false);

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (submitting.current) return;
    const normalizedAffiliation = affiliation.trim();
    const normalizedUserName = userName.trim();
    if (!normalizedAffiliation || !normalizedUserName) {
      setError("所属元名とユーザー名を入力してください。");
      return;
    }

    submitting.current = true;
    setBusy(true);
    setError(null);
    try {
      const result = await firstRunSetup(
        normalizedAffiliation,
        normalizedUserName,
      );
      if (mounted.current) onComplete(result.agent_key);
    } catch (cause) {
      submitting.current = false;
      if (mounted.current) {
        setError(errorMessage(cause));
        setBusy(false);
      }
    }
  }

  return (
    <section aria-labelledby="setup-title">
      <h2 id="setup-title" className="text-xl font-semibold">
        初回セットアップ
      </h2>
      <p className="mt-2 text-sm leading-6 text-neutral-400">
        記憶を分ける所属元と、このアプリで使うユーザーを登録します。
      </p>
      <form onSubmit={submit} className="mt-6 space-y-5" aria-busy={busy}>
        <div>
          <label htmlFor="affiliation" className="block text-sm font-medium">
            所属元名
          </label>
          <p id="affiliation-help" className="mt-1 text-xs text-neutral-400">
            情報を共有する範囲（scope）の名前です。会社名や個人用の名前を入力します。
          </p>
          <input
            id="affiliation"
            name="affiliation"
            value={affiliation}
            onChange={(event) => setAffiliation(event.target.value)}
            aria-describedby="affiliation-help"
            autoComplete="organization"
            required
            disabled={busy}
            placeholder="例: 個人用"
            className="mt-2 w-full rounded-md border border-neutral-700 bg-neutral-950 px-3 py-2 disabled:opacity-60"
          />
        </div>
        <div>
          <label htmlFor="user-name" className="block text-sm font-medium">
            ユーザー名
          </label>
          <p id="user-name-help" className="mt-1 text-xs text-neutral-400">
            このアプリから提案を承認する、人間のクライアント名です。
          </p>
          <input
            id="user-name"
            name="userName"
            value={userName}
            onChange={(event) => setUserName(event.target.value)}
            aria-describedby="user-name-help"
            autoComplete="username"
            required
            disabled={busy}
            placeholder="例: 自分の名前"
            className="mt-2 w-full rounded-md border border-neutral-700 bg-neutral-950 px-3 py-2 disabled:opacity-60"
          />
        </div>
        {error && (
          <p role="alert" className="break-words text-sm text-red-300">
            {error}
          </p>
        )}
        <button
          type="submit"
          disabled={busy || !affiliation.trim() || !userName.trim()}
          className="w-full rounded-md bg-neutral-100 px-4 py-2 font-medium text-neutral-950 hover:bg-white disabled:cursor-not-allowed disabled:opacity-50"
        >
          {busy ? "セットアップ中…" : "セットアップする"}
        </button>
      </form>
    </section>
  );
}
