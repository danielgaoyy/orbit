# Orbit Bilingual Showcase & Documentation Site (`apps/docs`)

Production static website and documentation center for **Orbit**, built with **Astro 5 + Starlight** and **Pagefind** static search (native English + CJK Simplified Chinese tokenization, zero backend required).

## Features

- **100% EN + zh-CN Bilingual Parity**: English at root (`/`) and Simplified Chinese at `/zh-cn/`, with instant topbar language & theme switching.
- **Orbital Quiet Precision Editorial Showcase** (`src/components/OrbitLanding.astro` + `src/styles/orbit-theme.css`): Shares Orbit Desktop's OKLCH 258° cool-slate palette (*Alabaster Paper* light / *Obsidian Graphite* dark), interactive **Personal Agent Fleet Dispatch SVG topology**, Light/Dark 2× Retina product preview switcher, differentiation matrix, and copyable quickstart terminal.
- **Reproducible Product Screenshots**: Run `node apps/desktop/scripts/capture-showcase-screenshots.mjs` from the repo root to refresh both `docs/assets/orbit-desktop-{dark,light}.png` and `apps/docs/public/images/orbit-desktop-{dark,light}.png`.

## Commands

```bash
cd apps/docs
npm install
npm run dev      # Start local dev server
npm run build    # Static build + Pagefind search index into dist/
npm run preview  # Preview the built static site locally
```
