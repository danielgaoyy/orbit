//! Pure-Rust 120Hz client rendering pipeline:
//! - Streaming CommonMark/GFM parser (`pulldown-cmark`) with unclosed-fence healing
//! - Zero-C-dependency syntax highlighting (`syntect` pure-Rust `default-fancy`)
//! - Unified & inline Git diff structural model (`similar`)
//! - Run-length coalesced VT100/ANSI terminal grid (`vt100`) for native mobile/desktop canvases

use std::sync::OnceLock;

use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
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

/// Highlight a code snippet into 24-bit `#RRGGBB` styled token lines in pure Rust.
pub fn highlight_code_lines(language: &str, code: &str) -> Vec<HighlightedLineView> {
    let ps = syntax_set();
    let lang_token = language.trim();
    let syntax = if lang_token.is_empty() {
        ps.find_syntax_plain_text()
    } else {
        ps.find_syntax_by_token(lang_token)
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
        let tokens = match highlighter.highlight_line(line, ps) {
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

    out
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
    let mut current_link: Option<String> = None;

    let mut in_heading: Option<u8> = None;
    let mut in_blockquote = false;
    let mut list_stack: Vec<Option<u64>> = Vec::new();
    let mut in_list_item = false;
    let mut item_checked: Option<bool> = None;

    let mut in_code_block: Option<String> = None;
    let mut code_buf = String::new();

    let push_span = |spans: &mut Vec<InlineSpanView>,
                     text: &str,
                     bold: bool,
                     italic: bool,
                     strikethrough: bool,
                     inline_code: bool,
                     link_url: &Option<String>| {
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
    };

    for event in parser {
        match event {
            Event::Start(tag) => match tag {
                Tag::Heading { level, .. } => {
                    cur_spans.clear();
                    in_heading = Some(heading_level_u8(level));
                }
                Tag::Paragraph => {
                    if !in_list_item && !in_blockquote {
                        cur_spans.clear();
                    } else if !cur_spans.is_empty() {
                        push_span(&mut cur_spans, "\n", false, false, false, false, &None);
                    }
                }
                Tag::CodeBlock(kind) => {
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
                    list_stack.push(start_idx);
                }
                Tag::Item => {
                    cur_spans.clear();
                    in_list_item = true;
                    item_checked = None;
                }
                Tag::BlockQuote(_) => {
                    cur_spans.clear();
                    in_blockquote = true;
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
                    if !in_list_item && !in_blockquote && !cur_spans.is_empty() {
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
                    in_list_item = false;
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
                            checked: item_checked.take(),
                            spans: std::mem::take(&mut cur_spans),
                        });
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
                TagEnd::TableRow => {
                    if !cur_spans.is_empty() {
                        blocks.push(RichBlockView::Paragraph {
                            spans: std::mem::take(&mut cur_spans),
                        });
                    }
                }
                TagEnd::TableCell => {
                    push_span(&mut cur_spans, "  │  ", false, false, false, false, &None);
                }
                _ => {}
            },
            Event::Text(text) => {
                if in_code_block.is_some() {
                    code_buf.push_str(&text);
                } else {
                    push_span(
                        &mut cur_spans,
                        &text,
                        bold_depth > 0,
                        italic_depth > 0,
                        strike_depth > 0,
                        false,
                        &current_link,
                    );
                }
            }
            Event::Code(code) => {
                push_span(
                    &mut cur_spans,
                    &code,
                    bold_depth > 0,
                    italic_depth > 0,
                    strike_depth > 0,
                    true,
                    &current_link,
                );
            }
            Event::SoftBreak | Event::HardBreak => {
                if in_code_block.is_some() {
                    code_buf.push('\n');
                } else {
                    push_span(&mut cur_spans, "\n", false, false, false, false, &None);
                }
            }
            Event::TaskListMarker(checked) => {
                item_checked = Some(checked);
            }
            Event::Rule => {
                blocks.push(RichBlockView::HorizontalRule);
            }
            _ => {}
        }
    }

    if !cur_spans.is_empty() {
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffLineView {
    pub kind: DiffLineKind,
    pub old_line: Option<u32>,
    pub new_line: Option<u32>,
    pub content: String,
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
        last.lines.push(DiffLineView {
            kind: DiffLineKind::Meta,
            old_line: None,
            new_line: None,
            content: format!("… {} more diff lines omitted", *omitted_lines),
        });
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
                DiffLineView {
                    kind: DiffLineKind::Meta,
                    old_line: None,
                    new_line: None,
                    content: raw_line.to_string(),
                },
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
                DiffLineView {
                    kind: DiffLineKind::HunkHeader,
                    old_line: None,
                    new_line: None,
                    content: raw_line.to_string(),
                },
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
                DiffLineView {
                    kind: DiffLineKind::Added,
                    old_line: None,
                    new_line: Some(ln),
                    content: added.to_string(),
                },
            );
        } else if let Some(removed) = raw_line.strip_prefix('-') {
            cur_file.deletions += 1;
            let ln = old_ln;
            old_ln = old_ln.saturating_add(1);
            push_capped_diff_line(
                cur_file,
                &mut omitted_lines,
                DiffLineView {
                    kind: DiffLineKind::Removed,
                    old_line: Some(ln),
                    new_line: None,
                    content: removed.to_string(),
                },
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
                DiffLineView {
                    kind: DiffLineKind::Context,
                    old_line: o,
                    new_line: n,
                    content,
                },
            );
        }
    }

    flush_omitted_diff_notice(&mut files, &mut omitted_lines);
    files.sort_by_key(|f| is_generated_or_lockfile_path(&f.path));
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
                DiffLineView {
                    kind: DiffLineKind::Context,
                    old_line: change.old_index().map(|i| (i + 1) as u32),
                    new_line: change.new_index().map(|i| (i + 1) as u32),
                    content,
                },
            ),
            ChangeTag::Delete => {
                file.deletions += 1;
                push_capped_diff_line(
                    &mut file,
                    &mut omitted_lines,
                    DiffLineView {
                        kind: DiffLineKind::Removed,
                        old_line: change.old_index().map(|i| (i + 1) as u32),
                        new_line: None,
                        content,
                    },
                );
            }
            ChangeTag::Insert => {
                file.additions += 1;
                push_capped_diff_line(
                    &mut file,
                    &mut omitted_lines,
                    DiffLineView {
                        kind: DiffLineKind::Added,
                        old_line: None,
                        new_line: change.new_index().map(|i| (i + 1) as u32),
                        content,
                    },
                );
            }
        }
    }

    if omitted_lines > 0 {
        file.lines.push(DiffLineView {
            kind: DiffLineKind::Meta,
            old_line: None,
            new_line: None,
            content: format!("… {omitted_lines} more diff lines omitted"),
        });
    }

    file
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
}
