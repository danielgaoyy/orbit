import type { Extension } from "@codemirror/state";
import { cpp } from "@codemirror/lang-cpp";
import { css } from "@codemirror/lang-css";
import { go } from "@codemirror/lang-go";
import { html } from "@codemirror/lang-html";
import { javascript } from "@codemirror/lang-javascript";
import { json } from "@codemirror/lang-json";
import { markdown } from "@codemirror/lang-markdown";
import { python } from "@codemirror/lang-python";
import { rust } from "@codemirror/lang-rust";
import { yaml } from "@codemirror/lang-yaml";
import {
  type CodeMirrorLanguageKey,
  resolveCodeMirrorLanguageKey,
} from "@/lib/codemirror-helpers";

/**
 * Build the official CodeMirror 6 language extension for the given file language & path.
 */
export function createCodeMirrorLanguageExtension(
  language?: string | null,
  filePath?: string | null,
): Extension {
  const key: CodeMirrorLanguageKey = resolveCodeMirrorLanguageKey(language, filePath);
  switch (key) {
    case "javascript":
      return javascript();
    case "jsx":
      return javascript({ jsx: true });
    case "typescript":
      return javascript({ typescript: true });
    case "tsx":
      return javascript({ typescript: true, jsx: true });
    case "rust":
      return rust();
    case "python":
      return python();
    case "json":
      return json();
    case "css":
      return css();
    case "html":
      return html();
    case "markdown":
      return markdown();
    case "go":
      return go();
    case "cpp":
      return cpp();
    case "yaml":
      return yaml();
    case "plain":
    default:
      return [];
  }
}
