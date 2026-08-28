import { afterEach, beforeEach, describe, expect, it } from "bun:test";
import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, statSync, unlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const helper = fileURLToPath(new URL("./release-metadata.mjs", import.meta.url));
const version = "0.1.0";
const head = "1".repeat(40);
const identity = "Developer ID Application: Fixture Only (TESTTEAMID)";
const endpoint = "https://github.com/btajp/gaia-library/releases/latest/download/latest.json";
const confPath = "desktop/src-tauri/tauri.conf.json";
const overlayPath = "desktop/src-tauri/tauri.updater-artifacts.conf.json";
const expectedNotes = "### Added\n\n- Fixture release only.";
let fixture;

function write(relative, value) {
  const path = join(fixture, relative);
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, value, { mode: 0o600 });
  return path;
}

function writeJson(relative, value) {
  return write(relative, JSON.stringify(value));
}

function read(relative) {
  return readFileSync(join(fixture, relative), "utf8");
}

function changeJson(relative, change) {
  const value = JSON.parse(read(relative));
  change(value);
  writeJson(relative, value);
}

function run(mode, { output, input = "", signingIdentity = identity } = {}) {
  const args = [helper, mode, fixture];
  if (mode !== "pins") args.push(version);
  if (output !== undefined) args.push(output);
  // 親の Apple 資格情報・updater 鍵・release.env は引き継がない。
  const env = { PATH: process.env.PATH ?? "" };
  if (signingIdentity !== null) env.APPLE_SIGNING_IDENTITY = signingIdentity;
  const result = spawnSync(process.execPath, args, {
    cwd: fixture,
    env,
    input,
    encoding: "utf8",
    timeout: 5_000,
  });
  expect(result.error).toBeUndefined();
  expect(result.signal).toBeNull();
  return result;
}

function expectSuccess(result) {
  expect(result.stderr).toBe("");
  expect(result.status).toBe(0);
}

function expectRejected(result, message) {
  expect(result.status).toBe(1);
  expect(result.stdout).toBe("");
  expect(result.stderr).toContain(message);
}

beforeEach(() => {
  fixture = mkdtempSync(join(tmpdir(), "gaia-release-metadata-test-"));
  writeJson("toolchain.json", { bun: "1.3.14", tauriCli: "2.11.4" });
  write("Cargo.toml", `[workspace.package]\nversion = "${version}"\n`);
  write("desktop/src-tauri/Cargo.toml", `[package]\nname = "gaia-desktop"\nversion = "${version}"\ndefault-run = "gaia-desktop"\n`);
  writeJson(confPath, {
    productName: "gaia-library",
    version,
    identifier: "com.local.gaia-library.desktop",
    bundle: { externalBin: ["binaries/gaia"], macOS: { signingIdentity: "-" } },
    plugins: { updater: { pubkey: "fixture-public-key-not-for-signing", endpoints: [endpoint] } },
  });
  writeJson(overlayPath, { bundle: { createUpdaterArtifacts: true } });
  write("CHANGELOG.md", `# Changelog\n\n## [Unreleased]\n\n- Future changes.\n\n## [${version}] - Draft\n\n${expectedNotes}\n\n## [0.0.9]\n\n- Previous changes.\n`);
});

afterEach(() => {
  // beforeEach で作成した固有ディレクトリだけを削除する。
  rmSync(fixture, { recursive: true, force: true });
});

describe("toolchain pins", () => {
  it("prints only the fixture's Bun and Tauri versions", () => {
    const result = run("pins");
    expectSuccess(result);
    expect(result.stdout).toBe("1.3.14\n2.11.4\n");
  });

  it.each(["bun", "tauriCli"])("rejects a missing %s pin", (key) => {
    changeJson("toolchain.json", (value) => { delete value[key]; });
    expectRejected(run("pins"), "toolchain.json の bun / tauriCli");
  });
});

describe("production release verification", () => {
  it("accepts matching versions and production settings without modifying them", () => {
    const before = read(confPath);
    expectSuccess(run("verify"));
    expect(read(confPath)).toBe(before);
  });

  const invalidCases = [
    ["workspace version", () => write("Cargo.toml", '[workspace.package]\nversion = "0.2.0"\n'), "workspace.package.version"],
    ["desktop version", () => write("desktop/src-tauri/Cargo.toml", read("desktop/src-tauri/Cargo.toml").replace('version = "0.1.0"', 'version = "0.2.0"')), "desktop Cargo.toml"],
    ["Tauri version", () => changeJson(confPath, (value) => { value.version = "99.0.0"; }), "tauri.conf.json の version"],
    ["default-run", () => write("desktop/src-tauri/Cargo.toml", read("desktop/src-tauri/Cargo.toml").replace('default-run = "gaia-desktop"', 'default-run = "verify-updater-signature"')), "default-run"],
    ["E2E identifier", () => changeJson(confPath, (value) => { value.identifier += ".updater-e2e"; }), "identifier"],
    ["product name", () => changeJson(confPath, (value) => { value.productName = "another-app"; }), "アプリ名"],
    ["local endpoint", () => changeJson(confPath, (value) => { value.plugins.updater.endpoints = ["http://127.0.0.1:8930/latest.json"]; }), "endpoint"],
    ["extra endpoint", () => changeJson(confPath, (value) => { value.plugins.updater.endpoints.push("https://example.invalid/latest.json"); }), "endpoint"],
    ["insecure transport", () => changeJson(confPath, (value) => { value.plugins.updater.dangerousInsecureTransportProtocol = true; }), "insecure updater transport"],
    ["empty public key", () => changeJson(confPath, (value) => { value.plugins.updater.pubkey = " "; }), "公開鍵が未設定"],
    ["missing CLI", () => changeJson(confPath, (value) => { value.bundle.externalBin = []; }), "同梱 CLI"],
  ];

  it.each(invalidCases)("rejects %s", (_label, change, error) => {
    change();
    expectRejected(run("verify"), error);
  });

  it("rejects malformed JSON without printing its contents", () => {
    const marker = "fixture-content-must-not-be-printed";
    write(confPath, `{invalid-json:${marker}}`);
    const result = run("verify");
    expectRejected(result, "JSON/TOML の形式不正");
    expect(result.stderr).not.toContain(marker);
  });
});

describe("CHANGELOG sections", () => {
  it.each([
    ["missing file", () => unlinkSync(join(fixture, "CHANGELOG.md"))],
    ["missing version", () => write("CHANGELOG.md", "# Changelog\n## [Unreleased]\n- Future.\n")],
    ["duplicate version", () => write("CHANGELOG.md", `${read("CHANGELOG.md")}\n## [${version}]\n- Duplicate.\n`)],
    ["empty section", () => write("CHANGELOG.md", `## [${version}]\n\n## [0.0.9]\n- Previous.\n`)],
    ["similar version", () => write("CHANGELOG.md", "## [0.1.00]\n- Not the requested version.\n")],
  ])("rejects %s", (_label, change) => {
    change();
    expectRejected(run("verify"), "リリースメタデータの確認に失敗");
  });

  it("accepts a CRLF section with no older release after it", () => {
    write("CHANGELOG.md", `## [${version}]\r\n\r\n- Fixture.\r\n`);
    expectSuccess(run("verify"));
  });
});

describe("missing output argument", () => {
  it.each([
    ["overlay", "<overlay-output.json>"],
    ["assets", "<staging-dir>"],
    ["notary", "<notary-result.json>"],
  ])("rejects %s without an output argument and prints the usage", (mode, placeholder) => {
    // 出力先以外は正常系と同じ入力にし、失敗理由が出力先の欠落だけになるようにする。
    if (mode === "assets") write("assets/gaia-library.app.tar.gz.sig", "fixture-signature");
    if (mode === "notary") writeJson("notary.json", { status: "Accepted" });
    const result = run(mode);
    expectRejected(result, `${mode} には ${placeholder} が必要です`);
    expect(result.stderr).toContain(`release-metadata.mjs ${mode} <repo> <version> ${placeholder}`);
    expect(result.stderr).not.toContain("undefined");
    expect(result.stderr).not.toContain("must be");
    expect(existsSync(join(fixture, "release.conf.json"))).toBe(false);
    expect(existsSync(join(fixture, "assets/latest.json"))).toBe(false);
    expect(existsSync(join(fixture, "assets/release-notes.md"))).toBe(false);
  });
});

describe("release signing overlay", () => {
  it("forces Developer ID and hardened runtime without changing source configs", () => {
    const base = read(confPath);
    const overlay = read(overlayPath);
    const output = join(fixture, "release.conf.json");
    expectSuccess(run("overlay", { output }));
    expect(JSON.parse(read("release.conf.json"))).toEqual({
      bundle: {
        createUpdaterArtifacts: true,
        macOS: { signingIdentity: identity, hardenedRuntime: true },
      },
    });
    expect(statSync(output).mode & 0o777).toBe(0o600);
    expect(read(confPath)).toBe(base);
    expect(read(overlayPath)).toBe(overlay);
  });

  it.each([
    ["missing", null],
    ["ad-hoc", "-"],
    ["development", "Apple Development: Fixture Only"],
  ])("rejects a %s signing identity", (_label, signingIdentity) => {
    const output = join(fixture, "release.conf.json");
    expectRejected(run("overlay", { output, signingIdentity }), "Developer ID の署名設定");
    expect(existsSync(output)).toBe(false);
  });

  it("does not overwrite an existing output", () => {
    const output = write("release.conf.json", "preserve-this-file");
    expectRejected(run("overlay", { output }), "リリースメタデータの確認に失敗");
    expect(read("release.conf.json")).toBe("preserve-this-file");
  });

  it.each([
    ["version", { version: "99.0.0", bundle: { createUpdaterArtifacts: true } }],
    ["endpoint", { bundle: { createUpdaterArtifacts: true }, plugins: { updater: { endpoints: ["http://127.0.0.1:8930/latest.json"] } } }],
    ["bundle override", { bundle: { createUpdaterArtifacts: true, macOS: { signingIdentity: "-" } } }],
    ["disabled artifacts", { bundle: { createUpdaterArtifacts: false } }],
  ])("rejects an unexpected %s overlay", (_label, value) => {
    writeJson(overlayPath, value);
    const output = join(fixture, "release.conf.json");
    expectRejected(run("overlay", { output }), "bundle.createUpdaterArtifacts=true 以外");
    expect(existsSync(output)).toBe(false);
  });
});

describe("release assets", () => {
  it("writes versioned Darwin metadata and only the matching release notes", () => {
    write("assets/gaia-library.app.tar.gz.sig", " fixture-signature-not-for-signing\n");
    expectSuccess(run("assets", { output: join(fixture, "assets") }));
    const latest = JSON.parse(read("assets/latest.json"));
    expect(latest).toEqual({
      version,
      pub_date: expect.stringMatching(/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z$/),
      platforms: {
        "darwin-aarch64": {
          signature: "fixture-signature-not-for-signing",
          url: `https://github.com/btajp/gaia-library/releases/download/v${version}/gaia-library.app.tar.gz`,
        },
      },
    });
    expect(Number.isNaN(Date.parse(latest.pub_date))).toBe(false);
    expect(read("assets/release-notes.md")).toBe(`${expectedNotes}\n`);
  });

  it("rejects an empty signature without writing metadata", () => {
    write("assets/gaia-library.app.tar.gz.sig", " \n");
    expectRejected(run("assets", { output: join(fixture, "assets") }), "updater 署名が空");
    expect(existsSync(join(fixture, "assets/latest.json"))).toBe(false);
    expect(existsSync(join(fixture, "assets/release-notes.md"))).toBe(false);
  });

  it("does not overwrite existing generated notes", () => {
    write("assets/gaia-library.app.tar.gz.sig", "fixture-signature");
    write("assets/release-notes.md", "preserve-notes");
    expectRejected(run("assets", { output: join(fixture, "assets") }), "リリースメタデータの確認に失敗");
    expect(read("assets/release-notes.md")).toBe("preserve-notes");
    expect(existsSync(join(fixture, "assets/latest.json"))).toBe(false);
  });
});

describe("draft publication gate", () => {
  function draft() {
    return {
      isDraft: true,
      targetCommitish: head,
      assets: ["latest.json", "checksums.txt", "gaia-library.app.tar.gz.sig", "gaia-library.app.tar.gz", `gaia-library_${version}_aarch64.dmg`]
        .map((name) => ({ name })),
    };
  }

  it("accepts a draft with all five assets and the verified commit", () => {
    expectSuccess(run("draft", { output: head, input: JSON.stringify(draft()) }));
  });

  it("rejects a missing head argument with the usage before reading the draft", () => {
    // 対象 HEAD が無いと targetCommitish の比較相手が undefined になる。正しい draft を渡しても止まること。
    const result = run("draft", { input: JSON.stringify(draft()) });
    expectRejected(result, "draft には <head-sha> が必要です");
    expect(result.stderr).toContain("release-metadata.mjs draft <repo> <version> <head-sha>");
    expect(result.stderr).not.toContain("undefined");
  });

  it.each([
    ["missing asset", (value) => { value.assets.pop(); }],
    ["extra asset", (value) => { value.assets.push({ name: "unexpected.zip" }); }],
    ["wrong HEAD", (value) => { value.targetCommitish = "2".repeat(40); }],
    ["published release", (value) => { value.isDraft = false; }],
  ])("rejects a %s", (_label, change) => {
    const value = draft();
    change(value);
    expectRejected(run("draft", { output: head, input: JSON.stringify(value) }), "draft");
  });
});

describe("notary result", () => {
  it("accepts only an Accepted result", () => {
    const output = writeJson("notary.json", { status: "Accepted" });
    expectSuccess(run("notary", { output }));
  });

  it.each([
    ["Invalid", "Invalid"],
    ["In Progress", "In Progress"],
    ["Rejected", "Rejected"],
    ["wrong case", "accepted"],
    ["null status", null],
  ])("rejects %s", (_label, status) => {
    const output = writeJson("notary.json", { status });
    expectRejected(run("notary", { output }), "Accepted ではありません");
  });
});
