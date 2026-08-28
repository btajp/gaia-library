import { mock } from "bun:test";

export const invoke = mock(() => Promise.resolve(undefined));
mock.module("@tauri-apps/api/core", () => ({ invoke }));
