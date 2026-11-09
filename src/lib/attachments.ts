import { convertFileSrc } from "@tauri-apps/api/core";

/** Markdown images captured by Lectern use `lectern-attachment:<path relative to the data dir>`. */
export const ATTACHMENT_SCHEME = "lectern-attachment:";

let dataDir: string | null = null;
export function setDataDir(dir: string) {
  dataDir = dir.replace(/[\\/]+$/, "");
}

/** Resolves an image src for display: attachment links become asset-protocol URLs. */
export function resolveImageSrc(src: string | null | undefined): string {
  if (!src) return "";
  if (!src.startsWith(ATTACHMENT_SCHEME)) return src;
  const rel = src.slice(ATTACHMENT_SCHEME.length);
  if (!dataDir || rel.includes("..")) return "";
  return convertFileSrc(`${dataDir}/${rel}`);
}

export function attachmentMarkdown(relPath: string, alt: string): string {
  return `![${alt}](${ATTACHMENT_SCHEME}${relPath})`;
}
