import { useEffect, useRef, useState, type KeyboardEvent } from "react";
import { useServerStatus } from "../hooks/useServerStatus";

const TABS = [
  { id: "search", label: "検索" },
  { id: "proposals", label: "提案" },
  { id: "add", label: "追加" },
  { id: "settings", label: "設定" },
] as const;

export type WorkspaceTab = (typeof TABS)[number]["id"];

type WorkspaceProps = {
  tab: WorkspaceTab;
  scope: string;
};

function Workspace({ tab, scope }: WorkspaceProps) {
  const label = TABS.find((item) => item.id === tab)?.label;
  return (
    <section
      id={`panel-${tab}`}
      role="tabpanel"
      aria-labelledby={`tab-${tab}`}
      tabIndex={0}
      className="rounded-lg border border-neutral-800 bg-neutral-900 p-6"
    >
      <h2 className="text-xl font-semibold">{label}</h2>
      <p className="mt-3 text-sm text-neutral-400">この画面は準備中です。</p>
      <p className="mt-2 text-sm text-neutral-400">
        対象 scope: {scope.trim() || "クライアントの既定値"}
      </p>
    </section>
  );
}

export default function MainShell() {
  const { status, error, loading, refresh } = useServerStatus();
  const [tab, setTab] = useState<WorkspaceTab>("search");
  const [scope, setScope] = useState("");
  const scopeEdited = useRef(false);
  const tabButtons = useRef<Array<HTMLButtonElement | null>>([]);

  useEffect(() => {
    if (!scopeEdited.current && status?.default_scope != null) {
      setScope(status.default_scope);
    }
  }, [status?.default_scope]);

  function moveTab(event: KeyboardEvent<HTMLButtonElement>, index: number) {
    let next: number;
    switch (event.key) {
      case "ArrowRight":
        next = (index + 1) % TABS.length;
        break;
      case "ArrowLeft":
        next = (index + TABS.length - 1) % TABS.length;
        break;
      case "Home":
        next = 0;
        break;
      case "End":
        next = TABS.length - 1;
        break;
      default:
        return;
    }
    event.preventDefault();
    setTab(TABS[next].id);
    tabButtons.current[next]?.focus();
  }

  const statusError = error ?? status?.error;

  return (
    <main className="min-h-screen bg-neutral-950 text-neutral-100">
      <header className="border-b border-neutral-800 px-6 py-5">
        <div className="mx-auto flex max-w-6xl flex-wrap items-start justify-between gap-4">
          <div>
            <h1 className="text-xl font-semibold">gaia-library</h1>
            <p className="mt-1 text-sm text-neutral-400">
              クライアント: {status?.client ?? "確認中"}
            </p>
          </div>
          <div className="flex max-w-full items-start gap-3">
            <div className="min-w-0 text-sm" aria-live="polite">
              {statusError ? (
                <p role="alert" className="max-w-xl break-words text-red-300">
                  HTTP: {statusError}
                </p>
              ) : status?.url ? (
                <p className="break-all text-neutral-300">HTTP: {status.url}</p>
              ) : (
                <p className="text-neutral-400">
                  {status ? "HTTP サーバーの起動を待っています…" : "HTTP 状態を確認中…"}
                </p>
              )}
              <p className="mt-1 text-xs text-neutral-500">15 秒ごとに状態を更新</p>
            </div>
            <button
              type="button"
              onClick={refresh}
              disabled={loading}
              className="shrink-0 rounded-md border border-neutral-700 px-3 py-1 text-sm hover:bg-neutral-800 disabled:opacity-50"
            >
              {loading ? "確認中…" : "再読込"}
            </button>
          </div>
        </div>
      </header>
      <div className="mx-auto max-w-6xl space-y-6 p-6">
        <div className="max-w-md">
          <label htmlFor="active-scope" className="block text-sm font-medium">
            対象 scope
          </label>
          <input
            id="active-scope"
            value={scope}
            onChange={(event) => {
              scopeEdited.current = true;
              setScope(event.target.value);
            }}
            aria-describedby="active-scope-help"
            autoComplete="off"
            placeholder="クライアントの既定値"
            className="mt-2 w-full rounded-md border border-neutral-700 bg-neutral-900 px-3 py-2"
          />
          <p id="active-scope-help" className="mt-1 text-xs text-neutral-400">
            情報を扱う範囲を指定します。空欄の場合はクライアントの既定値を使います。
          </p>
        </div>
        <nav role="tablist" aria-label="機能" className="flex flex-wrap gap-2">
          {TABS.map((item, index) => (
            <button
              key={item.id}
              ref={(element) => {
                tabButtons.current[index] = element;
              }}
              id={`tab-${item.id}`}
              type="button"
              role="tab"
              aria-selected={tab === item.id}
              aria-controls={`panel-${item.id}`}
              tabIndex={tab === item.id ? 0 : -1}
              onClick={() => setTab(item.id)}
              onKeyDown={(event) => moveTab(event, index)}
              className={`rounded-md px-4 py-2 text-sm font-medium ${
                tab === item.id
                  ? "bg-neutral-100 text-neutral-950"
                  : "text-neutral-300 hover:bg-neutral-800"
              }`}
            >
              {item.label}
            </button>
          ))}
        </nav>
        <Workspace tab={tab} scope={scope} />
      </div>
    </main>
  );
}
