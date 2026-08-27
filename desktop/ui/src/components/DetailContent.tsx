import { DETAIL_FACT_LIMIT, DETAIL_INTERACTION_LIMIT } from "../contextApi";
import type { DetailResult, OpenDetail, OrganizationSummary } from "../types";
import Badge, { ENTITY_LABELS } from "./Badges";
import { AliasList, DetailLink, EngagementList, GlossaryList, InteractionList, PeopleList } from "./ContextLists";
import FactList from "./FactList";
import RefList from "./RefList";

function OrganizationLink({ organization, orgId, orgName, openDetail }: { organization?: OrganizationSummary; orgId?: number; orgName?: string; openDetail: OpenDetail }) {
  const id = organization?.id ?? orgId;
  const name = organization?.name ?? orgName;
  return <p className="text-sm text-neutral-300">組織: {id !== undefined ? <DetailLink type="organization" id={id} openDetail={openDetail}>{name ?? `組織 #${id}`}</DetailLink> : name ?? "未登録"}</p>;
}

export default function DetailContent({ result, openDetail }: { result: DetailResult; openDetail: OpenDetail }) {
  const data = result.data;
  const name = result.type === "person" ? result.data.person.name : result.type === "organization" ? result.data.organization.name : result.data.engagement.name;
  return (
    <div className="space-y-6">
      <header className="space-y-3">
        <Badge>{ENTITY_LABELS[result.type]}</Badge>
        <h2 className="break-words text-2xl font-semibold">{name}</h2>
        {result.type === "person" && (
          <>
            <p className="text-sm text-neutral-300">役職: {result.data.person.role ?? "未登録"}</p>
            <OrganizationLink organization={result.data.organization} orgId={result.data.person.org_id} orgName={result.data.person.org_name} openDetail={openDetail} />
            <p className="text-xs text-neutral-400">初対面: {result.data.person.first_met ?? "未登録"} · 最終接点: {result.data.person.last_seen ?? "未登録"}</p>
            <AliasList aliases={result.data.person.aliases} />
          </>
        )}
        {result.type === "organization" && <p className="text-sm text-neutral-300">種別: {result.data.organization.kind ?? "未登録"}</p>}
        {result.type === "engagement" && (
          <>
            <div className="flex flex-wrap gap-2"><Badge>scope: {result.data.engagement.scope}</Badge><Badge>状態: {result.data.engagement.status ?? "未登録"}</Badge></div>
            <OrganizationLink organization={result.data.organization} orgId={result.data.engagement.org_id} orgName={result.data.engagement.org_name} openDetail={openDetail} />
            <p className="text-sm text-neutral-400">期間: {result.data.engagement.started_at ?? "開始未設定"} 〜 {result.data.engagement.ended_at ?? "終了未設定"}</p>
          </>
        )}
      </header>
      {result.type === "person" && <section><h3 className="mb-3 font-semibold">関わる案件</h3><EngagementList engagements={result.data.engagements} openDetail={openDetail} /></section>}
      {result.type === "organization" && (
        <>
          <section><h3 className="mb-3 font-semibold">所属する人物</h3><PeopleList people={result.data.people.map((person) => ({ person }))} openDetail={openDetail} /></section>
          <section><h3 className="mb-3 font-semibold">案件</h3><EngagementList engagements={result.data.engagements} openDetail={openDetail} /></section>
        </>
      )}
      {result.type === "engagement" && <section><h3 className="mb-3 font-semibold">関係者</h3><PeopleList people={result.data.people} openDetail={openDetail} /></section>}
      <section>
        <h3 className="mb-2 font-semibold">現行 facts（{data.facts.length} 件）</h3>
        <p className="mb-3 text-xs text-neutral-400">最大 {DETAIL_FACT_LIMIT} 件です。置換済みの履歴をまとめて取得する機能は未対応です。</p>
        {data.facts.length >= DETAIL_FACT_LIMIT && <p className="mb-3 text-sm text-amber-300">facts が取得上限に達しています。</p>}
        <FactList facts={data.facts} />
      </section>
      <section><h3 className="mb-3 font-semibold">参照（{data.refs.length} 件）</h3><RefList refs={data.refs} /></section>
      {result.type === "engagement" && <section><h3 className="mb-3 font-semibold">用語集</h3><GlossaryList glossary={result.data.glossary} openDetail={openDetail} /></section>}
      {result.type !== "organization" && (
        <section>
          <h3 className="mb-2 font-semibold">直近のやり取り</h3>
          <p className="mb-3 text-xs text-neutral-400">直近 {DETAIL_INTERACTION_LIMIT} 件まで表示します。</p>
          {result.data.interactions.length >= DETAIL_INTERACTION_LIMIT && <p className="mb-3 text-sm text-amber-300">やり取りが取得上限に達しています。</p>}
          <InteractionList interactions={result.data.interactions} openDetail={openDetail} />
        </section>
      )}
    </div>
  );
}
