import { useEffect, useState } from "react";
import { FILE_LOADING_GRACE_MS } from "@/lib/file-prefetch";

/**
 * Suppresses transient loading spinners for fast operations (e.g. local disk reads finishing
 * in 5–50ms), revealing the indicator only if loading genuinely exceeds `delayMs`.
 */
export function useDelayedLoading(
  active: boolean,
  delayMs: number = FILE_LOADING_GRACE_MS,
): boolean {
  const [visible, setVisible] = useState(false);

  useEffect(() => {
    if (!active) {
      setVisible(false);
      return;
    }
    const timer = window.setTimeout(() => {
      setVisible(true);
    }, delayMs);
    return () => {
      window.clearTimeout(timer);
    };
  }, [active, delayMs]);

  return active && visible;
}
