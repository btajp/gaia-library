import type { ReactNode } from "react";
import type { Alias, DetailType, EngagementPerson, EngagementSummary, GlossaryTerm, InteractionSummary, OpenDetail } from "../types";
import Badge from "./Badges";

type LinkProps = { type: DetailType; id: number; children: ReactNode; openDetail: OpenDetail };

export function DetailLink({ type, id, children, openDetail }: LinkProps) {
  return <button type="button" onClick={() => openDetail(type, id)} className="text-left text-neutral-100 underline decoration-neutral-600 underline-offset-4 hover:decoration-neutral-200">{children}</button>;
}

export function AliasList({ aliases }: { aliases: Alias[] }) {
  if (aliases.length === 0) return <p className="text-sm text-neutral-400">別名はありません。</p>;
  return (
    <ul aria-label="別名" className="flex flex-wrap gap-2">
      {aliases.map((alias, index) => <li key={`${alias.alias}:${alias.kind}:${index}`}><Badge>{alias.alias}{alias.kind ? ` (${alias.kind})` : ""}</Badge></li>)}
    </ul>
  );
}

export function PeopleList({ people, openDetail }: { people: EngagementPerson[]; openDetail: OpenDetail }) {
  if (people.length === 0) return <p className="text-sm text-neutral-400">人物は登録されていません。</p>;
  return (
    <ul className="space-y-3" aria-label="人物">
      {people.map(({ person, role }) => (
        <li key={person.id} className="rounded-md border border-neutral-800 p-3 text-sm">
          <DetailLink type="person" id={person.id} openDetail={openDetail}>{person.name}</DetailLink>
          {person.role && <span className="ml-3 text-neutral-400">役職: {person.role}</span>}
          {role && <p className="mt-1 text-neutral-300">案件での役割: {role}</p>}
          {person.org_name && <p className="mt-1 text-neutral-400">所属: {person.org_name}</p>}
          {person.aliases.length > 0 && <div className="mt-2"><AliasList aliases={person.aliases} /></div>}
        </li>
      ))}
    </ul>
  );
}

export function EngagementList({ engagements, openDetail }: { engagements: EngagementSummary[]; openDetail: OpenDetail }) {
  if (engagements.length === 0) return <p className="text-sm text-neutral-400">対象 scope の案件はありません。</p>;
  return (
    <ul aria-label="案件" className="space-y-3">
      {engagements.map((engagement) => (
        <li key={engagement.id} className="rounded-md border border-neutral-800 p-3 text-sm">
          <div className="flex flex-wrap items-center gap-2">
            <DetailLink type="engagement" id={engagement.id} openDetail={openDetail}>{engagement.name}</DetailLink>
            <Badge>scope: {engagement.scope}</Badge>
            {engagement.status && <Badge>{engagement.status}</Badge>}
          </div>
          {engagement.org_name && <p className="mt-1 text-neutral-400">組織: {engagement.org_name}</p>}
          <p className="mt-1 text-xs text-neutral-400">期間: {engagement.started_at ?? "開始未設定"} 〜 {engagement.ended_at ?? "終了未設定"}</p>
        </li>
      ))}
    </ul>
  );
}

export function GlossaryList({ glossary, openDetail }: { glossary: GlossaryTerm[]; openDetail: OpenDetail }) {
  if (glossary.length === 0) return <p className="text-sm text-neutral-400">用語はありません。</p>;
  return (
    <dl aria-label="用語集" className="space-y-3">
      {glossary.map((term) => (
        <div key={term.id} className="rounded-md border border-neutral-800 p-3">
          <dt className="text-sm font-medium">{term.term}{term.reading ? `（${term.reading}）` : ""}</dt>
          <dd className="mt-1 whitespace-pre-wrap break-words text-sm text-neutral-300">{term.definition ?? "定義は未登録です。"}</dd>
          <dd className="mt-2 flex flex-wrap items-center gap-2 text-xs">
            <Badge>scope: {term.scope}</Badge>
            {term.engagement_id !== undefined && <DetailLink type="engagement" id={term.engagement_id} openDetail={openDetail}>案件 #{term.engagement_id}</DetailLink>}
          </dd>
        </div>
      ))}
    </dl>
  );
}

export function InteractionList({ interactions, openDetail }: { interactions: InteractionSummary[]; openDetail: OpenDetail }) {
  if (interactions.length === 0) return <p className="text-sm text-neutral-400">やり取りはありません。</p>;
  return (
    <ul aria-label="やり取り" className="space-y-3">
      {interactions.map((interaction) => (
        <li key={interaction.id} className="rounded-md border border-neutral-800 p-3">
          <div className="flex flex-wrap items-center gap-2 text-xs text-neutral-400">
            <Badge>{interaction.kind}</Badge><Badge>scope: {interaction.scope}</Badge>
            <time dateTime={interaction.occurred_at}>{interaction.occurred_at}</time>
          </div>
          <p className="mt-2 whitespace-pre-wrap break-words text-sm">{interaction.summary}</p>
          <div className="mt-2 flex flex-wrap gap-3 text-xs">
            {interaction.engagement_id !== undefined && <DetailLink type="engagement" id={interaction.engagement_id} openDetail={openDetail}>案件 #{interaction.engagement_id}</DetailLink>}
            {interaction.person_ids.map((id) => <DetailLink key={id} type="person" id={id} openDetail={openDetail}>人物 #{id}</DetailLink>)}
          </div>
        </li>
      ))}
    </ul>
  );
}
