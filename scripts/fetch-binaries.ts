/**
 * Downloads the sidecar binaries for a Rust target triple into src-tauri/binaries/ using
 * Tauri's naming (`<name>-<triple>[.exe]`), verifying every download's SHA-256.
 *
 *   pnpm fetch-binaries                 # host triple
 *   pnpm fetch-binaries --target x86_64-pc-windows-msvc
 *   YTDLP_VERSION=2026.10.26 pnpm fetch-binaries
 *
 * Sources:
 * - yt-dlp: GitHub releases (pinned), SHA2-256SUMS.
 * - ffmpeg/ffprobe on macOS + Linux: ffmpeg.martin-riedl.de release builds (pinned 9.0.2), per-file .sha256.
 * - ffmpeg/ffprobe on Windows: gyan.dev release build (GyanD/codexffmpeg, pinned 9.0.2), SHA-256 from GitHub's
 *   asset digest, recorded below. Windows on ARM still uses yt-dlp/FFmpeg-Builds "latest" (no pinned build).
 */
import { createHash } from "node:crypto";
import { execFileSync } from "node:child_process";
import {
  chmodSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readdirSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const YTDLP_VERSION = process.env.YTDLP_VERSION ?? "2026.10.26";
const FFMPEG_VERSION = "9.0.2";

const YTDLP_ASSETS: Record<string, string> = {
  "aarch64-apple-darwin": "yt-dlp_macos",
  "x86_64-apple-darwin": "yt-dlp_macos",
  "x86_64-pc-windows-msvc": "yt-dlp.exe",
  "aarch64-pc-windows-msvc": "yt-dlp_arm64.exe",
  "x86_64-unknown-linux-gnu": "yt-dlp_linux",
  "aarch64-unknown-linux-gnu": "yt-dlp_linux_aarch64",
};

const MR = "https://ffmpeg.martin-riedl.de/download";
/** martin-riedl.de build directories for FFmpeg 9.0.2 (each holds ffmpeg.zip, ffprobe.zip + .sha256). */
const MR_DIRS: Record<string, string> = {
  "aarch64-apple-darwin": `${MR}/macos/arm64/1789931890_${FFMPEG_VERSION}`,
  "x86_64-apple-darwin": `${MR}/macos/amd64/1789931006_${FFMPEG_VERSION}`,
  "x86_64-unknown-linux-gnu": `${MR}/linux/amd64/1789931100_${FFMPEG_VERSION}`,
  "aarch64-unknown-linux-gnu": `${MR}/linux/arm64/1789931697_${FFMPEG_VERSION}`,
};
/** Pinned Windows x64 build: gyan.dev essentials, same FFmpeg version as macOS/Linux. */
const GYAN: Record<string, { url: string; sha256: string }> = {
  "x86_64-pc-windows-msvc": {
    url: `https://github.com/GyanD/codexffmpeg/releases/download/${FFMPEG_VERSION}/ffmpeg-${FFMPEG_VERSION}-essentials_build.zip`,
    sha256: "60f467265b1e312373dbcd92200c2618a74850f98d3d078e94296bb3fa2047ba",
  },
};
const FB = "https://github.com/yt-dlp/FFmpeg-Builds/releases/download/latest";
const FB_ZIPS: Record<string, string> = {
  "aarch64-pc-windows-msvc": "ffmpeg-master-latest-winarm64-gpl",
};

function hostTriple(): string {
  const out = execFileSync("rustc", ["-vV"], { encoding: "utf8" });
  const m = /^host: (.+)$/m.exec(out);
  if (!m) throw new Error("could not determine host triple from `rustc -vV`");
  return m[1]!.trim();
}

async function download(url: string): Promise<Buffer> {
  const res = await fetch(url, { redirect: "follow" });
  if (!res.ok) throw new Error(`GET ${url} -> HTTP ${res.status}`);
  return Buffer.from(await res.arrayBuffer());
}

function sha256(buf: Buffer): string {
  return createHash("sha256").update(buf).digest("hex");
}

/** Finds `name`'s hash in a `<hash>  <name>` checksum listing. */
function checksumFor(listing: string, name: string): string {
  const hit = listing
    .split("\n")
    .map((l) => l.trim().split(/\s+/))
    .find(([, n]) => n?.replace(/^\*/, "") === name)?.[0];
  if (!hit) throw new Error(`no checksum for ${name}`);
  return hit.toLowerCase();
}

function verify(buf: Buffer, expected: string, what: string) {
  const actual = sha256(buf);
  if (actual !== expected) throw new Error(`checksum mismatch for ${what}: ${actual} != ${expected}`);
}

/** Path of the first file named `name` inside a `bin` folder under `dir`. */
function findBin(dir: string, name: string): string | null {
  for (const e of readdirSync(dir, { withFileTypes: true })) {
    const p = join(dir, e.name);
    if (e.isDirectory()) {
      const hit = findBin(p, name);
      if (hit) return hit;
    } else if (e.name === name && dir.endsWith("bin")) return p;
  }
  return null;
}

/** Extracts a zip and returns several members, each found by file name under a `bin` folder. */
function unzipBins(zip: Buffer, names: string[]): Buffer[] {
  const dir = mkdtempSync(join(tmpdir(), "lectern-bin-"));
  try {
    const zipPath = join(dir, "a.zip");
    writeFileSync(zipPath, zip);
    if (process.platform === "linux") execFileSync("unzip", ["-q", "-o", zipPath, "-d", dir]);
    else execFileSync("tar", ["-xf", zipPath, "-C", dir]);
    return names.map((n) => {
      const p = findBin(dir, n);
      if (!p) throw new Error(`${n} not found in the archive`);
      return readFileSync(p);
    });
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

function unzip(zip: Buffer, member: string): Buffer {
  const dir = mkdtempSync(join(tmpdir(), "lectern-bin-"));
  try {
    const zipPath = join(dir, "a.zip");
    writeFileSync(zipPath, zip);
    // bsdtar reads zips on macOS and Windows 10+; Linux images usually have unzip.
    if (process.platform === "linux") execFileSync("unzip", ["-q", "-o", zipPath, "-d", dir]);
    else execFileSync("tar", ["-xf", zipPath, "-C", dir]);
    return readFileSync(join(dir, member));
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

interface Ctx {
  triple: string;
  outDir: string;
  ext: string;
}

function isFresh(out: string, version: string): boolean {
  const stamp = `${out}.version`;
  return existsSync(out) && existsSync(stamp) && readFileSync(stamp, "utf8").trim() === version;
}

function install(ctx: Ctx, name: string, bin: Buffer, version: string) {
  const out = join(ctx.outDir, `${name}-${ctx.triple}${ctx.ext}`);
  mkdirSync(ctx.outDir, { recursive: true });
  writeFileSync(out, bin);
  if (!ctx.ext) chmodSync(out, 0o755);
  writeFileSync(`${out}.version`, version);
  console.log(`ok: ${out} (sha256 ${sha256(bin).slice(0, 12)}…)`);
}

async function fetchYtDlp(ctx: Ctx) {
  const out = join(ctx.outDir, `yt-dlp-${ctx.triple}${ctx.ext}`);
  if (isFresh(out, YTDLP_VERSION)) return console.log(`yt-dlp ${YTDLP_VERSION} already present`);
  const asset = YTDLP_ASSETS[ctx.triple];
  if (!asset) throw new Error(`no yt-dlp asset mapped for ${ctx.triple}`);
  const base = `https://github.com/yt-dlp/yt-dlp/releases/download/${YTDLP_VERSION}`;
  console.log(`fetching yt-dlp ${YTDLP_VERSION} (${asset})…`);
  const [bin, sums] = await Promise.all([download(`${base}/${asset}`), download(`${base}/SHA2-256SUMS`)]);
  verify(bin, checksumFor(sums.toString("utf8"), asset), asset);
  install(ctx, "yt-dlp", bin, YTDLP_VERSION);
}

async function fetchFfmpeg(ctx: Ctx) {
  const names = ["ffmpeg", "ffprobe"] as const;
  const mr = MR_DIRS[ctx.triple];
  const gyan = GYAN[ctx.triple];
  const fb = FB_ZIPS[ctx.triple];
  const version = mr || gyan ? FFMPEG_VERSION : `fb-latest`;
  if (names.every((n) => isFresh(join(ctx.outDir, `${n}-${ctx.triple}${ctx.ext}`), version))) {
    return console.log(`ffmpeg ${version} already present`);
  }
  if (mr) {
    for (const n of names) {
      console.log(`fetching ${n} ${FFMPEG_VERSION}…`);
      const [zip, sum] = await Promise.all([download(`${mr}/${n}.zip`), download(`${mr}/${n}.zip.sha256`)]);
      verify(zip, checksumFor(sum.toString("utf8"), `${n}.zip`), `${n}.zip`);
      install(ctx, n, unzip(zip, n), version);
    }
  } else if (gyan) {
    console.log(`fetching ffmpeg ${FFMPEG_VERSION} (gyan.dev essentials)…`);
    const zip = await download(gyan.url);
    verify(zip, gyan.sha256, "ffmpeg essentials zip");
    const bins = unzipBins(zip, names.map((n) => `${n}.exe`));
    names.forEach((n, i) => install(ctx, n, bins[i]!, version));
  } else if (fb) {
    console.log(`fetching ${fb}.zip…`);
    const [zip, sums] = await Promise.all([download(`${FB}/${fb}.zip`), download(`${FB}/checksums.sha256`)]);
    verify(zip, checksumFor(sums.toString("utf8"), `${fb}.zip`), `${fb}.zip`);
    for (const n of names) install(ctx, n, unzip(zip, `${fb}/bin/${n}.exe`), version);
  } else {
    throw new Error(`no ffmpeg source mapped for ${ctx.triple}`);
  }
}

async function main() {
  const i = process.argv.indexOf("--target");
  const triple = i >= 0 ? process.argv[i + 1]! : hostTriple();
  const root = join(dirname(fileURLToPath(import.meta.url)), "..");
  const ctx: Ctx = { triple, outDir: join(root, "src-tauri", "binaries"), ext: triple.includes("windows") ? ".exe" : "" };
  await fetchYtDlp(ctx);
  await fetchFfmpeg(ctx);
}

main().catch((e: unknown) => {
  console.error(e instanceof Error ? e.message : e);
  process.exit(1);
});
