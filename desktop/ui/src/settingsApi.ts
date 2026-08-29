import { getVersion } from "@tauri-apps/api/app";
import { invoke } from "@tauri-apps/api/core";
import type { KeyStorage } from "./api";

export type AffiliationSummary = { id: number; name: string; identity: string | null };
export type ClientRole = "human" | "agent";
export type ClientSummary = {
  name: string;
  role: ClientRole;
  default_scope: string | null;
  has_key: boolean;
};
export type IssuedClientKey = { key: string; storage: KeyStorage };
export type ClientInput = {
  name: string;
  role: ClientRole;
  defaultScope: string;
  generateKey: boolean;
};
export type RenamedClient = {
  name: string;
  key_moved: KeyStorage["location"];
  key_error: string | null;
};
export type Transport = "http" | "stdio";
export type ConnectionSnippet = {
  text: string;
  key_storage: KeyStorage["location"];
};
export type CliLinkStatus =
  | { status: "ok" | "missing" | "not_symlink" }
  | { status: "wrong_target"; current: string };

const optionalText = (value: string | null) => value?.trim() || null;

export const adminAffiliationList = () => invoke<AffiliationSummary[]>("admin_affiliation_list");
export const adminAffiliationAdd = (name: string, identity: string | null) =>
  invoke<number>("admin_affiliation_add", { name: name.trim(), identity: optionalText(identity) });
export const adminClientList = () => invoke<ClientSummary[]>("admin_client_list");
export const adminClientAdd = (input: ClientInput) =>
  invoke<IssuedClientKey | null>("admin_client_add", {
    name: input.name.trim(),
    role: input.role,
    defaultScope: optionalText(input.defaultScope),
    generateKey: input.generateKey,
  });
export const adminClientKeygen = (name: string) => invoke<IssuedClientKey>("admin_client_keygen", { name });
export const adminClientRename = (oldName: string, newName: string) => {
  const name = newName.trim();
  if (!name) return Promise.reject(new Error("新しいクライアント名を入力してください"));
  if (name === oldName) return Promise.reject(new Error("現在と同じ名前です"));
  return invoke<RenamedClient>("admin_client_rename", { oldName, newName: name });
};
export const mcpConfigSnippet = (name: string, transport: Transport) =>
  invoke<ConnectionSnippet>("mcp_config_snippet", { name, transport });
export const cliLinkStatus = () => invoke<CliLinkStatus>("cli_link_status");
export const cliLinkCreate = (expectedTarget: string | null) => invoke<null>("cli_link_create", { expectedTarget });
export const appVersion = () => getVersion();
export const checkUpdates = () => invoke<void>("check_updates");
