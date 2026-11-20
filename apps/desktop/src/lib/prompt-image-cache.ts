import { orbitIpc } from "@/lib/orbit-ipc";

export type PromptImageEntry =
  | {
      status: "ready";
      url: string;
      resolvedPath: string;
      ownedBlob: boolean;
    }
  | {
      status: "error";
    };

export interface SentImagePreviewMeta {
  label: string;
  path: string | null;
  previewUrl: string | null;
}

interface RecentSentTurnImagesRecord {
  hostId: string;
  agentId: string | null;
  displayText: string;
  images: SentImagePreviewMeta[];
}

const MAX_CACHE_ENTRIES = 64;
const MAX_RECENT_SENT_TURNS = 24;

const imageCache = new Map<string, PromptImageEntry>();
const inflightLoads = new Map<string, Promise<PromptImageEntry>>();
const recentSentTurns: RecentSentTurnImagesRecord[] = [];

function buildCacheKey(hostId: string, path: string): string {
  return `${hostId}::${path.trim()}`;
}

function setCachedEntry(key: string, entry: PromptImageEntry): void {
  const prev = imageCache.get(key);
  if (prev) {
    if (prev.status === "ready" && prev.ownedBlob && (entry.status !== "ready" || entry.url !== prev.url)) {
      URL.revokeObjectURL(prev.url);
    }
    imageCache.delete(key);
  }
  imageCache.set(key, entry);

  while (imageCache.size > MAX_CACHE_ENTRIES) {
    const oldest = imageCache.entries().next().value;
    if (!oldest) break;
    const [oldKey, oldVal] = oldest;
    imageCache.delete(oldKey);
    if (oldVal.status === "ready" && oldVal.ownedBlob) {
      URL.revokeObjectURL(oldVal.url);
    }
  }
}

function base64ToBlob(base64: string, mimeType: string): Blob {
  const binary = atob(base64);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i += 1) {
    bytes[i] = binary.charCodeAt(i);
  }
  return new Blob([bytes], { type: mimeType });
}

export function seedPromptImagePreview(
  hostId: string,
  path: string,
  blobUrl: string,
  ownedBlob = false,
): void {
  const cleanPath = path.trim();
  if (!hostId || !cleanPath || !blobUrl) return;
  const key = buildCacheKey(hostId, cleanPath);
  setCachedEntry(key, {
    status: "ready",
    url: blobUrl,
    resolvedPath: cleanPath,
    ownedBlob,
  });
}

export function recordRecentSentTurnImages(
  hostId: string,
  agentId: string | null,
  displayText: string,
  images: SentImagePreviewMeta[],
): void {
  if (!hostId || images.length === 0) return;
  recentSentTurns.unshift({
    hostId,
    agentId,
    displayText: displayText.trim(),
    images: images.map((img) => ({
      label: img.label.trim(),
      path: img.path?.trim() || null,
      previewUrl: img.previewUrl || null,
    })),
  });
  if (recentSentTurns.length > MAX_RECENT_SENT_TURNS) {
    recentSentTurns.length = MAX_RECENT_SENT_TURNS;
  }
}

export function lookupRecentSentTurnImage(
  hostId: string,
  agentId: string | null,
  displayText: string,
  imageIndex: number,
  label: string,
): SentImagePreviewMeta | null {
  const cleanText = displayText.trim();
  const cleanLabel = label.trim();
  if (!hostId || !cleanLabel) return null;

  const findInRecord = (rec: RecentSentTurnImagesRecord): SentImagePreviewMeta | null => {
    const atIndex = rec.images[imageIndex];
    if (atIndex && (!cleanLabel || atIndex.label === cleanLabel)) {
      return atIndex;
    }
    if (atIndex && rec.images.length > imageIndex) {
      return atIndex;
    }
    return rec.images.find((img) => img.label === cleanLabel) ?? null;
  };

  let draftMatch: SentImagePreviewMeta | null = null;
  for (const rec of recentSentTurns) {
    if (rec.hostId !== hostId || rec.displayText !== cleanText) continue;
    const hit = findInRecord(rec);
    if (!hit) continue;
    if (agentId && rec.agentId === agentId) return hit;
    if (!draftMatch && rec.agentId === null) draftMatch = hit;
  }
  if (draftMatch) return draftMatch;

  // Fallback by agent/label match in case server normalized whitespace in display_prompt
  for (const rec of recentSentTurns) {
    if (rec.hostId !== hostId) continue;
    if (agentId && rec.agentId !== agentId && rec.agentId !== null) continue;
    const atIdx = rec.images[imageIndex];
    if (atIdx && atIdx.label === cleanLabel) return atIdx;
  }
  return null;
}

export function getCachedPromptImage(hostId: string, path: string): PromptImageEntry | null {
  const cleanPath = path.trim();
  if (!hostId || !cleanPath) return null;
  const key = buildCacheKey(hostId, cleanPath);
  const existing = imageCache.get(key);
  if (!existing) return null;
  // Refresh LRU recency
  imageCache.delete(key);
  imageCache.set(key, existing);
  return existing;
}

export async function loadPromptImage(
  hostId: string,
  cwd: string,
  path: string,
): Promise<PromptImageEntry> {
  const cleanPath = path.trim();
  if (!hostId || !cleanPath) {
    return { status: "error" };
  }

  const cached = getCachedPromptImage(hostId, cleanPath);
  if (cached) {
    return cached;
  }

  const key = buildCacheKey(hostId, cleanPath);
  const existingFlight = inflightLoads.get(key);
  if (existingFlight) {
    return existingFlight;
  }

  const promise = (async (): Promise<PromptImageEntry> => {
    try {
      const file = await orbitIpc.readWorkspaceFile(hostId, cwd || "~", cleanPath);
      if (file.content_kind === "image" && file.mime_type && file.content) {
        const blob = base64ToBlob(file.content, file.mime_type);
        const url = URL.createObjectURL(blob);
        const entry: PromptImageEntry = {
          status: "ready",
          url,
          resolvedPath: file.resolved_path || cleanPath,
          ownedBlob: true,
        };
        setCachedEntry(key, entry);
        return entry;
      }
      const errEntry: PromptImageEntry = { status: "error" };
      setCachedEntry(key, errEntry);
      return errEntry;
    } catch {
      const errEntry: PromptImageEntry = { status: "error" };
      setCachedEntry(key, errEntry);
      return errEntry;
    } finally {
      inflightLoads.delete(key);
    }
  })();

  inflightLoads.set(key, promise);
  return promise;
}
