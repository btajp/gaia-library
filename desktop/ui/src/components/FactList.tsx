import type { Fact } from "../types";
import Badge from "./Badges";

export default function FactList({ facts }: { facts: Fact[] }) {
  if (facts.length === 0) return <p className="text-sm text-neutral-400">facts はありません。</p>;
  return (
    <ul className="space-y-3" aria-label="facts">
      {facts.map((fact) => (
        <li key={fact.id} className="rounded-md border border-neutral-800 p-3">
          <div className="flex flex-wrap gap-2">
            <Badge tone={fact.kind === "fact" ? "green" : "amber"}>
              {fact.kind === "fact" ? "fact（事実）" : "inference（推測）"}
            </Badge>
            <Badge>scope: {fact.scope}</Badge>
            {fact.superseded_by !== undefined && <Badge tone="amber">置換済み → fact #{fact.superseded_by}</Badge>}
          </div>
          <p className="mt-2 whitespace-pre-wrap break-words text-sm">{fact.statement}</p>
          {(fact.predicate !== undefined || fact.value !== undefined) && (
            <p className="mt-2 break-words font-mono text-xs text-neutral-300">
              {fact.predicate ?? "未設定"} = {fact.value ?? "未設定"}
            </p>
          )}
          <dl className="mt-2 flex flex-wrap gap-x-4 gap-y-1 text-xs text-neutral-400">
            <div><dt className="inline">ID: </dt><dd className="inline">{fact.id}</dd></div>
            <div><dt className="inline">有効開始: </dt><dd className="inline">{fact.valid_from ?? "未設定"}</dd></div>
            <div><dt className="inline">作成: </dt><dd className="inline"><time dateTime={fact.created_at}>{fact.created_at}</time></dd></div>
          </dl>
        </li>
      ))}
    </ul>
  );
}
