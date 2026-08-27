import { afterEach, beforeEach, describe, expect, it } from "bun:test";
import { spawnSync } from "node:child_process";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const loader = fileURLToPath(new URL("./release-env.sh", import.meta.url));
const credentialNames = [
  "APPLE_SIGNING_IDENTITY", "APPLE_API_KEY", "APPLE_API_ISSUER", "APPLE_API_KEY_PATH",
  "TAURI_SIGNING_PRIVATE_KEY", "TAURI_SIGNING_PRIVATE_KEY_PASSWORD",
];
let fixture;
let credentials;
let logPath;

// Fixture-only commands: no real build, signing, Git, GitHub, or network operation runs.
// stat/id return deterministic macOS-format results for existing dummy files only.
const router = `
import { appendFileSync, readFileSync } from "node:fs";
const scenario = JSON.parse(readFileSync(process.env.GAIA_RELEASE_ENV_SCENARIO, "utf8"));
const [command, ...args] = process.argv.slice(2);
const names = Object.keys(scenario.credentials).filter((name) => name in process.env);
appendFileSync(scenario.log, JSON.stringify({
  command, args, names,
  matches: names.every((name) => process.env[name] === scenario.credentials[name]),
  localOnlyExported: "GAIA_RELEASE_TEST_LOCAL_ONLY" in process.env,
  ci: process.env.CI ?? null,
}) + "\\n");
if (command === "stat" && args[0] === "-f" && scenario.files.includes(args[2])) {
  if (args[1] === "%Lp") { console.log("600"); process.exit(0); }
  if (args[1] === "%u") { console.log("4242"); process.exit(0); }
}
if (command === "id" && JSON.stringify(args) === '["-u"]') {
  console.log("4242"); process.exit(0);
}
if (["cargo", "bun", "git", "gh"].includes(command) && args[0] === "fixture-probe") process.exit(0);
if (command === "cargo" && JSON.stringify(args.slice(0, 3)) === '["tauri","build","--config"]') {
  process.exit(scenario.signingStatus);
}
if (command === "bun" && args[1] === "overlay") process.exit(0);
process.stderr.write("Unexpected fixture command; no external operation ran\\n");
process.exit(99);
`;

function write(path, text, mode = 0o600) {
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, text, { mode });
}

function shellQuote(value) {
  return `'${value.replaceAll("'", `'"'"'`)}'`;
}

function run({ inherited = false, exported = false, autoExport = false, callerAutoExport = false,
  missingPassword = false, signingStatus = 0, trace = false } = {}) {
  const envFile = join(fixture, "fixture-release.env");
  const expected = { ...credentials };
  if (missingPassword) expected.TAURI_SIGNING_PRIVATE_KEY_PASSWORD = "";
  const assignments = Object.entries(credentials)
    .filter(([name]) => !missingPassword || name !== "TAURI_SIGNING_PRIVATE_KEY_PASSWORD")
    .map(([name, value]) => `${exported ? "export " : ""}${name}=${shellQuote(value)}`);
  write(envFile, [autoExport ? "set -a" : "", trace ? "set -x" : "", ...assignments].join("\n"));
  const scenarioFile = join(fixture, "scenario.json");
  write(scenarioFile, JSON.stringify({
    credentials: expected,
    files: [envFile, credentials.APPLE_API_KEY_PATH, credentials.TAURI_SIGNING_PRIVATE_KEY],
    log: logPath,
    signingStatus,
  }));
  write(logPath, "");
  const runnerPath = join(fixture, "runner.sh");
  write(runnerPath, `#!/bin/bash
set -euo pipefail
die() { printf 'ERROR: %s\\n' "$*" >&2; exit 1; }
source "$1"
load_release_environment "$2"
[[ "$-" != *a* ]]
GAIA_RELEASE_TEST_LOCAL_ONLY=fixture-local-only
for command in cargo bun git gh; do "$command" fixture-probe before; done
create_release_signing_overlay fixture-metadata "$3" 0.1.0 fixture-overlay.json
if build_signed_desktop "$3" fixture-overlay.json; then
  signing_status=0
else
  signing_status=$?
fi
[[ "$signing_status" == "$4" ]]
for command in cargo bun git gh; do "$command" fixture-probe after; done
`);
  const env = {
    PATH: [join(fixture, "bin"), "/usr/bin", "/bin"].join(":"),
    GAIA_RELEASE_ENV_BUN: process.execPath,
    GAIA_RELEASE_ENV_ROUTER: join(fixture, "router.mjs"),
    GAIA_RELEASE_ENV_SCENARIO: scenarioFile,
  };
  if (inherited) Object.assign(env, credentials);
  const args = [...(callerAutoExport ? ["-a"] : []), runnerPath, loader, envFile, fixture, String(signingStatus)];
  const result = spawnSync("/bin/bash", args, {
    cwd: fixture, env, encoding: "utf8", timeout: 5_000,
  });
  expect(result.error).toBeUndefined();
  expect(result.signal).toBeNull();
  expect(result.status).toBe(0);
  expect(result.stdout).toBe("");
  expect(result.stderr).toBe("");
  return readFileSync(logPath, "utf8").trim().split("\n").map((line) => JSON.parse(line));
}

function expectScopedCredentials(records) {
  const permissionChecks = records.filter((record) => ["stat", "id"].includes(record.command));
  expect(permissionChecks).toHaveLength(9);
  const normalCommands = records.filter((record) => record.args[0] === "fixture-probe");
  expect(normalCommands).toHaveLength(8);
  for (const record of [...permissionChecks, ...normalCommands]) {
    expect(record.names).toEqual([]);
    expect(record.localOnlyExported).toBe(false);
  }
  const overlay = records.filter((record) => record.command === "bun" && record.args[1] === "overlay");
  expect(overlay).toHaveLength(1);
  expect(overlay[0].names).toEqual(["APPLE_SIGNING_IDENTITY"]);
  expect(overlay[0].matches).toBe(true);
  const signing = records.filter((record) => record.command === "cargo" && record.args[0] === "tauri");
  expect(signing).toHaveLength(1);
  expect(signing[0].names).toEqual(credentialNames);
  expect(signing[0].matches).toBe(true);
  expect(signing[0].ci).toBe("true");
}

beforeEach(() => {
  fixture = mkdtempSync(join(tmpdir(), "gaia-release-env-test-"));
  logPath = join(fixture, "command-observations.jsonl");
  credentials = {
    APPLE_SIGNING_IDENTITY: "Developer ID Application: Fixture Only (TESTTEAMID)",
    APPLE_API_KEY: "TSTKEY1234",
    APPLE_API_ISSUER: "11111111-2222-3333-4444-555555555555",
    APPLE_API_KEY_PATH: join(fixture, "dummy apple key.p8"),
    TAURI_SIGNING_PRIVATE_KEY: join(fixture, "dummy updater.key"),
    TAURI_SIGNING_PRIVATE_KEY_PASSWORD: "fixture-only-password",
  };
  write(credentials.APPLE_API_KEY_PATH, "DUMMY ONLY - NOT AN APPLE KEY");
  write(credentials.TAURI_SIGNING_PRIVATE_KEY, "DUMMY ONLY - NOT AN UPDATER KEY");
  write(join(fixture, "router.mjs"), router);
  for (const command of ["stat", "id", "cargo", "bun", "git", "gh"]) {
    write(join(fixture, "bin", command), `#!/bin/bash
# Fixture-only shim. Never invokes the real command.
exec "$GAIA_RELEASE_ENV_BUN" "$GAIA_RELEASE_ENV_ROUTER" "\${0##*/}" "$@"
`, 0o700);
  }
});

afterEach(() => {
  rmSync(fixture, { recursive: true, force: true });
});

describe("release credential scope", () => {
  it.each([
    ["ordinary assignments", {}],
    ["inherited exports", { inherited: true }],
    ["exports in release.env", { exported: true }],
    ["set -a in release.env", { autoExport: true }],
    ["caller set -a", { callerAutoExport: true }],
    ["all export sources together", { inherited: true, exported: true, autoExport: true, callerAutoExport: true }],
  ])("isolates credentials with %s", (_label, options) => {
    expectScopedCredentials(run(options));
  });

  it("keeps an empty default password local and passes it only to signing", () => {
    expectScopedCredentials(run({ missingPassword: true }));
  });

  it("does not leak export attributes after a failed signing subprocess", () => {
    expectScopedCredentials(run({ inherited: true, signingStatus: 67 }));
  });

  it("suppresses settings-file xtrace and clears export attributes afterwards", () => {
    expectScopedCredentials(run({ exported: true, autoExport: true, trace: true }));
  });
});
