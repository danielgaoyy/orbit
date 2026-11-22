//! Pure-Rust LaTeX Math → MathML Core compiler and Markdown math delimiter preprocessor.
//!
//! Shared across Desktop, HTML Export, iOS, Android, and HarmonyOS with zero JS/Node/Python
//! dependencies and zero external font bundles.
//!
//! Security & Reliability Invariants:
//! - Zero `unsafe` code, zero `.unwrap()` / `.expect()` in production paths.
//! - Input byte limit (64 KiB), token limit, and bounded recursion depth (64) prevent
//!   adversarial stack overflows or CPU exhaustion.
//! - 100% strict XML/HTML entity escaping on every text/identifier/operator payload; only
//!   static allowlisted MathML Core tags and attributes are ever emitted (XSS-proof).

use std::collections::HashSet;
use std::fmt::Write as _;

const MAX_MATH_INPUT_BYTES: usize = 64 * 1024;
const MAX_RECURSION_DEPTH: usize = 64;
const MAX_TOKENS: usize = 32_768;
const MAX_TABLE_CELLS: usize = 4_096;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MathDisplay {
    Inline,
    Block,
}

/// Returns `true` when a fenced code block language (`math`, `latex`, `tex`, `katex`)
/// should be rendered as a display math block rather than a syntax-highlighted code listing.
/// Full LaTeX document sources (`\documentclass` / `\begin{document}`) remain code blocks.
pub fn is_math_code_fence_lang(language: &str, raw_code: &str) -> bool {
    let lang = language.trim().to_ascii_lowercase();
    match lang.as_str() {
        "math" | "katex" => true,
        "latex" | "tex" => {
            !raw_code.contains("\\documentclass") && !raw_code.contains("\\begin{document}")
        }
        _ => false,
    }
}

/// Normalize standard LaTeX math delimiters `\( ... \)` → `$...$` and `\[ ... \]` → `$$...$$`
/// so `pulldown-cmark` with `Options::ENABLE_MATH` parses all four delimiter styles uniformly.
///
/// Strictly preserves:
/// - Fenced code blocks (` ``` ` / ` ~~~ `, 3+ chars, up to 3 leading spaces)
/// - Inline code spans (exact backtick run length)
/// - Existing `$...$` and `$$...$$` spans
/// - Escaped `\$`, `\\(`, `\\[` sequences
pub fn preprocess_markdown_math(markdown: &str) -> String {
    if !markdown.contains("\\(") && !markdown.contains("\\[") {
        return markdown.to_string();
    }

    let bytes = markdown.as_bytes();
    let len = bytes.len();
    let mut out = String::with_capacity(len + 32);
    let mut i = 0usize;
    let mut at_line_start = true;
    // Active fenced code block: (fence_byte: b'`' | b'~', fence_len)
    let mut active_fence: Option<(u8, usize)> = None;
    // High-water failure markers guaranteeing strictly O(N) scanning on adversarial unclosed delimiters
    let mut failed_backtick_runs: HashSet<usize> = HashSet::new();
    let mut double_dollar_failed_until: usize = 0;
    let mut single_dollar_failed_until: usize = 0;
    let mut inline_failed_until: usize = 0;
    let mut display_failed_until: usize = 0;

    while i < len {
        // 1. Fenced code block detection / continuation at line start
        if at_line_start {
            let line_end = find_line_end(bytes, i);
            let line_slice = &markdown[i..line_end];

            if let Some((fence_ch, fence_len)) = active_fence {
                out.push_str(line_slice);
                if is_closing_fence(line_slice, fence_ch, fence_len) {
                    active_fence = None;
                }
                i = line_end;
                at_line_start = true;
                continue;
            }

            if let Some((fence_ch, fence_len)) = detect_opening_fence(line_slice) {
                active_fence = Some((fence_ch, fence_len));
                out.push_str(line_slice);
                i = line_end;
                at_line_start = true;
                continue;
            }
        }

        let b = bytes[i];

        if b == b'\n' {
            out.push('\n');
            i += 1;
            at_line_start = true;
            continue;
        }

        at_line_start = false;

        // 2. Inline code spans (exact matching backtick run length)
        if b == b'`' {
            let mut ticks = 0usize;
            while i + ticks < len && bytes[i + ticks] == b'`' {
                ticks += 1;
            }
            if !failed_backtick_runs.contains(&ticks) {
                if let Some(close_end) = find_closing_backticks(bytes, i + ticks, ticks) {
                    let span = &markdown[i..close_end];
                    out.push_str(span);
                    at_line_start = span.ends_with('\n');
                    i = close_end;
                    continue;
                }
                failed_backtick_runs.insert(ticks);
            }
            out.push_str(&markdown[i..i + ticks]);
            i += ticks;
            continue;
        }

        // 3. Existing `$$...$$` or `$...$` spans — preserve verbatim so internal `\(` / `\[` isn't rewritten
        if b == b'$' {
            if i + 1 < len && bytes[i + 1] == b'$' {
                if i + 2 >= double_dollar_failed_until {
                    match find_closing_double_dollar(bytes, i + 2) {
                        Ok(close_end) => {
                            let span = &markdown[i..close_end];
                            out.push_str(span);
                            at_line_start = span.ends_with('\n');
                            i = close_end;
                            continue;
                        }
                        Err(stop_pos) => {
                            double_dollar_failed_until = double_dollar_failed_until.max(stop_pos);
                        }
                    }
                }
                out.push_str("$$");
                i += 2;
                continue;
            }
            if i + 1 >= single_dollar_failed_until {
                match find_closing_single_dollar(bytes, i + 1) {
                    Ok(close_end) => {
                        let span = &markdown[i..close_end];
                        out.push_str(span);
                        at_line_start = span.ends_with('\n');
                        i = close_end;
                        continue;
                    }
                    Err(stop_pos) => {
                        single_dollar_failed_until = single_dollar_failed_until.max(stop_pos);
                    }
                }
            }
            out.push('$');
            i += 1;
            continue;
        }

        // 4. Backslash escapes & `\(...\)` / `\[...\]` delimiters
        if b == b'\\' && i + 1 < len {
            let next = bytes[i + 1];
            match next {
                // Escaped backslash (`\\(`, `\\[`, etc.) or escaped dollar (`\$`): preserve verbatim
                b'\\' | b'$' | b'`' => {
                    out.push('\\');
                    out.push(next as char);
                    i += 2;
                    continue;
                }
                b'(' => {
                    if i + 2 >= inline_failed_until {
                        match find_closing_latex_inline(bytes, i + 2) {
                            Ok((inner_end, after_close)) => {
                                let inner = markdown[i + 2..inner_end].trim();
                                if !inner.is_empty() {
                                    out.push('$');
                                    out.push_str(inner);
                                    out.push('$');
                                    i = after_close;
                                    continue;
                                }
                            }
                            Err(stop_pos) => {
                                inline_failed_until = inline_failed_until.max(stop_pos);
                            }
                        }
                    }
                    out.push_str("\\(");
                    i += 2;
                    continue;
                }
                b'[' => {
                    if i + 2 >= display_failed_until {
                        match find_closing_latex_display(bytes, i + 2) {
                            Ok((inner_end, after_close)) => {
                                let raw_inner = markdown[i + 2..inner_end].trim();
                                if !raw_inner.is_empty() {
                                    let collapsed = collapse_blank_lines(raw_inner);
                                    out.push_str("$$");
                                    if collapsed.contains('\n') {
                                        out.push('\n');
                                        out.push_str(&collapsed);
                                        out.push('\n');
                                    } else {
                                        out.push_str(&collapsed);
                                    }
                                    out.push_str("$$");
                                    i = after_close;
                                    continue;
                                }
                            }
                            Err(stop_pos) => {
                                display_failed_until = display_failed_until.max(stop_pos);
                            }
                        }
                    }
                    out.push_str("\\[");
                    i += 2;
                    continue;
                }
                _ => {
                    out.push('\\');
                    i += 1;
                    continue;
                }
            }
        }

        let ch_len = utf8_char_len(bytes, i);
        out.push_str(&markdown[i..i + ch_len]);
        i += ch_len;
    }

    out
}

fn find_line_end(bytes: &[u8], start: usize) -> usize {
    let mut j = start;
    while j < bytes.len() {
        if bytes[j] == b'\n' {
            return j + 1;
        }
        j += 1;
    }
    bytes.len()
}

fn detect_opening_fence(line: &str) -> Option<(u8, usize)> {
    let bytes = line.as_bytes();
    let mut p = 0usize;
    let mut spaces = 0usize;
    while p < bytes.len() && bytes[p] == b' ' && spaces < 3 {
        p += 1;
        spaces += 1;
    }
    if p >= bytes.len() {
        return None;
    }
    let ch = bytes[p];
    if ch != b'`' && ch != b'~' {
        return None;
    }
    let mut count = 0usize;
    while p + count < bytes.len() && bytes[p + count] == ch {
        count += 1;
    }
    if count < 3 {
        return None;
    }
    // CommonMark: backtick info strings cannot contain backticks
    if ch == b'`' && bytes[p + count..].contains(&b'`') {
        return None;
    }
    Some((ch, count))
}

fn is_closing_fence(line: &str, fence_ch: u8, min_len: usize) -> bool {
    let bytes = line.as_bytes();
    let mut p = 0usize;
    let mut spaces = 0usize;
    while p < bytes.len() && bytes[p] == b' ' && spaces < 3 {
        p += 1;
        spaces += 1;
    }
    let mut count = 0usize;
    while p + count < bytes.len() && bytes[p + count] == fence_ch {
        count += 1;
    }
    if count < min_len {
        return false;
    }
    bytes[p + count..]
        .iter()
        .all(|b| matches!(*b, b' ' | b'\t' | b'\r' | b'\n'))
}

fn find_closing_backticks(bytes: &[u8], start: usize, ticks: usize) -> Option<usize> {
    let mut j = start;
    while j < bytes.len() {
        if bytes[j] == b'`' {
            let mut run = 0usize;
            while j + run < bytes.len() && bytes[j + run] == b'`' {
                run += 1;
            }
            if run == ticks {
                return Some(j + run);
            }
            j += run;
            continue;
        }
        j += 1;
    }
    None
}

fn is_blank_line_break(bytes: &[u8], idx: usize) -> bool {
    if bytes.get(idx).copied() != Some(b'\n') {
        return false;
    }
    let mut k = idx + 1;
    while k < bytes.len() && matches!(bytes[k], b' ' | b'\t' | b'\r') {
        k += 1;
    }
    bytes.get(k).copied() == Some(b'\n')
}

fn find_closing_double_dollar(bytes: &[u8], start: usize) -> Result<usize, usize> {
    let mut j = start;
    while j < bytes.len() {
        if bytes[j] == b'\\' && j + 1 < bytes.len() {
            j += 2;
            continue;
        }
        if bytes[j] == b'$' && j + 1 < bytes.len() && bytes[j + 1] == b'$' {
            return Ok(j + 2);
        }
        if is_blank_line_break(bytes, j) {
            return Err(j);
        }
        j += 1;
    }
    Err(bytes.len())
}

fn find_closing_single_dollar(bytes: &[u8], start: usize) -> Result<usize, usize> {
    if start >= bytes.len() || matches!(bytes[start], b' ' | b'\t' | b'\n' | b'\r') {
        return Err(start);
    }
    let mut j = start;
    while j < bytes.len() {
        if bytes[j] == b'\\' && j + 1 < bytes.len() {
            // Stop if we hit a LaTeX delimiter before finding a closing `$` (e.g. `$5 and \(x\)`)
            if matches!(bytes[j + 1], b'(' | b'[') {
                return Err(j);
            }
            j += 2;
            continue;
        }
        if bytes[j] == b'$' {
            if j > start && !matches!(bytes[j - 1], b' ' | b'\t' | b'\n' | b'\r') {
                return Ok(j + 1);
            }
            return Err(j);
        }
        if is_blank_line_break(bytes, j) {
            return Err(j);
        }
        j += 1;
    }
    Err(bytes.len())
}

fn find_closing_latex_inline(bytes: &[u8], start: usize) -> Result<(usize, usize), usize> {
    let mut j = start;
    while j < bytes.len() {
        if bytes[j] == b'\\' && j + 1 < bytes.len() {
            if bytes[j + 1] == b')' {
                return Ok((j, j + 2));
            }
            if bytes[j + 1] == b'\\' {
                j += 2;
                continue;
            }
        }
        if is_blank_line_break(bytes, j) {
            return Err(j);
        }
        j += 1;
    }
    Err(bytes.len())
}

fn find_closing_latex_display(bytes: &[u8], start: usize) -> Result<(usize, usize), usize> {
    let mut j = start;
    let mut line_start = false;
    while j < bytes.len() {
        if line_start {
            let line_end = find_line_end(bytes, j);
            if let Ok(line) = std::str::from_utf8(&bytes[j..line_end])
                && detect_opening_fence(line).is_some()
            {
                return Err(j);
            }
        }
        if bytes[j] == b'\n' {
            line_start = true;
            j += 1;
            continue;
        }
        line_start = false;
        if bytes[j] == b'\\' && j + 1 < bytes.len() {
            if bytes[j + 1] == b']' {
                return Ok((j, j + 2));
            }
            if bytes[j + 1] == b'\\' {
                j += 2;
                continue;
            }
        }
        j += 1;
    }
    Err(bytes.len())
}

fn collapse_blank_lines(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for line in input.lines() {
        if line.trim().is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(line);
    }
    out
}

fn utf8_char_len(bytes: &[u8], idx: usize) -> usize {
    let b = bytes[idx];
    if b < 0x80 {
        1
    } else if (b & 0xE0) == 0xC0 {
        2.min(bytes.len() - idx)
    } else if (b & 0xF0) == 0xE0 {
        3.min(bytes.len() - idx)
    } else {
        4.min(bytes.len() - idx)
    }
}

// ---------------------------------------------------------------------------
// LaTeX → MathML Core Compiler
// ---------------------------------------------------------------------------

/// Compile a LaTeX math expression into clean, strictly escaped semantic MathML Core.
pub fn render_latex_to_mathml(latex: &str, display: MathDisplay) -> String {
    let display_attr = match display {
        MathDisplay::Inline => "inline",
        MathDisplay::Block => "block",
    };

    let trimmed = latex.trim();
    if trimmed.is_empty() {
        return format!(
            "<math xmlns=\"http://www.w3.org/1998/Math/MathML\" display=\"{display_attr}\"></math>"
        );
    }

    if trimmed.len() > MAX_MATH_INPUT_BYTES {
        let preview: String = trimmed.chars().take(128).collect();
        let mut out = String::with_capacity(256);
        let _ = write!(
            out,
            "<math xmlns=\"http://www.w3.org/1998/Math/MathML\" display=\"{display_attr}\"><mtext>"
        );
        push_xml_escaped(&mut out, &preview);
        out.push_str("…</mtext></math>");
        return out;
    }

    let tokens = tokenize(trimmed);
    let mut parser = MathParser::new(&tokens);
    let nodes = parser.parse_sequence(StopCondition::EndOfInput, 0);

    let mut out = String::with_capacity(trimmed.len().saturating_mul(3) + 96);
    let _ = write!(
        out,
        "<math xmlns=\"http://www.w3.org/1998/Math/MathML\" display=\"{display_attr}\">"
    );
    if nodes.len() == 1 {
        if let Some(first) = nodes.first() {
            emit_node(first, &mut out);
        }
    } else if !nodes.is_empty() {
        out.push_str("<mrow>");
        for node in &nodes {
            emit_node(node, &mut out);
        }
        out.push_str("</mrow>");
    }
    out.push_str("</math>");
    out
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    Command(String),
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    Caret,
    Underscore,
    Ampersand,
    Prime,
    Tilde,
    Number(String),
    Ident(char),
    Op(char),
    Space,
}

fn tokenize(input: &str) -> Vec<Token> {
    let mut tokens = Vec::with_capacity(input.len().min(MAX_TOKENS));
    let mut chars = input.chars().peekable();

    while let Some(ch) = chars.next() {
        if tokens.len() >= MAX_TOKENS {
            break;
        }
        match ch {
            '%' => {
                // LaTeX line comment
                for next in chars.by_ref() {
                    if next == '\n' {
                        break;
                    }
                }
            }
            '\\' => match chars.peek().copied() {
                Some(c) if c.is_ascii_alphabetic() => {
                    let mut name = String::new();
                    while let Some(&nc) = chars.peek() {
                        if nc.is_ascii_alphabetic() {
                            name.push(nc);
                            chars.next();
                        } else {
                            break;
                        }
                    }
                    // Optional trailing `*` for starred commands like `\operatorname*` or `\hspace*`
                    if chars.peek().copied() == Some('*')
                        && matches!(name.as_str(), "operatorname" | "hspace" | "vspace")
                    {
                        name.push('*');
                        chars.next();
                    }
                    tokens.push(Token::Command(name));
                }
                Some(sym) => {
                    chars.next();
                    tokens.push(Token::Command(sym.to_string()));
                }
                None => {
                    tokens.push(Token::Command(String::new()));
                }
            },
            '{' => tokens.push(Token::LBrace),
            '}' => tokens.push(Token::RBrace),
            '[' => tokens.push(Token::LBracket),
            ']' => tokens.push(Token::RBracket),
            '^' => tokens.push(Token::Caret),
            '_' => tokens.push(Token::Underscore),
            '&' => tokens.push(Token::Ampersand),
            '\'' => tokens.push(Token::Prime),
            '~' => tokens.push(Token::Tilde),
            c if c.is_ascii_whitespace() => {
                while let Some(&nc) = chars.peek() {
                    if nc.is_ascii_whitespace() {
                        chars.next();
                    } else {
                        break;
                    }
                }
                tokens.push(Token::Space);
            }
            c if c.is_ascii_digit() => {
                let mut num = String::new();
                num.push(c);
                let mut has_dot = false;
                while let Some(&nc) = chars.peek() {
                    if nc.is_ascii_digit() {
                        num.push(nc);
                        chars.next();
                    } else if nc == '.' && !has_dot {
                        // Look ahead via cloning iterator to verify next char is digit
                        let mut look = chars.clone();
                        look.next();
                        if look.peek().is_some_and(char::is_ascii_digit) {
                            has_dot = true;
                            num.push(nc);
                            chars.next();
                        } else {
                            break;
                        }
                    } else {
                        break;
                    }
                }
                tokens.push(Token::Number(num));
            }
            c if c.is_alphabetic() => tokens.push(Token::Ident(c)),
            c => tokens.push(Token::Op(c)),
        }
    }

    tokens
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StopCondition {
    EndOfInput,
    RBrace,
    RightDelim,
    EndEnvCell,
    EndEnvOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TableColumnAlign {
    Center,
    Left,
    Right,
    AlternatingRightLeft,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MathFont {
    Bold,
    Blackboard,
    Calligraphic,
    Roman,
    SansSerif,
    Monospace,
    BoldItalic,
    Fraktur,
    Italic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LimitsMode {
    Default,
    Limits,
    NoLimits,
}

#[derive(Debug, Clone)]
enum MathNode {
    Number(String),
    Ident {
        text: String,
        normal: bool,
    },
    Op {
        text: String,
        stretchy: Option<bool>,
        fence: bool,
        movablelimits: Option<bool>,
        largeop: bool,
    },
    Text {
        text: String,
        bold: bool,
        italic: bool,
    },
    Space(&'static str),
    Row(Vec<MathNode>),
    Frac {
        num: Box<MathNode>,
        den: Box<MathNode>,
        linethickness: Option<&'static str>,
    },
    Sqrt(Box<MathNode>),
    Root {
        radicand: Box<MathNode>,
        index: Box<MathNode>,
    },
    Sub {
        base: Box<MathNode>,
        sub: Box<MathNode>,
    },
    Sup {
        base: Box<MathNode>,
        sup: Box<MathNode>,
    },
    SubSup {
        base: Box<MathNode>,
        sub: Box<MathNode>,
        sup: Box<MathNode>,
    },
    Under {
        base: Box<MathNode>,
        under: Box<MathNode>,
        accentunder: bool,
    },
    Over {
        base: Box<MathNode>,
        over: Box<MathNode>,
        accent: bool,
    },
    UnderOver {
        base: Box<MathNode>,
        under: Box<MathNode>,
        over: Box<MathNode>,
    },
    Table {
        rows: Vec<Vec<MathNode>>,
        column_align: TableColumnAlign,
    },
    Style {
        display_style: Option<bool>,
        script_level: Option<&'static str>,
        child: Box<MathNode>,
    },
    Phantom(Box<MathNode>),
    Empty,
}

impl MathNode {
    fn op_simple(text: impl Into<String>) -> Self {
        Self::Op {
            text: text.into(),
            stretchy: None,
            fence: false,
            movablelimits: None,
            largeop: false,
        }
    }

    fn op_fence(text: impl Into<String>, stretchy: bool) -> Self {
        Self::Op {
            text: text.into(),
            stretchy: Some(stretchy),
            fence: true,
            movablelimits: None,
            largeop: false,
        }
    }

    fn op_large(text: impl Into<String>, movablelimits: bool) -> Self {
        Self::Op {
            text: text.into(),
            stretchy: None,
            fence: false,
            movablelimits: Some(movablelimits),
            largeop: true,
        }
    }

    fn supports_underover_limits(&self) -> bool {
        match self {
            Self::Op {
                largeop,
                movablelimits,
                ..
            } => *largeop || movablelimits.is_some(),
            Self::Ident { text, normal: true } => is_limit_operator_name(text),
            _ => false,
        }
    }

    fn with_movablelimits(mut self, movable: bool) -> Self {
        if let Self::Op {
            ref mut movablelimits,
            ..
        } = self
        {
            *movablelimits = Some(movable);
        }
        self
    }

    fn from_vec(mut nodes: Vec<MathNode>) -> Self {
        match nodes.len() {
            0 => Self::Empty,
            1 => nodes.pop().unwrap_or(Self::Empty),
            _ => Self::Row(nodes),
        }
    }
}

struct MathParser<'a> {
    tokens: &'a [Token],
    pos: usize,
}

impl<'a> MathParser<'a> {
    fn new(tokens: &'a [Token]) -> Self {
        Self { tokens, pos: 0 }
    }

    fn peek(&self) -> Option<&'a Token> {
        self.tokens.get(self.pos)
    }

    fn next(&mut self) -> Option<&'a Token> {
        let tok = self.tokens.get(self.pos)?;
        self.pos += 1;
        Some(tok)
    }

    fn skip_spaces(&mut self) {
        while matches!(self.peek(), Some(Token::Space)) {
            self.pos += 1;
        }
    }

    fn skip_to_stop(&mut self, stop: StopCondition) {
        match stop {
            StopCondition::EndOfInput => {
                self.pos = self.tokens.len();
            }
            StopCondition::RBrace => {
                let mut brace_depth = 1usize;
                while let Some(tok) = self.next() {
                    match tok {
                        Token::LBrace => brace_depth += 1,
                        Token::RBrace => {
                            brace_depth = brace_depth.saturating_sub(1);
                            if brace_depth == 0 {
                                break;
                            }
                        }
                        _ => {}
                    }
                }
            }
            StopCondition::RightDelim => {
                while let Some(tok) = self.peek() {
                    if matches!(tok, Token::Command(cmd) if cmd == "right") {
                        break;
                    }
                    self.pos += 1;
                }
            }
            StopCondition::EndEnvCell => {
                while let Some(tok) = self.peek() {
                    if matches!(tok, Token::Ampersand)
                        || matches!(tok, Token::Command(cmd) if matches!(cmd.as_str(), "end" | "\\" | "cr"))
                    {
                        break;
                    }
                    self.pos += 1;
                }
            }
            StopCondition::EndEnvOnly => {
                while let Some(tok) = self.peek() {
                    if matches!(tok, Token::Command(cmd) if cmd == "end") {
                        break;
                    }
                    self.pos += 1;
                }
            }
        }
    }

    fn parse_sequence(&mut self, stop: StopCondition, depth: usize) -> Vec<MathNode> {
        if depth >= MAX_RECURSION_DEPTH {
            self.skip_to_stop(stop);
            return vec![MathNode::Text {
                text: "…".to_string(),
                bold: false,
                italic: false,
            }];
        }

        let mut items: Vec<MathNode> = Vec::new();
        let mut active_display_style: Option<bool> = None;
        let mut active_script_level: Option<&'static str> = None;

        while self.pos < self.tokens.len() {
            self.skip_spaces();
            let Some(tok) = self.peek() else {
                break;
            };

            match (stop, tok) {
                (StopCondition::RBrace, Token::RBrace) => {
                    self.pos += 1;
                    break;
                }
                (StopCondition::RightDelim, Token::Command(cmd)) if cmd == "right" => {
                    break;
                }
                (StopCondition::EndEnvCell | StopCondition::EndEnvOnly, Token::Command(cmd))
                    if cmd == "end" =>
                {
                    break;
                }
                // Stop inside table row/cell parsing when encountering `&` or `\\`
                (StopCondition::EndEnvCell, Token::Ampersand) => break,
                (StopCondition::EndEnvCell, Token::Command(cmd)) if cmd == "\\" || cmd == "cr" => {
                    break;
                }
                // Unmatched `}` at top level: consume safely so parser always advances
                (StopCondition::EndOfInput, Token::RBrace) => {
                    self.pos += 1;
                    continue;
                }
                _ => {}
            }

            // Infix fraction commands (`\over`, `\choose`, `\atop`) split current sequence into num/den
            if let Token::Command(cmd) = tok
                && matches!(cmd.as_str(), "over" | "choose" | "atop")
            {
                let infix_kind = cmd.as_str();
                self.pos += 1;
                let den_nodes = self.parse_sequence(stop, depth + 1);
                let num = MathNode::from_vec(items);
                let den = MathNode::from_vec(den_nodes);
                let node = match infix_kind {
                    "over" => MathNode::Frac {
                        num: Box::new(num),
                        den: Box::new(den),
                        linethickness: None,
                    },
                    "choose" => MathNode::Row(vec![
                        MathNode::op_fence("(", true),
                        MathNode::Frac {
                            num: Box::new(num),
                            den: Box::new(den),
                            linethickness: Some("0"),
                        },
                        MathNode::op_fence(")", true),
                    ]),
                    _ => MathNode::Frac {
                        num: Box::new(num),
                        den: Box::new(den),
                        linethickness: Some("0"),
                    },
                };
                return vec![wrap_style(node, active_display_style, active_script_level)];
            }

            // Style switch commands affect the remainder of the current group
            if let Token::Command(cmd) = tok {
                match cmd.as_str() {
                    "displaystyle" => {
                        self.pos += 1;
                        active_display_style = Some(true);
                        continue;
                    }
                    "textstyle" => {
                        self.pos += 1;
                        active_display_style = Some(false);
                        continue;
                    }
                    "scriptstyle" => {
                        self.pos += 1;
                        active_display_style = Some(false);
                        active_script_level = Some("1");
                        continue;
                    }
                    "scriptscriptstyle" => {
                        self.pos += 1;
                        active_display_style = Some(false);
                        active_script_level = Some("2");
                        continue;
                    }
                    "nonumber" | "notag" => {
                        self.pos += 1;
                        continue;
                    }
                    "label" => {
                        self.pos += 1;
                        let _ = self.parse_raw_text_arg(depth + 1);
                        continue;
                    }
                    _ => {}
                }
            }

            let prev_pos = self.pos;
            if let Some(node) = self.parse_scripted_atom(depth + 1) {
                items.push(node);
            }
            if self.pos == prev_pos {
                // Unconditionally guarantee forward progress even on malformed or depth-capped tokens
                self.pos += 1;
            }
        }

        if active_display_style.is_some() || active_script_level.is_some() {
            let inner = MathNode::from_vec(items);
            vec![wrap_style(inner, active_display_style, active_script_level)]
        } else {
            items
        }
    }

    fn parse_scripted_atom(&mut self, depth: usize) -> Option<MathNode> {
        if depth >= MAX_RECURSION_DEPTH {
            if self.pos < self.tokens.len() {
                self.pos += 1;
            }
            return Some(MathNode::Text {
                text: "…".to_string(),
                bold: false,
                italic: false,
            });
        }
        self.skip_spaces();

        // Bare leading `^` or `_` with empty base (e.g. `{}_n C_k`)
        let base = if matches!(self.peek(), Some(Token::Caret | Token::Underscore)) {
            MathNode::Empty
        } else {
            self.parse_atom(depth + 1)?
        };

        let mut limits_mode = LimitsMode::Default;
        let mut primes: usize = 0;
        let mut sub: Option<MathNode> = None;
        let mut sup: Option<MathNode> = None;

        loop {
            self.skip_spaces();
            match self.peek() {
                Some(Token::Command(cmd)) if cmd == "limits" => {
                    self.pos += 1;
                    limits_mode = LimitsMode::Limits;
                }
                Some(Token::Command(cmd)) if cmd == "nolimits" => {
                    self.pos += 1;
                    limits_mode = LimitsMode::NoLimits;
                }
                Some(Token::Prime) => {
                    self.pos += 1;
                    primes = primes.saturating_add(1);
                }
                Some(Token::Underscore) => {
                    self.pos += 1;
                    let arg = self.parse_arg(depth + 1);
                    if sub.is_none() {
                        sub = Some(arg);
                    } else if let Some(prev) = sub.take() {
                        sub = Some(MathNode::Row(vec![prev, arg]));
                    }
                }
                Some(Token::Caret) => {
                    self.pos += 1;
                    let arg = self.parse_arg(depth + 1);
                    if sup.is_none() {
                        sup = Some(arg);
                    } else if let Some(prev) = sup.take() {
                        sup = Some(MathNode::Row(vec![prev, arg]));
                    }
                }
                _ => break,
            }
        }

        if primes > 0 {
            let prime_node = MathNode::op_simple(primes_to_unicode(primes));
            sup = match sup {
                None => Some(prime_node),
                Some(existing) => Some(MathNode::Row(vec![prime_node, existing])),
            };
        }

        let use_underover = match limits_mode {
            LimitsMode::Limits => true,
            LimitsMode::NoLimits => false,
            LimitsMode::Default => base.supports_underover_limits(),
        };

        let adjusted_base = match limits_mode {
            LimitsMode::Limits => base.with_movablelimits(false),
            LimitsMode::NoLimits => base.with_movablelimits(false),
            LimitsMode::Default => base,
        };

        Some(match (sub, sup) {
            (None, None) => adjusted_base,
            (Some(s), None) => {
                if use_underover {
                    MathNode::Under {
                        base: Box::new(adjusted_base),
                        under: Box::new(s),
                        accentunder: false,
                    }
                } else {
                    MathNode::Sub {
                        base: Box::new(adjusted_base),
                        sub: Box::new(s),
                    }
                }
            }
            (None, Some(p)) => {
                if use_underover {
                    MathNode::Over {
                        base: Box::new(adjusted_base),
                        over: Box::new(p),
                        accent: false,
                    }
                } else {
                    MathNode::Sup {
                        base: Box::new(adjusted_base),
                        sup: Box::new(p),
                    }
                }
            }
            (Some(s), Some(p)) => {
                if use_underover {
                    MathNode::UnderOver {
                        base: Box::new(adjusted_base),
                        under: Box::new(s),
                        over: Box::new(p),
                    }
                } else {
                    MathNode::SubSup {
                        base: Box::new(adjusted_base),
                        sub: Box::new(s),
                        sup: Box::new(p),
                    }
                }
            }
        })
    }

    fn parse_arg(&mut self, depth: usize) -> MathNode {
        if depth >= MAX_RECURSION_DEPTH {
            if self.pos < self.tokens.len() {
                self.pos += 1;
            }
            return MathNode::Text {
                text: "…".to_string(),
                bold: false,
                italic: false,
            };
        }
        self.skip_spaces();
        match self.peek() {
            Some(Token::LBrace) => {
                self.pos += 1;
                let nodes = self.parse_sequence(StopCondition::RBrace, depth + 1);
                MathNode::from_vec(nodes)
            }
            _ => self.parse_atom(depth + 1).unwrap_or(MathNode::Empty),
        }
    }

    fn parse_optional_bracket_arg(&mut self, depth: usize) -> Option<MathNode> {
        self.skip_spaces();
        if !matches!(self.peek(), Some(Token::LBracket)) {
            return None;
        }
        self.pos += 1;
        let mut items = Vec::new();
        while self.pos < self.tokens.len() {
            self.skip_spaces();
            if matches!(self.peek(), Some(Token::RBracket)) {
                self.pos += 1;
                break;
            }
            let prev = self.pos;
            if let Some(node) = self.parse_scripted_atom(depth + 1) {
                items.push(node);
            }
            if self.pos == prev {
                self.pos += 1;
            }
        }
        Some(MathNode::from_vec(items))
    }

    fn skip_optional_bracket_tokens(&mut self) {
        self.skip_spaces();
        if !matches!(self.peek(), Some(Token::LBracket)) {
            return;
        }
        self.pos += 1;
        let mut depth = 1usize;
        while let Some(tok) = self.next() {
            match tok {
                Token::LBracket => depth += 1,
                Token::RBracket => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        break;
                    }
                }
                _ => {}
            }
        }
    }

    fn parse_raw_text_arg(&mut self, depth: usize) -> String {
        if depth > MAX_RECURSION_DEPTH {
            return String::new();
        }
        self.skip_spaces();
        if !matches!(self.peek(), Some(Token::LBrace)) {
            return match self.next() {
                Some(Token::Ident(c)) => c.to_string(),
                Some(Token::Number(n)) => n.clone(),
                Some(Token::Op(c)) => c.to_string(),
                Some(Token::Command(cmd)) => escape_text_command(cmd),
                _ => String::new(),
            };
        }
        self.pos += 1;
        let mut out = String::new();
        let mut brace_depth = 1usize;
        while let Some(tok) = self.next() {
            match tok {
                Token::LBrace => {
                    brace_depth += 1;
                }
                Token::RBrace => {
                    brace_depth = brace_depth.saturating_sub(1);
                    if brace_depth == 0 {
                        break;
                    }
                }
                Token::Space => out.push(' '),
                Token::Tilde => out.push('\u{00A0}'),
                Token::Number(n) => out.push_str(n),
                Token::Ident(c) => out.push(*c),
                Token::Op(c) => out.push(*c),
                Token::LBracket => out.push('['),
                Token::RBracket => out.push(']'),
                Token::Caret => out.push('^'),
                Token::Underscore => out.push('_'),
                Token::Ampersand => out.push('&'),
                Token::Prime => out.push('\''),
                Token::Command(cmd) => out.push_str(&escape_text_command(cmd)),
            }
        }
        out
    }

    fn parse_atom(&mut self, depth: usize) -> Option<MathNode> {
        if depth >= MAX_RECURSION_DEPTH {
            if self.pos < self.tokens.len() {
                self.pos += 1;
            }
            return Some(MathNode::Text {
                text: "…".to_string(),
                bold: false,
                italic: false,
            });
        }
        self.skip_spaces();
        let tok = self.next()?;
        match tok {
            Token::LBrace => {
                let nodes = self.parse_sequence(StopCondition::RBrace, depth + 1);
                Some(MathNode::from_vec(nodes))
            }
            Token::RBrace => None,
            Token::LBracket => Some(MathNode::op_fence("[", false)),
            Token::RBracket => Some(MathNode::op_fence("]", false)),
            Token::Number(n) => Some(MathNode::Number(n.clone())),
            Token::Ident(c) => Some(MathNode::Ident {
                text: c.to_string(),
                normal: false,
            }),
            Token::Op(c) => Some(match c {
                '-' => MathNode::op_simple("\u{2212}"), // Unicode minus sign
                '*' => MathNode::op_simple("\u{2217}"), // Math asterisk operator
                '(' | ')' | '|' => MathNode::op_fence(c.to_string(), false),
                other => MathNode::op_simple(other.to_string()),
            }),
            Token::Tilde => Some(MathNode::Space("0.25em")),
            Token::Prime => Some(MathNode::op_simple("\u{2032}")),
            Token::Ampersand => Some(MathNode::Space("0.5em")),
            Token::Caret | Token::Underscore | Token::Space => None,
            Token::Command(cmd) => Some(self.parse_command(cmd, depth + 1)),
        }
    }

    fn parse_command(&mut self, cmd: &str, depth: usize) -> MathNode {
        if depth >= MAX_RECURSION_DEPTH {
            return MathNode::Text {
                text: "…".to_string(),
                bold: false,
                italic: false,
            };
        }

        // 1. Structural commands
        match cmd {
            "frac" => {
                let num = self.parse_arg(depth + 1);
                let den = self.parse_arg(depth + 1);
                return MathNode::Frac {
                    num: Box::new(num),
                    den: Box::new(den),
                    linethickness: None,
                };
            }
            "dfrac" => {
                let num = self.parse_arg(depth + 1);
                let den = self.parse_arg(depth + 1);
                return MathNode::Style {
                    display_style: Some(true),
                    script_level: None,
                    child: Box::new(MathNode::Frac {
                        num: Box::new(num),
                        den: Box::new(den),
                        linethickness: None,
                    }),
                };
            }
            "tfrac" => {
                let num = self.parse_arg(depth + 1);
                let den = self.parse_arg(depth + 1);
                return MathNode::Style {
                    display_style: Some(false),
                    script_level: None,
                    child: Box::new(MathNode::Frac {
                        num: Box::new(num),
                        den: Box::new(den),
                        linethickness: None,
                    }),
                };
            }
            "binom" | "dbinom" | "tbinom" => {
                let n = self.parse_arg(depth + 1);
                let k = self.parse_arg(depth + 1);
                let frac = MathNode::Row(vec![
                    MathNode::op_fence("(", true),
                    MathNode::Frac {
                        num: Box::new(n),
                        den: Box::new(k),
                        linethickness: Some("0"),
                    },
                    MathNode::op_fence(")", true),
                ]);
                return match cmd {
                    "dbinom" => MathNode::Style {
                        display_style: Some(true),
                        script_level: None,
                        child: Box::new(frac),
                    },
                    "tbinom" => MathNode::Style {
                        display_style: Some(false),
                        script_level: None,
                        child: Box::new(frac),
                    },
                    _ => frac,
                };
            }
            "sqrt" => {
                let idx = self.parse_optional_bracket_arg(depth + 1);
                let rad = self.parse_arg(depth + 1);
                return match idx {
                    Some(index) => MathNode::Root {
                        radicand: Box::new(rad),
                        index: Box::new(index),
                    },
                    None => MathNode::Sqrt(Box::new(rad)),
                };
            }
            "left" => {
                return self.parse_left_right(depth + 1);
            }
            "middle" => {
                if let Some(delim) = self.parse_delimiter() {
                    return MathNode::op_fence(delim, true);
                }
                return MathNode::Empty;
            }
            "big" | "Big" | "bigg" | "Bigg" | "bigl" | "bigr" | "Bigl" | "Bigr" | "biggl"
            | "biggr" | "Biggl" | "Biggr" => {
                if let Some(delim) = self.parse_delimiter() {
                    return MathNode::op_fence(delim, false);
                }
                return MathNode::Empty;
            }
            "begin" => {
                return self.parse_environment(depth + 1);
            }
            "end" => {
                let _ = self.parse_raw_text_arg(depth + 1);
                return MathNode::Empty;
            }
            "text" | "textrm" | "mbox" | "hbox" | "textnormal" => {
                let raw = self.parse_raw_text_arg(depth + 1);
                return MathNode::Text {
                    text: raw,
                    bold: false,
                    italic: false,
                };
            }
            "textbf" => {
                let raw = self.parse_raw_text_arg(depth + 1);
                return MathNode::Text {
                    text: raw,
                    bold: true,
                    italic: false,
                };
            }
            "textit" | "emph" => {
                let raw = self.parse_raw_text_arg(depth + 1);
                return MathNode::Text {
                    text: raw,
                    bold: false,
                    italic: true,
                };
            }
            "texttt" => {
                let raw = self.parse_raw_text_arg(depth + 1);
                return MathNode::Ident {
                    text: map_math_font(&raw, MathFont::Monospace),
                    normal: true,
                };
            }
            "textsf" => {
                let raw = self.parse_raw_text_arg(depth + 1);
                return MathNode::Ident {
                    text: map_math_font(&raw, MathFont::SansSerif),
                    normal: true,
                };
            }
            "operatorname" | "operatorname*" => {
                let raw = self.parse_raw_text_arg(depth + 1);
                let is_limits = cmd == "operatorname*";
                return if is_limits {
                    MathNode::Op {
                        text: raw,
                        stretchy: None,
                        fence: false,
                        movablelimits: Some(true),
                        largeop: false,
                    }
                } else {
                    MathNode::Ident {
                        text: raw,
                        normal: true,
                    }
                };
            }
            "mathbf" | "bf" => {
                let inner = self.parse_arg(depth + 1);
                return apply_font_to_node(inner, MathFont::Bold);
            }
            "mathbb" | "Bbb" => {
                let inner = self.parse_arg(depth + 1);
                return apply_font_to_node(inner, MathFont::Blackboard);
            }
            "mathcal" | "mathscr" => {
                let inner = self.parse_arg(depth + 1);
                return apply_font_to_node(inner, MathFont::Calligraphic);
            }
            "mathrm" | "rm" => {
                let inner = self.parse_arg(depth + 1);
                return apply_font_to_node(inner, MathFont::Roman);
            }
            "mathsf" | "sf" => {
                let inner = self.parse_arg(depth + 1);
                return apply_font_to_node(inner, MathFont::SansSerif);
            }
            "mathtt" | "tt" => {
                let inner = self.parse_arg(depth + 1);
                return apply_font_to_node(inner, MathFont::Monospace);
            }
            "boldsymbol" | "bm" => {
                let inner = self.parse_arg(depth + 1);
                return apply_font_to_node(inner, MathFont::BoldItalic);
            }
            "mathfrak" => {
                let inner = self.parse_arg(depth + 1);
                return apply_font_to_node(inner, MathFont::Fraktur);
            }
            "mathit" => {
                let inner = self.parse_arg(depth + 1);
                return apply_font_to_node(inner, MathFont::Italic);
            }
            // Accents & decorations
            "hat" => return self.make_accent("\u{02C6}", false, true, depth + 1),
            "widehat" => return self.make_accent("^", true, true, depth + 1),
            "bar" => return self.make_accent("\u{00AF}", false, true, depth + 1),
            "overline" => return self.make_accent("\u{203E}", true, false, depth + 1),
            "underline" => {
                let base = self.parse_arg(depth + 1);
                return MathNode::Under {
                    base: Box::new(base),
                    under: Box::new(MathNode::Op {
                        text: "_".to_string(),
                        stretchy: Some(true),
                        fence: false,
                        movablelimits: None,
                        largeop: false,
                    }),
                    accentunder: false,
                };
            }
            "vec" => return self.make_accent("\u{2192}", false, true, depth + 1),
            "overrightarrow" => return self.make_accent("\u{2192}", true, false, depth + 1),
            "overleftarrow" => return self.make_accent("\u{2190}", true, false, depth + 1),
            "tilde" => return self.make_accent("~", false, true, depth + 1),
            "widetilde" => return self.make_accent("~", true, true, depth + 1),
            "dot" => return self.make_accent("\u{02D9}", false, true, depth + 1),
            "ddot" => return self.make_accent("\u{00A8}", false, true, depth + 1),
            "dddot" => return self.make_accent("\u{20DB}", false, true, depth + 1),
            "check" => return self.make_accent("\u{02C7}", false, true, depth + 1),
            "breve" => return self.make_accent("\u{02D8}", false, true, depth + 1),
            "acute" => return self.make_accent("\u{00B4}", false, true, depth + 1),
            "grave" => return self.make_accent("`", false, true, depth + 1),
            "overbrace" => return self.make_accent("\u{23DE}", true, false, depth + 1),
            "underbrace" => {
                let base = self.parse_arg(depth + 1);
                return MathNode::Under {
                    base: Box::new(base),
                    under: Box::new(MathNode::Op {
                        text: "\u{23DF}".to_string(),
                        stretchy: Some(true),
                        fence: false,
                        movablelimits: None,
                        largeop: false,
                    }),
                    accentunder: false,
                };
            }
            "overset" | "stackrel" => {
                let over = self.parse_arg(depth + 1);
                let base = self.parse_arg(depth + 1);
                return MathNode::Over {
                    base: Box::new(base),
                    over: Box::new(over),
                    accent: false,
                };
            }
            "underset" => {
                let under = self.parse_arg(depth + 1);
                let base = self.parse_arg(depth + 1);
                return MathNode::Under {
                    base: Box::new(base),
                    under: Box::new(under),
                    accentunder: false,
                };
            }
            "xrightarrow" | "xleftarrow" => {
                let arrow = if cmd == "xrightarrow" {
                    "\u{2192}"
                } else {
                    "\u{2190}"
                };
                let under = self.parse_optional_bracket_arg(depth + 1);
                let over = self.parse_arg(depth + 1);
                let base = MathNode::Op {
                    text: arrow.to_string(),
                    stretchy: Some(true),
                    fence: false,
                    movablelimits: None,
                    largeop: false,
                };
                return match under {
                    Some(u) => MathNode::UnderOver {
                        base: Box::new(base),
                        under: Box::new(u),
                        over: Box::new(over),
                    },
                    None => MathNode::Over {
                        base: Box::new(base),
                        over: Box::new(over),
                        accent: false,
                    },
                };
            }
            "not" => {
                return self.parse_negated_relation(depth + 1);
            }
            "pmod" => {
                let arg = self.parse_arg(depth + 1);
                return MathNode::Row(vec![
                    MathNode::Space("0.5em"),
                    MathNode::op_fence("(", false),
                    MathNode::Ident {
                        text: "mod".to_string(),
                        normal: true,
                    },
                    MathNode::Space("0.25em"),
                    arg,
                    MathNode::op_fence(")", false),
                ]);
            }
            "bmod" | "mod" => {
                return MathNode::Row(vec![
                    MathNode::Space("0.222em"),
                    MathNode::Ident {
                        text: "mod".to_string(),
                        normal: true,
                    },
                    MathNode::Space("0.222em"),
                ]);
            }
            "phantom" | "hphantom" | "vphantom" => {
                let inner = self.parse_arg(depth + 1);
                return MathNode::Phantom(Box::new(inner));
            }
            "boxed" | "cancel" | "mathrel" | "mathbin" | "mathord" | "mathopen" | "mathclose"
            | "mathpunct" | "mathinner" | "mathop" => {
                return self.parse_arg(depth + 1);
            }
            "textcolor" => {
                let _color = self.parse_raw_text_arg(depth + 1);
                return self.parse_arg(depth + 1);
            }
            "color" => {
                let _color = self.parse_raw_text_arg(depth + 1);
                return MathNode::Empty;
            }
            "hspace" | "hspace*" | "vspace" | "vspace*" | "kern" | "mkern" => {
                let _len = self.parse_raw_text_arg(depth + 1);
                return MathNode::Space("0.5em");
            }
            "tag" => {
                let label = self.parse_raw_text_arg(depth + 1);
                return MathNode::Row(vec![
                    MathNode::Space("1.5em"),
                    MathNode::Text {
                        text: format!("({label})"),
                        bold: false,
                        italic: false,
                    },
                ]);
            }
            _ => {}
        }

        // 2. Spacing commands
        if let Some(width) = spacing_command_width(cmd) {
            return MathNode::Space(width);
        }

        // 3. Big operators (with movablelimits)
        if let Some(sym) = big_operator_symbol(cmd) {
            return MathNode::op_large(sym, true);
        }

        // 4. Named upright math functions (`\sin`, `\lim`, `\max`, `\log`, ...)
        if let Some(name) = named_function_text(cmd) {
            if is_limit_operator_name(name) {
                return MathNode::Op {
                    text: name.to_string(),
                    stretchy: None,
                    fence: false,
                    movablelimits: Some(true),
                    largeop: false,
                };
            }
            return MathNode::Ident {
                text: name.to_string(),
                normal: true,
            };
        }

        // 5. Greek letters
        if let Some((sym, upright)) = greek_letter(cmd) {
            return MathNode::Ident {
                text: sym.to_string(),
                normal: upright,
            };
        }

        // 6. Delimiter symbol commands (`\{`, `\lfloor`, `\langle`, ...)
        if let Some(delim) = delimiter_command(cmd) {
            return MathNode::op_fence(delim, false);
        }

        // 7. Math symbols, relations, arrows, dots
        if let Some(sym) = math_symbol(cmd) {
            return MathNode::op_simple(sym);
        }

        // 8. Misc identifier symbols (`\infty`, `\partial`, `\nabla`, `\emptyset`, `\ell`, ...)
        if let Some((sym, normal)) = misc_identifier_symbol(cmd) {
            return MathNode::Ident {
                text: sym.to_string(),
                normal,
            };
        }

        // 9. Escaped punctuation (`\%`, `\$`, `\#`, `\&`, `\_`)
        match cmd {
            "%" | "$" | "#" | "&" | "_" => {
                return MathNode::Text {
                    text: cmd.to_string(),
                    bold: false,
                    italic: false,
                };
            }
            "\\" | "cr" => {
                self.skip_optional_bracket_tokens();
                return MathNode::Space("0.5em");
            }
            "" => return MathNode::Empty,
            _ => {}
        }

        // 10. Graceful fallback for unrecognized command
        MathNode::Ident {
            text: format!("\\{cmd}"),
            normal: true,
        }
    }

    fn make_accent(
        &mut self,
        symbol: &'static str,
        stretchy: bool,
        accent: bool,
        depth: usize,
    ) -> MathNode {
        let base = self.parse_arg(depth + 1);
        MathNode::Over {
            base: Box::new(base),
            over: Box::new(MathNode::Op {
                text: symbol.to_string(),
                stretchy: Some(stretchy),
                fence: false,
                movablelimits: None,
                largeop: false,
            }),
            accent,
        }
    }

    fn parse_negated_relation(&mut self, depth: usize) -> MathNode {
        self.skip_spaces();
        let next = self.peek().cloned();
        match next {
            Some(Token::Op('=')) => {
                self.pos += 1;
                MathNode::op_simple("\u{2260}") // ≠
            }
            Some(Token::Op('<')) => {
                self.pos += 1;
                MathNode::op_simple("\u{226E}") // ≮
            }
            Some(Token::Op('>')) => {
                self.pos += 1;
                MathNode::op_simple("\u{226F}") // ≯
            }
            Some(Token::Command(ref c)) => {
                let mapped = match c.as_str() {
                    "in" => Some("\u{2209}"),       // ∉
                    "subset" => Some("\u{2284}"),   // ⊄
                    "subseteq" => Some("\u{2288}"), // ⊈
                    "supset" => Some("\u{2285}"),   // ⊅
                    "supseteq" => Some("\u{2289}"), // ⊉
                    "equiv" => Some("\u{2262}"),    // ≢
                    "approx" => Some("\u{2249}"),   // ≉
                    "sim" => Some("\u{2241}"),      // ≁
                    "simeq" => Some("\u{2244}"),    // ≄
                    "cong" => Some("\u{2247}"),     // ≇
                    "le" | "leq" => Some("\u{2270}"),
                    "ge" | "geq" => Some("\u{2271}"),
                    "mid" => Some("\u{2224}"),
                    "parallel" => Some("\u{2226}"),
                    "exists" => Some("\u{2204}"),
                    _ => None,
                };
                if let Some(sym) = mapped {
                    self.pos += 1;
                    MathNode::op_simple(sym)
                } else {
                    let inner = self.parse_arg(depth + 1);
                    MathNode::Row(vec![MathNode::op_simple("\u{00AC}"), inner])
                }
            }
            _ => MathNode::op_simple("\u{00AC}"),
        }
    }

    fn parse_delimiter(&mut self) -> Option<&'static str> {
        self.skip_spaces();
        let tok = self.peek()?;
        let delim = match tok {
            Token::Op('.') => {
                self.pos += 1;
                return None;
            }
            Token::Op('(') => "(",
            Token::Op(')') => ")",
            Token::Op('|') => "|",
            Token::Op('/') => "/",
            Token::LBracket => "[",
            Token::RBracket => "]",
            Token::Command(cmd) => delimiter_command(cmd)?,
            _ => return None,
        };
        self.pos += 1;
        Some(delim)
    }

    fn parse_left_right(&mut self, depth: usize) -> MathNode {
        let open = self.parse_delimiter();
        let body = self.parse_sequence(StopCondition::RightDelim, depth + 1);
        let mut close = None;
        if let Some(Token::Command(cmd)) = self.peek()
            && cmd == "right"
        {
            self.pos += 1;
            close = self.parse_delimiter();
        }

        let mut row = Vec::with_capacity(body.len() + 2);
        if let Some(o) = open {
            row.push(MathNode::op_fence(o, true));
        }
        row.extend(body);
        if let Some(c) = close {
            row.push(MathNode::op_fence(c, true));
        }
        MathNode::from_vec(row)
    }

    fn parse_environment(&mut self, depth: usize) -> MathNode {
        let env_name = self.parse_raw_text_arg(depth + 1);
        let env = env_name.trim();

        // `array` takes a column-format group `{ccc}` right after `\begin{array}`
        if env == "array" {
            let _col_spec = self.parse_raw_text_arg(depth + 1);
        }

        let is_starred_matrix = matches!(
            env,
            "matrix*"
                | "pmatrix*"
                | "bmatrix*"
                | "Bmatrix*"
                | "vmatrix*"
                | "Vmatrix*"
                | "smallmatrix*"
        );
        let mut explicit_align: Option<TableColumnAlign> = None;
        if is_starred_matrix {
            explicit_align = self.parse_optional_matrix_align();
        }

        let is_matrix_like = is_starred_matrix
            || matches!(
                env,
                "matrix"
                    | "pmatrix"
                    | "bmatrix"
                    | "Bmatrix"
                    | "vmatrix"
                    | "Vmatrix"
                    | "smallmatrix"
                    | "cases"
                    | "cases*"
                    | "dcases"
                    | "dcases*"
                    | "rcases"
                    | "rcases*"
                    | "aligned"
                    | "align"
                    | "align*"
                    | "alignedat"
                    | "eqnarray"
                    | "eqnarray*"
                    | "split"
                    | "flalign"
                    | "flalign*"
                    | "gather"
                    | "gather*"
                    | "gathered"
                    | "multline"
                    | "multline*"
                    | "array"
            );

        if !is_matrix_like {
            let body = self.parse_sequence(StopCondition::EndEnvOnly, depth + 1);
            self.consume_matching_end(depth + 1);
            return MathNode::from_vec(body);
        }

        let mut rows: Vec<Vec<MathNode>> = Vec::new();
        let mut cur_row: Vec<MathNode> = Vec::new();
        let mut total_cells: usize = 0;

        while self.pos < self.tokens.len() && total_cells < MAX_TABLE_CELLS {
            let cell_nodes = self.parse_sequence(StopCondition::EndEnvCell, depth + 1);
            let cell = MathNode::from_vec(cell_nodes);
            cur_row.push(cell);
            total_cells += 1;

            self.skip_spaces();
            match self.peek() {
                Some(Token::Ampersand) => {
                    self.pos += 1;
                }
                Some(Token::Command(cmd)) if cmd == "\\" || cmd == "cr" => {
                    self.pos += 1;
                    // Optional `*` and `[length]` after `\\`
                    if matches!(self.peek(), Some(Token::Op('*'))) {
                        self.pos += 1;
                    }
                    self.skip_optional_bracket_tokens();
                    rows.push(std::mem::take(&mut cur_row));
                }
                Some(Token::Command(cmd)) if cmd == "end" => {
                    self.consume_matching_end(depth + 1);
                    break;
                }
                _ => break,
            }
        }

        if !cur_row.is_empty() {
            // Avoid pushing a phantom empty trailing row when input ended with `\\` before `\end`
            let all_empty = cur_row.iter().all(|n| matches!(n, MathNode::Empty));
            if !all_empty || rows.is_empty() {
                rows.push(cur_row);
            }
        }

        let column_align = explicit_align.unwrap_or(match env {
            "cases" | "cases*" | "dcases" | "dcases*" | "rcases" | "rcases*" => {
                TableColumnAlign::Left
            }
            "aligned" | "align" | "align*" | "split" | "flalign" | "flalign*" | "eqnarray"
            | "eqnarray*" => TableColumnAlign::AlternatingRightLeft,
            _ => TableColumnAlign::Center,
        });

        let table = MathNode::Table { rows, column_align };

        match env {
            "pmatrix" | "pmatrix*" => MathNode::Row(vec![
                MathNode::op_fence("(", true),
                table,
                MathNode::op_fence(")", true),
            ]),
            "bmatrix" | "bmatrix*" => MathNode::Row(vec![
                MathNode::op_fence("[", true),
                table,
                MathNode::op_fence("]", true),
            ]),
            "Bmatrix" | "Bmatrix*" => MathNode::Row(vec![
                MathNode::op_fence("{", true),
                table,
                MathNode::op_fence("}", true),
            ]),
            "vmatrix" | "vmatrix*" => MathNode::Row(vec![
                MathNode::op_fence("|", true),
                table,
                MathNode::op_fence("|", true),
            ]),
            "Vmatrix" | "Vmatrix*" => MathNode::Row(vec![
                MathNode::op_fence("\u{2016}", true),
                table,
                MathNode::op_fence("\u{2016}", true),
            ]),
            "cases" | "cases*" | "dcases" | "dcases*" => {
                MathNode::Row(vec![MathNode::op_fence("{", true), table])
            }
            "rcases" | "rcases*" => MathNode::Row(vec![table, MathNode::op_fence("}", true)]),
            "smallmatrix" | "smallmatrix*" => MathNode::Style {
                display_style: Some(false),
                script_level: Some("1"),
                child: Box::new(table),
            },
            _ => table,
        }
    }

    fn parse_optional_matrix_align(&mut self) -> Option<TableColumnAlign> {
        self.skip_spaces();
        if !matches!(self.peek(), Some(Token::LBracket)) {
            return None;
        }
        self.pos += 1;
        let mut align = None;
        while let Some(tok) = self.next() {
            match tok {
                Token::RBracket => break,
                Token::Ident('l') | Token::Ident('L') => align = Some(TableColumnAlign::Left),
                Token::Ident('r') | Token::Ident('R') => align = Some(TableColumnAlign::Right),
                Token::Ident('c') | Token::Ident('C') => align = Some(TableColumnAlign::Center),
                _ => {}
            }
        }
        align
    }

    fn consume_matching_end(&mut self, depth: usize) {
        self.skip_spaces();
        if let Some(Token::Command(cmd)) = self.peek()
            && cmd == "end"
        {
            self.pos += 1;
            let _ = self.parse_raw_text_arg(depth + 1);
        }
    }
}

fn wrap_style(
    child: MathNode,
    display_style: Option<bool>,
    script_level: Option<&'static str>,
) -> MathNode {
    if display_style.is_none() && script_level.is_none() {
        child
    } else {
        MathNode::Style {
            display_style,
            script_level,
            child: Box::new(child),
        }
    }
}

fn primes_to_unicode(count: usize) -> String {
    match count {
        0 => String::new(),
        1 => "\u{2032}".to_string(),
        2 => "\u{2033}".to_string(),
        3 => "\u{2034}".to_string(),
        4 => "\u{2057}".to_string(),
        n => "\u{2032}".repeat(n.min(8)),
    }
}

fn escape_text_command(cmd: &str) -> String {
    match cmd {
        "{" | "}" | "%" | "$" | "#" | "&" | "_" | " " => cmd.to_string(),
        "," | ";" | ":" | ">" | "thinspace" | "medspace" | "thickspace" => " ".to_string(),
        "quad" => "  ".to_string(),
        "qquad" => "    ".to_string(),
        "\\" => " ".to_string(),
        _ => {
            if let Some((sym, _)) = greek_letter(cmd) {
                sym.to_string()
            } else if let Some(sym) = math_symbol(cmd) {
                sym.to_string()
            } else if let Some((sym, _)) = misc_identifier_symbol(cmd) {
                sym.to_string()
            } else {
                format!("\\{cmd}")
            }
        }
    }
}

fn apply_font_to_node(node: MathNode, font: MathFont) -> MathNode {
    match node {
        MathNode::Ident { text, .. } => {
            if font == MathFont::Roman {
                MathNode::Ident { text, normal: true }
            } else if font == MathFont::Italic {
                MathNode::Ident {
                    text,
                    normal: false,
                }
            } else {
                MathNode::Ident {
                    text: map_math_font(&text, font),
                    normal: true,
                }
            }
        }
        MathNode::Number(n) => {
            if matches!(
                font,
                MathFont::Bold
                    | MathFont::Blackboard
                    | MathFont::SansSerif
                    | MathFont::Monospace
                    | MathFont::BoldItalic
            ) {
                MathNode::Number(map_math_font(&n, font))
            } else {
                MathNode::Number(n)
            }
        }
        MathNode::Row(items) => {
            let mut mapped: Vec<MathNode> = Vec::with_capacity(items.len());
            for item in items {
                let next = apply_font_to_node(item, font);
                if font != MathFont::Italic
                    && let MathNode::Ident {
                        ref text,
                        normal: true,
                    } = next
                    && let Some(MathNode::Ident {
                        text: prev_text,
                        normal: true,
                    }) = mapped.last_mut()
                {
                    prev_text.push_str(text);
                    continue;
                }
                mapped.push(next);
            }
            MathNode::from_vec(mapped)
        }
        MathNode::Sub { base, sub } => MathNode::Sub {
            base: Box::new(apply_font_to_node(*base, font)),
            sub,
        },
        MathNode::Sup { base, sup } => MathNode::Sup {
            base: Box::new(apply_font_to_node(*base, font)),
            sup,
        },
        MathNode::SubSup { base, sub, sup } => MathNode::SubSup {
            base: Box::new(apply_font_to_node(*base, font)),
            sub,
            sup,
        },
        MathNode::Over { base, over, accent } => MathNode::Over {
            base: Box::new(apply_font_to_node(*base, font)),
            over,
            accent,
        },
        MathNode::Under {
            base,
            under,
            accentunder,
        } => MathNode::Under {
            base: Box::new(apply_font_to_node(*base, font)),
            under,
            accentunder,
        },
        other => other,
    }
}

/// Map ASCII Latin letters, digits, and Greek letters to Unicode Mathematical Alphanumeric
/// Symbols (respecting every BMP Letterlike Symbols exception per Unicode / MathML Core).
fn map_math_font(input: &str, font: MathFont) -> String {
    let mut out = String::with_capacity(input.len() * 2);
    for ch in input.chars() {
        out.push(map_char_math_font(ch, font));
    }
    out
}

fn map_char_math_font(ch: char, font: MathFont) -> char {
    match font {
        MathFont::Roman | MathFont::Italic => ch,
        MathFont::Bold => match ch {
            'A'..='Z' => char::from_u32(0x1D400 + (ch as u32 - 'A' as u32)).unwrap_or(ch),
            'a'..='z' => char::from_u32(0x1D41A + (ch as u32 - 'a' as u32)).unwrap_or(ch),
            '0'..='9' => char::from_u32(0x1D7CE + (ch as u32 - '0' as u32)).unwrap_or(ch),
            _ => map_greek_bold(ch, false).unwrap_or(ch),
        },
        MathFont::BoldItalic => match ch {
            'A'..='Z' => char::from_u32(0x1D468 + (ch as u32 - 'A' as u32)).unwrap_or(ch),
            'a'..='z' => char::from_u32(0x1D482 + (ch as u32 - 'a' as u32)).unwrap_or(ch),
            '0'..='9' => char::from_u32(0x1D7CE + (ch as u32 - '0' as u32)).unwrap_or(ch),
            _ => map_greek_bold(ch, true).unwrap_or(ch),
        },
        MathFont::Blackboard => match ch {
            'C' => '\u{2102}', // ℂ
            'H' => '\u{210D}', // ℍ
            'N' => '\u{2115}', // ℕ
            'P' => '\u{2119}', // ℙ
            'Q' => '\u{211A}', // ℚ
            'R' => '\u{211D}', // ℝ
            'Z' => '\u{2124}', // ℤ
            'A'..='Z' => char::from_u32(0x1D538 + (ch as u32 - 'A' as u32)).unwrap_or(ch),
            'a'..='z' => char::from_u32(0x1D552 + (ch as u32 - 'a' as u32)).unwrap_or(ch),
            '0'..='9' => char::from_u32(0x1D7D8 + (ch as u32 - '0' as u32)).unwrap_or(ch),
            _ => ch,
        },
        MathFont::Calligraphic => match ch {
            'B' => '\u{212C}', // ℬ
            'E' => '\u{2130}', // ℰ
            'F' => '\u{2131}', // ℱ
            'H' => '\u{210B}', // ℋ
            'I' => '\u{2110}', // ℐ
            'L' => '\u{2112}', // ℒ
            'M' => '\u{2133}', // ℳ
            'R' => '\u{211B}', // ℛ
            'e' => '\u{212F}', // ℯ
            'g' => '\u{210A}', // ℊ
            'o' => '\u{2134}', // ℴ
            'A'..='Z' => char::from_u32(0x1D49C + (ch as u32 - 'A' as u32)).unwrap_or(ch),
            'a'..='z' => char::from_u32(0x1D4B6 + (ch as u32 - 'a' as u32)).unwrap_or(ch),
            _ => ch,
        },
        MathFont::SansSerif => match ch {
            'A'..='Z' => char::from_u32(0x1D5A0 + (ch as u32 - 'A' as u32)).unwrap_or(ch),
            'a'..='z' => char::from_u32(0x1D5BA + (ch as u32 - 'a' as u32)).unwrap_or(ch),
            '0'..='9' => char::from_u32(0x1D7E2 + (ch as u32 - '0' as u32)).unwrap_or(ch),
            _ => ch,
        },
        MathFont::Monospace => match ch {
            'A'..='Z' => char::from_u32(0x1D670 + (ch as u32 - 'A' as u32)).unwrap_or(ch),
            'a'..='z' => char::from_u32(0x1D68A + (ch as u32 - 'a' as u32)).unwrap_or(ch),
            '0'..='9' => char::from_u32(0x1D7F6 + (ch as u32 - '0' as u32)).unwrap_or(ch),
            _ => ch,
        },
        MathFont::Fraktur => match ch {
            'C' => '\u{212D}', // ℭ
            'H' => '\u{210C}', // ℌ
            'I' => '\u{2111}', // ℑ
            'R' => '\u{211C}', // ℜ
            'Z' => '\u{2128}', // ℨ
            'A'..='Z' => char::from_u32(0x1D504 + (ch as u32 - 'A' as u32)).unwrap_or(ch),
            'a'..='z' => char::from_u32(0x1D51E + (ch as u32 - 'a' as u32)).unwrap_or(ch),
            _ => ch,
        },
    }
}

fn map_greek_bold(ch: char, italic: bool) -> Option<char> {
    // Uppercase Greek Α (0x0391) .. Ω (0x03A9), skipping reserved 0x03A2.
    // In Unicode Mathematical Alphanumeric Symbols, U+1D6B9 (𝚹) / U+1D72D (𝜭) sits at
    // offset 17 (0x03A2 - 0x0391), so Σ..Ω map directly at `base + (code - 0x0391)`.
    let code = ch as u32;
    if (0x0391..=0x03A9).contains(&code) && code != 0x03A2 {
        let offset = code - 0x0391;
        let base = if italic { 0x1D71C } else { 0x1D6A8 };
        return char::from_u32(base + offset);
    }
    // Lowercase Greek α (0x03B1) .. ω (0x03C9)
    if (0x03B1..=0x03C9).contains(&code) {
        let offset = code - 0x03B1;
        let base = if italic { 0x1D736 } else { 0x1D6C2 };
        return char::from_u32(base + offset);
    }
    // Variant lowercase Greek
    match (ch, italic) {
        ('\u{03F5}', false) => Some('\u{1D6DC}'), // ϵ
        ('\u{03F5}', true) => Some('\u{1D750}'),
        ('\u{03D1}', false) => Some('\u{1D6DD}'), // ϑ
        ('\u{03D1}', true) => Some('\u{1D751}'),
        ('\u{03F0}', false) => Some('\u{1D6DE}'), // ϰ
        ('\u{03F0}', true) => Some('\u{1D752}'),
        ('\u{03D5}', false) => Some('\u{1D6DF}'), // ϕ
        ('\u{03D5}', true) => Some('\u{1D753}'),
        ('\u{03F1}', false) => Some('\u{1D6E0}'), // ϱ
        ('\u{03F1}', true) => Some('\u{1D754}'),
        ('\u{03D6}', false) => Some('\u{1D6E1}'), // ϖ
        ('\u{03D6}', true) => Some('\u{1D755}'),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Symbol & Command Tables
// ---------------------------------------------------------------------------

fn spacing_command_width(cmd: &str) -> Option<&'static str> {
    match cmd {
        "," | "thinspace" => Some("0.167em"),
        ":" | ">" | "medspace" => Some("0.222em"),
        ";" | "thickspace" => Some("0.278em"),
        "!" | "negthinspace" => Some("-0.167em"),
        " " => Some("0.25em"),
        "enspace" => Some("0.5em"),
        "quad" => Some("1em"),
        "qquad" => Some("2em"),
        _ => None,
    }
}

fn big_operator_symbol(cmd: &str) -> Option<&'static str> {
    match cmd {
        "sum" => Some("\u{2211}"),       // ∑
        "prod" => Some("\u{220F}"),      // ∏
        "coprod" => Some("\u{2210}"),    // ∐
        "int" => Some("\u{222B}"),       // ∫
        "iint" => Some("\u{222C}"),      // ∬
        "iiint" => Some("\u{222D}"),     // ∭
        "oint" => Some("\u{222E}"),      // ∮
        "bigcup" => Some("\u{22C3}"),    // ⋃
        "bigcap" => Some("\u{22C2}"),    // ⋂
        "bigsqcup" => Some("\u{2A06}"),  // ⨆
        "bigvee" => Some("\u{22C1}"),    // ⋁
        "bigwedge" => Some("\u{22C0}"),  // ⋀
        "bigoplus" => Some("\u{2A01}"),  // ⨁
        "bigotimes" => Some("\u{2A02}"), // ⨂
        "bigodot" => Some("\u{2A00}"),   // ⨀
        "biguplus" => Some("\u{2A04}"),  // ⨄
        _ => None,
    }
}

fn is_limit_operator_name(name: &str) -> bool {
    matches!(
        name,
        "lim"
            | "lim inf"
            | "lim sup"
            | "max"
            | "min"
            | "inf"
            | "sup"
            | "det"
            | "Pr"
            | "gcd"
            | "arg max"
            | "arg min"
    )
}

fn named_function_text(cmd: &str) -> Option<&'static str> {
    match cmd {
        "sin" => Some("sin"),
        "cos" => Some("cos"),
        "tan" => Some("tan"),
        "cot" => Some("cot"),
        "sec" => Some("sec"),
        "csc" => Some("csc"),
        "arcsin" => Some("arcsin"),
        "arccos" => Some("arccos"),
        "arctan" => Some("arctan"),
        "sinh" => Some("sinh"),
        "cosh" => Some("cosh"),
        "tanh" => Some("tanh"),
        "coth" => Some("coth"),
        "exp" => Some("exp"),
        "ln" => Some("ln"),
        "log" => Some("log"),
        "lg" => Some("lg"),
        "det" => Some("det"),
        "dim" => Some("dim"),
        "gcd" => Some("gcd"),
        "hom" => Some("hom"),
        "inf" => Some("inf"),
        "ker" => Some("ker"),
        "lim" => Some("lim"),
        "liminf" => Some("lim inf"),
        "limsup" => Some("lim sup"),
        "max" => Some("max"),
        "min" => Some("min"),
        "Pr" => Some("Pr"),
        "sup" => Some("sup"),
        "arg" => Some("arg"),
        "deg" => Some("deg"),
        "argmax" => Some("arg max"),
        "argmin" => Some("arg min"),
        _ => None,
    }
}

/// Returns `(unicode_symbol, is_upright_normal)`.
fn greek_letter(cmd: &str) -> Option<(&'static str, bool)> {
    match cmd {
        // Lowercase (italic in TeX)
        "alpha" => Some(("\u{03B1}", false)),
        "beta" => Some(("\u{03B2}", false)),
        "gamma" => Some(("\u{03B3}", false)),
        "delta" => Some(("\u{03B4}", false)),
        "epsilon" => Some(("\u{03F5}", false)),
        "varepsilon" => Some(("\u{03B5}", false)),
        "zeta" => Some(("\u{03B6}", false)),
        "eta" => Some(("\u{03B7}", false)),
        "theta" => Some(("\u{03B8}", false)),
        "vartheta" => Some(("\u{03D1}", false)),
        "iota" => Some(("\u{03B9}", false)),
        "kappa" => Some(("\u{03BA}", false)),
        "varkappa" => Some(("\u{03F0}", false)),
        "lambda" => Some(("\u{03BB}", false)),
        "mu" => Some(("\u{03BC}", false)),
        "nu" => Some(("\u{03BD}", false)),
        "xi" => Some(("\u{03BE}", false)),
        "omicron" => Some(("\u{03BF}", false)),
        "pi" => Some(("\u{03C0}", false)),
        "varpi" => Some(("\u{03D6}", false)),
        "rho" => Some(("\u{03C1}", false)),
        "varrho" => Some(("\u{03F1}", false)),
        "sigma" => Some(("\u{03C3}", false)),
        "varsigma" => Some(("\u{03C2}", false)),
        "tau" => Some(("\u{03C4}", false)),
        "upsilon" => Some(("\u{03C5}", false)),
        "phi" => Some(("\u{03D5}", false)),
        "varphi" => Some(("\u{03C6}", false)),
        "chi" => Some(("\u{03C7}", false)),
        "psi" => Some(("\u{03C8}", false)),
        "omega" => Some(("\u{03C9}", false)),
        // Uppercase (upright in standard TeX)
        "Gamma" => Some(("\u{0393}", true)),
        "Delta" => Some(("\u{0394}", true)),
        "Theta" => Some(("\u{0398}", true)),
        "Lambda" => Some(("\u{039B}", true)),
        "Xi" => Some(("\u{039E}", true)),
        "Pi" => Some(("\u{03A0}", true)),
        "Sigma" => Some(("\u{03A3}", true)),
        "Upsilon" => Some(("\u{03A5}", true)),
        "Phi" => Some(("\u{03A6}", true)),
        "Psi" => Some(("\u{03A8}", true)),
        "Omega" => Some(("\u{03A9}", true)),
        // Italic uppercase variants
        "varGamma" => Some(("\u{0393}", false)),
        "varDelta" => Some(("\u{0394}", false)),
        "varTheta" => Some(("\u{0398}", false)),
        "varLambda" => Some(("\u{039B}", false)),
        "varXi" => Some(("\u{039E}", false)),
        "varPi" => Some(("\u{03A0}", false)),
        "varSigma" => Some(("\u{03A3}", false)),
        "varUpsilon" => Some(("\u{03A5}", false)),
        "varPhi" => Some(("\u{03A6}", false)),
        "varPsi" => Some(("\u{03A8}", false)),
        "varOmega" => Some(("\u{03A9}", false)),
        _ => None,
    }
}

fn delimiter_command(cmd: &str) -> Option<&'static str> {
    match cmd {
        "{" | "lbrace" => Some("{"),
        "}" | "rbrace" => Some("}"),
        "|" | "Vert" | "lVert" | "rVert" => Some("\u{2016}"), // ‖
        "vert" | "lvert" | "rvert" => Some("|"),
        "lfloor" => Some("\u{230A}"), // ⌊
        "rfloor" => Some("\u{230B}"), // ⌋
        "lceil" => Some("\u{2308}"),  // ⌈
        "rceil" => Some("\u{2309}"),  // ⌉
        "langle" => Some("\u{27E8}"), // ⟨
        "rangle" => Some("\u{27E9}"), // ⟩
        "backslash" => Some("\\"),
        "uparrow" => Some("\u{2191}"),
        "downarrow" => Some("\u{2193}"),
        "updownarrow" => Some("\u{2195}"),
        "Uparrow" => Some("\u{21D1}"),
        "Downarrow" => Some("\u{21D3}"),
        "Updownarrow" => Some("\u{21D5}"),
        _ => None,
    }
}

fn math_symbol(cmd: &str) -> Option<&'static str> {
    match cmd {
        // Binary operators
        "pm" => Some("\u{00B1}"),                 // ±
        "mp" => Some("\u{2213}"),                 // ∓
        "times" => Some("\u{00D7}"),              // ×
        "div" => Some("\u{00F7}"),                // ÷
        "cdot" | "centerdot" => Some("\u{22C5}"), // ⋅
        "ast" => Some("\u{2217}"),                // ∗
        "star" => Some("\u{22C6}"),               // ⋆
        "circ" => Some("\u{2218}"),               // ∘
        "bullet" => Some("\u{2219}"),             // ∙
        "oplus" => Some("\u{2295}"),              // ⊕
        "ominus" => Some("\u{2296}"),             // ⊖
        "otimes" => Some("\u{2297}"),             // ⊗
        "oslash" => Some("\u{2298}"),             // ⊘
        "odot" => Some("\u{2299}"),               // ⊙
        "dagger" => Some("\u{2020}"),             // †
        "ddagger" => Some("\u{2021}"),            // ‡
        "setminus" => Some("\u{2216}"),           // ∖
        "wr" => Some("\u{2240}"),                 // ≀
        "diamond" => Some("\u{22C4}"),            // ⋄
        "triangleleft" => Some("\u{25C1}"),
        "triangleright" => Some("\u{25B7}"),
        "vee" | "lor" => Some("\u{2228}"),    // ∨
        "wedge" | "land" => Some("\u{2227}"), // ∧
        "cup" => Some("\u{222A}"),            // ∪
        "cap" => Some("\u{2229}"),            // ∩
        "sqcup" => Some("\u{2294}"),          // ⊔
        "sqcap" => Some("\u{2293}"),          // ⊓
        "uplus" => Some("\u{228E}"),          // ⊎
        "neg" | "lnot" => Some("\u{00AC}"),   // ¬
        // Relations
        "le" | "leq" => Some("\u{2264}"),  // ≤
        "ge" | "geq" => Some("\u{2265}"),  // ≥
        "ll" => Some("\u{226A}"),          // ≪
        "gg" => Some("\u{226B}"),          // ≫
        "neq" | "ne" => Some("\u{2260}"),  // ≠
        "approx" => Some("\u{2248}"),      // ≈
        "equiv" => Some("\u{2261}"),       // ≡
        "sim" => Some("\u{223C}"),         // ∼
        "simeq" => Some("\u{2243}"),       // ≃
        "cong" => Some("\u{2245}"),        // ≅
        "propto" => Some("\u{221D}"),      // ∝
        "perp" => Some("\u{22A5}"),        // ⊥
        "parallel" => Some("\u{2225}"),    // ∥
        "mid" => Some("\u{2223}"),         // ∣
        "nmid" => Some("\u{2224}"),        // ∤
        "in" => Some("\u{2208}"),          // ∈
        "notin" => Some("\u{2209}"),       // ∉
        "ni" | "owns" => Some("\u{220B}"), // ∋
        "subset" => Some("\u{2282}"),      // ⊂
        "subseteq" => Some("\u{2286}"),    // ⊆
        "supset" => Some("\u{2283}"),      // ⊃
        "supseteq" => Some("\u{2287}"),    // ⊇
        "subsetneq" => Some("\u{228A}"),   // ⊊
        "supsetneq" => Some("\u{228B}"),   // ⊋
        "nsubseteq" => Some("\u{2288}"),   // ⊈
        "nsupseteq" => Some("\u{2289}"),   // ⊉
        "sqsubset" => Some("\u{228F}"),    // ⊏
        "sqsubseteq" => Some("\u{2291}"),  // ⊑
        "sqsupset" => Some("\u{2290}"),    // ⊐
        "sqsupseteq" => Some("\u{2292}"),  // ⊒
        "prec" => Some("\u{227A}"),        // ≺
        "preceq" => Some("\u{2AAF}"),      // ⪯
        "succ" => Some("\u{227B}"),        // ≻
        "succeq" => Some("\u{2AB0}"),      // ⪰
        "vdash" => Some("\u{22A2}"),       // ⊢
        "dashv" => Some("\u{22A3}"),       // ⊣
        "models" => Some("\u{22A8}"),      // ⊨
        "doteq" => Some("\u{2250}"),       // ≐
        "asymp" => Some("\u{224D}"),       // ≍
        "bowtie" => Some("\u{22C8}"),      // ⋈
        "coloneqq" => Some("\u{2254}"),    // ≔
        "eqqcolon" => Some("\u{2255}"),    // ≕
        "triangleq" => Some("\u{225C}"),   // ≜
        "colon" => Some(":"),
        // Quantifiers & logic
        "forall" => Some("\u{2200}"),    // ∀
        "exists" => Some("\u{2203}"),    // ∃
        "nexists" => Some("\u{2204}"),   // ∄
        "top" => Some("\u{22A4}"),       // ⊤
        "bot" => Some("\u{22A5}"),       // ⊥
        "therefore" => Some("\u{2234}"), // ∴
        "because" => Some("\u{2235}"),   // ∵
        // Arrows
        "to" | "rightarrow" => Some("\u{2192}"),  // →
        "leftarrow" | "gets" => Some("\u{2190}"), // ←
        "leftrightarrow" => Some("\u{2194}"),     // ↔
        "Rightarrow" => Some("\u{21D2}"),         // ⇒
        "Leftarrow" => Some("\u{21D0}"),          // ⇐
        "Leftrightarrow" => Some("\u{21D4}"),     // ⇔
        "mapsto" => Some("\u{21A6}"),             // ↦
        "longmapsto" => Some("\u{27FC}"),         // ⟼
        "longrightarrow" => Some("\u{27F6}"),     // ⟶
        "longleftarrow" => Some("\u{27F5}"),      // ⟵
        "longleftrightarrow" => Some("\u{27F7}"), // ⟷
        "Longrightarrow" | "implies" => Some("\u{27F9}"), // ⟹
        "Longleftarrow" | "impliedby" => Some("\u{27F8}"), // ⟸
        "Longleftrightarrow" | "iff" => Some("\u{27FA}"), // ⟺
        "nearrow" => Some("\u{2197}"),            // ↗
        "searrow" => Some("\u{2198}"),            // ↘
        "swarrow" => Some("\u{2199}"),            // ↙
        "nwarrow" => Some("\u{2196}"),            // ↖
        "hookrightarrow" => Some("\u{21AA}"),     // ↪
        "hookleftarrow" => Some("\u{21A9}"),      // ↩
        "rightharpoonup" => Some("\u{21C0}"),     // ⇀
        "rightharpoondown" => Some("\u{21C1}"),   // ⇁
        "leftharpoonup" => Some("\u{21BC}"),      // ↼
        "leftharpoondown" => Some("\u{21BD}"),    // ↽
        "rightleftharpoons" => Some("\u{21CC}"),  // ⇌
        // Ellipses & punctuation
        "ldots" | "dots" | "dotsc" => Some("\u{2026}"), // …
        "cdots" | "dotsb" | "dotsi" | "dotsm" => Some("\u{22EF}"), // ⋯
        "vdots" => Some("\u{22EE}"),                    // ⋮
        "ddots" => Some("\u{22F1}"),                    // ⋱
        "prime" => Some("\u{2032}"),                    // ′
        "degree" => Some("\u{00B0}"),                   // °
        "angle" => Some("\u{2220}"),                    // ∠
        "triangle" => Some("\u{25B3}"),                 // △
        "square" | "Box" => Some("\u{25A1}"),           // □
        "blacksquare" => Some("\u{25A0}"),              // ■
        "checkmark" => Some("\u{2713}"),                // ✓
        _ => None,
    }
}

fn misc_identifier_symbol(cmd: &str) -> Option<(&'static str, bool)> {
    match cmd {
        "infty" => Some(("\u{221E}", true)),      // ∞
        "partial" => Some(("\u{2202}", true)),    // ∂
        "nabla" => Some(("\u{2207}", true)),      // ∇
        "emptyset" => Some(("\u{2205}", true)),   // ∅
        "varnothing" => Some(("\u{2205}", true)), // ∅
        "aleph" => Some(("\u{2135}", true)),      // ℵ
        "hbar" => Some(("\u{210F}", false)),      // ℏ
        "ell" => Some(("\u{2113}", false)),       // ℓ
        "wp" => Some(("\u{2118}", false)),        // ℘
        "Re" => Some(("\u{211C}", true)),         // ℜ
        "Im" => Some(("\u{2111}", true)),         // ℑ
        "imath" => Some(("\u{0131}", false)),     // ı
        "jmath" => Some(("\u{0237}", false)),     // ȷ
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Strict XML-Escaped MathML Core Emitter
// ---------------------------------------------------------------------------

fn emit_node(node: &MathNode, out: &mut String) {
    match node {
        MathNode::Empty => out.push_str("<mrow/>"),
        MathNode::Number(n) => {
            out.push_str("<mn>");
            push_xml_escaped(out, n);
            out.push_str("</mn>");
        }
        MathNode::Ident { text, normal } => {
            if *normal {
                out.push_str("<mi mathvariant=\"normal\">");
            } else {
                out.push_str("<mi>");
            }
            push_xml_escaped(out, text);
            out.push_str("</mi>");
        }
        MathNode::Op {
            text,
            stretchy,
            fence,
            movablelimits,
            largeop,
        } => {
            out.push_str("<mo");
            if let Some(st) = stretchy {
                if *st {
                    out.push_str(" stretchy=\"true\"");
                } else {
                    out.push_str(" stretchy=\"false\"");
                }
            }
            if *fence {
                out.push_str(" fence=\"true\"");
            }
            if let Some(ml) = movablelimits {
                if *ml {
                    out.push_str(" movablelimits=\"true\"");
                } else {
                    out.push_str(" movablelimits=\"false\"");
                }
            }
            if *largeop {
                out.push_str(" largeop=\"true\"");
            }
            out.push('>');
            push_xml_escaped(out, text);
            out.push_str("</mo>");
        }
        MathNode::Text { text, bold, italic } => {
            out.push_str("<mtext");
            match (*bold, *italic) {
                (true, true) => out.push_str(" style=\"font-weight:700;font-style:italic\""),
                (true, false) => out.push_str(" style=\"font-weight:700\""),
                (false, true) => out.push_str(" style=\"font-style:italic\""),
                (false, false) => {}
            }
            out.push('>');
            // Preserve edge spaces inside `<mtext>` using NBSP so browsers keep word gaps
            let preserved = preserve_edge_spaces(text);
            push_xml_escaped(out, &preserved);
            out.push_str("</mtext>");
        }
        MathNode::Space(width) => {
            let _ = write!(out, "<mspace width=\"{width}\"/>");
        }
        MathNode::Row(items) => {
            out.push_str("<mrow>");
            for item in items {
                emit_node(item, out);
            }
            out.push_str("</mrow>");
        }
        MathNode::Frac {
            num,
            den,
            linethickness,
        } => {
            if let Some(lt) = linethickness {
                let _ = write!(out, "<mfrac linethickness=\"{lt}\">");
            } else {
                out.push_str("<mfrac>");
            }
            emit_node(num, out);
            emit_node(den, out);
            out.push_str("</mfrac>");
        }
        MathNode::Sqrt(rad) => {
            out.push_str("<msqrt>");
            emit_node(rad, out);
            out.push_str("</msqrt>");
        }
        MathNode::Root { radicand, index } => {
            // MathML `<mroot>` takes base (radicand) first, then index!
            out.push_str("<mroot>");
            emit_node(radicand, out);
            emit_node(index, out);
            out.push_str("</mroot>");
        }
        MathNode::Sub { base, sub } => {
            out.push_str("<msub>");
            emit_node(base, out);
            emit_node(sub, out);
            out.push_str("</msub>");
        }
        MathNode::Sup { base, sup } => {
            out.push_str("<msup>");
            emit_node(base, out);
            emit_node(sup, out);
            out.push_str("</msup>");
        }
        MathNode::SubSup { base, sub, sup } => {
            out.push_str("<msubsup>");
            emit_node(base, out);
            emit_node(sub, out);
            emit_node(sup, out);
            out.push_str("</msubsup>");
        }
        MathNode::Under {
            base,
            under,
            accentunder,
        } => {
            if *accentunder {
                out.push_str("<munder accentunder=\"true\">");
            } else {
                out.push_str("<munder>");
            }
            emit_node(base, out);
            emit_node(under, out);
            out.push_str("</munder>");
        }
        MathNode::Over { base, over, accent } => {
            if *accent {
                out.push_str("<mover accent=\"true\">");
            } else {
                out.push_str("<mover>");
            }
            emit_node(base, out);
            emit_node(over, out);
            out.push_str("</mover>");
        }
        MathNode::UnderOver { base, under, over } => {
            out.push_str("<munderover>");
            emit_node(base, out);
            emit_node(under, out);
            emit_node(over, out);
            out.push_str("</munderover>");
        }
        MathNode::Table { rows, column_align } => {
            match column_align {
                TableColumnAlign::Center => out.push_str("<mtable>"),
                TableColumnAlign::Left => out.push_str("<mtable columnalign=\"left\">"),
                TableColumnAlign::Right => out.push_str("<mtable columnalign=\"right\">"),
                TableColumnAlign::AlternatingRightLeft => {
                    out.push_str("<mtable columnalign=\"right left\">");
                }
            }
            for row in rows {
                out.push_str("<mtr>");
                for (col_idx, cell) in row.iter().enumerate() {
                    let cell_align_style = match column_align {
                        TableColumnAlign::Center => None,
                        TableColumnAlign::Left => Some("text-align:left"),
                        TableColumnAlign::Right => Some("text-align:right"),
                        TableColumnAlign::AlternatingRightLeft => {
                            if col_idx % 2 == 0 {
                                Some("text-align:right")
                            } else {
                                Some("text-align:left")
                            }
                        }
                    };
                    if let Some(style) = cell_align_style {
                        let _ = write!(out, "<mtd style=\"{style}\">");
                    } else {
                        out.push_str("<mtd>");
                    }
                    if !matches!(cell, MathNode::Empty) {
                        emit_node(cell, out);
                    }
                    out.push_str("</mtd>");
                }
                out.push_str("</mtr>");
            }
            out.push_str("</mtable>");
        }
        MathNode::Style {
            display_style,
            script_level,
            child,
        } => {
            out.push_str("<mstyle");
            if let Some(ds) = display_style {
                if *ds {
                    out.push_str(" displaystyle=\"true\"");
                } else {
                    out.push_str(" displaystyle=\"false\"");
                }
            }
            if let Some(sl) = script_level {
                let _ = write!(out, " scriptlevel=\"{sl}\"");
            }
            out.push('>');
            emit_node(child, out);
            out.push_str("</mstyle>");
        }
        MathNode::Phantom(child) => {
            out.push_str("<mphantom>");
            emit_node(child, out);
            out.push_str("</mphantom>");
        }
    }
}

fn preserve_edge_spaces(text: &str) -> String {
    if !text.starts_with(' ') && !text.ends_with(' ') {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len() + 4);
    let chars: Vec<char> = text.chars().collect();
    for (idx, &ch) in chars.iter().enumerate() {
        if ch == ' ' && (idx == 0 || idx + 1 == chars.len()) {
            out.push('\u{00A0}');
        } else {
            out.push(ch);
        }
    }
    out
}

fn push_xml_escaped(out: &mut String, raw: &str) {
    for ch in raw.chars() {
        let cp = ch as u32;
        // Skip illegal XML 1.0 C0 control characters (`\t`, `\n`, `\r` remain legal)
        if cp <= 0x08 || cp == 0x0B || cp == 0x0C || (0x0E..=0x1F).contains(&cp) {
            continue;
        }
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_inline_and_display_math_root_attributes() {
        let inline = render_latex_to_mathml("E = mc^2", MathDisplay::Inline);
        assert!(
            inline.starts_with(
                "<math xmlns=\"http://www.w3.org/1998/Math/MathML\" display=\"inline\">"
            )
        );
        assert!(inline.ends_with("</math>"));
        assert!(inline.contains("<mi>E</mi>"));
        assert!(inline.contains("<mo>=</mo>"));
        assert!(inline.contains("<msup><mi>c</mi><mn>2</mn></msup>"));

        let block = render_latex_to_mathml(r"a^2 + b^2 = c^2", MathDisplay::Block);
        assert!(
            block.starts_with(
                "<math xmlns=\"http://www.w3.org/1998/Math/MathML\" display=\"block\">"
            )
        );
        assert!(block.contains("<msup><mi>a</mi><mn>2</mn></msup>"));
    }

    #[test]
    fn renders_fractions_binomials_and_nested_roots() {
        let quadratic = render_latex_to_mathml(
            r"x = \frac{-b \pm \sqrt{b^2 - 4ac}}{2a}",
            MathDisplay::Block,
        );
        assert!(quadratic.contains("<mfrac>"));
        assert!(quadratic.contains("<msqrt>"));
        assert!(quadratic.contains("<mo>±</mo>"));
        assert!(quadratic.contains("<mo>−</mo>"));

        let nth_root = render_latex_to_mathml(r"\sqrt[3]{\sqrt{x + 1}}", MathDisplay::Inline);
        assert!(nth_root.contains("<mroot><msqrt>"));
        assert!(nth_root.contains("</msqrt><mn>3</mn></mroot>"));

        let binom = render_latex_to_mathml(r"\binom{n}{k}", MathDisplay::Inline);
        assert!(binom.contains("<mfrac linethickness=\"0\"><mi>n</mi><mi>k</mi></mfrac>"));
        assert!(binom.contains("stretchy=\"true\" fence=\"true\">(</mo>"));

        let dfrac = render_latex_to_mathml(r"\dfrac{1}{2} + \tfrac{3}{4}", MathDisplay::Inline);
        assert!(dfrac.contains("displaystyle=\"true\""));
        assert!(dfrac.contains("displaystyle=\"false\""));
    }

    #[test]
    fn renders_scripts_and_big_operator_underover_limits() {
        let subsup_a = render_latex_to_mathml("x_i^2", MathDisplay::Inline);
        let subsup_b = render_latex_to_mathml("x^2_i", MathDisplay::Inline);
        assert_eq!(subsup_a, subsup_b);
        assert!(subsup_a.contains("<msubsup><mi>x</mi><mi>i</mi><mn>2</mn></msubsup>"));

        let sum = render_latex_to_mathml(r"\sum_{i=1}^{n} i^2", MathDisplay::Block);
        assert!(sum.contains("<munderover>"));
        assert!(sum.contains("∑"));
        assert!(sum.contains("movablelimits=\"true\""));

        let integral = render_latex_to_mathml(r"\int_0^\infty e^{-x} dx", MathDisplay::Block);
        assert!(integral.contains("<munderover>"));
        assert!(integral.contains("∫"));
        assert!(integral.contains("∞"));

        let limit = render_latex_to_mathml(r"\lim_{x \to 0} \frac{\sin x}{x}", MathDisplay::Block);
        assert!(limit.contains("<munder>"));
        assert!(limit.contains("lim"));
        assert!(limit.contains("→"));
        assert!(limit.contains("<mi mathvariant=\"normal\">sin</mi>"));

        let union = render_latex_to_mathml(r"\bigcup_{i=1}^{k} A_i", MathDisplay::Block);
        assert!(union.contains("<munderover>"));
        assert!(union.contains("⋃"));
    }

    #[test]
    fn renders_matrices_cases_and_aligned_environments() {
        let pmat = render_latex_to_mathml(
            r"\begin{pmatrix} 1 & 2 \\ 3 & 4 \end{pmatrix}",
            MathDisplay::Block,
        );
        assert!(pmat.contains("<mtable>"));
        assert!(pmat.contains("<mtr><mtd><mn>1</mn></mtd><mtd><mn>2</mn></mtd></mtr>"));
        assert!(pmat.contains("<mtr><mtd><mn>3</mn></mtd><mtd><mn>4</mn></mtd></mtr>"));
        assert!(pmat.contains("fence=\"true\">(</mo>"));
        assert!(pmat.contains("fence=\"true\">)</mo>"));

        let cases = render_latex_to_mathml(
            r"|x| = \begin{cases} x & \text{if } x \ge 0 \\ -x & \text{otherwise} \end{cases}",
            MathDisplay::Block,
        );
        assert!(cases.contains("fence=\"true\">{</mo>"));
        assert!(cases.contains("<mtable columnalign=\"left\">"));
        assert!(cases.contains("<mtext>if\u{00A0}</mtext>"));
        assert!(cases.contains("≥"));

        let aligned = render_latex_to_mathml(
            r"\begin{aligned} f(x) &= x^2 + 1 \\ g(x) &= 2x \end{aligned}",
            MathDisplay::Block,
        );
        assert!(aligned.contains("<mtable columnalign=\"right left\">"));
        assert_eq!(aligned.matches("<mtr>").count(), 2);
    }

    #[test]
    fn renders_greek_operators_fonts_accents_and_delimiters() {
        let greek = render_latex_to_mathml(
            r"\alpha + \beta \cdot \Gamma - \varepsilon \varphi \vartheta \varrho \le \Omega",
            MathDisplay::Inline,
        );
        for expected in ["α", "β", "⋅", "Γ", "ε", "φ", "ϑ", "ϱ", "≤", "Ω"] {
            assert!(greek.contains(expected), "missing {expected} in {greek}");
        }

        let fonts = render_latex_to_mathml(
            r"\mathbb{R}^n \times \mathbb{C} \ni \mathbf{v}, \mathcal{L}, \mathsf{T}, \mathtt{id}, \boldsymbol{\theta}",
            MathDisplay::Inline,
        );
        assert!(fonts.contains("ℝ"), "expected ℝ in {fonts}");
        assert!(fonts.contains("ℂ"), "expected ℂ in {fonts}");
        assert!(fonts.contains("𝐯"), "expected bold v in {fonts}");
        assert!(fonts.contains("ℒ"), "expected calligraphic L in {fonts}");
        assert!(fonts.contains("𝖳"), "expected sans-serif T in {fonts}");
        assert!(fonts.contains("𝚒𝚍"), "expected monospace id in {fonts}");
        assert!(fonts.contains("𝜽"), "expected bold italic theta in {fonts}");

        let accents = render_latex_to_mathml(
            r"\hat{y} + \bar{x} + \overline{AB} + \underline{u} + \vec{v} + \tilde{z} + \dot{q} + \ddot{q} + \widehat{xyz}",
            MathDisplay::Inline,
        );
        assert!(accents.contains("<mover accent=\"true\">"));
        assert!(accents.contains("<munder>"));
        assert!(accents.contains("→"));
        assert!(accents.contains("¨"));

        let delims = render_latex_to_mathml(
            r"\left\lfloor \frac{x}{2} \right\rfloor + \left\langle u, v \right\rangle + \left\{ a \mid b \right\}",
            MathDisplay::Inline,
        );
        for d in ["⌊", "⌋", "⟨", "⟩", "{", "}", "∣"] {
            assert!(delims.contains(d), "missing delimiter {d} in {delims}");
        }
    }

    #[test]
    fn preprocessor_converts_latex_delimiters_and_skips_code_and_escapes() {
        let md = r#"Inline \(E = mc^2\) and display:

\[
\frac{a}{b}
\]

Escaped \\(not math\\) and \$5 and `\(inline code\)`:

```tex
\(inside fenced code\)
\[x + y\]
```

~~~python
x = "\(also fenced\)"
~~~
"#;
        let out = preprocess_markdown_math(md);
        assert!(out.contains("Inline $E = mc^2$ and display:"));
        assert!(out.contains("$$\\frac{a}{b}$$"));
        assert!(out.contains("Escaped \\\\(not math\\\\)"));
        assert!(out.contains("\\$5"));
        assert!(out.contains("`\\(inline code\\)`"));
        assert!(out.contains("\\(inside fenced code\\)"));
        assert!(out.contains("\\[x + y\\]"));
        assert!(out.contains("\\(also fenced\\)"));
    }

    #[test]
    fn strictly_escapes_xss_payloads_and_caps_recursion_and_size() {
        let xss = render_latex_to_mathml(
            r#"\text{<script>alert("xss")</script>} + \operatorname{a&b'c} + \unknown<img/onerror=1>"#,
            MathDisplay::Inline,
        );
        assert!(!xss.contains("<script"));
        assert!(!xss.contains("<img"));
        assert!(xss.contains("&lt;script&gt;alert(&quot;xss&quot;)&lt;/script&gt;"));
        assert!(xss.contains("a&amp;b&#39;c"));

        // Deeply nested groups must never stack overflow
        let deep = format!("{}x{}", "{".repeat(500), "}".repeat(500));
        let rendered_deep = render_latex_to_mathml(&deep, MathDisplay::Inline);
        assert!(rendered_deep.starts_with("<math "));
        assert!(rendered_deep.ends_with("</math>"));

        // Oversized input must truncate safely
        let huge = "x + ".repeat(30_000);
        let rendered_huge = render_latex_to_mathml(&huge, MathDisplay::Block);
        assert!(rendered_huge.starts_with("<math "));
        assert!(rendered_huge.contains("<mtext>"));
        assert!(rendered_huge.len() < 1024);

        // Empty & malformed inputs never panic
        assert!(render_latex_to_mathml("", MathDisplay::Inline).ends_with("</math>"));
        assert!(render_latex_to_mathml(r"\frac{1}", MathDisplay::Inline).ends_with("</math>"));
        assert!(render_latex_to_mathml(r"\left( x + 1", MathDisplay::Inline).ends_with("</math>"));
        assert!(
            render_latex_to_mathml(r"\begin{pmatrix} 1 & 2", MathDisplay::Block)
                .ends_with("</math>")
        );

        // Illegal XML 1.0 C0 control chars must be stripped cleanly
        let ctrl = render_latex_to_mathml("\\text{a\x00b\x07c\x1Fd}", MathDisplay::Inline);
        assert!(ctrl.contains("<mtext>abcd</mtext>"), "got: {ctrl}");
    }

    #[test]
    fn bold_uppercase_greek_starred_matrices_cell_align_and_linear_preprocessor() {
        // P0 #2: exact Unicode Mathematical Alphanumeric codepoints for bold & bold-italic Σ / Ω
        let bold_greek = render_latex_to_mathml(
            r"\mathbf{\Sigma} + \mathbf{\Omega} + \boldsymbol{\Sigma} + \boldsymbol{\Omega}",
            MathDisplay::Inline,
        );
        assert!(
            bold_greek.contains('\u{1D6BA}'),
            "expected bold Σ (U+1D6BA) in {bold_greek}"
        );
        assert!(
            bold_greek.contains('\u{1D6C0}'),
            "expected bold Ω (U+1D6C0) in {bold_greek}"
        );
        assert!(
            bold_greek.contains('\u{1D72E}'),
            "expected bold-italic Σ (U+1D72E) in {bold_greek}"
        );
        assert!(
            bold_greek.contains('\u{1D734}'),
            "expected bold-italic Ω (U+1D734) in {bold_greek}"
        );

        // P1 #5 & #9: MathML Core per-cell `text-align` + starred matrices + non-matrix env preservation
        let starred = render_latex_to_mathml(
            r"\begin{pmatrix*}[r] -1 & 2 \\ 3 & -4 \end{pmatrix*}",
            MathDisplay::Block,
        );
        assert!(starred.contains("fence=\"true\">(</mo>"));
        assert!(starred.contains("<mtd style=\"text-align:right\">"));

        let aligned = render_latex_to_mathml(
            r"\begin{aligned} a &= b \\ c &= d \end{aligned}",
            MathDisplay::Block,
        );
        assert!(aligned.contains("<mtd style=\"text-align:right\">"));
        assert!(aligned.contains("<mtd style=\"text-align:left\">"));

        let non_matrix = render_latex_to_mathml(
            r"\begin{equation} a + b \\ c + d \end{equation}",
            MathDisplay::Block,
        );
        assert!(non_matrix.contains("<mi>c</mi>"));
        assert!(non_matrix.contains("<mi>d</mi>"));

        // P2 #12: Currency dollar amounts must not be corrupted by preprocessor
        let currency = "It costs $5 and $10, plus \\(x + 1\\).";
        let pre_currency = preprocess_markdown_math(currency);
        assert_eq!(pre_currency, "It costs $5 and $10, plus $x + 1$.");

        // P0 #4: O(N) preprocessor stress test on 20,000 unclosed delimiters
        let unclosed_display = "\\[".repeat(20_000);
        let unclosed_inline = "\\(".repeat(20_000);
        let unclosed_ticks = "`".repeat(1_000) + &" \\(".repeat(5_000);
        let start = std::time::Instant::now();
        assert_eq!(
            preprocess_markdown_math(&unclosed_display),
            unclosed_display
        );
        assert_eq!(preprocess_markdown_math(&unclosed_inline), unclosed_inline);
        assert_eq!(preprocess_markdown_math(&unclosed_ticks), unclosed_ticks);
        assert!(
            start.elapsed() < std::time::Duration::from_secs(2),
            "preprocessor must run in strictly O(N) time"
        );
    }
}
