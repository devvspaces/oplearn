/**
 * Sets the app version everywhere it lives, so a release tag matches the build:
 * package.json, src-tauri/tauri.conf.json, src-tauri/Cargo.toml (+ Cargo.lock).
 *
 *   pnpm version:set 0.2.0
 */
import { execFileSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";

const version = process.argv[2]?.replace(/^v/, "");
if (!version || !/^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$/.test(version)) {
  console.error("usage: pnpm version:set <major.minor.patch>   (e.g. 0.2.0)");
  process.exit(1);
}

function setJson(path: string) {
  const text = readFileSync(path, "utf8");
  const next = text.replace(/("version"\s*:\s*")[^"]+(")/, `$1${version}$2`);
  if (next === text && !text.includes(`"version": "${version}"`)) throw new Error(`no version field in ${path}`);
  writeFileSync(path, next);
}

setJson("package.json");
setJson("src-tauri/tauri.conf.json");

const cargo = "src-tauri/Cargo.toml";
const toml = readFileSync(cargo, "utf8");
// Only the [package] version (the first `version = ` line), not dependency versions.
writeFileSync(cargo, toml.replace(/^version = "[^"]+"/m, `version = "${version}"`));
execFileSync("cargo", ["update", "--manifest-path", cargo, "-p", "lectern"], { stdio: "inherit" });

console.log(`\nVersion set to ${version}. Next:
  git commit -am "Release v${version}"
  git tag v${version}
  git push origin master v${version}
Then publish the draft release on GitHub once the release workflow finishes.`);
