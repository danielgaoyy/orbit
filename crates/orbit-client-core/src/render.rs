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
use syntect::easy::HighlightLines;
use syntect::highlighting::{FontStyle, Theme, ThemeSet};
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InlineSpanView {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    pub strikethrough: bool,
    pub inline_code: bool,
    pub link_url: Option<String>,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HighlightedTokenView {
    pub text: String,
    pub fg_hex: String,
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
    HorizontalRule,
}

fn syntax_set() -> &'static SyntaxSet {
    static PS: OnceLock<SyntaxSet> = OnceLock::new();
    PS.get_or_init(SyntaxSet::load_defaults_newlines)
}

fn default_dark_theme() -> Option<&'static Theme> {
    static TS: OnceLock<ThemeSet> = OnceLock::new();
    let ts = TS.get_or_init(ThemeSet::load_defaults);
    ts.themes
        .get("base16-ocean.dark")
        .or_else(|| ts.themes.values().next())
}

/// Heal an unclosed fenced code block while assistant tokens are still streaming so the UI never flickers.
fn heal_streaming_fences(markdown: &str) -> String {
    let mut open_fence: Option<&str> = None;
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
        }
    }

    if let Some(unclosed) = open_fence {
        format!("{markdown}\n{unclosed}")
    } else {
        markdown.to_string()
    }
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
        "html" | "htm" | "xml" | "svg" => "html",
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

fn fnv1a_64(bytes: &[u8]) -> u64 {
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

    let Some(theme) = default_dark_theme() else {
        return code
            .lines()
            .enumerate()
            .map(|(idx, line)| HighlightedLineView {
                line_number: (idx + 1) as u32,
                tokens: vec![HighlightedTokenView {
                    text: line.to_string(),
                    fg_hex: "#E2E8F0".to_string(),
                    bold: false,
                    italic: false,
                }],
            })
            .collect();
    };

    let mut highlighter = HighlightLines::new(syntax, theme);
    let mut out = Vec::new();

    for (idx, line) in LinesWithEndings::from(code).enumerate() {
        let tokens = if idx < max_highlighted_lines {
            match highlighter.highlight_line(line, ps) {
                Ok(ranges) => ranges
                    .into_iter()
                    .map(|(style, slice)| {
                        let clean = slice.trim_end_matches(&['\r', '\n'][..]);
                        HighlightedTokenView {
                            text: clean.to_string(),
                            fg_hex: format!(
                                "#{:02X}{:02X}{:02X}",
                                style.foreground.r, style.foreground.g, style.foreground.b
                            ),
                            bold: style.font_style.contains(FontStyle::BOLD),
                            italic: style.font_style.contains(FontStyle::ITALIC),
                        }
                    })
                    .filter(|t| !t.text.is_empty())
                    .collect(),
                Err(_) => vec![HighlightedTokenView {
                    text: line.trim_end_matches(&['\r', '\n'][..]).to_string(),
                    fg_hex: "#E2E8F0".to_string(),
                    bold: false,
                    italic: false,
                }],
            }
        } else {
            vec![HighlightedTokenView {
                text: line.trim_end_matches(&['\r', '\n'][..]).to_string(),
                fg_hex: "#E2E8F0".to_string(),
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
                fg_hex: "#E2E8F0".to_string(),
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
    });
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
        blocks.push(RichBlockView::ListItem {
            ordered,
            index: idx,
            checked: checked.take(),
            spans: std::mem::take(cur_spans),
        });
        *already_flushed = true;
    }
}

/// Parse streaming or completed Markdown into platform-agnostic native render blocks.
pub fn parse_markdown_blocks(markdown: &str, streaming: bool) -> Vec<RichBlockView> {
    let normalized = if streaming {
        heal_streaming_fences(markdown)
    } else {
        markdown.to_string()
    };

    let mut options = Options::empty();
    options
        .insert(Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS);
    let parser = Parser::new_ext(&normalized, options);

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

    for event in parser {
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
                    if list_item_stack.is_empty()
                        && !in_blockquote
                        && !in_table_cell
                        && !cur_spans.is_empty()
                    {
                        blocks.push(RichBlockView::Paragraph {
                            spans: std::mem::take(&mut cur_spans),
                        });
                    }
                }
                TagEnd::CodeBlock => {
                    let language = in_code_block.take().unwrap_or_default();
                    let raw_code = code_buf.trim_end_matches('\n').to_string();
                    let lines = highlight_code_lines(&language, &raw_code);
                    blocks.push(RichBlockView::CodeBlock {
                        language,
                        raw_code,
                        lines,
                    });
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
                                blocks.push(RichBlockView::ListItem {
                                    ordered,
                                    index: idx,
                                    checked,
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

fn push_capped_diff_line(file: &mut DiffFileView, omitted_lines: &mut usize, line: DiffLineView) {
    if file.lines.len() < MAX_RENDERED_DIFF_LINES_PER_FILE {
        file.lines.push(line);
    } else {
        *omitted_lines += 1;
    }
}

/// Parse a `git diff` unified output string into per-file structured diff lines with dual line numbers.
/// Counts exact `additions` / `deletions` across arbitrarily large files while capping rendered lines per file
/// and sorting handwritten source files ahead of lockfiles & generated bindings.
pub fn parse_unified_diff(unified_diff: &str) -> Vec<DiffFileView> {
    let mut files: Vec<DiffFileView> = Vec::new();
    let mut old_ln: u32 = 0;
    let mut new_ln: u32 = 0;
    let mut omitted_lines: usize = 0;

    for raw_line in unified_diff.lines() {
        if let Some(rest) = raw_line.strip_prefix("diff --git ") {
            flush_omitted_diff_notice(&mut files, &mut omitted_lines);
            let path = rest
                .split_whitespace()
                .last()
                .map(|p| p.strip_prefix("b/").unwrap_or(p).to_string())
                .unwrap_or_else(|| "modified".to_string());
            files.push(DiffFileView {
                path,
                additions: 0,
                deletions: 0,
                lines: Vec::new(),
            });
            old_ln = 0;
            new_ln = 0;
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

        if let Some(plus_path) = raw_line.strip_prefix("+++ b/") {
            cur_file.path = plus_path.trim().to_string();
            continue;
        }
        if raw_line.starts_with("--- ")
            || raw_line.starts_with("index ")
            || raw_line.starts_with("new file mode")
            || raw_line.starts_with("deleted file mode")
        {
            push_capped_diff_line(
                cur_file,
                &mut omitted_lines,
                DiffLineView::plain(DiffLineKind::Meta, None, None, raw_line.to_string()),
            );
            continue;
        }

        if raw_line.starts_with("@@ ") {
            let (o, n) = parse_hunk_header_numbers(raw_line);
            old_ln = o;
            new_ln = n;
            push_capped_diff_line(
                cur_file,
                &mut omitted_lines,
                DiffLineView::plain(DiffLineKind::HunkHeader, None, None, raw_line.to_string()),
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
            );
        } else if let Some(removed) = raw_line.strip_prefix('-') {
            cur_file.deletions += 1;
            let ln = old_ln;
            old_ln = old_ln.saturating_add(1);
            push_capped_diff_line(
                cur_file,
                &mut omitted_lines,
                DiffLineView::plain(DiffLineKind::Removed, Some(ln), None, removed.to_string()),
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
            );
        }
    }

    flush_omitted_diff_notice(&mut files, &mut omitted_lines);
    files.sort_by_key(|f| is_generated_or_lockfile_path(&f.path));
    for file in &mut files {
        enrich_diff_file_highlights_and_intraline(file);
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

    enrich_diff_file_highlights_and_intraline(&mut file);
    file
}

fn diff_lang_hint_from_path(path: &str) -> &str {
    let base = path.rsplit('/').next().unwrap_or(path);
    if base.eq_ignore_ascii_case("Dockerfile") {
        return "dockerfile";
    }
    if base.eq_ignore_ascii_case("Makefile") {
        return "makefile";
    }
    base.rsplit_once('.').map(|(_, ext)| ext).unwrap_or("")
}

fn enrich_diff_file_highlights_and_intraline(file: &mut DiffFileView) {
    if file.lines.is_empty() || is_generated_or_lockfile_path(&file.path) {
        return;
    }

    // 1. Syntax-highlight code lines via cached syntect pipeline
    let lang = diff_lang_hint_from_path(&file.path);
    if !lang.is_empty() {
        let mut code_indices: Vec<usize> = Vec::with_capacity(file.lines.len());
        let mut joined = String::new();
        for (idx, line) in file.lines.iter().enumerate() {
            if matches!(
                line.kind,
                DiffLineKind::Added | DiffLineKind::Removed | DiffLineKind::Context
            ) {
                code_indices.push(idx);
                joined.push_str(&line.content);
                joined.push('\n');
            }
        }
        if !code_indices.is_empty() {
            let highlighted =
                highlight_code_lines_capped(lang, &joined, MAX_RENDERED_DIFF_LINES_PER_FILE);
            for (line_pos, &line_idx) in code_indices.iter().enumerate() {
                if let Some(hl) = highlighted.get(line_pos) {
                    file.lines[line_idx].tokens = hl.tokens.clone();
                }
            }
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

pub struct TerminalGridEmulator {
    parser: vt100::Parser,
}

impl TerminalGridEmulator {
    pub fn new(cols: u16, rows: u16) -> Self {
        let r = std::num::NonZeroU16::new(rows.max(4)).unwrap_or(std::num::NonZeroU16::MIN);
        let c = std::num::NonZeroU16::new(cols.max(20)).unwrap_or(std::num::NonZeroU16::MIN);
        Self {
            parser: vt100::Parser::new(r, c, 2000),
        }
    }

    pub fn process(&mut self, bytes: &[u8]) {
        self.parser.process(bytes);
    }

    pub fn resize(&mut self, cols: u16, rows: u16) {
        let r = std::num::NonZeroU16::new(rows.max(4)).unwrap_or(std::num::NonZeroU16::MIN);
        let c = std::num::NonZeroU16::new(cols.max(20)).unwrap_or(std::num::NonZeroU16::MIN);
        if self.parser.screen().size() != (r, c) {
            self.parser.screen_mut().set_size(r, c);
        }
    }

    pub fn clear(&mut self) {
        let (rows, cols) = self.parser.screen().size();
        self.parser = vt100::Parser::new(rows, cols, 2000);
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
}
