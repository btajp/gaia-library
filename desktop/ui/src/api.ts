import { invoke } from "@tauri-apps/api/core";

export type GaiaErrorBody = {
  code: string;
  message: string;
  details?: unknown;
};

export class GaiaError extends Error {
  readonly code: string;
  readonly details?: unknown;

  constructor(body: GaiaErrorBody) {
    super(body.message);
    this.name = "GaiaError";
    this.code = body.code;
    this.details = body.details;
  }
}

export type KeyStorage = {
  location: "keychain" | "file" | null;
  error: string | null;
};

export type FirstRunResult = { agent_key: string; storage: KeyStorage };

export type ServerStatus = {
  url: string | null;
  error: string | null;
  client: string | null;
  default_scope: string | null;
};

export async function callTool<T = unknown>(
  name: string,
  args: unknown,
): Promise<T> {
  try {
    return await invoke<T>("call_tool", { name, args });
  } catch (raw) {
    if (
      raw !== null &&
      typeof raw === "object" &&
      "code" in raw &&
      typeof raw.code === "string" &&
      "message" in raw &&
      typeof raw.message === "string"
    ) {
      throw new GaiaError(raw as GaiaErrorBody);
    }
    throw raw;
  }
}

export const isInitialized = () => invoke<boolean>("is_initialized");

export const firstRunSetup = (affiliation: string, userName: string) =>
  invoke<FirstRunResult>("first_run_setup", {
    affiliation: affiliation.trim(),
    userName: userName.trim(),
  });

export const serverStatus = () => invoke<ServerStatus>("server_status");

export function errorMessage(error: unknown): string {
  if (error instanceof GaiaError) return `${error.code}: ${error.message}`;
  if (error instanceof Error && error.message) return error.message;
  if (typeof error === "string" && error) return error;
  if (
    error !== null &&
    typeof error === "object" &&
    "message" in error &&
    typeof error.message === "string" &&
    error.message
  ) {
    return error.message;
  }
  return "予期しないエラーが発生しました。";
}
