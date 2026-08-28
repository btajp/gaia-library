// リリース用の公開メタデータだけを扱う。秘密鍵・release.env は読み取らない。
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const [mode, repo, version, output] = process.argv.slice(2);
const repository = "btajp/gaia-library";
const endpoint = `https://github.com/${repository}/releases/latest/download/latest.json`;

function ensure(condition, message) {
  if (!condition) throw new Error(message);
}

function json(relative) {
  return JSON.parse(readFileSync(join(repo, relative), "utf8"));
}

function notes() {
  const lines = readFileSync(join(repo, "CHANGELOG.md"), "utf8").split(/\r?\n/);
  const heading = `## [${version}]`;
  const starts = lines.flatMap((line, index) => line === heading || line.startsWith(`${heading} `) ? [index] : []);
  ensure(starts.length === 1, "CHANGELOG に対象バージョンの節が一つ必要です");
  const rest = lines.slice(starts[0] + 1);
  const end = rest.findIndex((line) => line.startsWith("## ["));
  const body = (end < 0 ? rest : rest.slice(0, end)).join("\n").trim();
  ensure(body, "CHANGELOG の対象バージョンの節が空です");
  return body;
}

function writeNew(path, value) {
  writeFileSync(path, value, { flag: "wx", mode: 0o600 });
}

// 出力先引数が無いまま fs に渡すと Node の型エラーになるため、使い方を示して止める。
function requireOutput(placeholder) {
  ensure(output, `${mode} には ${placeholder} が必要です。使い方: release-metadata.mjs ${mode} <repo> <version> ${placeholder}`);
  return output;
}

try {
  ensure(repo, "リポジトリのパスが必要です");
  if (mode === "pins") {
    const pins = json("toolchain.json");
    ensure(typeof pins.bun === "string" && typeof pins.tauriCli === "string", "toolchain.json の bun / tauriCli が必要です");
    console.log(`${pins.bun}\n${pins.tauriCli}`);
  } else if (mode === "verify") {
    const root = Bun.TOML.parse(readFileSync(join(repo, "Cargo.toml"), "utf8"));
    const desktop = Bun.TOML.parse(readFileSync(join(repo, "desktop/src-tauri/Cargo.toml"), "utf8"));
    const conf = json("desktop/src-tauri/tauri.conf.json");
    ensure(root.workspace?.package?.version === version, "workspace.package.version が対象バージョンと一致しません");
    ensure(desktop.package?.version === version, "desktop Cargo.toml の version が一致しません");
    ensure(desktop.package?.["default-run"] === "gaia-desktop", "desktop の default-run は gaia-desktop が必要です");
    ensure(conf.version === version, "tauri.conf.json の version が一致しません");
    ensure(conf.productName === "gaia-library" && conf.identifier === "com.local.gaia-library.desktop", "本番のアプリ名・identifier が一致しません");
    ensure(conf.bundle?.externalBin?.includes("binaries/gaia"), "同梱 CLI binaries/gaia が設定されていません");
    const updater = conf.plugins?.updater;
    ensure(typeof updater?.pubkey === "string" && updater.pubkey.trim(), "本番 updater 公開鍵が未設定です");
    ensure(JSON.stringify(updater.endpoints) === JSON.stringify([endpoint]), "本番 updater endpoint が一致しません");
    ensure(!updater.dangerousInsecureTransportProtocol, "本番で insecure updater transport は使用できません");
    notes();
  } else if (mode === "overlay") {
    const overlayOutput = requireOutput("<overlay-output.json>");
    const overlay = json("desktop/src-tauri/tauri.updater-artifacts.conf.json");
    ensure(Object.keys(overlay).length === 1 && overlay.bundle?.createUpdaterArtifacts === true
      && Object.keys(overlay.bundle).length === 1,
    "release overlay には bundle.createUpdaterArtifacts=true 以外を入れないでください");
    const identity = process.env.APPLE_SIGNING_IDENTITY;
    ensure(identity?.startsWith("Developer ID Application: "), "Developer ID の署名設定が必要です");
    overlay.bundle.macOS = {
      ...overlay.bundle.macOS,
      signingIdentity: identity,
      hardenedRuntime: true,
    };
    writeNew(overlayOutput, `${JSON.stringify(overlay, null, 2)}\n`);
  } else if (mode === "assets") {
    const staging = requireOutput("<staging-dir>");
    const signature = readFileSync(join(staging, "gaia-library.app.tar.gz.sig"), "utf8").trim();
    ensure(signature, "updater 署名が空です");
    writeNew(join(staging, "release-notes.md"), `${notes()}\n`);
    writeNew(join(staging, "latest.json"), `${JSON.stringify({
      version,
      pub_date: new Date().toISOString().replace(/\.\d{3}Z$/, "Z"),
      platforms: {
        "darwin-aarch64": {
          signature,
          url: `https://github.com/${repository}/releases/download/v${version}/gaia-library.app.tar.gz`,
        },
      },
    }, null, 2)}\n`);
  } else if (mode === "draft") {
    const release = JSON.parse(await Bun.stdin.text());
    const names = release.assets?.map((asset) => asset.name).sort();
    const expected = [`gaia-library_${version}_aarch64.dmg`, "gaia-library.app.tar.gz", "gaia-library.app.tar.gz.sig", "latest.json", "checksums.txt"].sort();
    ensure(release.isDraft === true && release.targetCommitish === output, "draft の対象コミットが一致しません");
    ensure(JSON.stringify(names) === JSON.stringify(expected), "draft の配布ファイルが揃っていません");
  } else if (mode === "notary") {
    const notaryResult = requireOutput("<notary-result.json>");
    ensure(JSON.parse(readFileSync(notaryResult, "utf8")).status === "Accepted", "DMG の公証が Accepted ではありません");
  } else {
    throw new Error("未知の release metadata 操作です");
  }
} catch (error) {
  // 入力ファイルの全文や環境変数の値は表示しない。
  console.error(`ERROR: リリースメタデータの確認に失敗しました (${error instanceof SyntaxError ? "JSON/TOML の形式不正" : error.message})`);
  process.exit(1);
}
