import { afterEach, beforeEach, describe, expect, it } from "bun:test";
import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, unlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const policy = fileURLToPath(new URL("../check-updater-key-policy.sh", import.meta.url));
const configRelative = "desktop/src-tauri/tauri.conf.json";
const currentKey = "fixture-public-key-current";
const oldKey = "fixture-public-key-old";
const otherKey = "fixture-public-key-unrelated";
const privateMarker = "FIXTURE ONLY - NOT A CRYPTOGRAPHIC PRIVATE KEY";
let fixture;
let repo;
let privatePath;
let scenario;

// This fake Git never invokes real Git, contacts a remote, or performs external operations.
// It accepts only the exact tag/show requests used by the policy and returns fixture data.
const fakeGit = `#!/usr/bin/env bun
import { appendFileSync, readFileSync } from "node:fs";
const scenario = JSON.parse(readFileSync(process.env.GAIA_POLICY_FIXTURE_FILE, "utf8"));
const args = process.argv.slice(2);
appendFileSync(scenario.log, JSON.stringify(args) + "\\n");
const tagArgs = ["-C", scenario.repo, "tag", "--merged", "origin/main", "--sort=-version:refname", "--list", "v[0-9]*"];
const showArgs = ["-C", scenario.repo, "show", scenario.tag + ":desktop/src-tauri/tauri.conf.json"];
if (JSON.stringify(args) === JSON.stringify(tagArgs)) {
  if (scenario.tagStatus === 0) process.stdout.write(scenario.tags);
  process.exit(scenario.tagStatus);
}
if (JSON.stringify(args) === JSON.stringify(showArgs)) {
  if (scenario.showStatus === 0) process.stdout.write(scenario.previousConfig);
  process.exit(scenario.showStatus);
}
process.stderr.write("Unexpected fake Git command; no external operation was performed\\n");
process.exit(99);
`;

function write(path, value, mode = 0o600) {
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, value, { mode });
}

function config(key) {
  return { plugins: { updater: { pubkey: key } } };
}

function setCurrent(key) {
  write(join(repo, configRelative), JSON.stringify(config(key)));
}

function setSigning(key) {
  write(`${privatePath}.pub`, key);
}

function history(value = config(oldKey)) {
  scenario.tag = "v0.1.0";
  scenario.tags = `${scenario.tag}\n`;
  scenario.previousConfig = JSON.stringify(value);
}

function calls() {
  const text = readFileSync(scenario.log, "utf8").trim();
  return text ? text.split("\n").map((line) => JSON.parse(line)) : [];
}

function run({ allowRotation = false } = {}) {
  const scenarioFile = join(fixture, "git-fixture.json");
  write(scenarioFile, JSON.stringify(scenario));
  write(scenario.log, "");
  const args = [policy, "--repo", repo, "--private-key", privatePath];
  if (allowRotation) args.push("--allow-pubkey-rotation");
  const result = spawnSync("/bin/bash", args, {
    cwd: repo,
    // HOME は変更しない。親の認証情報・Git 設定用環境変数も引き継がない。
    env: {
      PATH: [join(fixture, "bin"), dirname(process.execPath), "/usr/bin", "/bin"].join(":"),
      GAIA_POLICY_FIXTURE_FILE: scenarioFile,
    },
    encoding: "utf8",
    timeout: 5_000,
  });
  expect(result.error).toBeUndefined();
  expect(result.signal).toBeNull();
  expect(result.stdout).not.toContain(privateMarker);
  expect(result.stderr).not.toContain(privateMarker);
  expect(result.stderr).not.toContain("Unexpected fake Git command");
  for (const call of calls()) {
    expect(call[0]).toBe("-C");
    expect(call[1]).toBe(repo);
    expect(["tag", "show"]).toContain(call[2]);
  }
  return result;
}

function expectSuccess(result, key = currentKey) {
  expect(result.status).toBe(0);
  expect(result.stdout).toBe(`${key}\n`);
  expect(result.stderr).not.toContain("ERROR:");
}

function expectRejected(result, message) {
  expect(result.status).toBe(1);
  expect(result.stdout).toBe("");
  expect(result.stderr).toContain(message);
}

beforeEach(() => {
  fixture = mkdtempSync(join(tmpdir(), "gaia-updater-key-policy-test-"));
  repo = join(fixture, "repository with spaces");
  privatePath = join(fixture, "dummy signing.key");
  setCurrent(currentKey);
  write(privatePath, privateMarker);
  setSigning(currentKey);
  write(join(fixture, "bin/git"), fakeGit, 0o700);
  scenario = {
    repo,
    log: join(fixture, "git-calls.jsonl"),
    tag: "",
    tags: "",
    tagStatus: 0,
    showStatus: 0,
    previousConfig: "{}",
  };
});

afterEach(() => {
  // このテストが作成した固有の一時ディレクトリだけを削除する。
  rmSync(fixture, { recursive: true, force: true });
});

describe("first updater release", () => {
  it("accepts matching configured and signing keys without a previous tag", () => {
    const result = run();
    expectSuccess(result);
    expect(result.stderr).toBe("");
    expect(calls().map((call) => call[2])).toEqual(["tag"]);
  });

  it("rejects a signing key that does not match the configured key", () => {
    setSigning(otherKey);
    expectRejected(run(), "署名鍵と設定の updater 公開鍵が一致しません");
  });

  it("normalizes whitespace around a signing public key", () => {
    setSigning(` \t${currentKey}\r\n`);
    expectSuccess(run());
  });
});

describe("previous release inspection", () => {
  it("treats a previous config without a public key as the first updater release", () => {
    history({ plugins: {} });
    const result = run();
    expectSuccess(result);
    expect(result.stderr).toContain("初回鍵として扱います");
    expect(calls().map((call) => call[2])).toEqual(["tag", "show"]);
  });

  it("still requires the current signing key when the previous release had no key", () => {
    history({});
    setSigning(otherKey);
    expectRejected(run({ allowRotation: true }), "署名鍵と設定の updater 公開鍵が一致しません");
  });

  it("does not treat malformed historical JSON as a missing public key", () => {
    history();
    scenario.previousConfig = "{invalid-json:fixture-only}";
    expectRejected(run(), "直前リリースの tauri.conf.json が不正です");
  });

  it("rejects an unreadable previous config", () => {
    history();
    scenario.showStatus = 128;
    expectRejected(run(), "直前リリースの tauri.conf.json を読めません");
  });

  it("rejects tag lookup failure instead of silently allowing a first release", () => {
    scenario.tagStatus = 128;
    expectRejected(run(), "過去のリリースタグを取得できません");
    expect(calls().map((call) => call[2])).toEqual(["tag"]);
  });

  it("uses the first version-sorted tag for historical config lookup", () => {
    history(config(currentKey));
    scenario.tag = "v0.2.0";
    scenario.tags = "v0.2.0\nv0.1.0\n";
    expectSuccess(run());
    expect(calls()[1]).toEqual(["-C", repo, "show", `v0.2.0:${configRelative}`]);
  });
});

describe("unchanged public key", () => {
  it("requires the configured, signing, and previous keys to match", () => {
    history(config(currentKey));
    expectSuccess(run());
  });

  it.each([["without", false], ["with", true]])("rejects a mismatched signing key %s the rotation flag", (_label, allowRotation) => {
    history(config(currentKey));
    setSigning(otherKey);
    expectRejected(run({ allowRotation }), "署名鍵と設定の updater 公開鍵が一致しません");
  });
});

describe("bridge release for key rotation", () => {
  it.each([oldKey, currentKey])("requires an explicit rotation flag when signing with %s", (signingKey) => {
    history();
    setSigning(signingKey);
    expectRejected(run(), "--allow-pubkey-rotation");
  });

  it("allows old-key signing and returns the old key for artifact verification", () => {
    history();
    setSigning(oldKey);
    const before = readFileSync(join(repo, configRelative), "utf8");
    const result = run({ allowRotation: true });
    expectSuccess(result, oldKey);
    expect(result.stderr).toContain("生成物は旧公開鍵で検証します");
    expect(readFileSync(join(repo, configRelative), "utf8")).toBe(before);
    expect(JSON.parse(before).plugins.updater.pubkey).toBe(currentKey);
  });

  it.each([currentKey, otherKey])("rejects a bridge signed by %s", (signingKey) => {
    history();
    setSigning(signingKey);
    expectRejected(run({ allowRotation: true }), "直前リリースの署名鍵で署名する必要があります");
  });
});

describe("missing and empty key inputs", () => {
  it.each(["", " \t\r\n"])("rejects an empty configured public key: %j", (key) => {
    setCurrent(key);
    expectRejected(run(), "tauri.conf.json に updater 公開鍵がありません");
    expect(calls()).toEqual([]);
  });

  it.each(["", " \t\r\n"])("rejects an empty signing public key: %j", (key) => {
    setSigning(key);
    expectRejected(run(), ".pub ファイルが空です");
    expect(calls()).toEqual([]);
  });

  it("rejects a missing public-key file without inspecting Git history", () => {
    unlinkSync(`${privatePath}.pub`);
    expectRejected(run(), "対応する .pub ファイルがありません");
    expect(calls()).toEqual([]);
  });

  it("rejects a missing private-key file", () => {
    unlinkSync(privatePath);
    expectRejected(run(), "署名秘密鍵");
    expect(calls()).toEqual([]);
  });

  it("rejects an empty private-key file", () => {
    write(privatePath, "");
    expectRejected(run(), "署名秘密鍵");
    expect(calls()).toEqual([]);
    expect(existsSync(privatePath)).toBe(true);
  });
});
