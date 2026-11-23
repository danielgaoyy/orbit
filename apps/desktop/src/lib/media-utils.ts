/**
 * Pure client-side helpers for multimodal attachments (PDF, Video, Audio, Image):
 * modality classification, safe bounded PDF page-count scanning, Blob URL helpers,
 * and concise citation formatting for Composer insertion.
 */

import { stripWindowsVerbatimPrefix } from "@/lib/stage-cache";

export type AttachmentModality = "image" | "pdf" | "audio" | "video";
export type RichMediaKind = "pdf" | "audio" | "video";

const IMAGE_EXTS = new Set([
  "png",
  "jpg",
  "jpeg",
  "webp",
  "gif",
  "bmp",
  "ico",
  "svg",
  "avif",
]);
const AUDIO_EXTS = new Set(["mp3", "wav", "m4a", "aac", "ogg", "flac", "opus", "weba"]);
const VIDEO_EXTS = new Set(["mp4", "mov", "m4v", "webm", "mkv"]);

export function basenameOfPath(rawPath: string): string {
  const withoutVerbatim = stripWindowsVerbatimPrefix(rawPath);
  const normalized = withoutVerbatim.replace(/\\/g, "/").replace(/\/+$/, "");
  const idx = normalized.lastIndexOf("/");
  const rawBase = idx >= 0 ? normalized.slice(idx + 1) : normalized;
  return rawBase.replace(/[\r\n`]+/g, "").trim();
}

export function extensionOfPath(rawPath: string): string {
  const base = basenameOfPath(rawPath).toLowerCase();
  const dot = base.lastIndexOf(".");
  return dot >= 0 ? base.slice(dot + 1) : "";
}

export function detectAttachmentModality(
  filename: string,
  mimeType?: string | null,
): AttachmentModality | null {
  const mime = (mimeType ?? "").trim().toLowerCase();
  if (mime.startsWith("image/")) return "image";
  if (mime === "application/pdf") return "pdf";
  if (mime.startsWith("audio/")) return "audio";
  if (mime.startsWith("video/")) return "video";

  const ext = extensionOfPath(filename);
  if (!ext) return null;
  if (ext === "pdf") return "pdf";
  if (IMAGE_EXTS.has(ext)) return "image";
  if (AUDIO_EXTS.has(ext)) return "audio";
  if (VIDEO_EXTS.has(ext)) return "video";
  return null;
}

export function detectRichMediaKind(
  filename: string,
  mimeType?: string | null,
): RichMediaKind | null {
  const modality = detectAttachmentModality(filename, mimeType);
  if (modality === "pdf" || modality === "audio" || modality === "video") {
    return modality;
  }
  return null;
}

export function isRichMediaContentKind(kind: string | null | undefined): kind is RichMediaKind {
  return kind === "pdf" || kind === "audio" || kind === "video";
}

export function defaultMimeForMedia(filename: string, kind: RichMediaKind): string {
  const ext = extensionOfPath(filename);
  if (kind === "pdf") return "application/pdf";
  if (kind === "audio") {
    switch (ext) {
      case "mp3":
        return "audio/mpeg";
      case "wav":
        return "audio/wav";
      case "ogg":
        return "audio/ogg";
      case "flac":
        return "audio/flac";
      case "m4a":
        return "audio/mp4";
      case "aac":
        return "audio/aac";
      case "opus":
      case "weba":
        return "audio/webm";
      default:
        return "audio/mpeg";
    }
  }
  switch (ext) {
    case "mov":
      return "video/quicktime";
    case "webm":
      return "video/webm";
    case "mkv":
      return "video/x-matroska";
    default:
      return "video/mp4";
  }
}

export function base64ToBytes(b64: string): Uint8Array {
  const binary = window.atob(b64);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i += 1) {
    bytes[i] = binary.charCodeAt(i);
  }
  return bytes;
}

export function base64ToBlobUrl(
  b64: string,
  mimeType: string,
): { url: string; bytes: Uint8Array } {
  const bytes = base64ToBytes(b64);
  const blob = new Blob([bytes.buffer as ArrayBuffer], { type: mimeType });
  return { url: URL.createObjectURL(blob), bytes };
}

function isPdfWhitespace(b: number): boolean {
  return b === 0x20 || b === 0x09 || b === 0x0d || b === 0x0a || b === 0x0c || b === 0x00;
}

function isPdfDelimiter(b: number): boolean {
  return (
    b === 0x28 || // (
    b === 0x29 || // )
    b === 0x3c || // <
    b === 0x3e || // >
    b === 0x5b || // [
    b === 0x5d || // ]
    b === 0x7b || // {
    b === 0x7d || // }
    b === 0x2f || // /
    b === 0x25 // %
  );
}

function isPdfBoundary(bytes: Uint8Array, idx: number): boolean {
  return idx >= bytes.length || isPdfWhitespace(bytes[idx]) || isPdfDelimiter(bytes[idx]);
}

function matchesAscii(bytes: Uint8Array, offset: number, token: string): boolean {
  if (offset + token.length > bytes.length) return false;
  for (let i = 0; i < token.length; i += 1) {
    if (bytes[offset + i] !== token.charCodeAt(i)) return false;
  }
  return true;
}

function parsePagesDictCount(bytes: Uint8Array, pagesPos: number): number | null {
  const backStart = Math.max(0, pagesPos - 8192);
  let dictStart: number | null = null;
  let backDepth = 0;
  let k = pagesPos;
  while (k >= backStart + 2) {
    k -= 1;
    if (bytes[k - 1] === 0x3e && bytes[k] === 0x3e) {
      backDepth += 1;
      k -= 1;
    } else if (bytes[k - 1] === 0x3c && bytes[k] === 0x3c) {
      if (backDepth === 0) {
        dictStart = k - 1;
        break;
      }
      backDepth -= 1;
      k -= 1;
    }
  }
  if (dictStart === null) return null;

  const endLimit = Math.min(bytes.length, pagesPos + 8192);
  let depth = 0;
  let i = dictStart;
  while (i < endLimit) {
    if (i + 1 < endLimit && bytes[i] === 0x3c && bytes[i + 1] === 0x3c) {
      depth += 1;
      i += 2;
      continue;
    }
    if (i + 1 < endLimit && bytes[i] === 0x3e && bytes[i + 1] === 0x3e) {
      depth -= 1;
      i += 2;
      if (depth <= 0) break;
      continue;
    }
    if (depth === 1 && matchesAscii(bytes, i, "/Count") && isPdfBoundary(bytes, i + 6)) {
      let j = i + 6;
      while (j < endLimit && isPdfWhitespace(bytes[j])) j += 1;
      let val = 0;
      let digits = 0;
      while (j < endLimit && bytes[j] >= 0x30 && bytes[j] <= 0x39 && digits < 9) {
        val = val * 10 + (bytes[j] - 0x30);
        j += 1;
        digits += 1;
      }
      if (digits > 0 && isPdfBoundary(bytes, j) && val > 0 && val <= 100_000) {
        return val;
      }
    }
    i += 1;
  }
  return null;
}

/**
 * Fast, bounded, pure-TS PDF page counter matching `orbit-providers/src/attachments.rs`.
 * Prefers the authoritative `/Count N` inside `/Type /Pages` dictionaries (avoiding
 * double-counting incrementally saved PDFs) and falls back to leaf `/Type /Page` counts.
 */
export function estimatePdfPageCount(bytes: Uint8Array): number | null {
  let start = 0;
  if (bytes.length >= 3 && bytes[0] === 0xef && bytes[1] === 0xbb && bytes[2] === 0xbf) {
    start = 3;
  }
  if (!matchesAscii(bytes, start, "%PDF-")) {
    return null;
  }
  const limit = Math.min(bytes.length, 26 * 1024 * 1024);
  let leafPages = 0;
  let maxDictCount: number | null = null;

  for (let i = start + 5; i < limit; ) {
    if (bytes[i] !== 0x2f) {
      i += 1;
      continue;
    }
    if (matchesAscii(bytes, i, "/Type") && isPdfBoundary(bytes, i + 5)) {
      let j = i + 5;
      while (j < limit && isPdfWhitespace(bytes[j])) j += 1;
      if (matchesAscii(bytes, j, "/Pages") && isPdfBoundary(bytes, j + 6)) {
        const count = parsePagesDictCount(bytes, j);
        if (count !== null && (maxDictCount === null || count > maxDictCount)) {
          maxDictCount = count;
        }
        i = j + 6;
        continue;
      }
      if (matchesAscii(bytes, j, "/Page") && isPdfBoundary(bytes, j + 5)) {
        leafPages += 1;
        i = j + 5;
        continue;
      }
      i += 5;
      continue;
    }
    i += 1;
  }

  return maxDictCount ?? (leafPages > 0 ? leafPages : null);
}

export function formatMediaDuration(seconds: number): string {
  if (!Number.isFinite(seconds) || seconds < 0) return "00:00";
  const whole = Math.floor(seconds);
  const hrs = Math.floor(whole / 3600);
  const mins = Math.floor((whole % 3600) / 60);
  const secs = whole % 60;
  const pad = (n: number) => String(n).padStart(2, "0");
  if (hrs > 0) {
    return `${hrs}:${pad(mins)}:${pad(secs)}`;
  }
  return `${pad(mins)}:${pad(secs)}`;
}

/**
 * Normalize a user-entered page spec (e.g. `4`, `4-6`, `p. 4-6`, `2, 5-7`) into a concise
 * Composer citation such as `spec.pdf (p. 4-6)`.
 */
export function parsePdfPageCitation(filename: string, rawInput: string): string | null {
  const cleaned = rawInput
    .trim()
    .replace(/^pp?\.?\s*/i, "")
    .replace(/\s+/g, " ");
  if (!cleaned) return null;
  if (!/^\d+(?:\s*[-–]\s*\d+)?(?:\s*,\s*\d+(?:\s*[-–]\s*\d+)?)*$/.test(cleaned)) {
    return null;
  }
  const normalized = cleaned.replace(/\s*[-–]\s*/g, "-").replace(/\s*,\s*/g, ", ");
  return `${basenameOfPath(filename)} (p. ${normalized})`;
}

export function formatTimestampCitation(filename: string, seconds: number): string {
  return `${basenameOfPath(filename)} @ ${formatMediaDuration(seconds)}`;
}

export function downloadBlobUrl(url: string, filename: string): void {
  const link = document.createElement("a");
  link.href = url;
  link.download = basenameOfPath(filename) || "download";
  link.rel = "noopener";
  document.body.appendChild(link);
  link.click();
  document.body.removeChild(link);
}
