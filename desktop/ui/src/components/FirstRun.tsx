import { useEffect, useRef, useState, type FormEvent } from "react";
import { errorMessage, firstRunSetup, type FirstRunResult } from "../api";

type Props = {
  onComplete: (result: FirstRunResult) => void;
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
      if (mounted.current) onComplete(result);
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
        エージェント「claude-code」の接続キーも発行し、Keychain または権限 0600 のファイルへ保管します。
      </p>
      <p className="mt-2 text-sm leading-6 text-neutral-400">
        エージェントは提案まで、承認はあなた（human）だけ、という役割分担で動きます。
      </p>
      <form onSubmit={submit} className="mt-6 space-y-5" aria-busy={busy}>
        <div>
          <label htmlFor="affiliation" className="block text-sm font-medium">
            所属元名
          </label>
          <p id="affiliation-help" className="mt-1 text-xs text-neutral-400">
            最初の機密境界（scope）の名前になります。データはこの境界の中に保存されます。会社名や個人用の名前を入力します。
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
            あなたの human クライアント名（{"desktop:<名前>"}）になります。承認・登録の履歴にこの名前が記録されます。後から設定画面で変更できます。
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
