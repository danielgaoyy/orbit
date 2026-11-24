# Orbit Design & Motion Constitution

Cross-platform UX, visual craft, and motion standards for `apps/desktop`, `apps/ios`, `apps/android`, and `apps/harmony`.

---

## 1. Product UX Principles — Absorb Complexity in the System

User experience comes first. Keep complexity inside the system; never leak it into the UI.

- **Decide what the system can decide** — Infer safe defaults, context, and routing automatically instead of asking the user to configure or choose.
- **Hide what isn't useful right now** — Earn every pixel. Omit idle chrome, dead actions, and controls irrelevant to the current state.
- **Act when intent is clear** — Execute safe, reversible actions directly instead of inserting redundant confirmations or intermediate steps.
- **One concept, one behavior everywhere** — Identical surfaces, spacing, affordances, and semantics across every pane, tab, and platform.
- **Immediate response, clean reversal** — Every press acknowledges instantly; every non-destructive action toggles back or undoes cleanly.
- **Never interrupt the user's flow** — Preserve focus, scroll position, selection, and unsent drafts across background updates and panel transitions.
- **Low friction for frequent actions; guarded for destructive ones** — Keep common paths one click or keystroke away; gate data-losing actions with clear confirmation or undo.
- **Self-explanatory interfaces** — Communicate through visual hierarchy, layout, and concise labels—never through explanatory paragraphs or internal jargon.

---

## 2. Motion Language — State-Transition First

### How to Audit Motion
Audit by **state change**, never by component list. For every transition—**appear, disappear, move, resize, swap content/icon/number, press, hover, load, complete, error**—ask: *Can the user immediately read what just happened, where it came from, and where it went, or did it snap abruptly?* This applies equally to full-pane transitions and a single icon, counter, or checkmark.

### Seven Motion Standards
1. **Explain, never decorate** — Motion clarifies origin, destination, and spatial relationship; never animate for ornament.
2. **One unified language** — Use one shared set of curves, durations, and choreography across the entire app (defined in `.interface-design/system.md` / platform equivalents). Never invent one-off timings.
3. **Restraint** — Keep transitions fast, light, and out of the way. Repeated actions should be even faster; stillness beats unnecessary movement.
4. **Direct and interruptible** — Drags and resizes track the pointer 1:1 with zero lag; new input mid-flight smoothly retargets the running animation.
5. **Zero performance tax** — Animate compositor-only properties (`transform`, `opacity`) so long timelines stay locked at 60/120fps. Measure before shipping.
6. **Zero regressions** — Never introduce jitter, flash, layout thrash, or clipping. Strictly honor OS reduced-motion preferences (`prefers-reduced-motion` / native equivalents).
7. **Universal across environments** — Every transition must hold across wide screens, narrow windows, mobile viewports, and light/dark themes.

---

## 3. Mandatory Workflow & Platform Alignment

Before modifying any UI, layout, token, or animation under `apps/`:

1. Read [`.agents/skills/interface-design/SKILL.md`](../../.agents/skills/interface-design/SKILL.md) (craft methodology) and `.interface-design/system.md` (Desktop tokens, typography, motion tokens, and spatial contracts).
2. On **Desktop (`apps/desktop`)**, read [`.agents/skills/shadcn/SKILL.md`](../../.agents/skills/shadcn/SKILL.md) and compose existing primitives in `src/components/ui/` (React 19 + Tailwind v4 + Base UI `@base-ui/react` + Lucide + `@xterm/xterm`). Default copy uses restrained English (`en.ts`) with strict 100% key/placeholder parity in `zh-CN.ts` and `ja.ts` (`i18n.test.ts`).
3. On **Mobile (`apps/ios`, `apps/android`, `apps/harmony`)**, follow native platform ergonomics (SwiftUI / Compose M3 / ArkUI V2) while preserving the same information architecture and principles above.
4. Keep business logic out of views—follow [`docs/CLIENT_BRIDGE_ARCHITECTURE.md`](../CLIENT_BRIDGE_ARCHITECTURE.md) so `orbit-client-core` remains the single source of truth.

### Pre-Submit Verification
- **Desktop**: `cd apps/desktop && npx tsc -p tsconfig.app.json --noEmit && node --experimental-strip-types --test "src/**/*.test.ts" && npm run build && npm run test:smoke` (`consoleErrors=0`).
- **iOS**: `xcodegen generate` (if structure changed) + `OrbitMobileLogicTests`.
- **Android / HarmonyOS**: Run the checks in the respective directory's `AGENTS.md`.
