import { beforeEach, describe, expect, it, mock } from "bun:test";
import { invoke } from "./test/tauriMock";

const getVersion = mock(() => Promise.resolve("0.1.0"));
mock.module("@tauri-apps/api/app", () => ({ getVersion }));
const api = await import("./settingsApi");

beforeEach(() => { invoke.mockReset(); getVersion.mockClear(); });

describe("settings IPC boundary", () => {
  it("starts the shared native update check only when explicitly invoked", async () => {
    invoke.mockResolvedValueOnce(undefined);
    await api.checkUpdates();
    expect(invoke).toHaveBeenCalledTimes(1);
    expect(invoke).toHaveBeenCalledWith("check_updates");
  });

  it("lists affiliation metadata without issuing or loading a key", async () => {
    const rows = [{ id: 1, name: "personal", identity: null }];
    invoke.mockResolvedValueOnce(rows);
    expect(await api.adminAffiliationList()).toBe(rows);
    expect(invoke).toHaveBeenCalledTimes(1);
    expect(invoke).toHaveBeenCalledWith("admin_affiliation_list");
  });

  it("trims affiliation fields and omits blank identity", async () => {
    invoke.mockResolvedValueOnce(1);
    expect(await api.adminAffiliationAdd(" personal ", "  ")).toBe(1);
    expect(invoke).toHaveBeenCalledWith("admin_affiliation_add", { name: "personal", identity: null });
  });

  it("keeps a supplied affiliation identity", async () => {
    await api.adminAffiliationAdd("company", " team ");
    expect(invoke).toHaveBeenCalledWith("admin_affiliation_add", { name: "company", identity: "team" });
  });

  it("lists only client summaries", async () => {
    const rows = [{ name: "reader", role: "agent", default_scope: "personal", has_key: true }];
    invoke.mockResolvedValueOnce(rows);
    expect(await api.adminClientList()).toBe(rows);
    expect(invoke).toHaveBeenCalledTimes(1);
    expect(invoke).toHaveBeenCalledWith("admin_client_list");
  });

  it("adds a client with camelCase Tauri arguments and no implicit key", async () => {
    invoke.mockResolvedValueOnce(null);
    expect(await api.adminClientAdd({ name: " reader ", role: "agent", defaultScope: " personal ", generateKey: false })).toBeNull();
    expect(invoke).toHaveBeenCalledWith("admin_client_add", { name: "reader", role: "agent", defaultScope: "personal", generateKey: false });
  });

  it("uses no default scope when the optional field is blank", async () => {
    await api.adminClientAdd({ name: "owner", role: "human", defaultScope: "  ", generateKey: false });
    expect(invoke).toHaveBeenCalledWith("admin_client_add", { name: "owner", role: "human", defaultScope: null, generateKey: false });
  });

  it("preserves a valid generated key when storage failed", async () => {
    const issued = { key: "test-only-issued-key", storage: { location: null, error: "storage failed" } };
    invoke.mockResolvedValueOnce(issued);
    expect(await api.adminClientAdd({ name: "reader", role: "agent", defaultScope: "personal", generateKey: true })).toBe(issued);
  });

  it("returns the explicit key issuance result unchanged", async () => {
    const issued = { key: "test-only-issued-key", storage: { location: "file", error: null } };
    invoke.mockResolvedValueOnce(issued);
    expect(await api.adminClientKeygen("reader")).toBe(issued);
    expect(invoke).toHaveBeenCalledWith("admin_client_keygen", { name: "reader" });
  });

  it("renames a client with the trimmed new name and returns the result unchanged", async () => {
    const renamed = { name: "writer", key_moved: "keychain", key_error: null };
    invoke.mockResolvedValueOnce(renamed);
    expect(await api.adminClientRename("reader", " writer ")).toBe(renamed);
    expect(invoke).toHaveBeenCalledTimes(1);
    expect(invoke).toHaveBeenCalledWith("admin_client_rename", { oldName: "reader", newName: "writer" });
  });

  it("does not invoke a rename for a blank or unchanged name", async () => {
    await expect(api.adminClientRename("reader", "   ")).rejects.toThrow("入力してください");
    await expect(api.adminClientRename("reader", " reader ")).rejects.toThrow("同じ名前");
    expect(invoke).not.toHaveBeenCalled();
  });

  it("preserves a duplicate-name failure without retrying", async () => {
    invoke.mockRejectedValueOnce("同じ名前のクライアントが既にあります");
    await expect(api.adminClientRename("reader", "owner")).rejects.toBe("同じ名前のクライアントが既にあります");
    expect(invoke).toHaveBeenCalledTimes(1);
  });

  for (const transport of ["http", "stdio"]) {
    it(`requests a ${transport} snippet only on invocation`, async () => {
      const snippet = { text: "test-only-snippet", key_storage: transport === "http" ? "keychain" : null };
      invoke.mockResolvedValueOnce(snippet);
      expect(await api.mcpConfigSnippet("reader", transport)).toBe(snippet);
      expect(invoke).toHaveBeenCalledTimes(1);
      expect(invoke).toHaveBeenCalledWith("mcp_config_snippet", { name: "reader", transport });
    });
  }

  it("preserves a failed snippet request rather than generating a replacement key", async () => {
    invoke.mockRejectedValueOnce("current key unavailable");
    await expect(api.mcpConfigSnippet("reader", "http")).rejects.toBe("current key unavailable");
    expect(invoke).toHaveBeenCalledTimes(1);
  });

  it("checks CLI link status without creating a link", async () => {
    const status = { status: "wrong_target", current: "/test/old-cli" };
    invoke.mockResolvedValueOnce(status);
    expect(await api.cliLinkStatus()).toBe(status);
    expect(invoke).toHaveBeenCalledTimes(1);
    expect(invoke).toHaveBeenCalledWith("cli_link_status");
  });

  it("requests a new CLI link without authorizing any existing target replacement", async () => {
    invoke.mockResolvedValueOnce(null);
    expect(await api.cliLinkCreate(null)).toBeNull();
    expect(invoke).toHaveBeenCalledWith("cli_link_create", { expectedTarget: null });
  });

  it("preserves the exact confirmed CLI link target in replacement requests", async () => {
    invoke.mockResolvedValueOnce(null);
    const target = " ../old cli ";
    expect(await api.cliLinkCreate(target)).toBeNull();
    expect(invoke).toHaveBeenCalledWith("cli_link_create", { expectedTarget: target });
  });

  it("does not retry or widen authority after the expected CLI link target changed", async () => {
    invoke.mockRejectedValueOnce("CLI link target changed");
    await expect(api.cliLinkCreate(null)).rejects.toBe("CLI link target changed");
    expect(invoke).toHaveBeenCalledTimes(1);
    expect(invoke).toHaveBeenCalledWith("cli_link_create", { expectedTarget: null });
  });

  it("gets the application version from Tauri app API", async () => {
    expect(await api.appVersion()).toBe("0.1.0");
    expect(getVersion).toHaveBeenCalledTimes(1);
    expect(invoke).not.toHaveBeenCalled();
  });
});
