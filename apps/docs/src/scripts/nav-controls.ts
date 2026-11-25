/**
 * Shared navbar behavior for both flagship pages (OrbitNavbar) and Starlight docs
 * pages (OrbitDocsHeader): live GitHub star counter + mutually-exclusive disclosure menus.
 */
export function initOrbitNavbar(selector: string): void {
  const nav = document.querySelector<HTMLElement>(selector);
  if (!nav) return;

  // 1. Live GitHub Star Count (shows real count; early-returns on fresh 10-min cache)
  const starEl = nav.querySelector<HTMLElement>("[data-gh-stars]");
  if (starEl) {
    const cacheKey = "orbit-gh-stars-v3";
    const renderStars = (n: number) => {
      starEl.textContent =
        n >= 1000 ? `${(n / 1000).toFixed(1).replace(/\.0$/, "")}k` : String(n);
    };
    let cacheFresh = false;
    try {
      const cached = sessionStorage.getItem(cacheKey);
      if (cached) {
        const parsed = JSON.parse(cached);
        if (parsed && typeof parsed.n === "number" && Date.now() - parsed.ts < 600_000) {
          renderStars(parsed.n);
          cacheFresh = true;
        }
      }
    } catch {}
    if (!cacheFresh) {
      fetch("https://api.github.com/repos/danielgaoyy/orbit")
        .then((r) => (r.ok ? r.json() : null))
        .then((data) => {
          if (data && typeof data.stargazers_count === "number") {
            renderStars(data.stargazers_count);
            try {
              sessionStorage.setItem(
                cacheKey,
                JSON.stringify({ n: data.stargazers_count, ts: Date.now() }),
              );
            } catch {}
          }
        })
        .catch(() => {});
    }
  }

  // 2. Mutually-exclusive Header Disclosure Dropdowns with Focus Restore on Escape
  const navMenus = Array.from(nav.querySelectorAll<HTMLElement>("[data-nav-menu]"));
  const closeAllMenus = (except?: HTMLElement) => {
    navMenus.forEach((m) => {
      if (m === except) return;
      const trig = m.querySelector<HTMLButtonElement>("[data-menu-trigger]");
      const pop = m.querySelector<HTMLElement>("[data-menu-pop]");
      if (pop) pop.hidden = true;
      trig?.setAttribute("aria-expanded", "false");
    });
  };
  navMenus.forEach((menu) => {
    const trigger = menu.querySelector<HTMLButtonElement>("[data-menu-trigger]");
    const pop = menu.querySelector<HTMLElement>("[data-menu-pop]");
    if (!trigger || !pop) return;
    trigger.addEventListener("click", (e) => {
      e.stopPropagation();
      const willOpen = pop.hidden;
      closeAllMenus(menu);
      pop.hidden = !willOpen;
      trigger.setAttribute("aria-expanded", willOpen ? "true" : "false");
    });
  });
  document.addEventListener("click", (e) => {
    const target = e.target as Node;
    if (!navMenus.some((m) => m.contains(target))) closeAllMenus();
  });
  document.addEventListener("keydown", (e) => {
    if (e.key !== "Escape") return;
    const openMenu = navMenus.find(
      (m) => m.querySelector<HTMLElement>("[data-menu-pop]")?.hidden === false,
    );
    closeAllMenus();
    openMenu?.querySelector<HTMLButtonElement>("[data-menu-trigger]")?.focus();
  });
}
