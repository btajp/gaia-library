import { callTool } from "./api";
import type {
  DetailResult,
  DetailTarget,
  DetailType,
  GetEngagementOutput,
  GetOrganizationOutput,
  GetPersonOutput,
  Reference,
  ResolveSourceOutput,
  SearchContextOutput,
} from "./types";

export const SEARCH_FACT_LIMIT = 20;
export const DETAIL_FACT_LIMIT = 50;
export const DETAIL_INTERACTION_LIMIT = 20;

export function scopeArgs(scope: string): { scope?: string } {
  const value = scope.trim();
  return value ? { scope: value } : {};
}

export function searchKey(query: string, scope: string, limit: number): string {
  return JSON.stringify([query.trim(), scope.trim(), limit]);
}

export function detailKey(target: DetailTarget, scope: string): string {
  return JSON.stringify([target.type, target.id, scope.trim()]);
}

export function resolveKey(reference: Pick<Reference, "id" | "scope">): string {
  return JSON.stringify([reference.scope.trim(), reference.id]);
}

export function isDetailType(type: string): type is DetailType {
  return type === "person" || type === "organization" || type === "engagement";
}

export async function searchContext(
  query: string,
  scope: string,
  limit: number,
): Promise<SearchContextOutput> {
  const normalized = query.trim();
  if (!normalized) throw new Error("検索語を入力してください。");
  if (!Number.isInteger(limit) || limit < 1 || limit > 50) {
    throw new Error("検索件数の上限は 1〜50 の整数で指定してください。");
  }
  return callTool<SearchContextOutput>("search_context", {
    query: normalized,
    ...scopeArgs(scope),
    limit,
  });
}

export async function loadDetail(target: DetailTarget, scope: string): Promise<DetailResult> {
  const scopes = scopeArgs(scope);
  switch (target.type) {
    case "person":
      return {
        type: "person",
        data: await callTool<GetPersonOutput>("get_person", { person_id: target.id, ...scopes }),
      };
    case "organization":
      return {
        type: "organization",
        data: await callTool<GetOrganizationOutput>("get_organization", { organization_id: target.id, ...scopes }),
      };
    case "engagement":
      return {
        type: "engagement",
        data: await callTool<GetEngagementOutput>("get_engagement", { engagement_id: target.id, ...scopes }),
      };
  }
}

export async function copyReferenceUri(
  uri: string,
  clipboard: Pick<Clipboard, "writeText"> | undefined = globalThis.navigator?.clipboard,
): Promise<void> {
  if (!clipboard?.writeText) throw new Error("コピー機能を利用できません。URI を選択してコピーしてください。");
  try {
    await clipboard.writeText(uri);
  } catch {
    throw new Error("URI をコピーできませんでした。URI を選択してコピーしてください。");
  }
}

export const RESOLVE_PENDING_NOTE = "取得中…（時間がかかることがあります）";

/// 参照自身の scope で resolve_source を呼ぶ（横断にならない）。結果は呼び出し側の state にだけ置く。
export async function resolveReference(reference: Reference): Promise<ResolveSourceOutput> {
  if (!Number.isInteger(reference.id) || reference.id < 1) throw new Error("参照 ID が不正です。");
  const scope = reference.scope.trim();
  if (!scope) throw new Error("参照の scope が不明なため取得できません。");
  return callTool<ResolveSourceOutput>("resolve_source", { ref_id: reference.id, scope });
}
