export type DetectedPlatform = "mac" | "win" | "linux" | "mobile";

/**
 * Single source of truth for client OS detection across the landing & download pages.
 * Detects desktop OSes for direct installer links and routes phones/tablets to /download/
 * so mobile visitors are never handed a macOS .dmg.
 */
export function detectClientPlatform(): DetectedPlatform {
  const nav = navigator as Navigator & { userAgentData?: { platform?: string; mobile?: boolean } };
  const ua = (navigator.userAgent || "").toLowerCase();
  if (nav.userAgentData?.mobile || /iphone|ipad|ipod|android|harmonyos|mobile/.test(ua)) {
    return "mobile";
  }
  const raw = (nav.userAgentData?.platform || navigator.platform || ua).toLowerCase();
  if (raw.includes("win")) return "win";
  if (raw.includes("linux")) return "linux";
  return "mac";
}

export function hydrateOsDownloadButtons(root: ParentNode, platform: DetectedPlatform): void {
  root.querySelectorAll<HTMLAnchorElement>("[data-os-download]").forEach((anchor) => {
    let href: string | undefined;
    let label: string | undefined;
    if (platform === "win") {
      href = anchor.dataset.hrefWin;
      label = anchor.dataset.labelWin;
    } else if (platform === "linux") {
      href = anchor.dataset.hrefLinux;
      label = anchor.dataset.labelLinux;
    } else if (platform === "mobile") {
      href = anchor.dataset.hrefMobile || anchor.dataset.hrefMac;
      label = anchor.dataset.labelMobile || anchor.dataset.labelMac;
    } else {
      href = anchor.dataset.hrefMac;
      label = anchor.dataset.labelMac;
    }
    if (href) anchor.href = href;
    const span = anchor.querySelector<HTMLElement>("[data-os-label]");
    if (span && label) span.textContent = label;
  });
}
