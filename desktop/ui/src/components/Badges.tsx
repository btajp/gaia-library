import type { ReactNode } from "react";
import type { EntityType } from "../types";

export const ENTITY_LABELS: Record<EntityType, string> = {
  person: "人物",
  organization: "組織",
  engagement: "案件",
  interaction: "やり取り",
  entity: "汎用エンティティ",
};

type Props = { children: ReactNode; tone?: "neutral" | "green" | "amber" };

export default function Badge({ children, tone = "neutral" }: Props) {
  const tones = {
    neutral: "bg-neutral-800 text-neutral-300",
    green: "bg-emerald-950 text-emerald-300",
    amber: "bg-amber-950 text-amber-300",
  };
  return <span className={`inline-block rounded px-2 py-0.5 text-xs ${tones[tone]}`}>{children}</span>;
}
