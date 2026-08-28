import { isDetailType, SEARCH_FACT_LIMIT } from "../contextApi";
import type { OpenDetail, SearchContextOutput } from "../types";
import Badge, { ENTITY_LABELS } from "./Badges";
import { DetailLink, GlossaryList, InteractionList } from "./ContextLists";
import FactList from "./FactList";
import RefList from "./RefList";

type Props = { result: SearchContextOutput; limit: number; openDetail: OpenDetail };

export default function SearchResults({ result, limit, openDetail }: Props) {
  const empty = result.entities.length === 0 && result.glossary.length === 0 && result.interactions.length === 0;
  const atLimit = [result.entities.length, result.glossary.length, result.interactions.length].some((count) => count >= limit);
  return (
    <div className="space-y-5">
      <div className="space-y-2 text-sm text-neutral-400">
        <p role="status">「{result.query}」: 対象 {result.entities.length} 件・用語 {result.glossary.length} 件・やり取り {result.interactions.length} 件</p>
        <p>検索した scope: {result.scopes.join(" / ")}</p>
        {result.cross_scope && <p className="text-amber-300">複数 scope を横断した結果です。横断は監査ログに記録されます。</p>}
        <p className="text-xs">各カテゴリ最大 {limit} 件。対象ごとの現行 facts は最大 {SEARCH_FACT_LIMIT} 件です。ページ送り・総件数の取得は未対応です。</p>
        {atLimit && <p className="text-amber-300">件数上限に達したカテゴリがあります。ほかにも結果がある可能性があるため、検索語を絞り込んでください。</p>}
        {result.hints.map((hint, index) => <p key={index} className="text-xs text-amber-300">{hint}</p>)}
      </div>
      {empty && <p className="rounded-md border border-neutral-800 p-4 text-sm text-neutral-400">該当する結果はありません。</p>}
      {result.entities.map((entity) => (
        <article key={`${entity.type}:${entity.id}`} className="rounded-lg border border-neutral-700 p-4">
          <div className="flex flex-wrap items-center gap-2">
            <Badge>{ENTITY_LABELS[entity.type]}</Badge>
            <h3 className="font-semibold">
              {isDetailType(entity.type) ? <DetailLink type={entity.type} id={entity.id} openDetail={openDetail}>{entity.name}</DetailLink> : entity.name}
            </h3>
            {!isDetailType(entity.type) && <span className="text-xs text-neutral-500">詳細画面は未対応</span>}
          </div>
          {entity.summary && <p className="mt-2 whitespace-pre-wrap break-words text-sm text-neutral-300">{entity.summary}</p>}
          <p className="mt-2 text-xs text-neutral-500">検索スコア {entity.score.toFixed(1)} · 一致箇所: {entity.matched_on.join(", ")}</p>
          <div className="mt-4 grid gap-5 xl:grid-cols-2">
            <section aria-label={`${entity.name} の facts`}>
              <h4 className="mb-2 text-sm font-medium">現行 facts</h4>
              <FactList facts={entity.facts} />
            </section>
            <section aria-label={`${entity.name} の参照`}>
              <h4 className="mb-2 text-sm font-medium">参照</h4>
              <RefList refs={entity.refs} />
            </section>
          </div>
        </article>
      ))}
      {result.glossary.length > 0 && <section><h3 className="mb-2 font-semibold">用語集</h3><GlossaryList glossary={result.glossary} openDetail={openDetail} /></section>}
      {result.interactions.length > 0 && <section><h3 className="mb-2 font-semibold">やり取り</h3><InteractionList interactions={result.interactions} openDetail={openDetail} /></section>}
    </div>
  );
}
