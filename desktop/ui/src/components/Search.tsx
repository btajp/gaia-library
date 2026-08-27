import { useEffect, useState, type FormEvent } from "react";
import { errorMessage } from "../api";
import { searchContext, searchKey } from "../contextApi";
import { useLatestRequest } from "../hooks/useLatestRequest";
import { snapshotForKey } from "../lib/latestRequest";
import type { OpenDetail, SearchContextOutput } from "../types";
import SearchResults from "./SearchResults";

type Props = { scope: string; openDetail: OpenDetail };

export default function Search({ scope, openDetail }: Props) {
  const [query, setQuery] = useState("");
  const [limit, setLimit] = useState(10);
  const { request, snapshot } = useLatestRequest<SearchContextOutput>();
  const key = searchKey(query, scope, limit);
  const current = snapshotForKey(snapshot, key);
  const busy = current?.status === "loading";

  useEffect(() => {
    request.reset();
    return request.invalidate;
  }, [request, scope]);

  function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!query.trim() || request.getSnapshot().status === "loading") return;
    void request.run(key, () => searchContext(query, scope, limit));
  }

  return (
    <div className="space-y-5">
      <h2 className="text-xl font-semibold">検索</h2>
      <form onSubmit={submit} className="space-y-3" aria-busy={busy}>
        <div>
          <label htmlFor="search-query" className="block text-sm font-medium">検索語</label>
          <input
            id="search-query"
            value={query}
            onChange={(event) => { request.reset(); setQuery(event.target.value); }}
            placeholder="人物・組織・案件・facts・用語・やり取り"
            aria-describedby="search-help"
            autoComplete="off"
            className="mt-2 w-full rounded-md border border-neutral-700 bg-neutral-950 px-3 py-2"
          />
          <p id="search-help" className="mt-1 text-xs text-neutral-400">検索語を入力して検索します。3 文字未満は部分一致検索になります。</p>
        </div>
        <div className="flex flex-wrap items-end gap-3">
          <div>
            <label htmlFor="search-limit" className="block text-xs text-neutral-400">各カテゴリの上限</label>
            <select id="search-limit" value={limit} onChange={(event) => { request.reset(); setLimit(Number(event.target.value)); }} className="mt-1 rounded-md border border-neutral-700 bg-neutral-950 px-3 py-2 text-sm">
              {[10, 20, 50].map((count) => <option key={count} value={count}>{count} 件</option>)}
            </select>
          </div>
          <button type="submit" disabled={busy || !query.trim()} className="rounded-md bg-neutral-100 px-4 py-2 text-sm font-medium text-neutral-950 hover:bg-white disabled:cursor-not-allowed disabled:opacity-50">
            {busy ? "検索中…" : "検索する"}
          </button>
        </div>
      </form>
      {busy && <p role="status" className="text-sm text-neutral-400">検索中です…</p>}
      {current?.status === "error" && <p role="alert" className="break-words text-sm text-red-300">{errorMessage(current.error)}</p>}
      {current?.status === "success" && current.data && <SearchResults result={current.data} limit={limit} openDetail={openDetail} />}
    </div>
  );
}
