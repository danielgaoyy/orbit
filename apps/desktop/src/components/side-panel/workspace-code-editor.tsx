import { memo, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Compartment, EditorSelection, EditorState, Prec } from "@codemirror/state";
import {
  EditorView,
  crosshairCursor,
  drawSelection,
  dropCursor,
  highlightActiveLine,
  highlightActiveLineGutter,
  highlightSpecialChars,
  hoverTooltip,
  keymap,
  lineNumbers,
  rectangularSelection,
} from "@codemirror/view";
import {
  defaultKeymap,
  history,
  historyKeymap,
  indentWithTab,
} from "@codemirror/commands";
import {
  bracketMatching,
  indentOnInput,
  indentUnit,
} from "@codemirror/language";
import {
  type CompletionContext,
  type CompletionResult,
  autocompletion,
  closeBrackets,
  closeBracketsKeymap,
  completionKeymap,
} from "@codemirror/autocomplete";
import {
  type Diagnostic as CmDiagnostic,
  lintGutter,
  lintKeymap,
  linter,
  setDiagnostics,
} from "@codemirror/lint";
import {
  highlightSelectionMatches,
  openSearchPanel,
  search,
  searchKeymap,
} from "@codemirror/search";
import {
  AlertTriangle,
  Check,
  ChevronDown,
  ChevronUp,
  Copy,
  FileCode2,
  Loader2,
  PencilLine,
  RefreshCw,
  Save,
  SearchCode,
  Sparkles,
  Wrench,
  X,
} from "lucide-react";
import {
  type LspCodeAction,
  type LspDiagnostic,
  type LspReferenceLocation,
  type WorkspaceFileConflict,
  orbitIpc,
} from "@/lib/orbit-ipc";
import {
  convertLspTextEditsToChanges,
  extractIdentifierAtOffset,
  formatEditorLanguageLabel,
  formatEditorLineEndingLabel,
  lineColToOffset,
  mapLspCompletionItems,
  mapLspDiagnosticsToCodeMirror,
  offsetToLineCol,
  parseHoverContentBlocks,
} from "@/lib/codemirror-helpers";
import { createCodeMirrorLanguageExtension } from "@/lib/codemirror-languages";
import { createOrbitCodeMirrorTheme } from "@/lib/codemirror-theme";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { ShortcutKbd } from "@/components/ui/kbd";
import { OrbitSpinner } from "@/components/ui/orbit-spinner";
import { SimpleTooltip } from "@/components/ui/overlays";
import { useI18n } from "@/lib/i18n";
import { useImeCompositionGuard } from "@/lib/ime";
import { modSymbol } from "@/lib/keybindings";
import { formatFileSizeCompact } from "@/lib/session-insights";
import { orbitActions } from "@/state/orbit-store";
import { cn } from "@/lib/utils";

export interface WorkspaceCodeEditorProps {
  hostId: string;
  cwd: string;
  filePath: string;
  relPath: string;
  value: string;
  language: string;
  lineEnding?: "lf" | "crlf" | "mixed" | string | null;
  isDark: boolean;
  dirty: boolean;
  saving: boolean;
  saveError: string | null;
  conflict: WorkspaceFileConflict | null;
  isLspEligible?: boolean;
  onChange: (nextValue: string) => void;
  onSave: (force?: boolean) => void | Promise<"saved" | "conflict" | "error">;
  onReloadFromConflict: () => void;
  onDismissError: () => void;
  onCopyMine: () => void;
  initialFocusLine?: number | null;
  diagnostics?: readonly LspDiagnostic[];
  jumpTarget?: { line: number; col?: number; nonce: number } | null;
  onGotoDefinition?: (line: number, col: number) => void;
  onJumpToFileLocation?: (targetPath: string, line: number, col?: number) => void;
  onAfterLspWorkspaceEdit?: (modifiedFiles: readonly string[]) => Promise<void> | void;
}

interface InlineAiEditState {
  startLine: number;
  endLine: number;
  selectedCode: string;
  surroundingContext: string;
  topPx: number;
}

interface InlineRenameState {
  line: number;
  col: number;
  originalName: string;
  nextName: string;
  topPx: number;
  submitting: boolean;
  error: string | null;
}

interface CodeActionsPopoverState {
  topPx: number;
  loading: boolean;
  actions: LspCodeAction[];
  activeIdx: number;
}

interface ReferencesDrawerState {
  open: boolean;
  collapsed: boolean;
  loading: boolean;
  symbolLabel: string;
  items: LspReferenceLocation[];
  error: string | null;
}

function normalizeRelCompare(p: string): string {
  return p.replace(/\\/g, "/").replace(/^\.\//, "");
}

function buildHoverDom(rawMarkdown: string): HTMLElement {
  const root = document.createElement("div");
  root.className = "orbit-cm-hover";
  const blocks = parseHoverContentBlocks(rawMarkdown);
  for (const block of blocks) {
    if (block.kind === "code") {
      const pre = document.createElement("pre");
      pre.className = "orbit-cm-hover-code";
      pre.textContent = block.code;
      root.appendChild(pre);
    } else if (block.kind === "divider") {
      const hr = document.createElement("div");
      hr.className = "orbit-cm-hover-divider";
      root.appendChild(hr);
    } else {
      const p = document.createElement("p");
      p.className = "orbit-cm-hover-para";
      p.textContent = block.text;
      root.appendChild(p);
    }
  }
  return root;
}

export const WorkspaceCodeEditor = memo(function WorkspaceCodeEditor(
  props: WorkspaceCodeEditorProps,
) {
  const {
    hostId,
    cwd,
    filePath,
    relPath,
    value,
    language,
    lineEnding,
    isDark,
    dirty,
    saving,
    saveError,
    conflict,
    isLspEligible = false,
    onSave,
    onReloadFromConflict,
    onDismissError,
    onCopyMine,
    initialFocusLine,
    diagnostics,
    jumpTarget,
    onJumpToFileLocation,
  } = props;

  const { t } = useI18n();
  const mod = modSymbol();
  const imeAi = useImeCompositionGuard();
  const imeRename = useImeCompositionGuard();

  const surfaceRef = useRef<HTMLDivElement | null>(null);
  const editorHostRef = useRef<HTMLDivElement | null>(null);
  const viewRef = useRef<EditorView | null>(null);
  const themeCompartmentRef = useRef<Compartment>(new Compartment());
  const languageCompartmentRef = useRef<Compartment>(new Compartment());
  const lastEmittedValueRef = useRef<string>(value);
  const copiedMineTimerRef = useRef<number | null>(null);
  const aiInputRef = useRef<HTMLInputElement | null>(null);
  const renameInputRef = useRef<HTMLInputElement | null>(null);
  const codeActionsMenuRef = useRef<HTMLDivElement | null>(null);
  const refsSeqRef = useRef<number>(0);
  const codeActionSeqRef = useRef<number>(0);

  // Latest-props ref so CodeMirror extensions never capture stale closures or force remounts
  const propsRef = useRef(props);
  propsRef.current = props;

  const [cursor, setCursor] = useState<{
    line: number;
    col: number;
    cursors: number;
    selectedChars: number;
  }>({
    line: initialFocusLine && initialFocusLine > 0 ? initialFocusLine : 1,
    col: 1,
    cursors: 1,
    selectedChars: 0,
  });
  const [copiedMine, setCopiedMine] = useState(false);

  // Inline Cmd+K AI Edit popover state
  const [aiEdit, setAiEdit] = useState<InlineAiEditState | null>(null);
  const [aiInstruction, setAiInstruction] = useState("");
  const [aiSubmitting, setAiSubmitting] = useState(false);

  // Inline F2 Rename popover state
  const [renameState, setRenameState] = useState<InlineRenameState | null>(null);

  // Quick Fix / Code Actions popover (`Mod-.`)
  const [codeActionsState, setCodeActionsState] = useState<CodeActionsPopoverState | null>(null);

  // Collapsible LSP References drawer (`Shift+F12`)
  const [refsDrawer, setRefsDrawer] = useState<ReferencesDrawerState>({
    open: false,
    collapsed: false,
    loading: false,
    symbolLabel: "",
    items: [],
    error: null,
  });

  useEffect(() => {
    return () => {
      if (copiedMineTimerRef.current !== null) {
        window.clearTimeout(copiedMineTimerRef.current);
      }
    };
  }, []);

  const computeAnchorTopPx = useCallback((view: EditorView, pos: number, overlayHeight = 64) => {
    const surface = surfaceRef.current;
    if (!surface) return 12;
    const surfaceRect = surface.getBoundingClientRect();
    const coords = view.coordsAtPos(pos);
    if (!coords) return 12;
    const relativeTop = coords.top - surfaceRect.top;
    const preferred = relativeTop - overlayHeight - 6;
    if (preferred >= 8) {
      return Math.min(preferred, Math.max(8, surfaceRect.height - overlayHeight - 10));
    }
    const below = coords.bottom - surfaceRect.top + 8;
    return Math.max(8, Math.min(below, Math.max(8, surfaceRect.height - overlayHeight - 10)));
  }, []);

  const applyCodeAction = useCallback(async (action: LspCodeAction) => {
    const p = propsRef.current;
    const view = viewRef.current;
    if (!action.edit || action.edit.file_edits.length === 0) {
      orbitActions.showToast(
        t("sidePanel.editorQuickFixUnavailable", { title: action.title }),
      );
      return;
    }

    const currentNorm = normalizeRelCompare(p.relPath);
    const singleSameFile =
      action.edit.file_edits.length === 1 &&
      normalizeRelCompare(action.edit.file_edits[0].path) === currentNorm;

    if (singleSameFile && view) {
      const docText = view.state.doc.toString();
      const changes = convertLspTextEditsToChanges(docText, action.edit.file_edits[0].edits);
      if (changes.length > 0) {
        view.dispatch({ changes });
        view.focus();
        orbitActions.showToast(
          t("sidePanel.editorQuickFixApplied", { title: action.title }),
        );
      }
      return;
    }

    if (p.dirty) {
      const outcome = await p.onSave(false);
      if (outcome && outcome !== "saved") {
        return;
      }
    }

    try {
      const res = await orbitIpc.lspApplyWorkspaceEdit({
        hostId: p.hostId,
        cwd: p.cwd,
        edit: action.edit,
      });
      if (res.applied) {
        await p.onAfterLspWorkspaceEdit?.(res.modified_files);
        orbitActions.showToast(
          t("sidePanel.editorQuickFixApplied", { title: action.title }),
        );
      }
    } catch (err) {
      orbitActions.showToast(
        t("sidePanel.editorQuickFixFailed", { error: String(err) }),
      );
    }
  }, [t]);

  const applyCodeActionRef = useRef(applyCodeAction);
  applyCodeActionRef.current = applyCodeAction;

  const openInlineAiEdit = useCallback(
    (view: EditorView) => {
      const sel = view.state.selection.main;
      const startLineObj = view.state.doc.lineAt(sel.from);
      const endPos = sel.to > sel.from && view.state.doc.lineAt(sel.to).from === sel.to
        ? sel.to - 1
        : sel.to;
      const endLineObj = view.state.doc.lineAt(Math.max(sel.from, endPos));
      const selectedCode = sel.empty ? "" : view.state.sliceDoc(sel.from, sel.to);

      let surroundingContext = "";
      if (sel.empty) {
        const ctxStartLine = Math.max(1, startLineObj.number - 2);
        const ctxEndLine = Math.min(view.state.doc.lines, startLineObj.number + 2);
        const ctxFrom = view.state.doc.line(ctxStartLine).from;
        const ctxTo = view.state.doc.line(ctxEndLine).to;
        surroundingContext = view.state.sliceDoc(ctxFrom, ctxTo);
      }

      const topPx = computeAnchorTopPx(view, sel.from, 74);
      setRenameState(null);
      setCodeActionsState(null);
      setAiInstruction("");
      setAiEdit({
        startLine: startLineObj.number,
        endLine: endLineObj.number,
        selectedCode,
        surroundingContext,
        topPx,
      });
      window.requestAnimationFrame(() => aiInputRef.current?.focus());
    },
    [computeAnchorTopPx],
  );

  const openInlineRename = useCallback(
    (view: EditorView) => {
      const p = propsRef.current;
      if (!p.isLspEligible) {
        orbitActions.showToast(t("sidePanel.editorLspUnavailable"));
        return;
      }
      const head = view.state.selection.main.head;
      const docText = view.state.doc.toString();
      const { line, col } = offsetToLineCol(docText, head);
      const ident = extractIdentifierAtOffset(docText, head);
      const initialName = ident?.word ?? "";
      const topPx = computeAnchorTopPx(view, head, 68);
      setAiEdit(null);
      setCodeActionsState(null);
      setRenameState({
        line,
        col,
        originalName: initialName,
        nextName: initialName,
        topPx,
        submitting: false,
        error: null,
      });
      window.requestAnimationFrame(() => {
        renameInputRef.current?.focus();
        renameInputRef.current?.select();
      });
    },
    [computeAnchorTopPx, t],
  );

  const triggerFindReferences = useCallback(
    async (view: EditorView) => {
      const p = propsRef.current;
      if (!p.isLspEligible) {
        orbitActions.showToast(t("sidePanel.editorLspUnavailable"));
        return;
      }
      const head = view.state.selection.main.head;
      const docText = view.state.doc.toString();
      const { line, col } = offsetToLineCol(docText, head);
      const ident = extractIdentifierAtOffset(docText, head);
      const symbolLabel = ident?.word || `${p.relPath}:${line}:${col}`;
      const seq = ++refsSeqRef.current;

      setRefsDrawer({
        open: true,
        collapsed: false,
        loading: true,
        symbolLabel,
        items: [],
        error: null,
      });

      try {
        const items = await orbitIpc.lspReferences({
          hostId: p.hostId,
          cwd: p.cwd,
          path: p.filePath,
          line,
          col,
          includeDeclaration: true,
          content: docText,
        });
        if (refsSeqRef.current !== seq) return;
        setRefsDrawer({
          open: true,
          collapsed: false,
          loading: false,
          symbolLabel,
          items,
          error: null,
        });
      } catch (err) {
        if (refsSeqRef.current !== seq) return;
        setRefsDrawer({
          open: true,
          collapsed: false,
          loading: false,
          symbolLabel,
          items: [],
          error: String(err),
        });
      }
    },
    [t],
  );

  const triggerCodeActionsAtCursor = useCallback(
    async (view: EditorView) => {
      const p = propsRef.current;
      if (!p.isLspEligible) {
        orbitActions.showToast(t("sidePanel.editorLspUnavailable"));
        return;
      }
      const sel = view.state.selection.main;
      const docText = view.state.doc.toString();
      const start = offsetToLineCol(docText, sel.from);
      const end = offsetToLineCol(docText, sel.to);
      const topPx = computeAnchorTopPx(view, sel.from, 140);
      const overlappingDiags = (p.diagnostics ?? []).filter(
        (d) => d.line <= end.line && Math.max(d.line, d.end_line) >= start.line,
      );
      const seq = ++codeActionSeqRef.current;

      setAiEdit(null);
      setRenameState(null);
      setCodeActionsState({
        topPx,
        loading: true,
        actions: [],
        activeIdx: 0,
      });

      try {
        const actions = await orbitIpc.lspCodeAction({
          hostId: p.hostId,
          cwd: p.cwd,
          path: p.filePath,
          line: start.line,
          col: start.col,
          endLine: end.line,
          endCol: end.col,
          diagnostics: overlappingDiags,
          content: docText,
        });
        if (codeActionSeqRef.current !== seq) return;
        if (actions.length === 0) {
          setCodeActionsState(null);
          orbitActions.showToast(t("sidePanel.editorNoCodeActions"));
          return;
        }
        setCodeActionsState({
          topPx,
          loading: false,
          actions,
          activeIdx: 0,
        });
        window.requestAnimationFrame(() => codeActionsMenuRef.current?.focus());
      } catch {
        if (codeActionSeqRef.current !== seq) return;
        setCodeActionsState(null);
        orbitActions.showToast(t("sidePanel.editorNoCodeActions"));
      }
    },
    [computeAnchorTopPx, t],
  );

  const openFindAndReplace = useCallback((view: EditorView) => {
    openSearchPanel(view);
    window.requestAnimationFrame(() => {
      const replaceInput = view.dom.querySelector<HTMLInputElement>(
        '.cm-panel.cm-search input[name="replace"]',
      );
      if (replaceInput) {
        replaceInput.focus();
        replaceInput.select();
      }
    });
  }, []);

  const handlersRef = useRef({
    openInlineAiEdit,
    openInlineRename,
    triggerFindReferences,
    triggerCodeActionsAtCursor,
    openFindAndReplace,
  });
  handlersRef.current = {
    openInlineAiEdit,
    openInlineRename,
    triggerFindReferences,
    triggerCodeActionsAtCursor,
    openFindAndReplace,
  };

  // Build & mount CodeMirror 6 once per editor lifecycle; reactive props reconfigure compartments cleanly
  useEffect(() => {
    const host = editorHostRef.current;
    if (!host) return;

    const lspHoverExtension = hoverTooltip(
      async (view, pos) => {
        const p = propsRef.current;
        if (!p.isLspEligible) return null;
        const docText = view.state.doc.toString();
        const ch = docText[pos] ?? docText[pos - 1] ?? "";
        if (!ch || /^\s$/.test(ch)) return null;
        const { line, col } = offsetToLineCol(docText, pos);
        try {
          const hover = await orbitIpc.lspHover({
            hostId: p.hostId,
            cwd: p.cwd,
            path: p.filePath,
            line,
            col,
            content: docText,
          });
          if (!hover || !hover.contents.trim()) return null;
          const rangeFrom = hover.range
            ? lineColToOffset(docText, hover.range.line, hover.range.col)
            : pos;
          const rangeTo = hover.range
            ? Math.max(
                rangeFrom,
                lineColToOffset(docText, hover.range.end_line, hover.range.end_col),
              )
            : undefined;
          return {
            pos: Math.max(0, Math.min(rangeFrom, docText.length)),
            end: rangeTo !== undefined ? Math.min(rangeTo, docText.length) : undefined,
            above: true,
            create() {
              return { dom: buildHoverDom(hover.contents) };
            },
          };
        } catch {
          return null;
        }
      },
      { hoverTime: 320 },
    );

    const lspCompletionSource = async (
      context: CompletionContext,
    ): Promise<CompletionResult | null> => {
      const p = propsRef.current;
      if (!p.isLspEligible) return null;

      const word = context.matchBefore(/[\w$]+/);
      const prevChar = context.pos > 0 ? context.state.sliceDoc(context.pos - 1, context.pos) : "";
      const isTrigger = [".", ":", ">", "/", "@", "#"].includes(prevChar);

      if (!context.explicit && !isTrigger && (!word || word.text.length < 2)) {
        return null;
      }

      const docText = context.state.doc.toString();
      const { line, col } = offsetToLineCol(docText, context.pos);
      try {
        const items = await orbitIpc.lspCompletion({
          hostId: p.hostId,
          cwd: p.cwd,
          path: p.filePath,
          line,
          col,
          triggerCharacter: isTrigger ? prevChar : null,
          content: docText,
        });
        if (context.aborted || items.length === 0) return null;
        const options = mapLspCompletionItems(items);
        return {
          from: word ? word.from : context.pos,
          options,
          validFor: /^[\w$]*$/,
        };
      } catch {
        return null;
      }
    };

    const customKeymap = Prec.high(
      keymap.of([
        {
          key: "Mod-s",
          preventDefault: true,
          run: () => {
            const p = propsRef.current;
            if (!p.saving) {
              void p.onSave(false);
            }
            return true;
          },
        },
        {
          key: "Mod-k",
          preventDefault: true,
          run: (view) => {
            handlersRef.current.openInlineAiEdit(view);
            return true;
          },
        },
        {
          key: "Mod-h",
          preventDefault: true,
          run: (view) => {
            handlersRef.current.openFindAndReplace(view);
            return true;
          },
        },
        {
          key: "Mod-.",
          preventDefault: true,
          run: (view) => {
            void handlersRef.current.triggerCodeActionsAtCursor(view);
            return true;
          },
        },
        {
          key: "F12",
          preventDefault: true,
          run: (view) => {
            const p = propsRef.current;
            if (!p.onGotoDefinition) return false;
            const pos = offsetToLineCol(
              view.state.doc.toString(),
              view.state.selection.main.head,
            );
            p.onGotoDefinition(pos.line, pos.col);
            return true;
          },
        },
        {
          key: "Shift-F12",
          preventDefault: true,
          run: (view) => {
            void handlersRef.current.triggerFindReferences(view);
            return true;
          },
        },
        {
          key: "F2",
          preventDefault: true,
          run: (view) => {
            handlersRef.current.openInlineRename(view);
            return true;
          },
        },
      ]),
    );

    const cmdClickGotoDef = EditorView.domEventHandlers({
      mousedown(event, view) {
        const p = propsRef.current;
        if (
          !p.onGotoDefinition ||
          !(event.metaKey || event.ctrlKey) ||
          event.altKey ||
          event.shiftKey ||
          event.button !== 0
        ) {
          return false;
        }
        const pos = view.posAtCoords({ x: event.clientX, y: event.clientY });
        if (pos === null) return false;
        event.preventDefault();
        const loc = offsetToLineCol(view.state.doc.toString(), pos);
        p.onGotoDefinition(loc.line, loc.col);
        return true;
      },
    });

    const updateListener = EditorView.updateListener.of((update) => {
      if (update.docChanged) {
        const nextText = update.state.doc.toString();
        lastEmittedValueRef.current = nextText;
        propsRef.current.onChange(nextText);
      }
      if (update.selectionSet || update.docChanged) {
        const main = update.state.selection.main;
        const lineObj = update.state.doc.lineAt(main.head);
        const col = main.head - lineObj.from + 1;
        const cursors = update.state.selection.ranges.length;
        const selectedChars = update.state.selection.ranges.reduce(
          (sum, r) => sum + Math.abs(r.to - r.from),
          0,
        );
        setCursor((prev) =>
          prev.line === lineObj.number &&
          prev.col === col &&
          prev.cursors === cursors &&
          prev.selectedChars === selectedChars
            ? prev
            : { line: lineObj.number, col, cursors, selectedChars },
        );
      }
    });

    const initialDoc = propsRef.current.value;
    lastEmittedValueRef.current = initialDoc;

    const startState = EditorState.create({
      doc: initialDoc,
      extensions: [
        themeCompartmentRef.current.of(createOrbitCodeMirrorTheme(propsRef.current.isDark)),
        languageCompartmentRef.current.of(
          createCodeMirrorLanguageExtension(propsRef.current.language, propsRef.current.filePath),
        ),
        lineNumbers(),
        highlightActiveLineGutter(),
        highlightSpecialChars(),
        history(),
        drawSelection(),
        dropCursor(),
        EditorState.allowMultipleSelections.of(true),
        EditorState.tabSize.of(2),
        indentUnit.of("  "),
        indentOnInput(),
        bracketMatching(),
        closeBrackets(),
        rectangularSelection(),
        crosshairCursor(),
        highlightActiveLine(),
        highlightSelectionMatches(),
        search({ top: true }),
        EditorView.clickAddsSelectionRange.of(
          (event) => event.altKey && !event.metaKey && !event.ctrlKey,
        ),
        lintGutter(),
        linter(null),
        lspHoverExtension,
        autocompletion({
          override: [lspCompletionSource],
          icons: true,
          activateOnTyping: true,
          maxRenderedOptions: 80,
        }),
        customKeymap,
        keymap.of([
          ...closeBracketsKeymap,
          ...defaultKeymap,
          ...searchKeymap,
          ...historyKeymap,
          ...completionKeymap,
          ...lintKeymap,
          indentWithTab,
        ]),
        cmdClickGotoDef,
        updateListener,
        EditorView.contentAttributes.of({
          "aria-label": "Workspace code editor",
          spellcheck: "false",
          autocorrect: "off",
          autocapitalize: "off",
        }),
      ],
    });

    const view = new EditorView({
      state: startState,
      parent: host,
    });
    viewRef.current = view;
    view.focus();

    // Preserve scroll line on entry from read-only View mode or link focus
    const entryLine = propsRef.current.initialFocusLine;
    if (entryLine && entryLine > 1) {
      const clampedLine = Math.min(entryLine, view.state.doc.lines);
      const lineInfo = view.state.doc.line(clampedLine);
      view.dispatch({
        selection: EditorSelection.cursor(lineInfo.from),
        effects: EditorView.scrollIntoView(lineInfo.from, { y: "center" }),
      });
    }

    return () => {
      viewRef.current = null;
      view.destroy();
    };
  }, []);

  // Reconfigure theme compartment without disturbing Undo/Redo history or selection
  useEffect(() => {
    const view = viewRef.current;
    if (!view) return;
    view.dispatch({
      effects: themeCompartmentRef.current.reconfigure(createOrbitCodeMirrorTheme(isDark)),
    });
  }, [isDark]);

  // Reconfigure language compartment reactively
  useEffect(() => {
    const view = viewRef.current;
    if (!view) return;
    view.dispatch({
      effects: languageCompartmentRef.current.reconfigure(
        createCodeMirrorLanguageExtension(language, filePath),
      ),
    });
  }, [language, filePath]);

  // Sync external buffer reloads (e.g. conflict reload or F2 Rename) without breaking local edits
  useEffect(() => {
    const view = viewRef.current;
    if (!view) return;
    const currentDoc = view.state.doc.toString();
    if (value === currentDoc || value === lastEmittedValueRef.current) {
      return;
    }
    lastEmittedValueRef.current = value;
    const prevHead = view.state.selection.main.head;
    const clampedHead = Math.min(prevHead, value.length);
    view.dispatch({
      changes: { from: 0, to: currentDoc.length, insert: value },
      selection: EditorSelection.cursor(clampedHead),
    });
  }, [value]);

  // Jump cleanly to diagnostic / Go-to-Definition / References target line & column
  useEffect(() => {
    if (!jumpTarget || jumpTarget.line < 1) return;
    const view = viewRef.current;
    if (!view) return;
    const docText = view.state.doc.toString();
    const offset = lineColToOffset(docText, jumpTarget.line, jumpTarget.col ?? 1);
    view.focus();
    view.dispatch({
      selection: EditorSelection.cursor(offset),
      effects: EditorView.scrollIntoView(offset, { y: "center" }),
    });
    // Trigger strictly on jumpTarget nonce updates
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [jumpTarget?.nonce]);

  // Push LSP diagnostics into CodeMirror's linter + attach async inline Quick Fix buttons inside tooltip
  useEffect(() => {
    const view = viewRef.current;
    if (!view) return;
    const docText = view.state.doc.toString();
    const mapped = mapLspDiagnosticsToCodeMirror(docText, diagnostics ?? []);

    const cmDiags: CmDiagnostic[] = mapped.map((item) => ({
      from: item.from,
      to: item.to,
      severity: item.severity,
      message: item.message,
      source: item.source,
      renderMessage: (editorView) => {
        const wrap = document.createElement("div");
        const msgEl = document.createElement("div");
        msgEl.textContent = item.message;
        wrap.appendChild(msgEl);

        if (item.source) {
          const srcEl = document.createElement("div");
          srcEl.className = "cm-diagnosticSource";
          srcEl.textContent = item.source;
          wrap.appendChild(srcEl);
        }

        const p = propsRef.current;
        if (p.isLspEligible) {
          const actionsEl = document.createElement("div");
          actionsEl.className = "orbit-cm-diag-actions";
          wrap.appendChild(actionsEl);

          const curText = editorView.state.doc.toString();
          void orbitIpc
            .lspCodeAction({
              hostId: p.hostId,
              cwd: p.cwd,
              path: p.filePath,
              line: item.original.line,
              col: item.original.col,
              endLine: Math.max(item.original.line, item.original.end_line),
              endCol: item.original.end_col,
              diagnostics: [item.original],
              content: curText,
            })
            .then((actions) => {
              if (!actionsEl.isConnected) return;
              const fixActions = actions.filter((a) => a.edit && a.edit.file_edits.length > 0);
              if (fixActions.length === 0) {
                actionsEl.remove();
                return;
              }
              for (const action of fixActions.slice(0, 4)) {
                const btn = document.createElement("button");
                btn.type = "button";
                btn.className = "orbit-cm-diag-fix-btn";
                if (action.is_preferred) {
                  btn.dataset.preferred = "true";
                }
                btn.textContent = action.title;
                btn.addEventListener("mousedown", (ev) => {
                  ev.preventDefault();
                  ev.stopPropagation();
                  void applyCodeActionRef.current(action);
                });
                actionsEl.appendChild(btn);
              }
            })
            .catch(() => {
              actionsEl.remove();
            });
        }
        return wrap;
      },
    }));

    view.dispatch(setDiagnostics(view.state, cmDiags));
  }, [diagnostics, hostId, cwd, filePath, isLspEligible]);

  const handleSubmitInlineAiEdit = useCallback(async () => {
    if (!aiEdit || aiSubmitting) return;
    const trimmed = aiInstruction.trim();
    if (!trimmed) return;
    setAiSubmitting(true);
    try {
      await orbitActions.dispatchInlineCodeEdit({
        filePath: relPath,
        language,
        startLine: aiEdit.startLine,
        endLine: aiEdit.endLine,
        selectedCode: aiEdit.selectedCode,
        surroundingContext: aiEdit.surroundingContext,
        instruction: trimmed,
      });
      setAiEdit(null);
      setAiInstruction("");
      viewRef.current?.focus();
    } catch (err) {
      orbitActions.showToast(
        t("sidePanel.editorAiEditFailed", { error: String(err) }),
      );
    } finally {
      setAiSubmitting(false);
    }
  }, [aiEdit, aiSubmitting, aiInstruction, relPath, language, t]);

  const handleSubmitRename = useCallback(async () => {
    if (!renameState || renameState.submitting) return;
    const trimmed = renameState.nextName.trim();
    if (!trimmed || trimmed === renameState.originalName) {
      setRenameState(null);
      viewRef.current?.focus();
      return;
    }

    setRenameState((prev) => (prev ? { ...prev, submitting: true, error: null } : null));
    try {
      if (propsRef.current.dirty) {
        const outcome = await propsRef.current.onSave(false);
        if (outcome && outcome !== "saved") {
          setRenameState((prev) =>
            prev
              ? {
                  ...prev,
                  submitting: false,
                  error: t("sidePanel.editorRenameSaveFirstError"),
                }
              : null,
          );
          return;
        }
      }
      const res = await orbitIpc.lspRename({
        hostId: propsRef.current.hostId,
        cwd: propsRef.current.cwd,
        path: propsRef.current.filePath,
        line: renameState.line,
        col: renameState.col,
        newName: trimmed,
      });
      if (!res.applied) {
        setRenameState((prev) =>
          prev
            ? {
                ...prev,
                submitting: false,
                error: t("sidePanel.editorRenameNoChanges"),
              }
            : null,
        );
        return;
      }
      await propsRef.current.onAfterLspWorkspaceEdit?.(res.modified_files);
      setRenameState(null);
      viewRef.current?.focus();
      orbitActions.showToast(
        t("sidePanel.editorRenameSuccess", {
          files: res.modified_files.length,
          edits: res.total_edits,
        }),
      );
    } catch (err) {
      setRenameState((prev) =>
        prev ? { ...prev, submitting: false, error: String(err) } : null,
      );
    }
  }, [renameState, t]);

  const handleReferenceClick = useCallback(
    (refLoc: LspReferenceLocation) => {
      const isSameFile = normalizeRelCompare(refLoc.path) === normalizeRelCompare(relPath);
      if (isSameFile && viewRef.current) {
        const view = viewRef.current;
        const offset = lineColToOffset(
          view.state.doc.toString(),
          refLoc.line,
          refLoc.character,
        );
        view.focus();
        view.dispatch({
          selection: EditorSelection.cursor(offset),
          effects: EditorView.scrollIntoView(offset, { y: "center" }),
        });
        return;
      }
      onJumpToFileLocation?.(refLoc.path, refLoc.line, refLoc.character);
    },
    [relPath, onJumpToFileLocation],
  );

  const lineEndingLabel = useMemo(() => formatEditorLineEndingLabel(lineEnding), [lineEnding]);
  const languageLabel = useMemo(() => formatEditorLanguageLabel(language), [language]);
  const referenceFileCount = useMemo(
    () => new Set(refsDrawer.items.map((r) => r.path)).size,
    [refsDrawer.items],
  );

  return (
    <div
      data-orbit-code-editor="true"
      className="flex min-h-0 flex-1 flex-col overflow-hidden"
    >
      {/* External Disk Conflict Banner */}
      {conflict && (
        <div className="border-signal-warning-border bg-signal-warning-bg border-b px-3 py-2 text-[12px]">
          <div className="flex flex-wrap items-center justify-between gap-2">
            <div className="flex min-w-0 items-center gap-2">
              <AlertTriangle className="text-signal-warning size-[14px] shrink-0" />
              <span className="text-ink-primary font-medium">
                {t("sidePanel.editorConflictTitle")}
              </span>
              <span className="text-ink-tertiary tabular-nums text-[11px]">
                ({formatFileSizeCompact(conflict.current_size_bytes)})
              </span>
            </div>
            <div className="flex shrink-0 items-center gap-1.5">
              <Button
                type="button"
                variant="outline"
                size="xs"
                disabled={saving}
                onClick={onReloadFromConflict}
                className="h-[24px] px-2 text-[11.5px]"
              >
                <RefreshCw className="size-[11.5px]" />
                {t("sidePanel.editorConflictReload")}
              </Button>
              <Button
                type="button"
                variant="secondary"
                size="xs"
                disabled={saving}
                onClick={() => void onSave(true)}
                className="border-signal-warning-border text-signal-warning h-[24px] px-2 text-[11.5px]"
              >
                <Save className="size-[11.5px]" />
                {t("sidePanel.editorConflictOverwrite")}
              </Button>
              <Button
                type="button"
                variant="ghost"
                size="xs"
                onClick={() => {
                  onCopyMine();
                  setCopiedMine(true);
                  if (copiedMineTimerRef.current !== null) {
                    window.clearTimeout(copiedMineTimerRef.current);
                  }
                  copiedMineTimerRef.current = window.setTimeout(() => {
                    copiedMineTimerRef.current = null;
                    setCopiedMine(false);
                  }, 1400);
                }}
                className="h-[24px] px-2 text-[11.5px]"
              >
                {copiedMine ? (
                  <Check className="text-signal-success size-[11.5px]" />
                ) : (
                  <Copy className="size-[11.5px]" />
                )}
                {t("sidePanel.editorConflictCopyMine")}
              </Button>
            </div>
          </div>
        </div>
      )}

      {/* Save Error Banner */}
      {saveError && (
        <div className="border-signal-danger-border bg-signal-danger-bg text-signal-danger flex items-center justify-between gap-2 border-b px-3 py-1.5 text-[11.5px]">
          <span className="min-w-0 truncate">{saveError}</span>
          <button
            type="button"
            onClick={onDismissError}
            aria-label={t("sidePanel.editorDismissError")}
            className="hover:bg-signal-danger/15 inline-flex size-[20px] shrink-0 cursor-pointer items-center justify-center rounded"
          >
            <X className="size-[12px]" />
          </button>
        </div>
      )}

      {/* Main CodeMirror 6 Editor Surface + Anchored Inline Overlays */}
      <div
        ref={surfaceRef}
        className="bg-surface-base relative flex min-h-0 flex-1 flex-col overflow-hidden"
      >
        <div ref={editorHostRef} className="min-h-0 flex-1 overflow-hidden" />

        {/* Inline Cmd+K AI Edit Popover */}
        {aiEdit && (
          <div
            role="dialog"
            aria-label={t("sidePanel.editorAiEditTitle")}
            style={{ top: `${aiEdit.topPx}px` }}
            onKeyDown={(e) => {
              if (e.key === "Escape") {
                e.preventDefault();
                e.stopPropagation();
                setAiEdit(null);
                viewRef.current?.focus();
              }
            }}
            className="floating-surface orbit-inline-menu-up absolute right-3 left-3 z-20 mx-auto max-w-[540px] rounded-xl border p-2"
          >
            <div className="flex items-center gap-1.5">
              <Sparkles className="text-signal-info size-[13px] shrink-0" />
              <span className="bg-surface-recessed border-border-subtle text-ink-secondary tabular-nums shrink-0 rounded-[5px] border px-1.5 py-0.5 font-mono text-[11px]">
                {aiEdit.startLine === aiEdit.endLine
                  ? `L${aiEdit.startLine}`
                  : `L${aiEdit.startLine}–L${aiEdit.endLine}`}
              </span>
              <Input
                ref={aiInputRef}
                type="text"
                value={aiInstruction}
                disabled={aiSubmitting}
                onChange={(e) => setAiInstruction(e.target.value)}
                {...imeAi.compositionProps}
                onKeyDown={(e) => {
                  if (imeAi.shouldIgnoreKeyDown(e)) return;
                  if (e.key === "Enter" && !e.shiftKey) {
                    e.preventDefault();
                    e.stopPropagation();
                    void handleSubmitInlineAiEdit();
                  }
                }}
                placeholder={
                  aiEdit.startLine === aiEdit.endLine
                    ? t("sidePanel.editorAiEditPlaceholderLine", {
                        line: aiEdit.startLine,
                      })
                    : t("sidePanel.editorAiEditPlaceholderRange", {
                        start: aiEdit.startLine,
                        end: aiEdit.endLine,
                      })
                }
                className="h-[26px] flex-1 border-transparent bg-transparent px-1 text-[12px] shadow-none focus-visible:border-transparent focus-visible:ring-0"
              />
              <Button
                type="button"
                size="xs"
                disabled={aiSubmitting || !aiInstruction.trim()}
                onClick={() => void handleSubmitInlineAiEdit()}
                className="h-[24px] px-2.5 text-[11.5px]"
              >
                {aiSubmitting ? <OrbitSpinner size={11} /> : t("sidePanel.editorAiEditSend")}
              </Button>
              <Button
                type="button"
                variant="ghost"
                size="icon-xs"
                aria-label={t("common.close")}
                onClick={() => {
                  setAiEdit(null);
                  viewRef.current?.focus();
                }}
                className="size-[22px]"
              >
                <X className="size-[12px]" />
              </Button>
            </div>
          </div>
        )}

        {/* Inline F2 Symbol Rename Popover */}
        {renameState && (
          <div
            role="dialog"
            aria-label={t("sidePanel.editorRenameTitle")}
            style={{ top: `${renameState.topPx}px` }}
            onKeyDown={(e) => {
              if (e.key === "Escape") {
                e.preventDefault();
                e.stopPropagation();
                setRenameState(null);
                viewRef.current?.focus();
              }
            }}
            className="floating-surface orbit-inline-menu-up absolute right-3 left-3 z-20 mx-auto max-w-[360px] rounded-xl border p-2"
          >
            <div className="flex items-center gap-1.5">
              <PencilLine className="text-ink-tertiary size-[13px] shrink-0" />
              <Input
                ref={renameInputRef}
                type="text"
                value={renameState.nextName}
                disabled={renameState.submitting}
                onChange={(e) =>
                  setRenameState((prev) =>
                    prev ? { ...prev, nextName: e.target.value, error: null } : null,
                  )
                }
                {...imeRename.compositionProps}
                onKeyDown={(e) => {
                  if (imeRename.shouldIgnoreKeyDown(e)) return;
                  if (e.key === "Enter") {
                    e.preventDefault();
                    e.stopPropagation();
                    void handleSubmitRename();
                  }
                }}
                placeholder={t("sidePanel.editorRenamePlaceholder")}
                className="h-[26px] flex-1 font-mono text-[12px]"
              />
              <Button
                type="button"
                size="xs"
                disabled={renameState.submitting || !renameState.nextName.trim()}
                onClick={() => void handleSubmitRename()}
                className="h-[24px] px-2.5 text-[11.5px]"
              >
                {renameState.submitting ? (
                  <OrbitSpinner size={11} />
                ) : (
                  t("sidePanel.editorRenameApply")
                )}
              </Button>
              <Button
                type="button"
                variant="ghost"
                size="icon-xs"
                aria-label={t("common.close")}
                onClick={() => {
                  setRenameState(null);
                  viewRef.current?.focus();
                }}
                className="size-[22px]"
              >
                <X className="size-[12px]" />
              </Button>
            </div>
            {renameState.error && (
              <div className="text-signal-danger mt-1.5 px-1 text-[11px]">
                {renameState.error}
              </div>
            )}
          </div>
        )}

        {/* Quick Fix / Code Actions Popover (Cmd+.) */}
        {codeActionsState && (
          <div
            ref={codeActionsMenuRef}
            role="menu"
            aria-label={t("sidePanel.editorQuickFixesTitle")}
            style={{ top: `${codeActionsState.topPx}px` }}
            tabIndex={-1}
            onKeyDown={(e) => {
              if (e.key === "Escape") {
                e.preventDefault();
                e.stopPropagation();
                setCodeActionsState(null);
                viewRef.current?.focus();
              } else if (e.key === "ArrowDown") {
                e.preventDefault();
                setCodeActionsState((prev) =>
                  prev && prev.actions.length > 0
                    ? { ...prev, activeIdx: (prev.activeIdx + 1) % prev.actions.length }
                    : prev,
                );
              } else if (e.key === "ArrowUp") {
                e.preventDefault();
                setCodeActionsState((prev) =>
                  prev && prev.actions.length > 0
                    ? {
                        ...prev,
                        activeIdx:
                          (prev.activeIdx - 1 + prev.actions.length) % prev.actions.length,
                      }
                    : prev,
                );
              } else if (e.key === "Enter") {
                e.preventDefault();
                const chosen = codeActionsState.actions[codeActionsState.activeIdx];
                if (chosen) {
                  setCodeActionsState(null);
                  void applyCodeAction(chosen);
                }
              }
            }}
            className="floating-surface orbit-inline-menu-up absolute right-3 left-3 z-20 mx-auto max-w-[420px] rounded-xl border p-1.5"
          >
            <div className="text-ink-tertiary flex items-center justify-between px-2 py-1 text-[11px] font-medium">
              <span className="inline-flex items-center gap-1.5">
                <Wrench className="size-[11.5px]" />
                {t("sidePanel.editorQuickFixesTitle")}
              </span>
              <button
                type="button"
                onClick={() => {
                  setCodeActionsState(null);
                  viewRef.current?.focus();
                }}
                aria-label={t("common.close")}
                className="hover:bg-accent text-ink-tertiary hover:text-ink-primary inline-flex size-[18px] cursor-pointer items-center justify-center rounded"
              >
                <X className="size-[11px]" />
              </button>
            </div>
            {codeActionsState.loading ? (
              <div className="text-ink-tertiary flex items-center gap-2 px-2 py-2 text-[12px]">
                <OrbitSpinner size={12} />
                <span>{t("sidePanel.editorLoadingQuickFixes")}</span>
              </div>
            ) : (
              <div className="max-h-[200px] overflow-auto">
                {codeActionsState.actions.map((action, idx) => (
                  <button
                    key={`${action.title}:${idx}`}
                    type="button"
                    role="menuitem"
                    onClick={() => {
                      setCodeActionsState(null);
                      void applyCodeAction(action);
                    }}
                    className={cn(
                      "flex w-full cursor-pointer items-center justify-between gap-2 rounded-md px-2 py-1.5 text-left text-[12px] transition-colors",
                      idx === codeActionsState.activeIdx
                        ? "bg-accent text-ink-primary"
                        : "text-ink-secondary hover:bg-accent hover:text-ink-primary",
                    )}
                  >
                    <span className="min-w-0 truncate font-medium">{action.title}</span>
                    <span className="flex shrink-0 items-center gap-1">
                      {action.is_preferred && (
                        <span className="bg-signal-info-bg text-signal-info rounded px-1 py-0.5 text-[10px] font-medium">
                          {t("sidePanel.editorPreferredBadge")}
                        </span>
                      )}
                      {action.kind && (
                        <span className="text-ink-faint font-mono text-[10px]">
                          {action.kind}
                        </span>
                      )}
                    </span>
                  </button>
                ))}
              </div>
            )}
          </div>
        )}
      </div>

      {/* Collapsible LSP References Drawer (Shift+F12) */}
      {refsDrawer.open && (
        <div className="border-border-default bg-surface-recessed/75 flex shrink-0 flex-col border-t">
          <div className="border-border-subtle flex h-[28px] items-center justify-between gap-2 border-b px-3 text-[11px]">
            <button
              type="button"
              onClick={() =>
                setRefsDrawer((prev) => ({ ...prev, collapsed: !prev.collapsed }))
              }
              className="text-ink-secondary hover:text-ink-primary flex min-w-0 cursor-pointer items-center gap-1.5 font-medium"
            >
              {refsDrawer.collapsed ? (
                <ChevronUp className="size-[12px] shrink-0" />
              ) : (
                <ChevronDown className="size-[12px] shrink-0" />
              )}
              <SearchCode className="size-[12px] shrink-0" />
              <span className="truncate font-mono">{refsDrawer.symbolLabel}</span>
              {!refsDrawer.loading && !refsDrawer.error && (
                <span className="text-ink-tertiary tabular-nums shrink-0 font-sans font-normal">
                  ·{" "}
                  {t("sidePanel.editorReferencesCount", {
                    count: refsDrawer.items.length,
                    files: referenceFileCount,
                  })}
                </span>
              )}
            </button>
            <button
              type="button"
              aria-label={t("common.close")}
              onClick={() => {
                setRefsDrawer((prev) => ({ ...prev, open: false }));
                viewRef.current?.focus();
              }}
              className="text-ink-tertiary hover:bg-accent hover:text-ink-primary inline-flex size-[20px] shrink-0 cursor-pointer items-center justify-center rounded"
            >
              <X className="size-[12px]" />
            </button>
          </div>

          {!refsDrawer.collapsed && (
            <div className="max-h-[168px] overflow-auto">
              {refsDrawer.loading ? (
                <div className="text-ink-tertiary flex items-center gap-2 px-3 py-3 text-[11.5px]">
                  <OrbitSpinner size={12} />
                  <span>{t("sidePanel.editorFindingReferences")}</span>
                </div>
              ) : refsDrawer.error ? (
                <div className="text-signal-danger px-3 py-2 text-[11.5px]">
                  {refsDrawer.error}
                </div>
              ) : refsDrawer.items.length === 0 ? (
                <div className="text-ink-tertiary px-3 py-3 text-[11.5px]">
                  {t("sidePanel.editorNoReferences")}
                </div>
              ) : (
                <div className="divide-border-subtle divide-y">
                  {refsDrawer.items.map((refLoc, idx) => (
                    <button
                      key={`${refLoc.path}:${refLoc.line}:${refLoc.character}:${idx}`}
                      type="button"
                      onClick={() => handleReferenceClick(refLoc)}
                      className="hover:bg-accent flex w-full cursor-pointer items-center gap-2 px-3 py-1.5 text-left transition-colors"
                    >
                      <FileCode2 className="text-ink-faint size-[12px] shrink-0" />
                      <span className="text-ink-secondary shrink-0 font-mono text-[11px]">
                        {refLoc.path}
                      </span>
                      <span className="text-ink-faint tabular-nums shrink-0 font-mono text-[10.5px]">
                        {refLoc.line}:{refLoc.character}
                      </span>
                      {refLoc.preview_line && (
                        <span className="text-ink-primary/85 min-w-0 flex-1 truncate font-mono text-[11px]">
                          {refLoc.preview_line}
                        </span>
                      )}
                    </button>
                  ))}
                </div>
              )}
            </div>
          )}
        </div>
      )}

      {/* Subtle Editor Status Footer (26px, T3 Meta typography) */}
      <div className="border-border-subtle bg-surface-recessed/70 text-ink-tertiary flex h-[26px] shrink-0 items-center justify-between gap-2 border-t px-3 font-mono text-[11px]">
        <div className="flex min-w-0 items-center gap-2">
          <span className="tabular-nums">
            Ln {cursor.line}, Col {cursor.col}
          </span>
          {cursor.cursors > 1 && (
            <>
              <span className="opacity-40">·</span>
              <span className="text-signal-info tabular-nums font-sans">
                {t("sidePanel.editorCursorsCount", { count: cursor.cursors })}
              </span>
            </>
          )}
          {cursor.selectedChars > 0 && (
            <>
              <span className="opacity-40">·</span>
              <span className="tabular-nums font-sans">
                {t("sidePanel.editorSelectedChars", { count: cursor.selectedChars })}
              </span>
            </>
          )}
          <span className="opacity-40">·</span>
          <span>{lineEndingLabel}</span>
          <span className="opacity-40">·</span>
          <span className="truncate font-sans">{languageLabel}</span>
        </div>

        <div className="flex shrink-0 items-center gap-2 font-sans">
          {isLspEligible && (
            <div className="hidden items-center gap-1 sm:flex">
              <SimpleTooltip label={t("sidePanel.editorFindReferencesTooltip")} shortcut="⇧F12">
                <button
                  type="button"
                  onClick={() => {
                    if (viewRef.current) void triggerFindReferences(viewRef.current);
                  }}
                  className="text-ink-tertiary hover:bg-accent hover:text-ink-primary inline-flex h-[19px] cursor-pointer items-center gap-1 rounded px-1.5 text-[11px] transition-colors"
                >
                  {t("sidePanel.editorRefsAction")}
                </button>
              </SimpleTooltip>
              <SimpleTooltip label={t("sidePanel.editorRenameTooltip")} shortcut="F2">
                <button
                  type="button"
                  onClick={() => {
                    if (viewRef.current) openInlineRename(viewRef.current);
                  }}
                  className="text-ink-tertiary hover:bg-accent hover:text-ink-primary inline-flex h-[19px] cursor-pointer items-center gap-1 rounded px-1.5 text-[11px] transition-colors"
                >
                  {t("sidePanel.editorRenameAction")}
                </button>
              </SimpleTooltip>
            </div>
          )}

          <SimpleTooltip label={t("sidePanel.editorAiEditTooltip")} shortcut={`${mod}K`}>
            <button
              type="button"
              onClick={() => {
                if (viewRef.current) openInlineAiEdit(viewRef.current);
              }}
              className="text-ink-secondary hover:bg-accent hover:text-ink-primary inline-flex h-[19px] cursor-pointer items-center gap-1 rounded px-1.5 text-[11px] font-medium transition-colors"
            >
              <Sparkles className="size-[10.5px]" />
              <span>{t("sidePanel.editorAiEditAction")}</span>
              <ShortcutKbd keys={`${mod}K`} size="xs" variant="subtle" />
            </button>
          </SimpleTooltip>

          <span className="opacity-30">|</span>

          {saving ? (
            <span className="inline-flex items-center gap-1">
              <Loader2 className="size-[11px] animate-spin" />
              {t("sidePanel.editorStatusSaving")}
            </span>
          ) : conflict ? (
            <span className="text-signal-warning">{t("sidePanel.editorStatusConflict")}</span>
          ) : dirty ? (
            <span className="inline-flex items-center gap-1.5">
              <span>{t("sidePanel.editorStatusUnsaved")}</span>
              <ShortcutKbd keys={`${mod}S`} size="xs" variant="subtle" />
            </span>
          ) : (
            <span className="opacity-70">{t("sidePanel.editorStatusSaved")}</span>
          )}
        </div>
      </div>
    </div>
  );
});

export default WorkspaceCodeEditor;
