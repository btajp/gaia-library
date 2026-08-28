import { beforeEach, describe, expect, it } from "bun:test";
import { createElement, StrictMode } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { invoke } from "../../test/tauriMock";

const { default: Settings } = await import("../Settings");
const { default: ClientForm } = await import("./ClientForm");
const { KeyConfirmation } = await import("./ClientsSettings");
const { IssuedClientSecret, SnippetPanel } = await import("./ClientSecrets");
const { CliLinkControls, CliLinkFailure } = await import("./CliSettings");
const { ServerStatusContent } = await import("./RuntimeSettings");
const render = (component, props) => renderToStaticMarkup(createElement(component, props));

beforeEach(() => invoke.mockReset());

describe("settings screen markup", () => {
  it("renders all sections without generating keys, links or snippets", () => {
    const html = renderToStaticMarkup(createElement(StrictMode, null, createElement(Settings)));
    for (const title of ["所属元", "クライアントと接続キー", "HTTP サーバー", "同梱 CLI", "バージョン"]) expect(html).toContain(title);
    expect(html).toContain("読込中…");
    expect(html).toContain("アップデートを確認…");
    expect(html).toContain("更新の適用と再起動は、その画面で選択できます");
    expect(html).not.toContain("<textarea");
    expect(html).not.toContain("Authorization");
    expect(invoke).not.toHaveBeenCalled();
  });

  it("defaults new clients to agent without implicit key issuance", () => {
    const html = render(ClientForm, { busy: false, onSubmit: async () => true });
    expect(html).toContain('value="agent" selected=""');
    expect(html).not.toContain('checked=""');
    expect(html).toContain('type="submit" disabled=""');
    expect(html).toContain("空欄なら既定値を持たず");
    expect(invoke).not.toHaveBeenCalled();
  });

  it("requires a second explicit action and explains key rotation", () => {
    const html = render(KeyConfirmation, { client: { name: "owner", role: "human", default_scope: "personal", has_key: true }, busy: false, confirm: () => {}, cancel: () => {} });
    expect(html).toContain("再発行しますか");
    expect(html).toContain("既存のキーがある場合は失効");
    expect(html).toContain("確認してキーを発行");
    expect(html).toContain("エージェントへ渡さないでください");
    expect(invoke).not.toHaveBeenCalled();
  });

  it("shows a valid issued key even when no plaintext store succeeded", () => {
    const html = render(IssuedClientSecret, { name: "reader", issued: { key: "test-only-issued-key", storage: { location: null, error: "storage failed" } }, onClose: () => {} });
    expect(html).toContain("test-only-issued-key");
    expect(html).toContain("キーの発行は完了しています");
    expect(html).toContain("storage failed");
    expect(html).toContain("閉じる（表示内容を破棄）");
  });

  it("does not show cached secrets before a snippet request resolves", () => {
    const html = render(SnippetPanel, { name: "reader", transport: "http", snapshot: null, refresh: () => {}, onClose: () => {} });
    expect(html).toContain("接続設定を取得しています");
    expect(html).not.toContain("Authorization");
    expect(invoke).not.toHaveBeenCalled();
  });

  it("renders an explicitly fetched snippet without making another IPC call", () => {
    const html = render(SnippetPanel, { name: "reader", transport: "http", snapshot: { key: "reader", status: "success", data: { text: "test-only-snippet", key_storage: "file" }, error: null }, refresh: () => {}, onClose: () => {} });
    expect(html).toContain("test-only-snippet");
    expect(html).toContain("権限 0600 のローカルファイル");
    expect(invoke).not.toHaveBeenCalled();
  });
});

describe("CLI link controls", () => {
  const callbacks = { busy: false, blocked: false, intent: null, begin: () => {}, create: () => {}, cancel: () => {} };

  it("offers no overwrite action for ordinary files", () => {
    const html = render(CliLinkControls, { ...callbacks, status: { status: "not_symlink" } });
    expect(html).toContain("この画面では上書きしません");
    expect(html).not.toContain("<button");
  });

  it("offers explicit creation for a missing link", () => {
    const html = render(CliLinkControls, { ...callbacks, intent: { expectedTarget: null }, status: { status: "missing" } });
    expect(html).toContain("~/.local/bin/gaia にリンクを作成");
    expect(html).not.toContain('disabled=""');
    expect(invoke).not.toHaveBeenCalled();
  });

  it("shows the current target before offering replacement confirmation", () => {
    const html = render(CliLinkControls, { ...callbacks, status: { status: "wrong_target", current: "/test/old-cli" } });
    expect(html).toContain("/test/old-cli");
    expect(html).toContain("リンク先の変更を確認");
    expect(html).not.toContain("確認してリンクを変更");
  });

  it("shows replacement impact only after confirmation is opened", () => {
    const html = render(CliLinkControls, { ...callbacks, intent: { expectedTarget: "/test/old-cli" }, status: { status: "wrong_target", current: "/test/old-cli" } });
    expect(html).toContain("確認してリンクを変更");
    expect(html).toContain("リンク先のファイル自体は削除しません");
  });

  it("does not display replacement controls for a different confirmed target", () => {
    const html = render(CliLinkControls, { ...callbacks, intent: { expectedTarget: "/test/old-cli" }, status: { status: "wrong_target", current: "/test/new-cli" } });
    expect(html).not.toContain("確認してリンクを変更");
    expect(html).toContain("リンク先の変更を確認");
  });

  it("disables creation when the displayed state has no valid intent", () => {
    const html = render(CliLinkControls, { ...callbacks, status: { status: "missing" } });
    expect(html).toContain('disabled=""');
  });

  it("blocks another attempt until a failed operation is followed by a fresh read", () => {
    const html = render(CliLinkControls, { ...callbacks, blocked: true, intent: { expectedTarget: null }, status: { status: "missing" } });
    expect(html).toContain('disabled=""');
    const warning = render(CliLinkFailure, { error: "CLI link target changed", needsRefresh: true });
    expect(warning).toContain("CLI link target changed");
    expect(warning).toContain("「再読込」で現在の状態を取得");
    expect(warning).toContain("リンク先を確認し直してください");
  });
});

describe("server status messages", () => {
  it("does not invent a key-related cause for a stopped server", () => {
    const html = render(ServerStatusContent, { status: { url: null, error: null, client: "owner", default_scope: null }, error: null, loading: false });
    expect(html).toContain("停止理由は取得できていません");
    expect(html).not.toContain("キー未発行");
  });

  it("displays the actual server error", () => {
    const html = render(ServerStatusContent, { status: { url: null, error: "test port unavailable", client: "owner", default_scope: "personal" }, error: null, loading: false });
    expect(html).toContain("test port unavailable");
    expect(html).not.toContain("停止理由は取得できていません");
  });

  it("displays the actual bound URL rather than an assumed port", () => {
    const html = render(ServerStatusContent, { status: { url: "http://127.0.0.1:41234/mcp", error: null, client: "owner", default_scope: "personal" }, error: null, loading: false });
    expect(html).toContain("http://127.0.0.1:41234/mcp");
    expect(html).not.toContain(":4111/");
  });

  it("does not present a stale URL as current when status retrieval fails", () => {
    const html = render(ServerStatusContent, { status: { url: "http://127.0.0.1:41234/mcp", error: null, client: "owner", default_scope: "personal" }, error: "test read failed", loading: false });
    expect(html).toContain("test read failed");
    expect(html).not.toContain("http://127.0.0.1");
  });
});
