import { beforeEach, describe, expect, it } from "bun:test";
import { createElement, StrictMode } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { invoke } from "./test/tauriMock";

const api = await import("./api");
const { default: App } = await import("./App");
const { default: FirstRun } = await import("./components/FirstRun");
const { default: IssuedKey } = await import("./components/IssuedKey");

beforeEach(() => invoke.mockReset());

describe("Tauri command boundary", () => {
  it("returns the uninitialized state without starting setup", async () => {
    invoke.mockResolvedValueOnce(false);
    expect(await api.isInitialized()).toBe(false);
    expect(invoke).toHaveBeenCalledWith("is_initialized");
    expect(invoke).toHaveBeenCalledTimes(1);
  });

  it("does not mistake an initialization error for an uninitialized state", async () => {
    invoke.mockRejectedValueOnce("config is unreadable");
    await expect(api.isInitialized()).rejects.toBe("config is unreadable");
  });

  it("trims setup fields and returns the issued key and storage status", async () => {
    const result = { agent_key: "test-only-issued-key", storage: { location: "keychain", error: null } };
    invoke.mockResolvedValueOnce(result);
    expect(await api.firstRunSetup("  personal  ", " user ")).toBe(result);
    expect(invoke).toHaveBeenCalledWith("first_run_setup", {
      affiliation: "personal",
      userName: "user",
    });
  });

  it("preserves an issued setup key when plaintext storage fails", async () => {
    const result = { agent_key: "test-only-issued-key", storage: { location: null, error: "storage failed" } };
    invoke.mockResolvedValueOnce(result);
    expect(await api.firstRunSetup("personal", "user")).toBe(result);
  });

  it("preserves server status fields", async () => {
    const status = {
      url: null,
      error: "port is unavailable",
      client: "user",
      default_scope: "personal",
    };
    invoke.mockResolvedValueOnce(status);
    expect(await api.serverStatus()).toBe(status);
    expect(invoke).toHaveBeenCalledWith("server_status");
  });

  it("forwards tool calls and results unchanged", async () => {
    const args = { query: "memory", scope: ["personal"] };
    const result = { entities: [] };
    invoke.mockResolvedValueOnce(result);
    expect(await api.callTool("search_context", args)).toBe(result);
    expect(invoke).toHaveBeenCalledWith("call_tool", {
      name: "search_context",
      args,
    });
  });

  it("preserves structured error code, message and details", async () => {
    const body = {
      code: "scope_denied",
      message: "scope is not allowed",
      details: { scope: "private" },
    };
    invoke.mockRejectedValueOnce(body);
    try {
      await api.callTool("list_proposals", {});
      throw new Error("expected rejection");
    } catch (error) {
      expect(error).toBeInstanceOf(api.GaiaError);
      expect(error.code).toBe(body.code);
      expect(error.message).toBe(body.message);
      expect(error.details).toBe(body.details);
      expect(api.errorMessage(error)).toBe("scope_denied: scope is not allowed");
    }
  });

  it("does not replace unexpected IPC errors with malformed GaiaError objects", async () => {
    const error = { code: 42, details: "unknown" };
    invoke.mockRejectedValueOnce(error);
    await expect(api.callTool("get_server_info", {})).rejects.toBe(error);
  });

  it("shows string errors and avoids serializing arbitrary error payloads", () => {
    expect(api.errorMessage("read failed")).toBe("read failed");
    expect(api.errorMessage(new Error("connection failed"))).toBe("connection failed");
    expect(api.errorMessage({ message: "server failed" })).toBe("server failed");
    expect(api.errorMessage({ token: "must-not-be-shown" })).toBe(
      "予期しないエラーが発生しました。",
    );
  });
});

describe("initial screen markup", () => {
  it("starts in a loading state, including StrictMode, without creating data", () => {
    const html = renderToStaticMarkup(createElement(StrictMode, null, createElement(App)));
    expect(html).toContain("起動状態を確認しています");
    expect(invoke).not.toHaveBeenCalled();
  });

  it("labels required setup fields and disables empty submission", () => {
    const html = renderToStaticMarkup(createElement(FirstRun, { onComplete: () => {} }));
    expect(html).toContain("エージェントは提案まで、承認はあなた（human）だけ");
    expect(html).toContain("最初の機密境界（scope）の名前になります");
    expect(html).toContain("desktop:&lt;名前&gt;");
    expect(html).toContain("後から設定画面で変更できます");
    expect(html).toContain('for="affiliation"');
    expect(html).toContain('for="user-name"');
    expect(html).toContain('type="submit" disabled=""');
    expect(invoke).not.toHaveBeenCalled();
  });

  it("labels the issued key as secret and explains Keychain storage", () => {
    const html = renderToStaticMarkup(
      createElement(IssuedKey, { agentKey: "test-only-issued-key", storage: { location: "keychain", error: null }, onClose: () => {} }),
    );
    expect(html).toContain("秘密情報");
    expect(html).toContain("Keychain に保管しました");
    expect(html).toContain("接続設定を再表示できます");
    expect(html).not.toContain("再表示できません");
    expect(html).not.toContain("平文はアプリに保存しません");
    expect(html).toContain("閉じてメイン画面へ");
    expect(html).toContain("test-only-issued-key");
    expect(invoke).not.toHaveBeenCalled();
  });

  it("explains file fallback without hiding the issued key", () => {
    const html = renderToStaticMarkup(
      createElement(IssuedKey, { agentKey: "test-only-issued-key", storage: { location: "file", error: null }, onClose: () => {} }),
    );
    expect(html).toContain("権限 0600");
    expect(html).toContain("Keychain を利用できなかったため");
    expect(html).toContain("test-only-issued-key");
  });

  it("keeps an unpreserved valid key visible and warns before closing", () => {
    const html = renderToStaticMarkup(
      createElement(IssuedKey, { agentKey: "test-only-issued-key", storage: { location: null, error: "storage failed" }, onClose: () => {} }),
    );
    expect(html).toContain("キーの発行は完了しています");
    expect(html).toContain("閉じる前に安全な場所へコピー");
    expect(html).toContain("再表示できません");
    expect(html).toContain("storage failed");
    expect(html).toContain("test-only-issued-key");
  });
});
