import { useEffect, useState } from "react";
import { errorMessage, isInitialized } from "./api";
import FirstRun from "./components/FirstRun";
import IssuedKey from "./components/IssuedKey";
import MainShell from "./components/MainShell";

type StartupState =
  | { phase: "checking" }
  | { phase: "first-run" }
  | { phase: "issued-key"; agentKey: string }
  | { phase: "ready" }
  | { phase: "error"; message: string };

export default function App() {
  const [startup, setStartup] = useState<StartupState>({ phase: "checking" });
  const [attempt, setAttempt] = useState(0);

  useEffect(() => {
    let active = true;
    setStartup({ phase: "checking" });
    isInitialized()
      .then((initialized) => {
        if (active) {
          setStartup({ phase: initialized ? "ready" : "first-run" });
        }
      })
      .catch((cause: unknown) => {
        if (active) setStartup({ phase: "error", message: errorMessage(cause) });
      });
    return () => {
      active = false;
    };
  }, [attempt]);

  if (startup.phase === "ready") return <MainShell />;

  return (
    <main className="flex min-h-screen items-center justify-center bg-neutral-950 p-6 text-neutral-100">
      <div className="w-full max-w-lg rounded-xl border border-neutral-800 bg-neutral-900 p-7">
        <h1 className="text-2xl font-semibold">gaia-library</h1>
        <p className="mb-8 mt-2 text-sm text-neutral-400">仕事の記憶の索引</p>
        {startup.phase === "checking" && (
          <p role="status" className="text-sm text-neutral-300">
            起動状態を確認しています…
          </p>
        )}
        {startup.phase === "error" && (
          <section aria-labelledby="startup-error-title">
            <h2 id="startup-error-title" className="text-lg font-semibold">
              アプリを起動できませんでした
            </h2>
            <p role="alert" className="mt-3 break-words text-sm text-red-300">
              {startup.message}
            </p>
            <p className="mt-4 text-sm leading-6 text-neutral-400">
              設定やファイルへのアクセス権を確認して再試行してください。
              改善しない場合は、アプリを終了して起動し直してください。
            </p>
            <button
              type="button"
              onClick={() => setAttempt((value) => value + 1)}
              className="mt-5 rounded-md bg-neutral-100 px-4 py-2 text-sm font-medium text-neutral-950 hover:bg-white"
            >
              再試行
            </button>
          </section>
        )}
        {startup.phase === "first-run" && (
          <FirstRun
            onComplete={(agentKey) => setStartup({ phase: "issued-key", agentKey })}
          />
        )}
        {startup.phase === "issued-key" && (
          <IssuedKey
            agentKey={startup.agentKey}
            onClose={() => setStartup({ phase: "ready" })}
          />
        )}
      </div>
    </main>
  );
}
