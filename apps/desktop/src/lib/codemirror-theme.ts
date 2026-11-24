import type { Extension } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { HighlightStyle, syntaxHighlighting } from "@codemirror/language";
import { tags as t } from "@lezer/highlight";

const ORBIT_MONO_STACK =
  '"JetBrains Mono", "SF Mono", ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace';

const ORBIT_SANS_STACK =
  '-apple-system, BlinkMacSystemFont, "SF Pro Text", "PingFang SC", "Hiragino Sans GB", "Inter", "Segoe UI", Roboto, sans-serif';

/**
 * Restrained OKLCH-harmonized syntax palette for Light mode (`Alabaster Paper`).
 * Every token keeps disciplined chroma (<= 0.145) and WCAG-AA legibility against `--surface-base`.
 */
export const orbitLightHighlightStyle = HighlightStyle.define([
  { tag: [t.keyword, t.modifier, t.controlKeyword, t.moduleKeyword], color: "oklch(0.47 0.145 308)" },
  { tag: [t.operatorKeyword, t.definitionKeyword], color: "oklch(0.47 0.145 308)" },
  { tag: [t.function(t.variableName), t.function(t.propertyName), t.definition(t.function(t.variableName))], color: "oklch(0.44 0.14 254)" },
  { tag: [t.className, t.typeName, t.namespace, t.labelName, t.macroName], color: "oklch(0.46 0.125 218)" },
  { tag: [t.string, t.special(t.string), t.regexp, t.inserted], color: "oklch(0.43 0.125 152)" },
  { tag: [t.number, t.integer, t.float, t.bool, t.atom, t.constant(t.variableName)], color: "oklch(0.48 0.14 55)" },
  { tag: [t.propertyName, t.attributeName], color: "oklch(0.42 0.105 258)" },
  { tag: [t.variableName, t.self], color: "var(--ink-primary)" },
  { tag: [t.tagName, t.deleted], color: "oklch(0.47 0.145 24)" },
  { tag: [t.comment, t.lineComment, t.blockComment, t.docComment], color: "oklch(0.56 0.01 258)", fontStyle: "italic" },
  { tag: [t.meta, t.processingInstruction, t.annotation], color: "oklch(0.50 0.08 258)" },
  { tag: [t.operator, t.punctuation, t.separator, t.bracket, t.angleBracket], color: "oklch(0.45 0.01 258)" },
  { tag: [t.heading], color: "var(--ink-primary)", fontWeight: "600" },
  { tag: [t.strong], fontWeight: "600" },
  { tag: [t.emphasis], fontStyle: "italic" },
  { tag: [t.link, t.url], color: "var(--signal-info)", textDecoration: "underline" },
  { tag: [t.invalid], color: "var(--signal-danger)" },
]);

/**
 * Restrained OKLCH-harmonized syntax palette for Dark mode (`Obsidian Graphite`).
 */
export const orbitDarkHighlightStyle = HighlightStyle.define([
  { tag: [t.keyword, t.modifier, t.controlKeyword, t.moduleKeyword], color: "oklch(0.76 0.115 308)" },
  { tag: [t.operatorKeyword, t.definitionKeyword], color: "oklch(0.76 0.115 308)" },
  { tag: [t.function(t.variableName), t.function(t.propertyName), t.definition(t.function(t.variableName))], color: "oklch(0.76 0.115 248)" },
  { tag: [t.className, t.typeName, t.namespace, t.labelName, t.macroName], color: "oklch(0.79 0.105 84)" },
  { tag: [t.string, t.special(t.string), t.regexp, t.inserted], color: "oklch(0.76 0.115 150)" },
  { tag: [t.number, t.integer, t.float, t.bool, t.atom, t.constant(t.variableName)], color: "oklch(0.76 0.12 52)" },
  { tag: [t.propertyName, t.attributeName], color: "oklch(0.78 0.09 212)" },
  { tag: [t.variableName, t.self], color: "var(--ink-primary)" },
  { tag: [t.tagName, t.deleted], color: "oklch(0.74 0.13 22)" },
  { tag: [t.comment, t.lineComment, t.blockComment, t.docComment], color: "oklch(0.56 0.012 258)", fontStyle: "italic" },
  { tag: [t.meta, t.processingInstruction, t.annotation], color: "oklch(0.70 0.065 258)" },
  { tag: [t.operator, t.punctuation, t.separator, t.bracket, t.angleBracket], color: "oklch(0.70 0.01 258)" },
  { tag: [t.heading], color: "var(--ink-primary)", fontWeight: "600" },
  { tag: [t.strong], fontWeight: "600" },
  { tag: [t.emphasis], fontStyle: "italic" },
  { tag: [t.link, t.url], color: "var(--signal-info)", textDecoration: "underline" },
  { tag: [t.invalid], color: "var(--signal-danger)" },
]);

/**
 * Tear-free CodeMirror 6 chrome + syntax theme bound directly to Orbit's OKLCH CSS tokens.
 */
export function createOrbitCodeMirrorTheme(isDark: boolean): Extension {
  const activeLineBg = isDark ? "oklch(1 0 0 / 3.6%)" : "oklch(0.18 0.01 258 / 3.2%)";
  const selectionBg = isDark ? "oklch(0.72 0.13 252 / 28%)" : "oklch(0.62 0.14 252 / 20%)";
  const selectionMatchBg = isDark ? "oklch(0.72 0.13 252 / 14%)" : "oklch(0.62 0.14 252 / 11%)";
  const floatingShadow = isDark
    ? "inset 0 0.5px 0 0 oklch(1 0 0 / 7.5%), 0 8px 28px -4px oklch(0 0 0 / 42%), 0 2px 8px 0 oklch(0 0 0 / 24%)"
    : "0 4px 16px -2px oklch(0 0 0 / 8%), 0 1px 4px 0 oklch(0 0 0 / 4%)";

  const chromeTheme = EditorView.theme(
    {
      "&": {
        height: "100%",
        backgroundColor: "var(--surface-base)",
        color: "var(--ink-primary)",
        fontFamily: ORBIT_MONO_STACK,
        fontSize: "13px",
        lineHeight: "20px",
        outline: "none",
      },
      "&.cm-focused": {
        outline: "none",
      },
      ".cm-scroller": {
        fontFamily: ORBIT_MONO_STACK,
        fontSize: "13px",
        lineHeight: "20px",
        overflow: "auto",
        overscrollBehavior: "contain",
      },
      ".cm-content": {
        padding: "10px 0 28px 0",
        caretColor: "var(--ink-primary)",
        userSelect: "text",
        WebkitUserSelect: "text",
      },
      ".cm-line": {
        padding: "0 14px 0 10px",
      },
      ".cm-cursor, .cm-dropCursor": {
        borderLeftColor: "var(--ink-primary)",
        borderLeftWidth: "1.5px",
      },
      "&.cm-focused > .cm-scroller > .cm-selectionLayer .cm-selectionBackground, .cm-selectionBackground, .cm-content ::selection":
        {
          backgroundColor: selectionBg,
        },
      ".cm-selectionMatch": {
        backgroundColor: selectionMatchBg,
        borderRadius: "2px",
      },
      ".cm-activeLine": {
        backgroundColor: activeLineBg,
      },
      ".cm-gutters": {
        backgroundColor: "var(--surface-base)",
        color: "var(--ink-faint)",
        borderRight: "1px solid var(--border-subtle)",
        fontFamily: ORBIT_MONO_STACK,
        fontSize: "11px",
        fontVariantNumeric: "tabular-nums",
        userSelect: "none",
      },
      ".cm-lineNumbers .cm-gutterElement": {
        padding: "0 10px 0 10px",
        minWidth: "36px",
        lineHeight: "20px",
      },
      ".cm-activeLineGutter": {
        backgroundColor: activeLineBg,
        color: "var(--ink-primary)",
        fontWeight: "500",
      },
      ".cm-gutter-lint": {
        width: "14px",
      },
      ".cm-lint-marker": {
        width: "8px",
        height: "8px",
        marginTop: "6px",
        marginLeft: "4px",
        borderRadius: "9999px",
        content: '""',
      },
      ".cm-lint-marker-error": {
        content: '""',
        backgroundColor: "var(--signal-danger)",
      },
      ".cm-lint-marker-warning": {
        content: '""',
        backgroundColor: "var(--signal-warning)",
      },
      ".cm-lint-marker-info": {
        content: '""',
        backgroundColor: "var(--signal-info)",
      },
      ".cm-lint-marker-hint": {
        content: '""',
        backgroundColor: "var(--ink-tertiary)",
      },
      ".cm-lintRange-error": {
        backgroundImage: "none",
        textDecoration: "underline wavy var(--signal-danger)",
        textUnderlineOffset: "2px",
        textDecorationThickness: "1.2px",
      },
      ".cm-lintRange-warning": {
        backgroundImage: "none",
        textDecoration: "underline wavy var(--signal-warning)",
        textUnderlineOffset: "2px",
        textDecorationThickness: "1.2px",
      },
      ".cm-lintRange-info": {
        backgroundImage: "none",
        textDecoration: "underline wavy var(--signal-info)",
        textUnderlineOffset: "2px",
        textDecorationThickness: "1.2px",
      },
      ".cm-lintRange-hint": {
        backgroundImage: "none",
        textDecoration: "underline dotted var(--ink-tertiary)",
        textUnderlineOffset: "2px",
      },
      "&.cm-focused .cm-matchingBracket": {
        backgroundColor: "var(--accent)",
        outline: "1px solid var(--border-strong)",
        borderRadius: "2px",
      },
      ".cm-searchMatch": {
        backgroundColor: "var(--signal-warning-bg)",
        outline: "1px solid var(--signal-warning-border)",
        borderRadius: "2px",
      },
      ".cm-searchMatch.cm-searchMatch-selected": {
        backgroundColor: "var(--signal-info-bg)",
        outline: "1px solid var(--signal-info-border)",
      },
      /* Built-in Cmd+F / Cmd+H Find & Replace Panel */
      ".cm-panels": {
        backgroundColor: "var(--surface-recessed)",
        color: "var(--ink-primary)",
        fontFamily: ORBIT_SANS_STACK,
        fontSize: "12px",
        zIndex: 5,
      },
      ".cm-panels.cm-panels-top": {
        borderBottom: "1px solid var(--border-default)",
      },
      ".cm-panels.cm-panels-bottom": {
        borderTop: "1px solid var(--border-default)",
      },
      ".cm-panel.cm-search": {
        padding: "6px 10px",
        display: "flex",
        flexWrap: "wrap",
        alignItems: "center",
        gap: "5px 6px",
        backgroundColor: "var(--surface-recessed)",
      },
      ".cm-panel.cm-search br": {
        flexBasis: "100%",
        height: "0",
      },
      ".cm-panel.cm-search input[main-field], .cm-panel.cm-search .cm-textfield": {
        backgroundColor: "var(--surface-overlay)",
        color: "var(--ink-primary)",
        border: "1px solid var(--border-default)",
        borderRadius: "6px",
        padding: "2px 8px",
        height: "24px",
        fontSize: "12px",
        fontFamily: ORBIT_MONO_STACK,
        outline: "none",
        minWidth: "150px",
      },
      ".cm-panel.cm-search .cm-textfield:focus": {
        borderColor: "var(--border-strong)",
      },
      ".cm-panel.cm-search .cm-button": {
        backgroundImage: "none",
        backgroundColor: "var(--surface-overlay)",
        color: "var(--ink-primary)",
        border: "1px solid var(--border-default)",
        borderRadius: "6px",
        padding: "0 8px",
        height: "24px",
        fontSize: "11.5px",
        fontWeight: "500",
        fontFamily: ORBIT_SANS_STACK,
        cursor: "pointer",
        lineHeight: "22px",
      },
      ".cm-panel.cm-search .cm-button:hover": {
        backgroundColor: "var(--accent)",
        borderColor: "var(--border-strong)",
      },
      ".cm-panel.cm-search label": {
        display: "inline-flex",
        alignItems: "center",
        gap: "4px",
        fontSize: "11.5px",
        color: "var(--ink-secondary)",
        cursor: "pointer",
        margin: "0 2px",
      },
      ".cm-panel.cm-search button[name=close]": {
        backgroundColor: "transparent",
        color: "var(--ink-tertiary)",
        border: "none",
        borderRadius: "6px",
        cursor: "pointer",
        fontSize: "15px",
        width: "22px",
        height: "22px",
        top: "6px",
        right: "8px",
      },
      ".cm-panel.cm-search button[name=close]:hover": {
        backgroundColor: "var(--accent)",
        color: "var(--ink-primary)",
      },
      /* Floating Tooltips: Hover, Autocomplete & Lint Diagnostics */
      ".cm-tooltip": {
        backgroundColor: "var(--surface-overlay)",
        color: "var(--ink-primary)",
        border: "1px solid var(--border-default)",
        borderRadius: "8px",
        boxShadow: floatingShadow,
        fontFamily: ORBIT_SANS_STACK,
        fontSize: "12px",
        overflow: "hidden",
      },
      ".cm-tooltip.cm-tooltip-autocomplete": {
        padding: "4px",
        minWidth: "220px",
        maxWidth: "380px",
      },
      ".cm-tooltip.cm-tooltip-autocomplete > ul": {
        fontFamily: ORBIT_MONO_STACK,
        fontSize: "12px",
        maxHeight: "220px",
      },
      ".cm-tooltip.cm-tooltip-autocomplete > ul > li": {
        display: "flex",
        alignItems: "center",
        gap: "6px",
        padding: "3px 8px",
        borderRadius: "5px",
        lineHeight: "18px",
      },
      ".cm-tooltip.cm-tooltip-autocomplete > ul > li[aria-selected]": {
        backgroundColor: "var(--accent)",
        color: "var(--ink-primary)",
      },
      ".cm-completionLabel": {
        color: "var(--ink-primary)",
        fontWeight: "500",
      },
      ".cm-completionMatchedText": {
        textDecoration: "none",
        color: "var(--signal-info)",
        fontWeight: "600",
      },
      ".cm-completionDetail": {
        marginLeft: "auto",
        color: "var(--ink-tertiary)",
        fontSize: "11px",
        fontStyle: "normal",
        overflow: "hidden",
        textOverflow: "ellipsis",
        maxWidth: "160px",
        whiteSpace: "nowrap",
      },
      ".cm-completionIcon": {
        width: "14px",
        fontSize: "10.5px",
        color: "var(--ink-tertiary)",
        opacity: "0.9",
        paddingRight: "0",
      },
      ".cm-tooltip.cm-completionInfo": {
        padding: "8px 10px",
        maxWidth: "340px",
        maxHeight: "220px",
        overflow: "auto",
        color: "var(--ink-secondary)",
        fontSize: "12px",
        lineHeight: "1.45",
        whiteSpace: "pre-wrap",
      },
      ".cm-tooltip-lint": {
        maxWidth: "440px",
        padding: "0",
      },
      ".cm-diagnostic": {
        padding: "8px 10px",
        borderLeftWidth: "3px",
        borderLeftStyle: "solid",
        fontSize: "12px",
        lineHeight: "1.45",
      },
      ".cm-diagnostic-error": {
        borderLeftColor: "var(--signal-danger)",
      },
      ".cm-diagnostic-warning": {
        borderLeftColor: "var(--signal-warning)",
      },
      ".cm-diagnostic-info": {
        borderLeftColor: "var(--signal-info)",
      },
      ".cm-diagnostic-hint": {
        borderLeftColor: "var(--ink-tertiary)",
      },
      ".cm-diagnosticSource": {
        fontFamily: ORBIT_MONO_STACK,
        fontSize: "10.5px",
        color: "var(--ink-tertiary)",
        marginTop: "2px",
      },
      ".cm-diagnosticAction": {
        marginTop: "6px",
        marginRight: "6px",
        padding: "2px 8px",
        borderRadius: "5px",
        border: "1px solid var(--border-default)",
        backgroundColor: "var(--surface-recessed)",
        color: "var(--ink-primary)",
        fontSize: "11px",
        fontWeight: "500",
        cursor: "pointer",
      },
      ".cm-diagnosticAction:hover": {
        backgroundColor: "var(--accent)",
        borderColor: "var(--border-strong)",
      },
      /* Orbit LSP Hover & Quick Fix DOM cards inside CM tooltips */
      ".orbit-cm-hover": {
        padding: "8px 10px",
        maxWidth: "440px",
        maxHeight: "260px",
        overflow: "auto",
        display: "flex",
        flexDirection: "column",
        gap: "6px",
      },
      ".orbit-cm-hover-code": {
        margin: "0",
        padding: "6px 8px",
        borderRadius: "6px",
        backgroundColor: "var(--surface-recessed)",
        border: "1px solid var(--border-subtle)",
        fontFamily: ORBIT_MONO_STACK,
        fontSize: "12px",
        lineHeight: "18px",
        color: "var(--ink-primary)",
        whiteSpace: "pre-wrap",
        wordBreak: "break-word",
      },
      ".orbit-cm-hover-para": {
        margin: "0",
        fontSize: "12px",
        lineHeight: "1.5",
        color: "var(--ink-secondary)",
        whiteSpace: "pre-wrap",
        wordBreak: "break-word",
      },
      ".orbit-cm-hover-divider": {
        height: "1px",
        backgroundColor: "var(--border-subtle)",
        margin: "2px 0",
      },
      ".orbit-cm-diag-actions": {
        display: "flex",
        flexWrap: "wrap",
        gap: "5px",
        marginTop: "6px",
      },
      ".orbit-cm-diag-fix-btn": {
        display: "inline-flex",
        alignItems: "center",
        gap: "4px",
        padding: "2px 8px",
        borderRadius: "5px",
        border: "1px solid var(--border-default)",
        backgroundColor: "var(--surface-recessed)",
        color: "var(--ink-primary)",
        fontFamily: ORBIT_SANS_STACK,
        fontSize: "11px",
        fontWeight: "500",
        cursor: "pointer",
      },
      ".orbit-cm-diag-fix-btn:hover": {
        backgroundColor: "var(--accent)",
        borderColor: "var(--border-strong)",
      },
      ".orbit-cm-diag-fix-btn[data-preferred='true']": {
        borderColor: "var(--signal-info-border)",
        backgroundColor: "var(--signal-info-bg)",
      },
      /* M8.1 Inline FIM Ghost Text (Orbital Quiet Precision) */
      ".orbit-cm-ghost-text": {
        color: "var(--ink-faint)",
        opacity: isDark ? "0.78" : "0.76",
        fontFamily: ORBIT_MONO_STACK,
        fontSize: "13px",
        lineHeight: "20px",
        fontStyle: "italic",
        whiteSpace: "pre",
        pointerEvents: "none",
        userSelect: "none",
        WebkitUserSelect: "none",
      },
      ".orbit-cm-ghost-text-multiline": {
        display: "inline",
      },
    },
    { dark: isDark },
  );

  return [
    chromeTheme,
    syntaxHighlighting(isDark ? orbitDarkHighlightStyle : orbitLightHighlightStyle),
  ];
}
