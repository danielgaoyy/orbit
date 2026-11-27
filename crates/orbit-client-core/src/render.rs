//! Pure-Rust 120Hz client rendering pipeline:
//! - Streaming CommonMark/GFM parser (`pulldown-cmark`) with unclosed-fence healing
//! - Zero-C-dependency syntax highlighting (`syntect` pure-Rust `default-fancy`)
//! - Unified & inline Git diff structural model (`similar`)
//! - Run-length coalesced VT100/ANSI terminal grid (`vt100`) for native mobile/desktop canvases

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, OnceLock};

use pulldown_cmark::{Alignment, CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use serde::{Deserialize, Serialize};
use similar::{ChangeTag, TextDiff};
use syntect::highlighting::{
    FontStyle, HighlightIterator, HighlightState, Highlighter, Style, Theme, ThemeSet,
};
use syntect::parsing::{ParseState, ScopeStack, SyntaxSet};
use syntect::util::LinesWithEndings;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InlineSpanView {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    pub strikethrough: bool,
    pub inline_code: bool,
    pub link_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub math_ml: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TableAlignmentView {
    None,
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TableCellView {
    pub spans: Vec<InlineSpanView>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TableRowView {
    pub cells: Vec<TableCellView>,
}

pub const DARK_PLAIN_FG_HEX: &str = "#E2E8F0";
pub const LIGHT_PLAIN_FG_HEX: &str = "#24292F";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SyntaxThemeKind {
    Dark,
    Light,
}

fn default_light_fg_hex_string() -> String {
    LIGHT_PLAIN_FG_HEX.to_string()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HighlightedTokenView {
    pub text: String,
    pub fg_hex: String,
    #[serde(default = "default_light_fg_hex_string")]
    pub light_fg_hex: String,
    pub bold: bool,
    pub italic: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HighlightedLineView {
    pub line_number: u32,
    pub tokens: Vec<HighlightedTokenView>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RichBlockView {
    Heading {
        level: u8,
        spans: Vec<InlineSpanView>,
    },
    Paragraph {
        spans: Vec<InlineSpanView>,
    },
    CodeBlock {
        language: String,
        raw_code: String,
        lines: Vec<HighlightedLineView>,
    },
    ListItem {
        ordered: bool,
        index: u32,
        checked: Option<bool>,
        #[serde(default)]
        depth: u8,
        spans: Vec<InlineSpanView>,
    },
    BlockQuote {
        spans: Vec<InlineSpanView>,
    },
    Table {
        alignments: Vec<TableAlignmentView>,
        headers: Vec<TableCellView>,
        rows: Vec<TableRowView>,
    },
    MathBlock {
        tex: String,
        math_ml: String,
    },
    HorizontalRule,
}

fn syntax_set() -> &'static SyntaxSet {
    static PS: OnceLock<SyntaxSet> = OnceLock::new();
    PS.get_or_init(SyntaxSet::load_defaults_newlines)
}

fn bundled_theme_set() -> &'static ThemeSet {
    static TS: OnceLock<ThemeSet> = OnceLock::new();
    TS.get_or_init(ThemeSet::load_defaults)
}

fn default_dark_theme() -> Option<&'static Theme> {
    let ts = bundled_theme_set();
    ts.themes
        .get("base16-ocean.dark")
        .or_else(|| ts.themes.values().next())
}

fn default_light_theme() -> Option<&'static Theme> {
    let ts = bundled_theme_set();
    ts.themes
        .get("InspiredGitHub")
        .or_else(|| ts.themes.get("Solarized (light)"))
        .or_else(|| ts.themes.values().next())
}

/// Resolve the bundled `syntect` theme for the given appearance mode.
pub fn syntax_theme(kind: SyntaxThemeKind) -> Option<&'static Theme> {
    match kind {
        SyntaxThemeKind::Dark => default_dark_theme(),
        SyntaxThemeKind::Light => default_light_theme(),
    }
}

/// Default plain-text token foreground color for the given appearance mode.
pub fn default_plain_fg_hex(kind: SyntaxThemeKind) -> &'static str {
    match kind {
        SyntaxThemeKind::Dark => DARK_PLAIN_FG_HEX,
        SyntaxThemeKind::Light => LIGHT_PLAIN_FG_HEX,
    }
}

/// Heal an unclosed fenced code block or trailing unclosed display math block (`$$` / `\[`)
/// while assistant tokens are still streaming so the UI never flickers.
/// Returns `(healed_markdown, had_unclosed_code_fence)`.
fn heal_streaming_fences(markdown: &str) -> (String, bool) {
    let mut open_fence: Option<&str> = None;
    let mut open_dollar_block = false;
    let mut open_bracket_block = false;

    for line in markdown.lines() {
        let trimmed = line.trim_start();
        let fence_marker = if trimmed.starts_with("````") {
            Some("````")
        } else if trimmed.starts_with("```") {
            Some("```")
        } else if trimmed.starts_with("~~~") {
            Some("~~~")
        } else {
            None
        };

        if let Some(marker) = fence_marker {
            match open_fence {
                None => open_fence = Some(marker),
                Some(active) if active == marker && trimmed.trim() == active => {
                    open_fence = None;
                }
                _ => {}
            }
            continue;
        }

        if open_fence.is_some() {
            continue;
        }

        let clean = trimmed.trim();
        if open_dollar_block {
            if clean == "$$" || clean.ends_with("$$") {
                open_dollar_block = false;
            }
        } else if open_bracket_block {
            if clean.contains("\\]") {
                open_bracket_block = false;
            }
        } else if clean == "$$" {
            open_dollar_block = true;
        } else if let Some(rest) = clean.strip_prefix("$$") {
            if !rest.contains("$$") {
                open_dollar_block = true;
            }
        } else if let Some(rest) = clean.strip_prefix("\\[")
            && !rest.contains("\\]")
        {
            open_bracket_block = true;
        }
    }

    if let Some(unclosed) = open_fence {
        (format!("{markdown}\n{unclosed}"), true)
    } else if open_dollar_block {
        (format!("{markdown}\n$$"), false)
    } else if open_bracket_block {
        (format!("{markdown}\n\\]"), false)
    } else {
        (markdown.to_string(), false)
    }
}

/// Fast plain-token code line builder used for actively streaming unclosed code blocks
/// so incremental token chunks never invoke `syntect` or pollute the global LRU highlight cache.
fn plain_code_lines(code: &str) -> Vec<HighlightedLineView> {
    let mut out: Vec<HighlightedLineView> = LinesWithEndings::from(code)
        .enumerate()
        .map(|(idx, line)| HighlightedLineView {
            line_number: (idx + 1) as u32,
            tokens: vec![HighlightedTokenView {
                text: line.trim_end_matches(&['\r', '\n'][..]).to_string(),
                fg_hex: DARK_PLAIN_FG_HEX.to_string(),
                light_fg_hex: LIGHT_PLAIN_FG_HEX.to_string(),
                bold: false,
                italic: false,
            }],
        })
        .collect();
    if out.is_empty() && !code.is_empty() {
        out.push(HighlightedLineView {
            line_number: 1,
            tokens: vec![HighlightedTokenView {
                text: code.to_string(),
                fg_hex: DARK_PLAIN_FG_HEX.to_string(),
                light_fg_hex: LIGHT_PLAIN_FG_HEX.to_string(),
                bold: false,
                italic: false,
            }],
        });
    }
    out
}

/// Map a file path or filename to a canonical syntax highlighting token for `syntect`.
pub fn detect_language_for_path(path: &str) -> &'static str {
    let norm = path.replace('\\', "/");
    let basename = norm.rsplit('/').next().unwrap_or(norm.as_str());
    let lower_base = basename.to_ascii_lowercase();
    match lower_base.as_str() {
        "dockerfile" => return "dockerfile",
        "makefile" | "gnumakefile" => return "makefile",
        "cargo.toml" | "pyproject.toml" => return "toml",
        "cargo.lock" => return "toml",
        ".gitignore" | ".env" | ".env.local" | ".env.example" => return "sh",
        _ => {}
    }
    let Some((_, ext)) = basename.rsplit_once('.') else {
        return "text";
    };
    match ext.to_ascii_lowercase().as_str() {
        "rs" => "rs",
        "ts" | "mts" | "cts" => "ts",
        "tsx" => "tsx",
        "js" | "mjs" | "cjs" => "js",
        "jsx" => "jsx",
        "json" | "jsonc" | "json5" => "json",
        "toml" => "toml",
        "yaml" | "yml" => "yaml",
        "md" | "mdx" | "markdown" => "markdown",
        "py" | "pyi" => "py",
        "go" => "go",
        "swift" => "swift",
        "kt" | "kts" => "kotlin",
        "java" => "java",
        "c" | "h" => "c",
        "cc" | "cpp" | "cxx" | "hpp" | "hh" => "cpp",
        "cs" => "cs",
        "rb" => "rb",
        "php" => "php",
        "sh" | "bash" | "zsh" => "sh",
        "fish" => "sh",
        "css" => "css",
        "scss" | "sass" => "scss",
        "svg" => "svg",
        "html" | "htm" | "xml" => "html",
        "sql" => "sql",
        "graphql" | "gql" => "graphql",
        "lua" => "lua",
        "zig" => "zig",
        "ets" => "ts",
        "diff" | "patch" => "diff",
        "ini" | "conf" | "cfg" => "ini",
        _ => "text",
    }
}

/// Highlight a code snippet into 24-bit `#RRGGBB` styled token lines in pure Rust.
pub fn highlight_code_lines(language: &str, code: &str) -> Vec<HighlightedLineView> {
    highlight_code_lines_capped(language, code, usize::MAX)
}

const CODE_HIGHLIGHT_CACHE_CAP: usize = 512;
const CODE_HIGHLIGHT_MAX_CACHE_BYTES: usize = 128 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct CodeHighlightCacheKey {
    lang_token: String,
    content_hash: u64,
    content_len: usize,
    max_highlighted_lines: usize,
}

struct CodeHighlightBucketEntry {
    raw_code: Arc<str>,
    lines: Arc<Vec<HighlightedLineView>>,
}

struct CodeHighlightLruCache {
    order: VecDeque<(CodeHighlightCacheKey, u64)>,
    entries: HashMap<CodeHighlightCacheKey, Vec<CodeHighlightBucketEntry>>,
    total_entries: usize,
}

impl CodeHighlightLruCache {
    fn new() -> Self {
        Self {
            order: VecDeque::new(),
            entries: HashMap::new(),
            total_entries: 0,
        }
    }

    fn get(&mut self, key: &CodeHighlightCacheKey, code: &str) -> Option<Vec<HighlightedLineView>> {
        let bucket = self.entries.get(key)?;
        let hit = bucket
            .iter()
            .find(|entry| entry.raw_code.as_ref() == code)?;
        Some((*hit.lines).clone())
    }

    fn insert(&mut self, key: CodeHighlightCacheKey, code: &str, lines: Vec<HighlightedLineView>) {
        if let Some(bucket) = self.entries.get_mut(&key) {
            if let Some(existing) = bucket.iter_mut().find(|e| e.raw_code.as_ref() == code) {
                existing.lines = Arc::new(lines);
                return;
            }
            bucket.push(CodeHighlightBucketEntry {
                raw_code: Arc::from(code),
                lines: Arc::new(lines),
            });
            self.order.push_back((key, fnv1a_64(code.as_bytes())));
            self.total_entries += 1;
        } else {
            self.entries.insert(
                key.clone(),
                vec![CodeHighlightBucketEntry {
                    raw_code: Arc::from(code),
                    lines: Arc::new(lines),
                }],
            );
            self.order.push_back((key, fnv1a_64(code.as_bytes())));
            self.total_entries += 1;
        }

        while self.total_entries > CODE_HIGHLIGHT_CACHE_CAP {
            let Some((old_key, _)) = self.order.pop_front() else {
                break;
            };
            if let Some(mut bucket) = self.entries.remove(&old_key) {
                if !bucket.is_empty() {
                    bucket.remove(0);
                    self.total_entries = self.total_entries.saturating_sub(1);
                }
                if !bucket.is_empty() {
                    self.entries.insert(old_key, bucket);
                }
            }
        }
    }
}

pub(crate) fn fnv1a_64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for &b in bytes {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

fn code_highlight_cache() -> &'static Mutex<CodeHighlightLruCache> {
    static CACHE: OnceLock<Mutex<CodeHighlightLruCache>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(CodeHighlightLruCache::new()))
}

#[cfg(test)]
pub(crate) fn code_highlight_cache_len() -> usize {
    code_highlight_cache()
        .lock()
        .map(|g| g.total_entries)
        .unwrap_or(0)
}

#[cfg(test)]
pub(crate) fn code_highlight_cache_contains(lang: &str, code: &str) -> bool {
    let key = CodeHighlightCacheKey {
        lang_token: lang.trim().to_string(),
        content_hash: fnv1a_64(code.as_bytes()),
        content_len: code.len(),
        max_highlighted_lines: usize::MAX,
    };
    code_highlight_cache().lock().ok().is_some_and(|g| {
        g.entries
            .get(&key)
            .is_some_and(|bucket| bucket.iter().any(|e| e.raw_code.as_ref() == code))
    })
}

/// Hard upper bound for workspace file preview syntax highlighting on mobile/HarmonyOS clients.
pub const MAX_WORKSPACE_HIGHLIGHT_LINES: usize = 250;
/// Maximum byte length of a single line passed to `syntect` regex grammar parsing.
/// Longer lines (e.g. minified JS/JSON bundles) gracefully fall back to a plain-text span in O(1) time.
pub const MAX_HIGHLIGHT_LINE_BYTES: usize = 4_096;
/// Strict per-line regex budget for `syntect` inside bounded preview highlighting so 4 KiB
/// minified/long-identifier lines never trigger 10–100 ms regex backtracking.
pub const MAX_SYNTECT_REGEX_LINE_BYTES: usize = 1_024;
/// Maximum cumulative source bytes passed through `syntect` per highlight call.
pub const MAX_HIGHLIGHT_TOTAL_BYTES: usize = 64 * 1024;
/// Maximum UTF-8 bytes materialized per line in `highlight_workspace_code_capped` so huge
/// single-line minified inputs never explode synchronous FFI JSON serialization size.
pub const MAX_WORKSPACE_HIGHLIGHT_LINE_PREVIEW_BYTES: usize = 4_096;

/// UTF-8 safe prefix truncation returning `(slice, was_truncated)` with byte length `<= max_bytes`.
fn truncate_utf8_floor(text: &str, max_bytes: usize) -> (&str, bool) {
    if text.len() <= max_bytes {
        return (text, false);
    }
    let mut end = max_bytes;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    (text.get(..end).unwrap_or(""), true)
}

/// Fast safe SWAR byte-level newline counter (`0x0A`) — 8 bytes per step, zero `unsafe`.
fn count_newlines(bytes: &[u8]) -> usize {
    const LF_MASK: u64 = 0x0A0A_0A0A_0A0A_0A0A;
    const LO_BITS: u64 = 0x0101_0101_0101_0101;
    const HI_BITS: u64 = 0x8080_8080_8080_8080;
    let mut total = 0usize;
    let (chunks, rem) = bytes.as_chunks::<8>();
    for chunk in chunks {
        let word = u64::from_ne_bytes(*chunk);
        let xored = word ^ LF_MASK;
        let has_lf = xored.wrapping_sub(LO_BITS) & !xored & HI_BITS;
        total += has_lf.count_ones() as usize;
    }
    for &b in rem {
        if b == b'\n' {
            total += 1;
        }
    }
    total
}

/// Fast safe SWAR index of the first `\n` (`0x0A`) in `bytes`.
fn find_newline(bytes: &[u8]) -> Option<usize> {
    const LF_MASK: u64 = 0x0A0A_0A0A_0A0A_0A0A;
    const LO_BITS: u64 = 0x0101_0101_0101_0101;
    const HI_BITS: u64 = 0x8080_8080_8080_8080;
    let mut offset = 0usize;
    let (chunks, rem) = bytes.as_chunks::<8>();
    for chunk in chunks {
        let word = u64::from_ne_bytes(*chunk);
        let xored = word ^ LF_MASK;
        let has_lf = xored.wrapping_sub(LO_BITS) & !xored & HI_BITS;
        if has_lf != 0 {
            for (i, &b) in chunk.iter().enumerate() {
                if b == b'\n' {
                    return Some(offset + i);
                }
            }
        }
        offset += 8;
    }
    for (i, &b) in rem.iter().enumerate() {
        if b == b'\n' {
            return Some(offset + i);
        }
    }
    None
}

/// Fast byte-level line count matching `LinesWithEndings::from(code).count()` for non-empty inputs.
fn fast_count_lines(code: &str) -> usize {
    if code.is_empty() {
        return 0;
    }
    let newlines = count_newlines(code.as_bytes());
    if code.ends_with('\n') {
        newlines
    } else {
        newlines.saturating_add(1)
    }
}

fn default_total_lines_exact() -> bool {
    true
}

/// Highlighted workspace file preview with hard line-cap truncation metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HighlightedCodeView {
    pub language: String,
    pub lines: Vec<HighlightedLineView>,
    pub total_lines: u32,
    /// `true` when `total_lines` reflects the exact line count of the source buffer;
    /// `false` when a synchronous bounded scan hit its prefix window and `total_lines`
    /// is only a lower bound (`R4-P1-2`).
    #[serde(default = "default_total_lines_exact")]
    pub total_lines_exact: bool,
    pub truncated: bool,
}

/// Highlight up to `max_lines` (capped at [`MAX_WORKSPACE_HIGHLIGHT_LINES`] = 250) lines while
/// enforcing per-line (`MAX_WORKSPACE_HIGHLIGHT_LINE_PREVIEW_BYTES` = 4 KiB) and total syntax
/// parse (`MAX_HIGHLIGHT_TOTAL_BYTES` = 64 KiB) budgets BEFORE allocating line strings.
/// Returns `(lines, was_truncated_by_lines_or_bytes)` so both sync `highlight_workspace_code_json`
/// and async `read_workspace_file_json` share the exact same bounded preview budget.
pub fn highlight_code_lines_prefix_bounded(
    language: &str,
    code: &str,
    max_lines: usize,
) -> (Vec<HighlightedLineView>, bool) {
    highlight_code_lines_prefix_bounded_with_plain_lines(language, code, max_lines, &[])
}

/// Bounded prefix highlighter with optional caller-provided 1-based `plain_line_numbers`
/// (e.g. lines already clipped from >4 KiB source lines on the client before FFI serialization).
/// Any pre-clipped line or line exceeding [`MAX_SYNTECT_REGEX_LINE_BYTES`] bypasses `syntect`
/// regex parsing in O(1) time and emits a single bounded plain-text span.
pub fn highlight_code_lines_prefix_bounded_with_plain_lines(
    language: &str,
    code: &str,
    max_lines: usize,
    plain_line_numbers: &[u32],
) -> (Vec<HighlightedLineView>, bool) {
    let effective_cap = max_lines.min(MAX_WORKSPACE_HIGHLIGHT_LINES);
    if code.is_empty() {
        return (Vec::new(), false);
    }
    if effective_cap == 0 {
        return (Vec::new(), true);
    }

    let lang_token = language.trim();
    let ps = syntax_set();
    let syntax = if lang_token.is_empty() {
        ps.find_syntax_plain_text()
    } else {
        ps.find_syntax_by_token(lang_token)
            .or_else(|| match lang_token {
                "ts" | "tsx" => ps.find_syntax_by_token("js"),
                "toml" => ps.find_syntax_by_token("ini"),
                _ => None,
            })
            .unwrap_or_else(|| ps.find_syntax_plain_text())
    };
    let themes = default_dark_theme().zip(default_light_theme());
    let mut parse_state = ParseState::new(syntax);
    let mut highlight_engines = themes.map(|(dark_theme, light_theme)| {
        let dark_highlighter = Highlighter::new(dark_theme);
        let light_highlighter = Highlighter::new(light_theme);
        let dark_state = HighlightState::new(&dark_highlighter, ScopeStack::new());
        let light_state = HighlightState::new(&light_highlighter, ScopeStack::new());
        (dark_highlighter, light_highlighter, dark_state, light_state)
    });

    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    #[inline]
    fn rgb_to_hex(r: u8, g: u8, b: u8, fallback: &str) -> String {
        let bytes = [
            b'#',
            HEX[(r >> 4) as usize],
            HEX[(r & 0x0F) as usize],
            HEX[(g >> 4) as usize],
            HEX[(g & 0x0F) as usize],
            HEX[(b >> 4) as usize],
            HEX[(b & 0x0F) as usize],
        ];
        String::from_utf8(bytes.to_vec()).unwrap_or_else(|_| fallback.to_string())
    }

    fn merge_dual_theme_spans(
        dark_ranges: &[(Style, &str)],
        light_ranges: &[(Style, &str)],
    ) -> Vec<HighlightedTokenView> {
        let mut coalesced: Vec<HighlightedTokenView> =
            Vec::with_capacity(dark_ranges.len().max(light_ranges.len()));
        let mut di = 0usize;
        let mut doff = 0usize;
        let mut li = 0usize;
        let mut loff = 0usize;
        while di < dark_ranges.len() && li < light_ranges.len() {
            let (d_style, d_str) = dark_ranges[di];
            let (l_style, l_str) = light_ranges[li];
            let d_rem = d_str.get(doff..).unwrap_or("");
            let l_rem = l_str.get(loff..).unwrap_or("");
            if d_rem.is_empty() {
                di += 1;
                doff = 0;
                continue;
            }
            if l_rem.is_empty() {
                li += 1;
                loff = 0;
                continue;
            }
            let take = d_rem.len().min(l_rem.len());
            if let Some(piece) = d_rem.get(..take) {
                let clean = piece.trim_end_matches(&['\r', '\n'][..]);
                if !clean.is_empty() {
                    let fg_hex = rgb_to_hex(
                        d_style.foreground.r,
                        d_style.foreground.g,
                        d_style.foreground.b,
                        DARK_PLAIN_FG_HEX,
                    );
                    let light_fg_hex = rgb_to_hex(
                        l_style.foreground.r,
                        l_style.foreground.g,
                        l_style.foreground.b,
                        LIGHT_PLAIN_FG_HEX,
                    );
                    let bold = d_style.font_style.contains(FontStyle::BOLD);
                    let italic = d_style.font_style.contains(FontStyle::ITALIC);
                    if let Some(prev) = coalesced.last_mut()
                        && prev.fg_hex == fg_hex
                        && prev.light_fg_hex == light_fg_hex
                        && prev.bold == bold
                        && prev.italic == italic
                    {
                        prev.text.push_str(clean);
                    } else {
                        coalesced.push(HighlightedTokenView {
                            text: clean.to_string(),
                            fg_hex,
                            light_fg_hex,
                            bold,
                            italic,
                        });
                    }
                }
            }
            doff += take;
            if doff >= d_str.len() {
                di += 1;
                doff = 0;
            }
            loff += take;
            if loff >= l_str.len() {
                li += 1;
                loff = 0;
            }
        }
        coalesced
    }

    let mut out = Vec::with_capacity(effective_cap.min(64));
    let mut cursor = 0usize;
    let mut highlighted_bytes = 0usize;
    let mut truncated = false;
    let bytes = code.as_bytes();

    while cursor < code.len() && out.len() < effective_cap {
        let rest = &bytes[cursor..];
        let (line_end, next_cursor) = match find_newline(rest) {
            Some(rel_nl) => (cursor + rel_nl + 1, cursor + rel_nl + 1),
            None => (code.len(), code.len()),
        };
        let raw_line = code.get(cursor..line_end).unwrap_or("");
        let trimmed_line = raw_line.trim_end_matches(&['\r', '\n'][..]);
        let line_number = (out.len() + 1) as u32;

        let is_caller_clipped = plain_line_numbers.contains(&line_number);
        let line_over_preview =
            is_caller_clipped || trimmed_line.len() > MAX_WORKSPACE_HIGHLIGHT_LINE_PREVIEW_BYTES;
        if line_over_preview {
            truncated = true;
            let (clipped, _) =
                truncate_utf8_floor(trimmed_line, MAX_WORKSPACE_HIGHLIGHT_LINE_PREVIEW_BYTES);
            out.push(HighlightedLineView {
                line_number,
                tokens: if clipped.is_empty() {
                    Vec::new()
                } else {
                    vec![HighlightedTokenView {
                        text: clipped.to_string(),
                        fg_hex: DARK_PLAIN_FG_HEX.to_string(),
                        light_fg_hex: LIGHT_PLAIN_FG_HEX.to_string(),
                        bold: false,
                        italic: false,
                    }]
                },
            });
            cursor = next_cursor;
            continue;
        }

        let within_syntax_budget = raw_line.len() <= MAX_SYNTECT_REGEX_LINE_BYTES
            && raw_line.len() < MAX_HIGHLIGHT_LINE_BYTES
            && highlighted_bytes.saturating_add(raw_line.len()) <= MAX_HIGHLIGHT_TOTAL_BYTES;
        let tokens = if within_syntax_budget {
            if let Some((dark_hl, light_hl, dark_state, light_state)) = highlight_engines.as_mut() {
                highlighted_bytes = highlighted_bytes.saturating_add(raw_line.len());
                match parse_state.parse_line(raw_line, ps) {
                    Ok(ops) => {
                        let dark_ranges: Vec<(Style, &str)> =
                            HighlightIterator::new(dark_state, &ops, raw_line, dark_hl).collect();
                        let light_ranges: Vec<(Style, &str)> =
                            HighlightIterator::new(light_state, &ops, raw_line, light_hl).collect();
                        merge_dual_theme_spans(&dark_ranges, &light_ranges)
                    }
                    Err(_) => vec![HighlightedTokenView {
                        text: trimmed_line.to_string(),
                        fg_hex: DARK_PLAIN_FG_HEX.to_string(),
                        light_fg_hex: LIGHT_PLAIN_FG_HEX.to_string(),
                        bold: false,
                        italic: false,
                    }],
                }
            } else {
                vec![HighlightedTokenView {
                    text: trimmed_line.to_string(),
                    fg_hex: DARK_PLAIN_FG_HEX.to_string(),
                    light_fg_hex: LIGHT_PLAIN_FG_HEX.to_string(),
                    bold: false,
                    italic: false,
                }]
            }
        } else {
            vec![HighlightedTokenView {
                text: trimmed_line.to_string(),
                fg_hex: DARK_PLAIN_FG_HEX.to_string(),
                light_fg_hex: LIGHT_PLAIN_FG_HEX.to_string(),
                bold: false,
                italic: false,
            }]
        };

        out.push(HighlightedLineView {
            line_number,
            tokens,
        });
        cursor = next_cursor;
    }

    if cursor < code.len() {
        truncated = true;
    }
    (out, truncated)
}

/// Highlight up to `max_lines` (hard-capped at [`MAX_WORKSPACE_HIGHLIGHT_LINES`] = 250) lines
/// of workspace code using `syntect` with UTF-8 boundary safety, per-line/total byte budgets,
/// and graceful plain-text fallback on unknown languages or minified single-line bundles.
pub fn highlight_workspace_code_capped(
    language: &str,
    code: &str,
    max_lines: usize,
) -> HighlightedCodeView {
    highlight_workspace_code_capped_with_plain_lines(language, code, max_lines, &[])
}

/// Variant of [`highlight_workspace_code_capped`] accepting caller-supplied 1-based line numbers
/// that were pre-clipped from oversized source lines before FFI transfer (`R3-P1-3`).
pub fn highlight_workspace_code_capped_with_plain_lines(
    language: &str,
    code: &str,
    max_lines: usize,
    plain_line_numbers: &[u32],
) -> HighlightedCodeView {
    let lang_token = language.trim();
    let effective_cap = max_lines.min(MAX_WORKSPACE_HIGHLIGHT_LINES);
    if code.is_empty() {
        return HighlightedCodeView {
            language: lang_token.to_string(),
            lines: Vec::new(),
            total_lines: 0,
            total_lines_exact: true,
            truncated: false,
        };
    }
    let total_lines_usize = fast_count_lines(code);
    let total_lines = u32::try_from(total_lines_usize).unwrap_or(u32::MAX);
    let (lines, preview_truncated) = highlight_code_lines_prefix_bounded_with_plain_lines(
        lang_token,
        code,
        effective_cap,
        plain_line_numbers,
    );
    let truncated = preview_truncated || total_lines_usize > effective_cap;
    HighlightedCodeView {
        language: lang_token.to_string(),
        lines,
        total_lines,
        total_lines_exact: true,
        truncated,
    }
}

/// Highlight only the first `max_highlighted_lines` lines with `syntect` without materializing
/// plain fallback wrappers for trailing lines. Ideal for virtualized viewers that render
/// off-screen lines directly from raw content and lazily highlight deeper chunks on scroll.
pub fn highlight_code_lines_prefix(
    language: &str,
    code: &str,
    max_highlighted_lines: usize,
) -> Vec<HighlightedLineView> {
    if max_highlighted_lines == 0 || code.is_empty() {
        return Vec::new();
    }
    let mut lines_seen = 0usize;
    let mut end_byte = code.len();
    for (idx, b) in code.bytes().enumerate() {
        if b == b'\n' {
            lines_seen += 1;
            if lines_seen >= max_highlighted_lines {
                end_byte = idx + 1;
                break;
            }
        }
    }
    let prefix = code.get(..end_byte).unwrap_or(code);
    highlight_code_lines_capped(language, prefix, max_highlighted_lines)
}

/// Highlight up to `max_highlighted_lines` with `syntect`, emitting fast plain-styled lines beyond the cap.
pub fn highlight_code_lines_capped(
    language: &str,
    code: &str,
    max_highlighted_lines: usize,
) -> Vec<HighlightedLineView> {
    let lang_token = language.trim();
    let cacheable = code.len() <= CODE_HIGHLIGHT_MAX_CACHE_BYTES;
    let cache_key = if cacheable {
        let key = CodeHighlightCacheKey {
            lang_token: lang_token.to_string(),
            content_hash: fnv1a_64(code.as_bytes()),
            content_len: code.len(),
            max_highlighted_lines,
        };
        if let Ok(mut guard) = code_highlight_cache().lock()
            && let Some(cached) = guard.get(&key, code)
        {
            return cached;
        }
        Some(key)
    } else {
        None
    };

    let ps = syntax_set();
    let syntax = if lang_token.is_empty() {
        ps.find_syntax_plain_text()
    } else {
        ps.find_syntax_by_token(lang_token)
            .or_else(|| match lang_token {
                "ts" | "tsx" => ps.find_syntax_by_token("js"),
                "toml" => ps.find_syntax_by_token("ini"),
                _ => None,
            })
            .unwrap_or_else(|| ps.find_syntax_plain_text())
    };

    let (Some(dark_theme), Some(light_theme)) = (default_dark_theme(), default_light_theme())
    else {
        return code
            .lines()
            .enumerate()
            .map(|(idx, line)| HighlightedLineView {
                line_number: (idx + 1) as u32,
                tokens: vec![HighlightedTokenView {
                    text: line.to_string(),
                    fg_hex: DARK_PLAIN_FG_HEX.to_string(),
                    light_fg_hex: LIGHT_PLAIN_FG_HEX.to_string(),
                    bold: false,
                    italic: false,
                }],
            })
            .collect();
    };

    let dark_highlighter = Highlighter::new(dark_theme);
    let light_highlighter = Highlighter::new(light_theme);
    let mut parse_state = ParseState::new(syntax);
    let mut dark_state = HighlightState::new(&dark_highlighter, ScopeStack::new());
    let mut light_state = HighlightState::new(&light_highlighter, ScopeStack::new());
    let mut out = Vec::with_capacity(
        code.bytes()
            .filter(|&b| b == b'\n')
            .count()
            .saturating_add(1),
    );

    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    #[inline]
    fn rgb_to_hex(r: u8, g: u8, b: u8, fallback: &str) -> String {
        let bytes = [
            b'#',
            HEX[(r >> 4) as usize],
            HEX[(r & 0x0F) as usize],
            HEX[(g >> 4) as usize],
            HEX[(g & 0x0F) as usize],
            HEX[(b >> 4) as usize],
            HEX[(b & 0x0F) as usize],
        ];
        String::from_utf8(bytes.to_vec()).unwrap_or_else(|_| fallback.to_string())
    }

    fn merge_dual_theme_spans(
        dark_ranges: &[(Style, &str)],
        light_ranges: &[(Style, &str)],
    ) -> Vec<HighlightedTokenView> {
        let mut coalesced: Vec<HighlightedTokenView> =
            Vec::with_capacity(dark_ranges.len().max(light_ranges.len()));
        let mut di = 0usize;
        let mut doff = 0usize;
        let mut li = 0usize;
        let mut loff = 0usize;

        while di < dark_ranges.len() && li < light_ranges.len() {
            let (d_style, d_str) = dark_ranges[di];
            let (l_style, l_str) = light_ranges[li];
            let d_rem = d_str.get(doff..).unwrap_or("");
            let l_rem = l_str.get(loff..).unwrap_or("");

            if d_rem.is_empty() {
                di += 1;
                doff = 0;
                continue;
            }
            if l_rem.is_empty() {
                li += 1;
                loff = 0;
                continue;
            }

            let take = d_rem.len().min(l_rem.len());
            if let Some(piece) = d_rem.get(..take) {
                let clean = piece.trim_end_matches(&['\r', '\n'][..]);
                if !clean.is_empty() {
                    let fg_hex = rgb_to_hex(
                        d_style.foreground.r,
                        d_style.foreground.g,
                        d_style.foreground.b,
                        DARK_PLAIN_FG_HEX,
                    );
                    let light_fg_hex = rgb_to_hex(
                        l_style.foreground.r,
                        l_style.foreground.g,
                        l_style.foreground.b,
                        LIGHT_PLAIN_FG_HEX,
                    );
                    let bold = d_style.font_style.contains(FontStyle::BOLD);
                    let italic = d_style.font_style.contains(FontStyle::ITALIC);
                    if let Some(prev) = coalesced.last_mut()
                        && prev.fg_hex == fg_hex
                        && prev.light_fg_hex == light_fg_hex
                        && prev.bold == bold
                        && prev.italic == italic
                    {
                        prev.text.push_str(clean);
                    } else {
                        coalesced.push(HighlightedTokenView {
                            text: clean.to_string(),
                            fg_hex,
                            light_fg_hex,
                            bold,
                            italic,
                        });
                    }
                }
            }

            doff += take;
            if doff >= d_str.len() {
                di += 1;
                doff = 0;
            }
            loff += take;
            if loff >= l_str.len() {
                li += 1;
                loff = 0;
            }
        }
        coalesced
    }

    let mut highlighted_bytes: usize = 0;
    for (idx, line) in LinesWithEndings::from(code).enumerate() {
        let within_budget = idx < max_highlighted_lines
            && line.len() <= MAX_HIGHLIGHT_LINE_BYTES
            && highlighted_bytes.saturating_add(line.len()) <= MAX_HIGHLIGHT_TOTAL_BYTES;
        let tokens = if within_budget {
            highlighted_bytes = highlighted_bytes.saturating_add(line.len());
            match parse_state.parse_line(line, ps) {
                Ok(ops) => {
                    // Single grammar parse pass feeds both native dark (`base16-ocean.dark`) and
                    // native light (`InspiredGitHub`) themes, then coalesces adjacent identical tokens.
                    let dark_ranges: Vec<(Style, &str)> =
                        HighlightIterator::new(&mut dark_state, &ops, line, &dark_highlighter)
                            .collect();
                    let light_ranges: Vec<(Style, &str)> =
                        HighlightIterator::new(&mut light_state, &ops, line, &light_highlighter)
                            .collect();
                    merge_dual_theme_spans(&dark_ranges, &light_ranges)
                }
                Err(_) => vec![HighlightedTokenView {
                    text: line.trim_end_matches(&['\r', '\n'][..]).to_string(),
                    fg_hex: DARK_PLAIN_FG_HEX.to_string(),
                    light_fg_hex: LIGHT_PLAIN_FG_HEX.to_string(),
                    bold: false,
                    italic: false,
                }],
            }
        } else {
            vec![HighlightedTokenView {
                text: line.trim_end_matches(&['\r', '\n'][..]).to_string(),
                fg_hex: DARK_PLAIN_FG_HEX.to_string(),
                light_fg_hex: LIGHT_PLAIN_FG_HEX.to_string(),
                bold: false,
                italic: false,
            }]
        };

        out.push(HighlightedLineView {
            line_number: (idx + 1) as u32,
            tokens,
        });
    }

    if out.is_empty() && !code.is_empty() {
        out.push(HighlightedLineView {
            line_number: 1,
            tokens: vec![HighlightedTokenView {
                text: code.to_string(),
                fg_hex: DARK_PLAIN_FG_HEX.to_string(),
                light_fg_hex: LIGHT_PLAIN_FG_HEX.to_string(),
                bold: false,
                italic: false,
            }],
        });
    }

    if let Some(key) = cache_key
        && let Ok(mut guard) = code_highlight_cache().lock()
    {
        guard.insert(key, code, out.clone());
    }

    out
}

fn map_table_alignment(align: Alignment) -> TableAlignmentView {
    match align {
        Alignment::None => TableAlignmentView::None,
        Alignment::Left => TableAlignmentView::Left,
        Alignment::Center => TableAlignmentView::Center,
        Alignment::Right => TableAlignmentView::Right,
    }
}

fn is_self_closing_html_tag(tag_lower: &str, name: &str) -> bool {
    let Some(rest) = tag_lower
        .strip_prefix('<')
        .and_then(|s| s.strip_suffix('>'))
    else {
        return false;
    };
    let rest = rest.trim();
    if !rest.starts_with(name) {
        return false;
    }
    let after_name = &rest[name.len()..];
    after_name.is_empty()
        || after_name
            .chars()
            .all(|c| c.is_ascii_whitespace() || c == '/')
}

fn matches_open_html_tag(tag_lower: &str, name: &str) -> bool {
    let Some(rest) = tag_lower
        .strip_prefix('<')
        .and_then(|s| s.strip_suffix('>'))
    else {
        return false;
    };
    let rest = rest.trim();
    if !rest.starts_with(name) || rest.ends_with('/') {
        return false;
    }
    let after = &rest[name.len()..];
    after.is_empty() || after.starts_with(|c: char| c.is_ascii_whitespace())
}

fn matches_close_html_tag(tag_lower: &str, name: &str) -> bool {
    let Some(rest) = tag_lower
        .strip_prefix("</")
        .and_then(|s| s.strip_suffix('>'))
    else {
        return false;
    };
    rest.trim() == name
}

fn push_inline_span(
    spans: &mut Vec<InlineSpanView>,
    text: &str,
    bold: bool,
    italic: bool,
    strikethrough: bool,
    inline_code: bool,
    link_url: &Option<String>,
) {
    if text.is_empty() {
        return;
    }
    if let Some(last) = spans.last_mut()
        && last.math_ml.is_none()
        && last.bold == bold
        && last.italic == italic
        && last.strikethrough == strikethrough
        && last.inline_code == inline_code
        && &last.link_url == link_url
    {
        last.text.push_str(text);
        return;
    }
    spans.push(InlineSpanView {
        text: text.to_string(),
        bold,
        italic,
        strikethrough,
        inline_code,
        link_url: link_url.clone(),
        math_ml: None,
    });
}

fn push_inline_math_span(
    spans: &mut Vec<InlineSpanView>,
    tex: &str,
    display: crate::math::MathDisplay,
    bold: bool,
    italic: bool,
    strikethrough: bool,
    link_url: &Option<String>,
) {
    let trimmed = tex.trim();
    if trimmed.is_empty() {
        return;
    }
    let math_ml = crate::math::render_latex_to_mathml(trimmed, display);
    spans.push(InlineSpanView {
        text: trimmed.to_string(),
        bold,
        italic,
        strikethrough,
        inline_code: false,
        link_url: link_url.clone(),
        math_ml: Some(math_ml),
    });
}

fn spans_have_visible_content(spans: &[InlineSpanView]) -> bool {
    spans
        .iter()
        .any(|s| s.math_ml.is_some() || !s.text.trim().is_empty())
}

fn trim_edge_breaks(spans: &mut Vec<InlineSpanView>) {
    while let Some(first) = spans.first_mut() {
        if first.math_ml.is_some() || first.inline_code {
            break;
        }
        let trimmed = first.text.trim_start_matches('\n');
        if trimmed.len() != first.text.len() {
            first.text = trimmed.to_string();
        }
        if first.text.is_empty() {
            spans.remove(0);
        } else {
            break;
        }
    }
    while let Some(last) = spans.last_mut() {
        if last.math_ml.is_some() || last.inline_code {
            break;
        }
        let trimmed = last.text.trim_end_matches('\n');
        if trimmed.len() != last.text.len() {
            last.text = trimmed.to_string();
        }
        if last.text.is_empty() {
            spans.pop();
        } else {
            break;
        }
    }
}

fn flush_active_list_item_prefix(
    blocks: &mut Vec<RichBlockView>,
    cur_spans: &mut Vec<InlineSpanView>,
    list_stack: &mut [Option<u64>],
    list_item_stack: &mut [(Option<bool>, bool)],
) {
    if let Some((checked, already_flushed)) = list_item_stack.last_mut()
        && !*already_flushed
        && !cur_spans.is_empty()
    {
        let (ordered, idx) = match list_stack.last_mut() {
            Some(Some(counter)) => {
                let cur = *counter as u32;
                *counter += 1;
                (true, cur)
            }
            _ => (false, 0),
        };
        let depth = u8::try_from(list_stack.len().saturating_sub(1)).unwrap_or(u8::MAX);
        blocks.push(RichBlockView::ListItem {
            ordered,
            index: idx,
            checked: checked.take(),
            depth,
            spans: std::mem::take(cur_spans),
        });
        *already_flushed = true;
    }
}

/// Parse streaming or completed Markdown into platform-agnostic native render blocks.
pub fn parse_markdown_blocks(markdown: &str, streaming: bool) -> Vec<RichBlockView> {
    parse_markdown_blocks_with_math(markdown, streaming, true)
}

/// Parse Markdown into native render blocks with explicit control over `$...$` / `$$...$$` math.
pub fn parse_markdown_blocks_with_math(
    markdown: &str,
    streaming: bool,
    enable_math: bool,
) -> Vec<RichBlockView> {
    let (normalized, had_unclosed_fence) = if streaming {
        heal_streaming_fences(markdown)
    } else {
        (markdown.to_string(), false)
    };
    let preprocessed = if enable_math {
        crate::math::preprocess_markdown_math(&normalized)
    } else {
        normalized
    };

    let mut options = Options::empty();
    options
        .insert(Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS);
    if enable_math {
        options.insert(Options::ENABLE_MATH);
    }
    let parser = Parser::new_ext(&preprocessed, options).into_offset_iter();

    let mut blocks = Vec::new();
    let mut cur_spans: Vec<InlineSpanView> = Vec::new();
    let mut bold_depth: u32 = 0;
    let mut italic_depth: u32 = 0;
    let mut strike_depth: u32 = 0;
    let mut html_code_depth: u32 = 0;
    let mut current_link: Option<String> = None;

    let mut in_heading: Option<u8> = None;
    let mut in_blockquote = false;
    let mut list_stack: Vec<Option<u64>> = Vec::new();
    let mut list_item_stack: Vec<(Option<bool>, bool)> = Vec::new();

    let mut in_table = false;
    let mut in_table_head = false;
    let mut in_table_cell = false;
    let mut table_alignments: Vec<TableAlignmentView> = Vec::new();
    let mut table_headers: Vec<TableCellView> = Vec::new();
    let mut table_rows: Vec<TableRowView> = Vec::new();
    let mut cur_table_row: Vec<TableCellView> = Vec::new();

    let mut in_code_block: Option<String> = None;
    let mut code_buf = String::new();
    let preprocessed_len = preprocessed.len();

    for (event, range) in parser {
        match event {
            Event::Start(tag) => match tag {
                Tag::Heading { level, .. } => {
                    cur_spans.clear();
                    in_heading = Some(heading_level_u8(level));
                }
                Tag::Paragraph => {
                    if list_item_stack.is_empty() && !in_blockquote && !in_table_cell {
                        cur_spans.clear();
                    } else if !cur_spans.is_empty() {
                        push_inline_span(&mut cur_spans, "\n", false, false, false, false, &None);
                    }
                }
                Tag::CodeBlock(kind) => {
                    flush_active_list_item_prefix(
                        &mut blocks,
                        &mut cur_spans,
                        &mut list_stack,
                        &mut list_item_stack,
                    );
                    let lang = match kind {
                        CodeBlockKind::Fenced(info) => info
                            .split_whitespace()
                            .next()
                            .unwrap_or_default()
                            .to_string(),
                        CodeBlockKind::Indented => String::new(),
                    };
                    in_code_block = Some(lang);
                    code_buf.clear();
                }
                Tag::List(start_idx) => {
                    flush_active_list_item_prefix(
                        &mut blocks,
                        &mut cur_spans,
                        &mut list_stack,
                        &mut list_item_stack,
                    );
                    list_stack.push(start_idx);
                }
                Tag::Item => {
                    cur_spans.clear();
                    list_item_stack.push((None, false));
                }
                Tag::BlockQuote(_) => {
                    flush_active_list_item_prefix(
                        &mut blocks,
                        &mut cur_spans,
                        &mut list_stack,
                        &mut list_item_stack,
                    );
                    cur_spans.clear();
                    in_blockquote = true;
                }
                Tag::Table(aligns) => {
                    flush_active_list_item_prefix(
                        &mut blocks,
                        &mut cur_spans,
                        &mut list_stack,
                        &mut list_item_stack,
                    );
                    if !cur_spans.is_empty() {
                        blocks.push(RichBlockView::Paragraph {
                            spans: std::mem::take(&mut cur_spans),
                        });
                    }
                    in_table = true;
                    in_table_head = false;
                    in_table_cell = false;
                    table_alignments = aligns.into_iter().map(map_table_alignment).collect();
                    table_headers.clear();
                    table_rows.clear();
                    cur_table_row.clear();
                }
                Tag::TableHead => {
                    in_table_head = true;
                    cur_table_row.clear();
                }
                Tag::TableRow => {
                    cur_table_row.clear();
                }
                Tag::TableCell => {
                    in_table_cell = true;
                    cur_spans.clear();
                }
                Tag::Strong => bold_depth += 1,
                Tag::Emphasis => italic_depth += 1,
                Tag::Strikethrough => strike_depth += 1,
                Tag::Link { dest_url, .. } => {
                    current_link = Some(dest_url.to_string());
                }
                _ => {}
            },
            Event::End(tag_end) => match tag_end {
                TagEnd::Heading(_) => {
                    let level = in_heading.take().unwrap_or(2);
                    if !cur_spans.is_empty() {
                        blocks.push(RichBlockView::Heading {
                            level,
                            spans: std::mem::take(&mut cur_spans),
                        });
                    }
                }
                TagEnd::Paragraph => {
                    if list_item_stack.is_empty() && !in_blockquote && !in_table_cell {
                        trim_edge_breaks(&mut cur_spans);
                        if spans_have_visible_content(&cur_spans) {
                            blocks.push(RichBlockView::Paragraph {
                                spans: std::mem::take(&mut cur_spans),
                            });
                        } else {
                            cur_spans.clear();
                        }
                    }
                }
                TagEnd::CodeBlock => {
                    let language = in_code_block.take().unwrap_or_default();
                    let raw_code = code_buf.trim_end_matches('\n').to_string();
                    let is_streaming_tail = had_unclosed_fence && range.end >= preprocessed_len;
                    if !is_streaming_tail
                        && crate::math::is_math_code_fence_lang(&language, &raw_code)
                        && !raw_code.trim().is_empty()
                    {
                        let tex = raw_code.trim().to_string();
                        let math_ml = crate::math::render_latex_to_mathml(
                            &tex,
                            crate::math::MathDisplay::Block,
                        );
                        blocks.push(RichBlockView::MathBlock { tex, math_ml });
                    } else {
                        let lines = if is_streaming_tail {
                            plain_code_lines(&raw_code)
                        } else {
                            highlight_code_lines(&language, &raw_code)
                        };
                        blocks.push(RichBlockView::CodeBlock {
                            language,
                            raw_code,
                            lines,
                        });
                    }
                    code_buf.clear();
                }
                TagEnd::Item => {
                    if let Some((checked, already_flushed)) = list_item_stack.pop() {
                        if !already_flushed {
                            let (ordered, idx) = match list_stack.last_mut() {
                                Some(Some(counter)) => {
                                    let cur = *counter as u32;
                                    *counter += 1;
                                    (true, cur)
                                }
                                _ => (false, 0),
                            };
                            if !cur_spans.is_empty() {
                                let depth = u8::try_from(list_stack.len().saturating_sub(1))
                                    .unwrap_or(u8::MAX);
                                blocks.push(RichBlockView::ListItem {
                                    ordered,
                                    index: idx,
                                    checked,
                                    depth,
                                    spans: std::mem::take(&mut cur_spans),
                                });
                            }
                        } else if !cur_spans.is_empty() {
                            blocks.push(RichBlockView::Paragraph {
                                spans: std::mem::take(&mut cur_spans),
                            });
                        }
                    }
                }
                TagEnd::List(_) => {
                    list_stack.pop();
                }
                TagEnd::BlockQuote(_) => {
                    in_blockquote = false;
                    if !cur_spans.is_empty() {
                        blocks.push(RichBlockView::BlockQuote {
                            spans: std::mem::take(&mut cur_spans),
                        });
                    }
                }
                TagEnd::Strong => bold_depth = bold_depth.saturating_sub(1),
                TagEnd::Emphasis => italic_depth = italic_depth.saturating_sub(1),
                TagEnd::Strikethrough => strike_depth = strike_depth.saturating_sub(1),
                TagEnd::Link => current_link = None,
                TagEnd::TableCell => {
                    in_table_cell = false;
                    cur_table_row.push(TableCellView {
                        spans: std::mem::take(&mut cur_spans),
                    });
                }
                TagEnd::TableHead => {
                    in_table_head = false;
                    table_headers = std::mem::take(&mut cur_table_row);
                }
                TagEnd::TableRow => {
                    table_rows.push(TableRowView {
                        cells: std::mem::take(&mut cur_table_row),
                    });
                }
                TagEnd::Table => {
                    in_table = false;
                    in_table_head = false;
                    in_table_cell = false;
                    if !table_headers.is_empty() || !table_rows.is_empty() {
                        blocks.push(RichBlockView::Table {
                            alignments: std::mem::take(&mut table_alignments),
                            headers: std::mem::take(&mut table_headers),
                            rows: std::mem::take(&mut table_rows),
                        });
                    }
                }
                _ => {}
            },
            Event::Text(text) => {
                if in_code_block.is_some() {
                    code_buf.push_str(&text);
                } else {
                    push_inline_span(
                        &mut cur_spans,
                        &text,
                        bold_depth > 0,
                        italic_depth > 0,
                        strike_depth > 0,
                        html_code_depth > 0,
                        &current_link,
                    );
                }
            }
            Event::Code(code) => {
                push_inline_span(
                    &mut cur_spans,
                    &code,
                    bold_depth > 0,
                    italic_depth > 0,
                    strike_depth > 0,
                    true,
                    &current_link,
                );
            }
            Event::InlineHtml(html) | Event::Html(html) => {
                if in_code_block.is_some() {
                    code_buf.push_str(&html);
                } else {
                    let lower = html.trim().to_ascii_lowercase();
                    if is_self_closing_html_tag(&lower, "br") {
                        push_inline_span(&mut cur_spans, "\n", false, false, false, false, &None);
                    } else if is_self_closing_html_tag(&lower, "hr") {
                        if in_table_cell {
                            push_inline_span(
                                &mut cur_spans,
                                "\n",
                                false,
                                false,
                                false,
                                false,
                                &None,
                            );
                        } else {
                            if !cur_spans.is_empty()
                                && in_heading.is_none()
                                && !in_blockquote
                                && list_item_stack.is_empty()
                            {
                                blocks.push(RichBlockView::Paragraph {
                                    spans: std::mem::take(&mut cur_spans),
                                });
                            }
                            blocks.push(RichBlockView::HorizontalRule);
                        }
                    } else if matches_open_html_tag(&lower, "b")
                        || matches_open_html_tag(&lower, "strong")
                    {
                        bold_depth += 1;
                    } else if matches_close_html_tag(&lower, "b")
                        || matches_close_html_tag(&lower, "strong")
                    {
                        bold_depth = bold_depth.saturating_sub(1);
                    } else if matches_open_html_tag(&lower, "i")
                        || matches_open_html_tag(&lower, "em")
                    {
                        italic_depth += 1;
                    } else if matches_close_html_tag(&lower, "i")
                        || matches_close_html_tag(&lower, "em")
                    {
                        italic_depth = italic_depth.saturating_sub(1);
                    } else if matches_open_html_tag(&lower, "s")
                        || matches_open_html_tag(&lower, "del")
                        || matches_open_html_tag(&lower, "strike")
                    {
                        strike_depth += 1;
                    } else if matches_close_html_tag(&lower, "s")
                        || matches_close_html_tag(&lower, "del")
                        || matches_close_html_tag(&lower, "strike")
                    {
                        strike_depth = strike_depth.saturating_sub(1);
                    } else if matches_open_html_tag(&lower, "code") {
                        html_code_depth += 1;
                    } else if matches_close_html_tag(&lower, "code") {
                        html_code_depth = html_code_depth.saturating_sub(1);
                    }
                }
            }
            Event::SoftBreak | Event::HardBreak => {
                if in_code_block.is_some() {
                    code_buf.push('\n');
                } else {
                    push_inline_span(&mut cur_spans, "\n", false, false, false, false, &None);
                }
            }
            Event::TaskListMarker(checked) => {
                if let Some((chk, _)) = list_item_stack.last_mut() {
                    *chk = Some(checked);
                }
            }
            Event::InlineMath(tex) => {
                if in_code_block.is_some() {
                    code_buf.push_str(&tex);
                } else {
                    push_inline_math_span(
                        &mut cur_spans,
                        &tex,
                        crate::math::MathDisplay::Inline,
                        bold_depth > 0,
                        italic_depth > 0,
                        strike_depth > 0,
                        &current_link,
                    );
                }
            }
            Event::DisplayMath(tex) => {
                if in_code_block.is_some() {
                    code_buf.push_str(&tex);
                } else if in_table_cell || in_heading.is_some() || in_blockquote {
                    push_inline_math_span(
                        &mut cur_spans,
                        &tex,
                        crate::math::MathDisplay::Block,
                        bold_depth > 0,
                        italic_depth > 0,
                        strike_depth > 0,
                        &current_link,
                    );
                } else {
                    if !list_item_stack.is_empty() {
                        flush_active_list_item_prefix(
                            &mut blocks,
                            &mut cur_spans,
                            &mut list_stack,
                            &mut list_item_stack,
                        );
                    }
                    trim_edge_breaks(&mut cur_spans);
                    if spans_have_visible_content(&cur_spans) {
                        blocks.push(RichBlockView::Paragraph {
                            spans: std::mem::take(&mut cur_spans),
                        });
                    } else {
                        cur_spans.clear();
                    }
                    let trimmed = tex.trim();
                    if !trimmed.is_empty() {
                        let math_ml = crate::math::render_latex_to_mathml(
                            trimmed,
                            crate::math::MathDisplay::Block,
                        );
                        blocks.push(RichBlockView::MathBlock {
                            tex: trimmed.to_string(),
                            math_ml,
                        });
                    }
                }
            }
            Event::Rule => {
                blocks.push(RichBlockView::HorizontalRule);
            }
            _ => {}
        }
    }

    if in_table {
        if in_table_cell {
            cur_table_row.push(TableCellView {
                spans: std::mem::take(&mut cur_spans),
            });
        }
        if !cur_table_row.is_empty() {
            if in_table_head && table_headers.is_empty() {
                table_headers = std::mem::take(&mut cur_table_row);
            } else {
                table_rows.push(TableRowView {
                    cells: std::mem::take(&mut cur_table_row),
                });
            }
        }
        if !table_headers.is_empty() || !table_rows.is_empty() {
            blocks.push(RichBlockView::Table {
                alignments: table_alignments,
                headers: table_headers,
                rows: table_rows,
            });
        }
    } else if !cur_spans.is_empty() {
        blocks.push(RichBlockView::Paragraph { spans: cur_spans });
    }

    blocks
}

fn heading_level_u8(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

// --- Structured Git Unified Diff & Inline Diff ---

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiffLineKind {
    Meta,
    HunkHeader,
    Context,
    Added,
    Removed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct InlineDiffSpan {
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffLineView {
    pub kind: DiffLineKind,
    pub old_line: Option<u32>,
    pub new_line: Option<u32>,
    pub content: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tokens: Vec<HighlightedTokenView>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub inline_changes: Vec<InlineDiffSpan>,
}

impl DiffLineView {
    fn plain(
        kind: DiffLineKind,
        old_line: Option<u32>,
        new_line: Option<u32>,
        content: String,
    ) -> Self {
        Self {
            kind,
            old_line,
            new_line,
            content,
            tokens: Vec::new(),
            inline_changes: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffFileView {
    pub path: String,
    pub additions: u32,
    pub deletions: u32,
    pub lines: Vec<DiffLineView>,
}

const MAX_RENDERED_DIFF_LINES_PER_FILE: usize = 400;

fn is_generated_or_lockfile_path(path: &str) -> bool {
    let norm = path.replace('\\', "/");
    let base = norm.rsplit('/').next().unwrap_or(norm.as_str());
    matches!(
        base,
        "Cargo.lock"
            | "package-lock.json"
            | "pnpm-lock.yaml"
            | "yarn.lock"
            | "Podfile.lock"
            | "Gemfile.lock"
            | "composer.lock"
            | "poetry.lock"
            | "uv.lock"
            | "bun.lockb"
            | "bun.lock"
            | "go.sum"
    ) || base.ends_with(".lock")
        || base.ends_with(".pbxproj")
        || base.ends_with(".min.js")
        || base.ends_with(".min.css")
        || base.starts_with("orbit_ffi_uniffi.")
        || norm.contains("/Generated/")
        || norm.starts_with("Generated/")
}

fn flush_omitted_diff_notice(files: &mut [DiffFileView], omitted_lines: &mut usize) {
    if *omitted_lines > 0
        && let Some(last) = files.last_mut()
    {
        last.lines.push(DiffLineView::plain(
            DiffLineKind::Meta,
            None,
            None,
            format!("… {} more diff lines omitted", *omitted_lines),
        ));
    }
    *omitted_lines = 0;
}

fn push_capped_diff_line(
    file: &mut DiffFileView,
    omitted_lines: &mut usize,
    line: DiffLineView,
    max_lines: usize,
) {
    if file.lines.len() < max_lines {
        file.lines.push(line);
    } else {
        *omitted_lines += 1;
    }
}

/// Fast single-pass count of added and removed lines in a unified diff without syntax highlighting.
pub fn count_unified_diff_stats(unified_diff: &str) -> (u32, u32) {
    let mut additions = 0u32;
    let mut deletions = 0u32;
    for raw_line in unified_diff.lines() {
        if raw_line.starts_with("+++ ") || raw_line.starts_with("--- ") {
            continue;
        }
        if raw_line.starts_with('+') {
            additions = additions.saturating_add(1);
        } else if raw_line.starts_with('-') {
            deletions = deletions.saturating_add(1);
        }
    }
    (additions, deletions)
}

/// Parse a `git diff` unified output string into per-file structured diff lines with dual line numbers.
/// Counts exact `additions` / `deletions` across arbitrarily large files while capping rendered lines per file
/// and sorting handwritten source files ahead of lockfiles & generated bindings.
pub fn parse_unified_diff(unified_diff: &str) -> Vec<DiffFileView> {
    parse_unified_diff_capped(unified_diff, MAX_RENDERED_DIFF_LINES_PER_FILE, true)
}

/// Parse a unified diff with a caller-specified per-file rendered line cap and optional syntax
/// highlighting + word-level intra-line diffing.
/// Unescape a Git C-quoted path token starting with `"` and return `(decoded_path, tail)`.
fn parse_git_c_quoted_token(input: &str) -> Option<(String, &str)> {
    let inner = input.strip_prefix('"')?;
    let bytes = inner.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0usize;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'"' {
            let decoded = String::from_utf8_lossy(&out).into_owned();
            return Some((decoded, &inner[i + 1..]));
        }
        if b == b'\\' && i + 1 < bytes.len() {
            i += 1;
            match bytes[i] {
                b'\\' => out.push(b'\\'),
                b'"' => out.push(b'"'),
                b't' => out.push(b'\t'),
                b'n' => out.push(b'\n'),
                b'r' => out.push(b'\r'),
                b'a' => out.push(0x07),
                b'b' => out.push(0x08),
                b'f' => out.push(0x0c),
                b'v' => out.push(0x0b),
                d0 @ b'0'..=b'7'
                    if i + 2 < bytes.len()
                        && (b'0'..=b'7').contains(&bytes[i + 1])
                        && (b'0'..=b'7').contains(&bytes[i + 2]) =>
                {
                    let val =
                        ((d0 - b'0') << 6) | ((bytes[i + 1] - b'0') << 3) | (bytes[i + 2] - b'0');
                    out.push(val);
                    i += 2;
                }
                other => out.push(other),
            }
        } else {
            out.push(b);
        }
        i += 1;
    }
    None
}

/// Parse the path from a unified diff `--- ` or `+++ ` header line, preserving leading/trailing
/// literal spaces (e.g. `report.txt `), stripping Git's optional `\t<timestamp>` separator on
/// unquoted paths, and decoding Git C-quoted escapes (`"..."`).
fn parse_unified_header_path(after_marker: &str, strip_prefix: &str) -> Option<String> {
    let raw = after_marker.trim_end_matches(['\r', '\n']);
    let decoded = if raw.starts_with('"') {
        let (path, _tail) = parse_git_c_quoted_token(raw)?;
        path
    } else {
        raw.split('\t').next().unwrap_or(raw).to_string()
    };
    if decoded.is_empty() || decoded == "/dev/null" {
        return None;
    }
    let stripped = decoded.strip_prefix(strip_prefix).unwrap_or(&decoded);
    (!stripped.is_empty()).then(|| stripped.to_string())
}

/// Parse the literal path from a Git `rename from`, `rename to`, `copy from`, or `copy to`
/// header line. Git does not prepend `a/` or `b/` and does not append a timestamp tab; if the
/// path contains special bytes Git wraps it in C-quotes (`"..."`).
fn parse_git_rename_copy_path(after_marker: &str) -> Option<String> {
    let raw = after_marker.trim_end_matches(['\r', '\n']);
    if raw.is_empty() {
        return None;
    }
    if raw.starts_with('"')
        && let Some((decoded, tail)) = parse_git_c_quoted_token(raw)
        && tail.is_empty()
    {
        return (!decoded.is_empty()).then_some(decoded);
    }
    Some(raw.to_string())
}

/// Parse the target path from a `diff --git <a_path> <b_path>` header without stripping spaces
/// from filenames (e.g. `diff --git a/report.txt  b/report.txt `).
fn parse_git_diff_header_path(rest: &str) -> String {
    let line = rest.trim_end_matches(['\r', '\n']);
    if line.starts_with('"')
        && let Some((a_tok, after_a)) = parse_git_c_quoted_token(line)
    {
        let b_part = after_a.strip_prefix(' ').unwrap_or(after_a);
        let b_tok = if b_part.starts_with('"') {
            parse_git_c_quoted_token(b_part).map(|(p, _)| p)
        } else {
            (!b_part.is_empty()).then(|| b_part.to_string())
        };
        if let Some(b_raw) = b_tok {
            let clean = b_raw.strip_prefix("b/").unwrap_or(&b_raw);
            if !clean.is_empty() {
                return clean.to_string();
            }
        }
        let clean_a = a_tok.strip_prefix("a/").unwrap_or(&a_tok);
        if !clean_a.is_empty() {
            return clean_a.to_string();
        }
    }
    if let Some(after_a) = line.strip_prefix("a/")
        && after_a.len() >= 3
        && (after_a.len() - 3).is_multiple_of(2)
    {
        let x_len = (after_a.len() - 3) / 2;
        if x_len > 0
            && after_a.is_char_boundary(x_len)
            && after_a.is_char_boundary(x_len + 3)
            && &after_a[x_len..x_len + 3] == " b/"
            && after_a[..x_len] == after_a[x_len + 3..]
        {
            return after_a[..x_len].to_string();
        }
    }
    if line.ends_with('"') {
        for (idx, _) in line.match_indices(" \"") {
            if let Some((b_raw, tail)) = parse_git_c_quoted_token(&line[idx + 1..])
                && tail.is_empty()
            {
                let clean = b_raw.strip_prefix("b/").unwrap_or(&b_raw);
                if !clean.is_empty() {
                    return clean.to_string();
                }
            }
        }
    }
    if let Some((_, after_b)) = line.rsplit_once(" b/")
        && !after_b.is_empty()
    {
        return after_b.to_string();
    }
    line.split_whitespace()
        .last()
        .map(|p| p.strip_prefix("b/").unwrap_or(p).to_string())
        .filter(|p| !p.is_empty())
        .unwrap_or_else(|| "modified".to_string())
}

pub fn parse_unified_diff_capped(
    unified_diff: &str,
    max_lines_per_file: usize,
    highlight: bool,
) -> Vec<DiffFileView> {
    let max_lines = max_lines_per_file.max(1);
    let mut files: Vec<DiffFileView> = Vec::new();
    let mut old_ln: u32 = 0;
    let mut new_ln: u32 = 0;
    let mut in_hunk = false;
    let mut omitted_lines: usize = 0;

    for raw_line in unified_diff.lines() {
        if let Some(rest) = raw_line.strip_prefix("diff --git ") {
            flush_omitted_diff_notice(&mut files, &mut omitted_lines);
            let path = parse_git_diff_header_path(rest);
            files.push(DiffFileView {
                path,
                additions: 0,
                deletions: 0,
                lines: Vec::new(),
            });
            old_ln = 0;
            new_ln = 0;
            in_hunk = false;
            continue;
        }

        if files.is_empty() && !raw_line.trim().is_empty() {
            files.push(DiffFileView {
                path: "Working Tree".to_string(),
                additions: 0,
                deletions: 0,
                lines: Vec::new(),
            });
        }

        let Some(cur_file) = files.last_mut() else {
            continue;
        };

        if !in_hunk {
            if let Some(after_plus) = raw_line.strip_prefix("+++ ") {
                if let Some(parsed) = parse_unified_header_path(after_plus, "b/") {
                    cur_file.path = parsed;
                } else {
                    push_capped_diff_line(
                        cur_file,
                        &mut omitted_lines,
                        DiffLineView::plain(DiffLineKind::Meta, None, None, raw_line.to_string()),
                        max_lines,
                    );
                }
                continue;
            }
            if let Some(after_minus) = raw_line.strip_prefix("--- ") {
                if (cur_file.path == "Working Tree" || cur_file.path == "modified")
                    && let Some(parsed) = parse_unified_header_path(after_minus, "a/")
                {
                    cur_file.path = parsed;
                }
                push_capped_diff_line(
                    cur_file,
                    &mut omitted_lines,
                    DiffLineView::plain(DiffLineKind::Meta, None, None, raw_line.to_string()),
                    max_lines,
                );
                continue;
            }
            if let Some(after_to) = raw_line
                .strip_prefix("rename to ")
                .or_else(|| raw_line.strip_prefix("copy to "))
            {
                if let Some(parsed) = parse_git_rename_copy_path(after_to) {
                    cur_file.path = parsed;
                }
                push_capped_diff_line(
                    cur_file,
                    &mut omitted_lines,
                    DiffLineView::plain(DiffLineKind::Meta, None, None, raw_line.to_string()),
                    max_lines,
                );
                continue;
            }
            if let Some(after_from) = raw_line
                .strip_prefix("rename from ")
                .or_else(|| raw_line.strip_prefix("copy from "))
            {
                if (cur_file.path == "Working Tree" || cur_file.path == "modified")
                    && let Some(parsed) = parse_git_rename_copy_path(after_from)
                {
                    cur_file.path = parsed;
                }
                push_capped_diff_line(
                    cur_file,
                    &mut omitted_lines,
                    DiffLineView::plain(DiffLineKind::Meta, None, None, raw_line.to_string()),
                    max_lines,
                );
                continue;
            }
            if raw_line.starts_with("index ")
                || raw_line.starts_with("new file mode")
                || raw_line.starts_with("deleted file mode")
                || raw_line.starts_with("old mode ")
                || raw_line.starts_with("new mode ")
                || raw_line.starts_with("similarity index ")
                || raw_line.starts_with("dissimilarity index ")
            {
                push_capped_diff_line(
                    cur_file,
                    &mut omitted_lines,
                    DiffLineView::plain(DiffLineKind::Meta, None, None, raw_line.to_string()),
                    max_lines,
                );
                continue;
            }
        }

        if raw_line.starts_with("@@ ") {
            let (o, n) = parse_hunk_header_numbers(raw_line);
            old_ln = o;
            new_ln = n;
            in_hunk = true;
            push_capped_diff_line(
                cur_file,
                &mut omitted_lines,
                DiffLineView::plain(DiffLineKind::HunkHeader, None, None, raw_line.to_string()),
                max_lines,
            );
            continue;
        }

        if let Some(added) = raw_line.strip_prefix('+') {
            cur_file.additions += 1;
            let ln = new_ln;
            new_ln = new_ln.saturating_add(1);
            push_capped_diff_line(
                cur_file,
                &mut omitted_lines,
                DiffLineView::plain(DiffLineKind::Added, None, Some(ln), added.to_string()),
                max_lines,
            );
        } else if let Some(removed) = raw_line.strip_prefix('-') {
            cur_file.deletions += 1;
            let ln = old_ln;
            old_ln = old_ln.saturating_add(1);
            push_capped_diff_line(
                cur_file,
                &mut omitted_lines,
                DiffLineView::plain(DiffLineKind::Removed, Some(ln), None, removed.to_string()),
                max_lines,
            );
        } else {
            let content = raw_line.strip_prefix(' ').unwrap_or(raw_line).to_string();
            let o = if old_ln > 0 { Some(old_ln) } else { None };
            let n = if new_ln > 0 { Some(new_ln) } else { None };
            if old_ln > 0 {
                old_ln = old_ln.saturating_add(1);
            }
            if new_ln > 0 {
                new_ln = new_ln.saturating_add(1);
            }
            push_capped_diff_line(
                cur_file,
                &mut omitted_lines,
                DiffLineView::plain(DiffLineKind::Context, o, n, content),
                max_lines,
            );
        }
    }

    flush_omitted_diff_notice(&mut files, &mut omitted_lines);
    files.sort_by_key(|f| is_generated_or_lockfile_path(&f.path));
    if highlight {
        // Budget eager syntax highlighting across huge multi-file diffs so IPC payloads stay small
        // and parsing stays < 15ms; single-file diffs always highlight in full.
        let mut remaining_highlight_budget: usize = if files.len() <= 1 {
            max_lines
        } else {
            2400.min(max_lines.saturating_mul(files.len()))
        };
        for file in &mut files {
            enrich_diff_file_highlights_and_intraline(file, &mut remaining_highlight_budget);
        }
    }
    files
}

/// Compute a line-by-line diff between two snapshots in pure Rust using `similar`.
pub fn diff_two_texts(path: &str, old_text: &str, new_text: &str) -> DiffFileView {
    let diff = TextDiff::from_lines(old_text, new_text);
    let mut file = DiffFileView {
        path: path.to_string(),
        additions: 0,
        deletions: 0,
        lines: Vec::new(),
    };
    let mut omitted_lines = 0usize;

    for change in diff.iter_all_changes() {
        let content = change
            .value()
            .trim_end_matches(&['\r', '\n'][..])
            .to_string();
        match change.tag() {
            ChangeTag::Equal => push_capped_diff_line(
                &mut file,
                &mut omitted_lines,
                DiffLineView::plain(
                    DiffLineKind::Context,
                    change.old_index().map(|i| (i + 1) as u32),
                    change.new_index().map(|i| (i + 1) as u32),
                    content,
                ),
                MAX_RENDERED_DIFF_LINES_PER_FILE,
            ),
            ChangeTag::Delete => {
                file.deletions += 1;
                push_capped_diff_line(
                    &mut file,
                    &mut omitted_lines,
                    DiffLineView::plain(
                        DiffLineKind::Removed,
                        change.old_index().map(|i| (i + 1) as u32),
                        None,
                        content,
                    ),
                    MAX_RENDERED_DIFF_LINES_PER_FILE,
                );
            }
            ChangeTag::Insert => {
                file.additions += 1;
                push_capped_diff_line(
                    &mut file,
                    &mut omitted_lines,
                    DiffLineView::plain(
                        DiffLineKind::Added,
                        None,
                        change.new_index().map(|i| (i + 1) as u32),
                        content,
                    ),
                    MAX_RENDERED_DIFF_LINES_PER_FILE,
                );
            }
        }
    }

    if omitted_lines > 0 {
        file.lines.push(DiffLineView::plain(
            DiffLineKind::Meta,
            None,
            None,
            format!("… {omitted_lines} more diff lines omitted"),
        ));
    }

    let mut budget = MAX_RENDERED_DIFF_LINES_PER_FILE;
    enrich_diff_file_highlights_and_intraline(&mut file, &mut budget);
    file
}

fn enrich_diff_file_highlights_and_intraline(
    file: &mut DiffFileView,
    remaining_highlight_budget: &mut usize,
) {
    if file.lines.is_empty() || is_generated_or_lockfile_path(&file.path) {
        return;
    }

    // 1. Syntax-highlight code lines via cached syntect pipeline within multi-file budget
    let lang = detect_language_for_path(&file.path);
    if !lang.is_empty() && lang != "text" && *remaining_highlight_budget > 0 {
        let mut code_indices: Vec<usize> = Vec::with_capacity(file.lines.len());
        let mut joined = String::new();
        for (idx, line) in file.lines.iter().enumerate() {
            if code_indices.len() >= *remaining_highlight_budget {
                break;
            }
            if matches!(
                line.kind,
                DiffLineKind::Added | DiffLineKind::Removed | DiffLineKind::Context
            ) && line.content.len() <= 600
            {
                code_indices.push(idx);
                joined.push_str(&line.content);
                joined.push('\n');
            }
        }
        if !code_indices.is_empty() {
            let cap = code_indices.len().min(MAX_RENDERED_DIFF_LINES_PER_FILE);
            let highlighted = highlight_code_lines_capped(lang, &joined, cap);
            for (line_pos, &line_idx) in code_indices.iter().enumerate() {
                if let Some(hl) = highlighted.get(line_pos) {
                    file.lines[line_idx].tokens = hl.tokens.clone();
                }
            }
            *remaining_highlight_budget =
                remaining_highlight_budget.saturating_sub(code_indices.len());
        }
    }

    // 2. Word-level intra-line diff for paired Removed/Added batches
    let mut i = 0usize;
    let total = file.lines.len();
    while i < total {
        if file.lines[i].kind != DiffLineKind::Removed {
            i += 1;
            continue;
        }
        let rem_start = i;
        while i < total && file.lines[i].kind == DiffLineKind::Removed {
            i += 1;
        }
        let rem_end = i;
        let add_start = i;
        while i < total && file.lines[i].kind == DiffLineKind::Added {
            i += 1;
        }
        let add_end = i;

        let pair_count = (rem_end - rem_start).min(add_end - add_start).min(32);
        for k in 0..pair_count {
            let old_s = file.lines[rem_start + k].content.as_str();
            let new_s = file.lines[add_start + k].content.as_str();
            if old_s.len() > 2048 || new_s.len() > 2048 || old_s == new_s {
                continue;
            }
            let (old_spans, new_spans) = compute_intraline_word_spans(old_s, new_s);
            file.lines[rem_start + k].inline_changes = old_spans;
            file.lines[add_start + k].inline_changes = new_spans;
        }
    }
}

fn compute_intraline_word_spans(
    old_line: &str,
    new_line: &str,
) -> (Vec<InlineDiffSpan>, Vec<InlineDiffSpan>) {
    let diff = TextDiff::from_words(old_line, new_line);
    let mut old_spans: Vec<InlineDiffSpan> = Vec::new();
    let mut new_spans: Vec<InlineDiffSpan> = Vec::new();
    let mut old_pos = 0usize;
    let mut new_pos = 0usize;

    for change in diff.iter_all_changes() {
        let len_utf16 = change.value().encode_utf16().count();
        match change.tag() {
            ChangeTag::Equal => {
                old_pos += len_utf16;
                new_pos += len_utf16;
            }
            ChangeTag::Delete => {
                if len_utf16 > 0 {
                    if let Some(last) = old_spans.last_mut()
                        && last.end == old_pos
                    {
                        last.end = old_pos + len_utf16;
                    } else {
                        old_spans.push(InlineDiffSpan {
                            start: old_pos,
                            end: old_pos + len_utf16,
                        });
                    }
                    old_pos += len_utf16;
                }
            }
            ChangeTag::Insert => {
                if len_utf16 > 0 {
                    if let Some(last) = new_spans.last_mut()
                        && last.end == new_pos
                    {
                        last.end = new_pos + len_utf16;
                    } else {
                        new_spans.push(InlineDiffSpan {
                            start: new_pos,
                            end: new_pos + len_utf16,
                        });
                    }
                    new_pos += len_utf16;
                }
            }
        }
    }

    (old_spans, new_spans)
}

fn parse_hunk_header_numbers(header: &str) -> (u32, u32) {
    let mut old_start = 1u32;
    let mut new_start = 1u32;
    for token in header.split_whitespace() {
        if let Some(old_spec) = token.strip_prefix('-') {
            if let Some(num_str) = old_spec.split(',').next()
                && let Ok(n) = num_str.parse::<u32>()
            {
                old_start = n;
            }
        } else if let Some(new_spec) = token.strip_prefix('+')
            && let Some(num_str) = new_spec.split(',').next()
            && let Ok(n) = num_str.parse::<u32>()
        {
            new_start = n;
        }
    }
    (old_start, new_start)
}

// --- Pure-Rust VT100 Terminal Grid State for Native 120Hz Canvases ---

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalSpanView {
    pub text: String,
    pub fg_hex: String,
    pub bg_hex: Option<String>,
    pub bold: bool,
    #[serde(default)]
    pub underline: bool,
    #[serde(default)]
    pub inverse: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalRowView {
    pub row_index: u16,
    pub spans: Vec<TerminalSpanView>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalScreenView {
    pub cols: u16,
    pub rows: u16,
    pub cursor_row: u16,
    pub cursor_col: u16,
    pub rows_data: Vec<TerminalRowView>,
}

/// Incremental dirty-row delta for terminal screens so clients only patch modified rows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalScreenDeltaView {
    pub revision: u64,
    pub slot: u8,
    pub reset: bool,
    pub cols: u16,
    pub rows: u16,
    pub cursor_row: u16,
    pub cursor_col: u16,
    pub changed_rows: Vec<TerminalRowView>,
}

fn hash_terminal_row(row: &TerminalRowView) -> u64 {
    let mut buf = Vec::with_capacity(64);
    buf.extend_from_slice(&row.row_index.to_le_bytes());
    for span in &row.spans {
        buf.extend_from_slice(&(span.text.len() as u32).to_le_bytes());
        buf.extend_from_slice(span.text.as_bytes());
        buf.extend_from_slice(&(span.fg_hex.len() as u32).to_le_bytes());
        buf.extend_from_slice(span.fg_hex.as_bytes());
        match &span.bg_hex {
            Some(bg) => {
                buf.push(1);
                buf.extend_from_slice(&(bg.len() as u32).to_le_bytes());
                buf.extend_from_slice(bg.as_bytes());
            }
            None => buf.push(0),
        }
        let flags =
            u8::from(span.bold) | (u8::from(span.underline) << 1) | (u8::from(span.inverse) << 2);
        buf.push(flags);
    }
    fnv1a_64(&buf)
}

#[derive(Debug, Clone, Copy, Default)]
enum AnsiClearState {
    #[default]
    Ground,
    Esc,
    EscIntermediate,
    Csi {
        params: [u8; 32],
        len: usize,
        overflow: bool,
        has_intermediate: bool,
    },
}

/// Stateful streaming scanner that detects full-screen erase / terminal reset sequences
/// (`CSI 2 J`, `CSI 3 J`, `ESC c`) even when split across arbitrary PTY byte chunks.
#[derive(Debug, Clone, Default)]
struct AnsiClearScanner {
    state: AnsiClearState,
}

impl AnsiClearScanner {
    fn reset(&mut self) {
        self.state = AnsiClearState::Ground;
    }

    fn csi_params_include_full_erase(params: &[u8]) -> bool {
        // ECMA-48 / VT100 `CSI Ps J` (ED) only evaluates the FIRST parameter (defaulting to 0 when
        // omitted). Multi-parameter forms like `CSI 0;2 J` execute ED(0) (erase below cursor),
        // NOT full-screen erase (`2` or `3`).
        let Some(first) = params.split(|&b| b == b';').next() else {
            return false;
        };
        if first.is_empty() || !first.iter().all(|b| b.is_ascii_digit()) {
            return false;
        }
        let mut val: u32 = 0;
        for &d in first {
            val = val.saturating_mul(10).saturating_add(u32::from(d - b'0'));
        }
        val == 2 || val == 3
    }

    fn feed(&mut self, bytes: &[u8]) -> bool {
        let mut saw_clear = false;
        for &b in bytes {
            match self.state {
                AnsiClearState::Ground => {
                    if b == 0x1B {
                        self.state = AnsiClearState::Esc;
                    }
                }
                AnsiClearState::Esc => match b {
                    b'[' => {
                        self.state = AnsiClearState::Csi {
                            params: [0u8; 32],
                            len: 0,
                            overflow: false,
                            has_intermediate: false,
                        };
                    }
                    b'c' => {
                        saw_clear = true;
                        self.state = AnsiClearState::Ground;
                    }
                    0x1B => {
                        self.state = AnsiClearState::Esc;
                    }
                    0x20..=0x2F => {
                        self.state = AnsiClearState::EscIntermediate;
                    }
                    0x00..=0x17 | 0x19 | 0x1C..=0x1F => {}
                    _ => {
                        self.state = AnsiClearState::Ground;
                    }
                },
                AnsiClearState::EscIntermediate => match b {
                    0x1B => {
                        self.state = AnsiClearState::Esc;
                    }
                    0x20..=0x2F | 0x00..=0x17 | 0x19 | 0x1C..=0x1F => {}
                    _ => {
                        self.state = AnsiClearState::Ground;
                    }
                },
                AnsiClearState::Csi {
                    mut params,
                    mut len,
                    mut overflow,
                    mut has_intermediate,
                } => match b {
                    0x1B => {
                        self.state = AnsiClearState::Esc;
                    }
                    0x18 | 0x1A => {
                        self.state = AnsiClearState::Ground;
                    }
                    0x00..=0x17 | 0x19 | 0x1C..=0x1F => {}
                    0x30..=0x3F => {
                        if !has_intermediate {
                            if len < params.len() {
                                params[len] = b;
                                len += 1;
                            } else {
                                overflow = true;
                            }
                        }
                        self.state = AnsiClearState::Csi {
                            params,
                            len,
                            overflow,
                            has_intermediate,
                        };
                    }
                    0x20..=0x2F => {
                        has_intermediate = true;
                        self.state = AnsiClearState::Csi {
                            params,
                            len,
                            overflow,
                            has_intermediate,
                        };
                    }
                    0x40..=0x7E => {
                        if b == b'J'
                            && !has_intermediate
                            && !overflow
                            && Self::csi_params_include_full_erase(&params[..len])
                        {
                            saw_clear = true;
                        }
                        self.state = AnsiClearState::Ground;
                    }
                    _ => {
                        self.state = AnsiClearState::Ground;
                    }
                },
            }
        }
        saw_clear
    }
}

pub struct TerminalGridEmulator {
    parser: vt100::Parser,
    clear_scanner: AnsiClearScanner,
    scrollback_lines: Vec<String>,
    revision: u64,
    reset_revision: u64,
    last_cols: u16,
    last_rows: u16,
    last_cursor_row: u16,
    last_cursor_col: u16,
    row_hashes: Vec<u64>,
    row_revisions: Vec<u64>,
}

impl TerminalGridEmulator {
    pub fn new(cols: u16, rows: u16) -> Self {
        let r = std::num::NonZeroU16::new(rows.max(4)).unwrap_or(std::num::NonZeroU16::MIN);
        let c = std::num::NonZeroU16::new(cols.max(20)).unwrap_or(std::num::NonZeroU16::MIN);
        let mut emu = Self {
            parser: vt100::Parser::new(r, c, 2000),
            clear_scanner: AnsiClearScanner::default(),
            scrollback_lines: Vec::new(),
            revision: 1,
            reset_revision: 1,
            last_cols: c.get(),
            last_rows: r.get(),
            last_cursor_row: 0,
            last_cursor_col: 0,
            row_hashes: Vec::new(),
            row_revisions: Vec::new(),
        };
        let snap = emu.snapshot();
        emu.row_hashes = snap.rows_data.iter().map(hash_terminal_row).collect();
        emu.row_revisions = vec![1; snap.rows_data.len()];
        emu
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    fn sync_delta_state(&mut self, force_reset: bool) {
        let snap = self.snapshot();
        let dims_changed = snap.cols != self.last_cols
            || snap.rows != self.last_rows
            || self.row_hashes.len() != snap.rows_data.len();

        if force_reset || dims_changed {
            self.revision = self.revision.saturating_add(1);
            self.reset_revision = self.revision;
            self.last_cols = snap.cols;
            self.last_rows = snap.rows;
            self.last_cursor_row = snap.cursor_row;
            self.last_cursor_col = snap.cursor_col;
            self.row_hashes = snap.rows_data.iter().map(hash_terminal_row).collect();
            self.row_revisions = vec![self.revision; snap.rows_data.len()];
            return;
        }

        let mut dirty_rows: Vec<(usize, u64)> = Vec::new();
        for (idx, row) in snap.rows_data.iter().enumerate() {
            let new_hash = hash_terminal_row(row);
            if self.row_hashes.get(idx).copied() != Some(new_hash) {
                dirty_rows.push((idx, new_hash));
            }
        }
        let cursor_changed =
            snap.cursor_row != self.last_cursor_row || snap.cursor_col != self.last_cursor_col;
        if !dirty_rows.is_empty() || cursor_changed {
            self.revision = self.revision.saturating_add(1);
            self.last_cursor_row = snap.cursor_row;
            self.last_cursor_col = snap.cursor_col;
            for (idx, new_hash) in dirty_rows {
                if let Some(slot_hash) = self.row_hashes.get_mut(idx) {
                    *slot_hash = new_hash;
                }
                if let Some(slot_rev) = self.row_revisions.get_mut(idx) {
                    *slot_rev = self.revision;
                }
            }
        }
    }

    pub fn process(&mut self, bytes: &[u8]) {
        let saw_clear = self.clear_scanner.feed(bytes);
        self.parser.process(bytes);
        self.refresh_scrollback_cache();
        self.sync_delta_state(saw_clear);
    }

    pub fn resize(&mut self, cols: u16, rows: u16) {
        let r = std::num::NonZeroU16::new(rows.max(4)).unwrap_or(std::num::NonZeroU16::MIN);
        let c = std::num::NonZeroU16::new(cols.max(20)).unwrap_or(std::num::NonZeroU16::MIN);
        if self.parser.screen().size() != (r, c) {
            self.parser.screen_mut().set_size(r, c);
            self.refresh_scrollback_cache();
            self.sync_delta_state(true);
        }
    }

    pub fn clear(&mut self) {
        let (rows, cols) = self.parser.screen().size();
        self.parser = vt100::Parser::new(rows, cols, 2000);
        self.clear_scanner.reset();
        self.scrollback_lines.clear();
        self.sync_delta_state(true);
    }

    pub fn snapshot_with_delta(&mut self, prev_revision: u64) -> TerminalScreenDeltaView {
        self.snapshot_with_delta_for_slot(1, prev_revision)
    }

    pub fn snapshot_with_delta_for_slot(
        &mut self,
        slot: u8,
        prev_revision: u64,
    ) -> TerminalScreenDeltaView {
        let mut snap = self.snapshot();
        if snap.cols != self.last_cols
            || snap.rows != self.last_rows
            || self.row_hashes.len() != snap.rows_data.len()
        {
            self.sync_delta_state(true);
            snap = self.snapshot();
        }
        let normalized_slot = slot.max(1);
        let reset = prev_revision == 0
            || prev_revision > self.revision
            || prev_revision < self.reset_revision;
        let changed_rows = if reset {
            snap.rows_data
        } else if prev_revision == self.revision {
            Vec::new()
        } else {
            snap.rows_data
                .into_iter()
                .enumerate()
                .filter(|(idx, _)| {
                    self.row_revisions
                        .get(*idx)
                        .copied()
                        .unwrap_or(self.revision)
                        > prev_revision
                })
                .map(|(_, row)| row)
                .collect()
        };
        TerminalScreenDeltaView {
            revision: self.revision,
            slot: normalized_slot,
            reset,
            cols: snap.cols,
            rows: snap.rows,
            cursor_row: snap.cursor_row,
            cursor_col: snap.cursor_col,
            changed_rows,
        }
    }

    pub fn scrollback_lines(&self) -> &[String] {
        &self.scrollback_lines
    }

    fn refresh_scrollback_cache(&mut self) {
        let screen = self.parser.screen_mut();
        let (rows_nz, cols_nz) = screen.size();
        let rows = usize::from(rows_nz.get());
        let cols = cols_nz.get();
        screen.set_scrollback(usize::MAX);
        let depth = screen.scrollback();
        if depth == 0 {
            screen.set_scrollback(0);
            self.scrollback_lines.clear();
            return;
        }
        let mut lines = Vec::with_capacity(depth);
        let mut remaining = depth;
        while remaining > 0 {
            screen.set_scrollback(remaining);
            let take = remaining.min(rows);
            for line in screen.rows(0, cols).take(take) {
                lines.push(line.trim_end_matches(' ').to_string());
            }
            remaining -= take;
        }
        screen.set_scrollback(0);
        self.scrollback_lines = lines;
    }

    /// Render a clean, self-contained ANSI reset + full screen state resync sequence from the
    /// current emulator grid so consumers recovering from queue overflow never observe mid-UTF-8
    /// or mid-escape sequence corruption.
    pub fn render_resync_bytes(&self) -> Vec<u8> {
        let formatted = self.parser.screen().state_formatted();
        let mut out = Vec::with_capacity(12 + formatted.len());
        out.extend_from_slice(b"\x1b[0m\x1b[2J\x1b[H");
        out.extend_from_slice(formatted.as_bytes());
        out
    }

    pub fn snapshot(&self) -> TerminalScreenView {
        let screen = self.parser.screen();
        let (rows_nz, cols_nz) = screen.size();
        let rows = rows_nz.get();
        let cols = cols_nz.get();
        let (cursor_row, cursor_col) = screen.cursor_position();
        let mut rows_data = Vec::with_capacity(usize::from(rows));

        for r in 0..rows {
            let mut spans: Vec<TerminalSpanView> = Vec::new();
            for c in 0..cols {
                let Some(cell) = screen.cell(r, c) else {
                    continue;
                };
                if cell.is_wide_continuation() {
                    continue;
                }
                let ch_str = cell.contents();
                let rendered = if ch_str.is_empty() { " " } else { ch_str };

                let (raw_fg, raw_bg) = if cell.inverse() {
                    (
                        vt_color_to_hex(cell.bgcolor(), false),
                        Some(vt_color_to_hex(cell.fgcolor(), true)),
                    )
                } else {
                    let fg = vt_color_to_hex(cell.fgcolor(), true);
                    let bg = match cell.bgcolor() {
                        vt100::Color::Default => None,
                        other => Some(vt_color_to_hex(other, false)),
                    };
                    (fg, bg)
                };
                let bold = cell.bold();
                let underline = cell.underline();
                let inverse = cell.inverse();

                if let Some(last) = spans.last_mut()
                    && last.fg_hex == raw_fg
                    && last.bg_hex == raw_bg
                    && last.bold == bold
                    && last.underline == underline
                    && last.inverse == inverse
                {
                    last.text.push_str(rendered);
                } else {
                    spans.push(TerminalSpanView {
                        text: rendered.to_string(),
                        fg_hex: raw_fg,
                        bg_hex: raw_bg,
                        bold,
                        underline,
                        inverse,
                    });
                }
            }

            // Trim trailing default-background whitespace for lean native layout
            if let Some(last) = spans.last_mut()
                && last.bg_hex.is_none()
                && !last.underline
                && !last.inverse
            {
                let trimmed_len = last.text.trim_end_matches(' ').len();
                last.text.truncate(trimmed_len);
                if last.text.is_empty() {
                    spans.pop();
                }
            }

            rows_data.push(TerminalRowView {
                row_index: r,
                spans,
            });
        }

        TerminalScreenView {
            cols,
            rows,
            cursor_row,
            cursor_col,
            rows_data,
        }
    }
}

fn vt_color_to_hex(color: vt100::Color, is_fg: bool) -> String {
    match color {
        vt100::Color::Default => {
            if is_fg {
                "#E2E8F0".to_string()
            } else {
                "#090D16".to_string()
            }
        }
        vt100::Color::Rgb(r, g, b) => format!("#{:02X}{:02X}{:02X}", r, g, b),
        vt100::Color::Idx(idx) => ansi_idx_to_hex(idx),
    }
}

fn ansi_idx_to_hex(idx: u8) -> String {
    const ANSI_16: [&str; 16] = [
        "#1E293B", "#F87171", "#4ADE80", "#FACC15", "#60A5FA", "#C084FC", "#22D3EE", "#E2E8F0",
        "#64748B", "#FCA5A5", "#86EFAC", "#FDE047", "#93C5FD", "#D8B4FE", "#67E8F9", "#F8FAFC",
    ];
    if let Some(named) = ANSI_16.get(usize::from(idx)) {
        return (*named).to_string();
    }
    if idx < 232 {
        let cube = idx - 16;
        let r = (cube / 36) % 6;
        let g = (cube / 6) % 6;
        let b = cube % 6;
        let map_c = |v: u8| if v == 0 { 0 } else { 55 + v * 40 };
        return format!("#{:02X}{:02X}{:02X}", map_c(r), map_c(g), map_c(b));
    }
    let gray = 8 + (idx - 232) * 10;
    format!("#{:02X}{:02X}{:02X}", gray, gray, gray)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn streaming_markdown_heals_unclosed_code_fence_and_highlights_rust() {
        let md =
            "### Architecture\n\nHere is **Rust** code:\n```rust\npub fn main() {\n    let x = 42;";
        let blocks = parse_markdown_blocks(md, true);
        assert_eq!(blocks.len(), 3);
        match &blocks[2] {
            RichBlockView::CodeBlock {
                language, lines, ..
            } => {
                assert_eq!(language, "rust");
                assert_eq!(lines.len(), 2);
                assert!(!lines[0].tokens.is_empty());
            }
            other => panic!("expected CodeBlock, got {other:?}"),
        }
    }

    #[test]
    fn unified_diff_parser_tracks_line_numbers() {
        let sample = "\
diff --git a/src/lib.rs b/src/lib.rs
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -10,3 +10,4 @@
 fn hello() {
-    println!(\"old\");
+    println!(\"new\");
+    println!(\"extra\");
 }";
        let files = parse_unified_diff(sample);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, "src/lib.rs");
        assert_eq!(files[0].additions, 2);
        assert_eq!(files[0].deletions, 1);
    }

    #[test]
    fn unified_diff_parser_preserves_trailing_space_tab_delimiter_and_c_quoted_paths() {
        // 1. Synthetic untracked diff for `report.txt ` (trailing space, no `diff --git`, no tab)
        let synth_spaced = "--- /dev/null\n+++ b/report.txt \n@@ -0,0 +1,1 @@\n+spaced-body\n";
        let synth_files = parse_unified_diff(synth_spaced);
        assert_eq!(synth_files.len(), 1);
        assert_eq!(synth_files[0].path, "report.txt ");
        assert_eq!(synth_files[0].additions, 1);

        // 2. Real git diff with coexisting `report.txt` and `report.txt ` (git appends `\t` on `---`/`+++`)
        let multi = concat!(
            "diff --git a/report.txt b/report.txt\n",
            "index 1111111..2222222 100644\n",
            "--- a/report.txt\n",
            "+++ b/report.txt\n",
            "@@ -1 +1 @@\n",
            "-old-plain\n",
            "+new-plain\n",
            "diff --git a/report.txt  b/report.txt \n",
            "index 3333333..4444444 100644\n",
            "--- a/report.txt \t\n",
            "+++ b/report.txt \t\n",
            "@@ -1 +1 @@\n",
            "-old-spaced\n",
            "+new-spaced\n",
            "diff --git a/deleted.txt  b/deleted.txt \n",
            "deleted file mode 100644\n",
            "index 5555555..0000000\n",
            "--- a/deleted.txt \t\n",
            "+++ /dev/null\n",
            "@@ -1 +0,0 @@\n",
            "-gone\n",
            "diff --git \"a/dir/quote \\\"x\\\" \" \"b/dir/quote \\\"x\\\" \"\n",
            "--- \"a/dir/quote \\\"x\\\" \"\t2026-11-27\n",
            "+++ \"b/dir/quote \\\"x\\\" \"\t2026-11-27\n",
            "@@ -0,0 +1 @@\n",
            "+quoted-body\n",
        );
        let files = parse_unified_diff(multi);
        assert_eq!(files.len(), 4);
        assert_eq!(files[0].path, "report.txt");
        assert_eq!(files[1].path, "report.txt ");
        assert_eq!(files[2].path, "deleted.txt ");
        assert_eq!(files[2].additions, 0);
        assert_eq!(files[2].deletions, 1);
        assert_eq!(files[3].path, "dir/quote \"x\" ");
        assert_eq!(files[3].additions, 1);

        // 3. R14-P1-1: Pure 100% rename (`README.md -> part b/tail`) has NO `+++` header;
        //    `rename to` MUST override the ambiguous `diff --git` guess (`tail`), keeping it
        //    strictly distinct from an independent staged file named `tail`. Also verify
        //    C-quoted `rename to` and `copy to` headers.
        let rename_and_tail = concat!(
            "diff --git a/README.md b/part b/tail\n",
            "similarity index 100%\n",
            "rename from README.md\n",
            "rename to part b/tail\n",
            "diff --git a/tail b/tail\n",
            "new file mode 100644\n",
            "index 0000000..1111111\n",
            "--- /dev/null\n",
            "+++ b/tail\n",
            "@@ -0,0 +1 @@\n",
            "+independent-tail\n",
            "diff --git \"a/old.txt\" \"b/part b/\\\"q\\\" \\t.txt \"\n",
            "similarity index 100%\n",
            "rename from old.txt\n",
            "rename to \"part b/\\\"q\\\" \\t.txt \"\n",
            "diff --git a/src.txt b/dst b/copy.txt \n",
            "similarity index 100%\n",
            "copy from src.txt\n",
            "copy to dst b/copy.txt \n",
        );
        let rename_files = parse_unified_diff(rename_and_tail);
        assert_eq!(rename_files.len(), 4);
        assert_eq!(rename_files[0].path, "part b/tail");
        assert_eq!(rename_files[1].path, "tail");
        assert_eq!(rename_files[2].path, "part b/\"q\" \t.txt ");
        assert_eq!(rename_files[3].path, "dst b/copy.txt ");

        // 4. R14-N1: Daemon-synthesized untracked diff with C-quoted tab filename decodes verbatim.
        let synth_tab = "--- /dev/null\n+++ \"b/budget\\t2026.txt\"\n@@ -0,0 +1,1 @@\n+q1\n";
        let tab_files = parse_unified_diff(synth_tab);
        assert_eq!(tab_files.len(), 1);
        assert_eq!(tab_files[0].path, "budget\t2026.txt");
    }

    #[test]
    fn unified_diff_parser_caps_large_files_and_sorts_lockfiles_last() {
        let mut raw = String::from(
            "diff --git a/Cargo.lock b/Cargo.lock\n--- a/Cargo.lock\n+++ b/Cargo.lock\n@@ -1,2 +1,502 @@\n",
        );
        for i in 0..500 {
            raw.push_str(&format!("+pkg = \"{i}\"\n"));
        }
        raw.push_str(
            "diff --git a/src/main.rs b/src/main.rs\n--- a/src/main.rs\n+++ b/src/main.rs\n@@ -1,1 +1,2 @@\n fn main() {}\n+fn helper() {}\n",
        );
        let files = parse_unified_diff(&raw);
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].path, "src/main.rs");
        assert_eq!(files[0].additions, 1);
        assert_eq!(files[1].path, "Cargo.lock");
        assert_eq!(files[1].additions, 500);
        assert_eq!(files[1].lines.len(), MAX_RENDERED_DIFF_LINES_PER_FILE + 1);
        assert!(
            files[1]
                .lines
                .last()
                .map(|l| l.content.contains("omitted"))
                .unwrap_or(false)
        );
    }

    #[test]
    fn vt100_grid_coalesces_ansi_colored_spans() {
        let mut term = TerminalGridEmulator::new(80, 24);
        term.process(b"orbit$ \x1b[32mcargo test\x1b[0m\r\nPASS");
        let snap = term.snapshot();
        assert_eq!(snap.rows_data[0].spans.len(), 2);
        assert_eq!(snap.rows_data[0].spans[0].text, "orbit$ ");
        assert_eq!(snap.rows_data[0].spans[1].text, "cargo test");
        assert_eq!(snap.rows_data[0].spans[1].fg_hex, "#4ADE80");
        assert!(!snap.rows_data[0].spans[1].underline);
        assert!(!snap.rows_data[0].spans[1].inverse);
        assert_eq!(snap.rows_data[1].spans[0].text, "PASS");
    }

    #[test]
    fn vt100_grid_tracks_underline_and_inverse_attributes() {
        let mut term = TerminalGridEmulator::new(80, 24);
        term.process(b"\x1b[4mlink\x1b[0m \x1b[7mSEL\x1b[0m");
        let snap = term.snapshot();
        assert_eq!(snap.rows_data[0].spans.len(), 3);
        assert_eq!(snap.rows_data[0].spans[0].text, "link");
        assert!(snap.rows_data[0].spans[0].underline);
        assert!(!snap.rows_data[0].spans[0].inverse);
        assert_eq!(snap.rows_data[0].spans[2].text, "SEL");
        assert!(snap.rows_data[0].spans[2].inverse);
    }

    #[test]
    fn gfm_table_parses_alignments_headers_rows_and_inline_html_br() {
        let md = "\
| 模块 | 状态 | 说明 |
| :--- | :---: | ---: |
| **Markdown** | `fixed` | 第一行<br>第二行<BR />[文档](https://example.com) |
| <b>附件</b> | <em>OK</em> | <del>旧路径</del> |";
        let blocks = parse_markdown_blocks(md, false);
        assert_eq!(blocks.len(), 1);
        match &blocks[0] {
            RichBlockView::Table {
                alignments,
                headers,
                rows,
            } => {
                assert_eq!(
                    alignments,
                    &vec![
                        TableAlignmentView::Left,
                        TableAlignmentView::Center,
                        TableAlignmentView::Right
                    ]
                );
                assert_eq!(headers.len(), 3);
                assert_eq!(headers[0].spans[0].text, "模块");
                assert_eq!(headers[1].spans[0].text, "状态");
                assert_eq!(headers[2].spans[0].text, "说明");

                assert_eq!(rows.len(), 2);
                assert_eq!(rows[0].cells[0].spans[0].text, "Markdown");
                assert!(rows[0].cells[0].spans[0].bold);
                assert_eq!(rows[0].cells[1].spans[0].text, "fixed");
                assert!(rows[0].cells[1].spans[0].inline_code);
                let cell_text: String = rows[0].cells[2]
                    .spans
                    .iter()
                    .map(|s| s.text.as_str())
                    .collect();
                assert_eq!(cell_text, "第一行\n第二行\n文档");
                assert_eq!(
                    rows[0].cells[2]
                        .spans
                        .last()
                        .and_then(|s| s.link_url.as_deref()),
                    Some("https://example.com")
                );

                assert_eq!(rows[1].cells[0].spans[0].text, "附件");
                assert!(rows[1].cells[0].spans[0].bold);
                assert_eq!(rows[1].cells[1].spans[0].text, "OK");
                assert!(rows[1].cells[1].spans[0].italic);
                assert_eq!(rows[1].cells[2].spans[0].text, "旧路径");
                assert!(rows[1].cells[2].spans[0].strikethrough);
            }
            other => panic!("expected Table, got {other:?}"),
        }
    }

    #[test]
    fn streaming_unclosed_table_and_inline_html_tags() {
        let md = "Line1<br/>Line2\n\n<hr />\n\n| ColA | ColB |\n| --- | --- |\n| Val1 | Val2";
        let blocks = parse_markdown_blocks(md, true);
        assert!(blocks.len() >= 3);
        match &blocks[0] {
            RichBlockView::Paragraph { spans } => {
                let joined: String = spans.iter().map(|s| s.text.as_str()).collect();
                assert_eq!(joined, "Line1\nLine2");
            }
            other => panic!("expected Paragraph, got {other:?}"),
        }
        assert!(matches!(blocks[1], RichBlockView::HorizontalRule));
        match &blocks[2] {
            RichBlockView::Table { headers, rows, .. } => {
                assert_eq!(headers.len(), 2);
                assert_eq!(rows.len(), 1);
                assert_eq!(rows[0].cells[0].spans[0].text, "Val1");
                assert_eq!(rows[0].cells[1].spans[0].text, "Val2");
            }
            other => panic!("expected Table, got {other:?}"),
        }
    }

    #[test]
    fn list_item_with_code_block_and_nested_list_preserves_parent_text() {
        let md = "\
1. Parent step
   ```bash
   echo hi
   ```
2. Second step
   - Nested child";
        let blocks = parse_markdown_blocks(md, false);
        assert_eq!(blocks.len(), 4);
        match &blocks[0] {
            RichBlockView::ListItem {
                ordered,
                index,
                spans,
                ..
            } => {
                assert!(*ordered);
                assert_eq!(*index, 1);
                assert_eq!(spans[0].text, "Parent step");
            }
            other => panic!("expected ListItem 1, got {other:?}"),
        }
        assert!(matches!(blocks[1], RichBlockView::CodeBlock { .. }));
        match &blocks[2] {
            RichBlockView::ListItem {
                ordered,
                index,
                spans,
                ..
            } => {
                assert!(*ordered);
                assert_eq!(*index, 2);
                assert_eq!(spans[0].text, "Second step");
            }
            other => panic!("expected ListItem 2, got {other:?}"),
        }
        match &blocks[3] {
            RichBlockView::ListItem { ordered, spans, .. } => {
                assert!(!*ordered);
                assert_eq!(spans[0].text, "Nested child");
            }
            other => panic!("expected nested ListItem, got {other:?}"),
        }
    }

    #[test]
    fn highlight_code_lines_cache_hits_and_stays_bounded() {
        let snippet = "fn orbit_fast(x: i32) -> i32 { x + 1 }\n";
        let first = highlight_code_lines_capped("rust", snippet, 200);
        let second = highlight_code_lines_capped("rust", snippet, 200);
        assert_eq!(first, second);

        for idx in 0..600 {
            let s = format!("let unique_cache_var_{idx} = {idx};");
            let _ = highlight_code_lines_capped("rust", &s, 50);
        }
        assert!(code_highlight_cache_len() <= CODE_HIGHLIGHT_CACHE_CAP);
    }

    #[test]
    fn highlight_code_lines_prefix_bounds_output_without_plain_tail() {
        let mut big = String::new();
        for idx in 1..=400 {
            big.push_str(&format!("pub fn f_{idx}() -> usize {{ {idx} }}\n"));
        }
        let prefix = highlight_code_lines_prefix("rust", &big, 120);
        assert_eq!(prefix.len(), 120);
        assert_eq!(prefix[0].line_number, 1);
        assert_eq!(prefix[119].line_number, 120);
        assert!(
            prefix[0].tokens.len() > 1,
            "first line should be syntax highlighted"
        );

        let short = highlight_code_lines_prefix("rust", "let a = 1;\nlet b = 2;", 250);
        assert_eq!(short.len(), 2);
        assert!(highlight_code_lines_prefix("rust", "", 250).is_empty());
        assert!(highlight_code_lines_prefix("rust", "let a = 1;", 0).is_empty());
    }

    #[test]
    fn streaming_unclosed_code_block_uses_plain_lines_without_polluting_cache() {
        let streaming_md =
            "Intro\n```rust\nfn unique_streaming_prefix_9911(a: i32) {\n    let x = a + 1;";
        let streaming_blocks = parse_markdown_blocks(streaming_md, true);
        match streaming_blocks.last() {
            Some(RichBlockView::CodeBlock {
                language,
                raw_code,
                lines,
            }) => {
                assert_eq!(language, "rust");
                assert!(raw_code.contains("unique_streaming_prefix_9911"));
                assert_eq!(lines.len(), 2);
                assert_eq!(lines[0].tokens.len(), 1);
                assert_eq!(lines[0].tokens[0].fg_hex, DARK_PLAIN_FG_HEX);
                assert_eq!(lines[0].tokens[0].light_fg_hex, LIGHT_PLAIN_FG_HEX);
                assert!(
                    !code_highlight_cache_contains("rust", raw_code),
                    "streaming unclosed code prefix must not pollute global highlight cache"
                );
            }
            other => panic!("expected CodeBlock, got {other:?}"),
        }

        // Once finalized (closed fence / streaming = false), full syntect highlighting applies
        let final_md = format!("{streaming_md}\n}}\n```");
        let final_blocks = parse_markdown_blocks(&final_md, false);
        match final_blocks.last() {
            Some(RichBlockView::CodeBlock {
                raw_code, lines, ..
            }) => {
                assert!(
                    lines[0].tokens.len() > 1,
                    "finalized rust code block must be syntax-highlighted"
                );
                assert!(
                    code_highlight_cache_contains("rust", raw_code),
                    "finalized code block should be cached for subsequent renders"
                );
            }
            other => panic!("expected CodeBlock, got {other:?}"),
        }
    }

    #[test]
    fn parses_inline_and_display_math_and_fenced_math_blocks() {
        let md = concat!(
            "Euler identity $e^{i\\pi} + 1 = 0$ and \\(\\alpha + \\beta\\).\n\n",
            "$$\\int_0^\\infty e^{-x^2}\\,dx = \\frac{\\sqrt{\\pi}}{2}$$\n\n",
            "\\[\n\\begin{pmatrix} 1 & 0 \\\\ 0 & 1 \\end{pmatrix}\n\\]\n\n",
            "```math\n\\sum_{k=1}^{n} k = \\frac{n(n+1)}{2}\n```\n\n",
            "```latex\n\\mathbb{R}^3\n```\n\n",
            "Price is \\$5 and `\\(not_math\\)`."
        );
        let blocks = parse_markdown_blocks(md, false);
        assert_eq!(blocks.len(), 6, "blocks: {blocks:#?}");

        match &blocks[0] {
            RichBlockView::Paragraph { spans } => {
                let math_spans: Vec<_> = spans.iter().filter(|s| s.math_ml.is_some()).collect();
                assert_eq!(math_spans.len(), 2);
                assert_eq!(math_spans[0].text, "e^{i\\pi} + 1 = 0");
                assert!(
                    math_spans[0]
                        .math_ml
                        .as_deref()
                        .expect("inline mathml")
                        .contains("display=\"inline\"")
                );
                assert_eq!(math_spans[1].text, "\\alpha + \\beta");
                assert!(
                    math_spans[1]
                        .math_ml
                        .as_deref()
                        .expect("latex paren mathml")
                        .contains("α")
                );
            }
            other => panic!("expected Paragraph, got {other:?}"),
        }

        for (idx, block) in blocks.iter().enumerate().take(5).skip(1) {
            match block {
                RichBlockView::MathBlock { tex, math_ml } => {
                    assert!(!tex.is_empty());
                    assert!(
                        math_ml.contains("display=\"block\""),
                        "block {idx}: {math_ml}"
                    );
                }
                other => panic!("expected MathBlock at {idx}, got {other:?}"),
            }
        }

        match &blocks[5] {
            RichBlockView::Paragraph { spans } => {
                assert!(spans.iter().all(|s| s.math_ml.is_none()));
                assert!(
                    spans
                        .iter()
                        .any(|s| s.inline_code && s.text == "\\(not_math\\)")
                );
            }
            other => panic!("expected trailing Paragraph, got {other:?}"),
        }
    }

    #[test]
    fn streaming_heals_unclosed_display_math_without_italic_flicker() {
        let streaming_dollar = "Derivation:\n\n$$\nx_1 + x_2 = \\sum_{i=1}^n a_i";
        let blocks = parse_markdown_blocks(streaming_dollar, true);
        assert_eq!(blocks.len(), 2, "blocks: {blocks:#?}");
        assert!(matches!(blocks[0], RichBlockView::Paragraph { .. }));
        match &blocks[1] {
            RichBlockView::MathBlock { tex, math_ml } => {
                assert!(tex.contains("x_1 + x_2"));
                assert!(math_ml.contains("<msub><mi>x</mi><mn>1</mn></msub>"));
            }
            other => panic!("expected healed MathBlock, got {other:?}"),
        }

        let streaming_bracket = "Next:\n\n\\[\na_1 + b_2";
        let blocks2 = parse_markdown_blocks(streaming_bracket, true);
        assert_eq!(blocks2.len(), 2, "blocks2: {blocks2:#?}");
        assert!(matches!(blocks2[1], RichBlockView::MathBlock { .. }));
    }

    fn hex_relative_luminance(hex: &str) -> f64 {
        let clean = hex.trim_start_matches('#');
        assert_eq!(clean.len(), 6, "expected #RRGGBB hex, got {hex}");
        let r = u8::from_str_radix(&clean[0..2], 16).expect("red") as f64 / 255.0;
        let g = u8::from_str_radix(&clean[2..4], 16).expect("green") as f64 / 255.0;
        let b = u8::from_str_radix(&clean[4..6], 16).expect("blue") as f64 / 255.0;
        0.2126 * r + 0.7152 * g + 0.0722 * b
    }

    #[test]
    fn native_light_syntect_theme_emits_inspired_github_colors_alongside_dark() {
        assert!(syntax_theme(SyntaxThemeKind::Dark).is_some());
        assert!(syntax_theme(SyntaxThemeKind::Light).is_some());

        let rust_code = "/// Doc comment\npub fn greet(name: &str) -> String {\n    format!(\"hello {name}\")\n}\n";
        let lines = highlight_code_lines("rs", rust_code);
        assert_eq!(lines.len(), 4);

        let all_tokens: Vec<&HighlightedTokenView> =
            lines.iter().flat_map(|l| l.tokens.iter()).collect();
        assert!(
            all_tokens.len() > 6,
            "expected multiple highlighted tokens, got {}",
            all_tokens.len()
        );

        let mut distinct_light_colors = std::collections::HashSet::new();
        let mut dark_lum_sum = 0.0;
        let mut light_lum_sum = 0.0;
        for tok in &all_tokens {
            assert!(tok.fg_hex.starts_with('#') && tok.fg_hex.len() == 7);
            assert!(tok.light_fg_hex.starts_with('#') && tok.light_fg_hex.len() == 7);
            distinct_light_colors.insert(tok.light_fg_hex.as_str());
            dark_lum_sum += hex_relative_luminance(&tok.fg_hex);
            light_lum_sum += hex_relative_luminance(&tok.light_fg_hex);
        }

        assert!(
            distinct_light_colors.len() >= 3,
            "InspiredGitHub light theme should produce distinct syntax colors, got {distinct_light_colors:?}"
        );
        let avg_dark = dark_lum_sum / all_tokens.len() as f64;
        let avg_light = light_lum_sum / all_tokens.len() as f64;
        assert!(
            avg_light < avg_dark,
            "light-theme ink luminance ({avg_light:.3}) must be darker than dark-theme ink ({avg_dark:.3})"
        );
    }

    #[test]
    fn nested_lists_emit_exact_zero_based_depth_across_three_levels_and_mixed_kinds() {
        let md = "\
- Level 0 first
  - Level 1 bullet
    1. Level 2 ordered one
    2. Level 2 ordered two
  - [x] Level 1 checklist done
- Level 0 second";
        let blocks = parse_markdown_blocks(md, false);
        assert_eq!(blocks.len(), 6, "blocks: {blocks:#?}");

        let extracted: Vec<(bool, u32, Option<bool>, u8, String)> = blocks
            .into_iter()
            .map(|b| match b {
                RichBlockView::ListItem {
                    ordered,
                    index,
                    checked,
                    depth,
                    spans,
                } => (
                    ordered,
                    index,
                    checked,
                    depth,
                    spans.into_iter().map(|s| s.text).collect::<String>(),
                ),
                other => panic!("expected ListItem, got {other:?}"),
            })
            .collect();

        assert_eq!(
            extracted,
            vec![
                (false, 0, None, 0, "Level 0 first".to_string()),
                (false, 0, None, 1, "Level 1 bullet".to_string()),
                (true, 1, None, 2, "Level 2 ordered one".to_string()),
                (true, 2, None, 2, "Level 2 ordered two".to_string()),
                (
                    false,
                    0,
                    Some(true),
                    1,
                    "Level 1 checklist done".to_string()
                ),
                (false, 0, None, 0, "Level 0 second".to_string()),
            ]
        );

        // Verify backward-compatible deserialization defaults `depth` to 0 when omitted
        let legacy_json = r#"{"kind":"list_item","ordered":false,"index":0,"checked":null,"spans":[{"text":"old","bold":false,"italic":false,"strikethrough":false,"inline_code":false,"link_url":null}]}"#;
        let parsed: RichBlockView = serde_json::from_str(legacy_json).expect("legacy json");
        assert!(matches!(parsed, RichBlockView::ListItem { depth: 0, .. }));
    }

    #[test]
    fn terminal_emulator_tracks_scrolled_off_scrollback_without_duplication() {
        let mut emu = TerminalGridEmulator::new(80, 24);
        let mut payload = String::new();
        for n in 1..=120 {
            payload.push_str(&format!("{n}\r\n"));
        }
        emu.process(payload.as_bytes());
        let scrollback = emu.scrollback_lines();
        assert!(
            scrollback.len() >= 90,
            "expected >= 90 scrollback lines for 120 lines on 24-row viewport, got {}",
            scrollback.len()
        );
        assert_eq!(scrollback.first().map(String::as_str), Some("1"));
        assert!(scrollback.iter().any(|l| l == "50"));
        assert!(scrollback.iter().any(|l| l == "90"));

        let snap = emu.snapshot();
        let visible_lines: Vec<String> = snap
            .rows_data
            .iter()
            .map(|r| r.spans.iter().map(|s| s.text.as_str()).collect::<String>())
            .collect();
        assert!(visible_lines.iter().any(|l| l.trim() == "120"));

        emu.clear();
        assert!(emu.scrollback_lines().is_empty());
    }

    #[test]
    fn terminal_delta_tracks_dirty_rows_clear_resize_same_rev_and_stale_rev() {
        let mut emu = TerminalGridEmulator::new(80, 24);
        let initial = emu.snapshot_with_delta(0);
        assert!(initial.reset);
        assert_eq!(initial.slot, 1);
        assert_eq!(initial.cols, 80);
        assert_eq!(initial.rows, 24);
        assert_eq!(initial.changed_rows.len(), 24);

        // Same revision with no terminal mutations -> empty delta
        let noop = emu.snapshot_with_delta(initial.revision);
        assert!(!noop.reset);
        assert_eq!(noop.revision, initial.revision);
        assert!(noop.changed_rows.is_empty());

        // Simulate `ls` modifying only 2 rows
        emu.process(b"Cargo.toml  README.md\r\napps  crates\r\n");
        let ls_delta = emu.snapshot_with_delta(initial.revision);
        assert!(!ls_delta.reset);
        assert!(ls_delta.revision > initial.revision);
        assert!(
            (1..=3).contains(&ls_delta.changed_rows.len()),
            "expected 1..=3 dirty rows for ls output, got {}",
            ls_delta.changed_rows.len()
        );
        let changed_text: Vec<String> = ls_delta
            .changed_rows
            .iter()
            .map(|r| r.spans.iter().map(|s| s.text.as_str()).collect::<String>())
            .collect();
        assert!(changed_text.iter().any(|t| t.contains("Cargo.toml")));
        assert!(changed_text.iter().any(|t| t.contains("crates")));

        // Repeating the same revision returns no changed rows
        let stable = emu.snapshot_with_delta(ls_delta.revision);
        assert!(!stable.reset);
        assert!(stable.changed_rows.is_empty());

        // ANSI `clear` triggers reset + full screen rows
        emu.process(b"\x1b[2J\x1b[H");
        let ansi_clear = emu.snapshot_with_delta(ls_delta.revision);
        assert!(ansi_clear.reset);
        assert!(ansi_clear.revision > ls_delta.revision);
        assert_eq!(ansi_clear.changed_rows.len(), 24);
        assert!(ansi_clear.changed_rows.iter().all(|r| r.spans.is_empty()));

        // Explicit clear() also triggers reset
        emu.process(b"after-clear\r\n");
        let after_line = emu.snapshot_with_delta(ansi_clear.revision);
        assert!(!after_line.reset);
        assert_eq!(after_line.changed_rows.len(), 1);
        emu.clear();
        let explicit_clear = emu.snapshot_with_delta(after_line.revision);
        assert!(explicit_clear.reset);
        assert_eq!(explicit_clear.changed_rows.len(), 24);

        // Resize triggers reset=true with new dimensions
        emu.resize(120, 40);
        let resized = emu.snapshot_with_delta(explicit_clear.revision);
        assert!(resized.reset);
        assert_eq!(resized.cols, 120);
        assert_eq!(resized.rows, 40);
        assert_eq!(resized.changed_rows.len(), 40);

        // Future/stale since_revision > current revision triggers reset=true
        let stale = emu.snapshot_with_delta(resized.revision + 50);
        assert!(stale.reset);
        assert_eq!(stale.revision, resized.revision);
        assert_eq!(stale.changed_rows.len(), 40);
    }

    #[test]
    fn highlight_workspace_code_capped_enforces_250_cap_utf8_empty_and_unknown_lang() {
        let big: String = (1..=400)
            .map(|i| format!("let v_{i}: usize = {i}; // 第{i}行 🚀\n"))
            .collect();
        let capped = highlight_workspace_code_capped("rust", &big, 999);
        assert_eq!(capped.language, "rust");
        assert_eq!(capped.total_lines, 400);
        assert!(capped.truncated);
        assert_eq!(capped.lines.len(), MAX_WORKSPACE_HIGHLIGHT_LINES);
        assert_eq!(capped.lines.first().map(|l| l.line_number), Some(1));
        assert_eq!(capped.lines.last().map(|l| l.line_number), Some(250));
        let first_text: String = capped.lines[0]
            .tokens
            .iter()
            .map(|t| t.text.as_str())
            .collect();
        assert!(first_text.contains("第1行 🚀"));

        let small = highlight_workspace_code_capped("rust", "fn main() {}\nlet x = 1;\n", 250);
        assert_eq!(small.total_lines, 2);
        assert!(!small.truncated);
        assert_eq!(small.lines.len(), 2);

        let empty = highlight_workspace_code_capped("rust", "", 250);
        assert_eq!(empty.total_lines, 0);
        assert!(!empty.truncated);
        assert!(empty.lines.is_empty());

        let unknown =
            highlight_workspace_code_capped("unknown_lang_pr5", "你好 世界\nline two\n", 10);
        assert_eq!(unknown.language, "unknown_lang_pr5");
        assert_eq!(unknown.total_lines, 2);
        assert!(!unknown.truncated);
        assert_eq!(unknown.lines.len(), 2);
        let line1: String = unknown.lines[0]
            .tokens
            .iter()
            .map(|t| t.text.as_str())
            .collect();
        assert_eq!(line1, "你好 世界");
    }

    #[test]
    fn terminal_delta_detects_split_chunk_screen_clear_at_every_boundary() {
        let sequences: &[&[u8]] = &[b"\x1b[2J", b"\x1b[3J", b"\x1bc"];
        for seq in sequences {
            for split_at in 1..seq.len() {
                let mut emu = TerminalGridEmulator::new(80, 24);
                let init = emu.snapshot_with_delta(0);
                emu.process(b"line-one\r\nline-two\r\n");
                let after_out = emu.snapshot_with_delta(init.revision);
                assert!(!after_out.reset);

                emu.process(&seq[..split_at]);
                emu.process(&seq[split_at..]);
                let after_clear = emu.snapshot_with_delta(after_out.revision);
                assert!(
                    after_clear.reset,
                    "seq {:?} split at {split_at} must trigger reset=true",
                    String::from_utf8_lossy(seq)
                );
                assert_eq!(
                    after_clear.changed_rows.len(),
                    24,
                    "seq {:?} split at {split_at} must return all 24 rows on reset",
                    String::from_utf8_lossy(seq)
                );
            }
        }

        // Control: CSI 0 J and CSI 0;2 J (first parameter 0 = erase below cursor) must NOT force
        // full-screen reset and must preserve lines above cursor in vt100.
        for partial_seq in [b"\x1b[0J".as_slice(), b"\x1b[0;2J".as_slice()] {
            for split_at in 1..partial_seq.len() {
                let mut emu = TerminalGridEmulator::new(80, 24);
                let init = emu.snapshot_with_delta(0);
                emu.process(b"keep-top\r\nsecond\r\n");
                let after_out = emu.snapshot_with_delta(init.revision);
                emu.process(&partial_seq[..split_at]);
                emu.process(&partial_seq[split_at..]);
                let after_partial = emu.snapshot_with_delta(after_out.revision);
                assert!(
                    !after_partial.reset,
                    "partial erase {:?} split at {split_at} must not force full-screen reset",
                    String::from_utf8_lossy(partial_seq)
                );
                let snap = emu.snapshot();
                let top_line: String = snap.rows_data[0]
                    .spans
                    .iter()
                    .map(|s| s.text.as_str())
                    .collect();
                assert!(
                    top_line.contains("keep-top"),
                    "partial erase {:?} must keep visible text above cursor, got {top_line:?}",
                    String::from_utf8_lossy(partial_seq)
                );
            }
        }
    }

    #[test]
    fn highlight_workspace_code_capped_bounds_minified_single_line_large_file_and_utf8() {
        // 1. 88 KB single-line minified JS must bypass regex explosion, stay UTF-8 safe, and emit bounded JSON.
        let minified_js = "const a=1,b=2;function f(x){return x+a+b;}".repeat(2_100);
        assert!(minified_js.len() > 88_000);
        let minified_view = highlight_workspace_code_capped("js", &minified_js, 250);
        assert_eq!(minified_view.total_lines, 1);
        assert_eq!(minified_view.lines.len(), 1);
        assert!(minified_view.truncated);
        let minified_json = serde_json::to_string(&minified_view).expect("serialize minified");
        assert!(
            minified_json.len() < 16 * 1024,
            "minified JS highlight JSON must be tightly bounded, got {} bytes",
            minified_json.len()
        );

        // 2. Multi-byte UTF-8 single line exceeding per-line byte budget truncates cleanly on char boundary.
        let cjk_long = "鸿蒙🚀".repeat(1_000);
        assert!(cjk_long.len() > MAX_WORKSPACE_HIGHLIGHT_LINE_PREVIEW_BYTES);
        let cjk_view = highlight_workspace_code_capped("rust", &cjk_long, 250);
        assert_eq!(cjk_view.total_lines, 1);
        assert!(cjk_view.truncated);
        let cjk_line_bytes: usize = cjk_view.lines[0].tokens.iter().map(|t| t.text.len()).sum();
        assert!(cjk_line_bytes <= MAX_WORKSPACE_HIGHLIGHT_LINE_PREVIEW_BYTES);
        assert!(cjk_line_bytes > 0);

        // 3. 80,000-line multi-line JS reports exact total_lines and caps highlighted output at 250 lines.
        let giant_js = "export const n = 42;\n".repeat(80_000);
        let giant_view = highlight_workspace_code_capped("js", &giant_js, 250);
        assert_eq!(giant_view.total_lines, 80_000);
        assert!(giant_view.truncated);
        assert_eq!(giant_view.lines.len(), MAX_WORKSPACE_HIGHLIGHT_LINES);

        // 4. R3-P1-3: Pre-clipped 4,095-byte single JS identifier (from emoji surrogate boundary)
        //    marked in `plain_line_numbers = &[1]` bypasses syntect regex backtracking in O(1),
        //    emitting 1 single plain-text span with `truncated == true`.
        let long_ident = "a".repeat(4_095);
        let clipped_view =
            highlight_workspace_code_capped_with_plain_lines("js", &long_ident, 250, &[1]);
        assert!(clipped_view.truncated);
        assert_eq!(clipped_view.lines.len(), 1);
        assert_eq!(clipped_view.lines[0].tokens.len(), 1);
        assert_eq!(clipped_view.lines[0].tokens[0].text.len(), 4_095);
        assert_eq!(clipped_view.lines[0].tokens[0].fg_hex, DARK_PLAIN_FG_HEX);

        // 5. R5-Nit-1: Large 16,250-line file with empty line 250 returns full 250 lines on BOTH
        //    the full async buffer AND the sync pre-clipped buffer ending in `\n\n`!
        let mut full_empty_250 = "const a = 12345;\n".repeat(249);
        full_empty_250.push('\n');
        full_empty_250.push_str(&"const b = 67890;\n".repeat(16_000));
        let async_empty_250 = highlight_workspace_code_capped("js", &full_empty_250, 250);
        assert_eq!(async_empty_250.total_lines, 16_250);
        assert!(async_empty_250.total_lines_exact);
        assert!(async_empty_250.truncated);
        assert_eq!(async_empty_250.lines.len(), 250);
        assert_eq!(async_empty_250.lines[249].line_number, 250);
        assert!(async_empty_250.lines[249].tokens.is_empty());

        let mut sync_clipped_empty_250 = "const a = 12345;\n".repeat(249);
        sync_clipped_empty_250.push('\n');
        let sync_empty_250 = highlight_workspace_code_capped_with_plain_lines(
            "js",
            &sync_clipped_empty_250,
            250,
            &[],
        );
        assert_eq!(sync_empty_250.total_lines, 250);
        assert_eq!(sync_empty_250.lines.len(), 250);
        assert_eq!(sync_empty_250.lines[249].line_number, 250);
        assert!(sync_empty_250.lines[249].tokens.is_empty());

        // 6. R5-P1: Mid-file 65,536-char line 2 clipped to 4,096 B with `plain_line_numbers = &[2]`
        //    emits 250 lines with syntax tokens on lines 1 and 3 and 1 plain span on line 2.
        let mut mid_clipped = String::from("const first = 1;\n");
        mid_clipped.push_str("// ");
        mid_clipped.push_str(&"c".repeat(4_093));
        mid_clipped.push('\n');
        mid_clipped.push_str(&"const val = 42;\n".repeat(248));
        let mid_view =
            highlight_workspace_code_capped_with_plain_lines("js", &mid_clipped, 250, &[2]);
        assert!(mid_view.truncated);
        assert_eq!(mid_view.lines.len(), 250);
        assert!(mid_view.lines[0].tokens.len() >= 3);
        assert_eq!(mid_view.lines[1].tokens.len(), 1);
        assert_eq!(mid_view.lines[1].tokens[0].fg_hex, DARK_PLAIN_FG_HEX);
        assert!(mid_view.lines[2].tokens.len() >= 3);
    }
}
