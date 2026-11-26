import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { hasDraggedFiles } from "./drag-constants";

const handledFileDropEvents = new WeakSet<Event>();

/**
 * Installs a window-level guard preventing WKWebView / WebView2 / Browser from navigating to a
 * `file://` URL when a user drops an OS file outside an active drop zone (required when Tauri
 * `dragDropEnabled: false` routes native drops to HTML5 DOM events).
 */
export function installGlobalFileDropGuard(): () => void {
  if (typeof window === "undefined") return () => {};
  const onDragEnter = (e: DragEvent) => {
    if (hasDraggedFiles(e.dataTransfer)) {
      e.preventDefault();
    }
  };
  const onDragOver = (e: DragEvent) => {
    if (hasDraggedFiles(e.dataTransfer)) {
      e.preventDefault();
    }
  };
  const onDrop = (e: DragEvent) => {
    if (hasDraggedFiles(e.dataTransfer)) {
      e.preventDefault();
    }
  };
  window.addEventListener("dragenter", onDragEnter);
  window.addEventListener("dragover", onDragOver);
  window.addEventListener("drop", onDrop);
  return () => {
    window.removeEventListener("dragenter", onDragEnter);
    window.removeEventListener("dragover", onDragOver);
    window.removeEventListener("drop", onDrop);
  };
}

export interface UseFileDropOptions {
  disabled?: boolean;
  onDropFiles: (files: File[]) => void;
}

/**
 * Unified external file drop hook with strict `Files` MIME filtering, balanced depth tracking
 * (zero child-boundary flicker), global dragend/drop reset, and cross-ancestor drop deduplication.
 */
export function useFileDrop({ disabled = false, onDropFiles }: UseFileDropOptions) {
  const [isDraggingFiles, setIsDraggingFiles] = useState(false);
  const depthRef = useRef(0);
  const onDropFilesRef = useRef(onDropFiles);
  onDropFilesRef.current = onDropFiles;

  const reset = useCallback(() => {
    depthRef.current = 0;
    setIsDraggingFiles(false);
  }, []);

  useEffect(() => {
    if (disabled) reset();
  }, [disabled, reset]);

  useEffect(() => {
    if (typeof window === "undefined") return;
    const onGlobalReset = () => reset();
    window.addEventListener("dragend", onGlobalReset);
    window.addEventListener("drop", onGlobalReset);
    window.addEventListener("blur", onGlobalReset);
    return () => {
      window.removeEventListener("dragend", onGlobalReset);
      window.removeEventListener("drop", onGlobalReset);
      window.removeEventListener("blur", onGlobalReset);
    };
  }, [reset]);

  const onDragEnter = useCallback(
    (e: React.DragEvent<HTMLElement>) => {
      if (disabled || !hasDraggedFiles(e.dataTransfer)) return;
      e.preventDefault();
      depthRef.current += 1;
      setIsDraggingFiles(true);
    },
    [disabled],
  );

  const onDragOver = useCallback(
    (e: React.DragEvent<HTMLElement>) => {
      if (disabled || !hasDraggedFiles(e.dataTransfer)) return;
      e.preventDefault();
      try {
        e.dataTransfer.dropEffect = "copy";
      } catch {
        // Ignore read-only dropEffect in synthetic test events
      }
      setIsDraggingFiles(true);
    },
    [disabled],
  );

  const onDragLeave = useCallback(
    (e: React.DragEvent<HTMLElement>) => {
      if (disabled) return;
      if (depthRef.current > 0) {
        depthRef.current -= 1;
      }
      // On macOS WKWebView / Safari, `e.relatedTarget` is often `null` during child-boundary
      // transitions; only treat `relatedTarget` as an exit signal when non-null.
      const related = e.relatedTarget as Node | null;
      if (depthRef.current === 0 || (related !== null && !e.currentTarget.contains(related))) {
        depthRef.current = 0;
        setIsDraggingFiles(false);
      }
    },
    [disabled],
  );

  const onDrop = useCallback(
    (e: React.DragEvent<HTMLElement>) => {
      reset();
      if (disabled || !hasDraggedFiles(e.dataTransfer)) return;
      e.preventDefault();
      const nativeEv = e.nativeEvent;
      if (handledFileDropEvents.has(nativeEv)) return;
      handledFileDropEvents.add(nativeEv);

      const files = Array.from(e.dataTransfer.files ?? []);
      if (files.length > 0) {
        onDropFilesRef.current(files);
      }
    },
    [disabled, reset],
  );

  const dropHandlers = useMemo(
    () => ({
      onDragEnter,
      onDragOver,
      onDragLeave,
      onDrop,
    }),
    [onDragEnter, onDragOver, onDragLeave, onDrop],
  );

  return {
    isDraggingFiles,
    dropHandlers,
  };
}
