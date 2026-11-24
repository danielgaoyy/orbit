//! Discovery rules and independent target-source checks.
use super::repository::File;
use orbit_protocol::{PrPatrolCategory as Category, PrPatrolSeverity as Severity};
use regex::{Regex, RegexSet};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Rule {
    Secret,
    Unsafe,
    Panic,
    Sql,
    Shell,
    IgnoredIo,
}

impl Rule {
    pub fn description(self) -> (Severity, Category, &'static str, &'static str, &'static str) {
        match self {
            Self::Secret => (
                Severity::P0,
                Category::Security,
                "Hardcoded credential",
                "The changed line contains a literal credential.",
                "Remove the literal, rotate the credential, and load its replacement from a secret store.",
            ),
            Self::Unsafe => (
                Severity::P1,
                Category::Correctness,
                "Unsafe block violates repository policy",
                "This Rust block conflicts with the repository's unsafe_code lint.",
                "Use a safe API or move the operation behind an existing safe abstraction.",
            ),
            Self::Panic => (
                Severity::P1,
                Category::Correctness,
                "Production code can panic",
                "The changed call panics on None or Err.",
                "Propagate the error or handle the empty case explicitly.",
            ),
            Self::Sql => (
                Severity::P1,
                Category::Security,
                "Runtime value interpolated into SQL",
                "The statement constructs SQL with a runtime value in the query text.",
                "Bind data values through the database driver's parameter API.",
            ),
            Self::Shell => (
                Severity::P1,
                Category::Security,
                "Dynamic shell command",
                "A runtime expression supplies command text to a shell.",
                "Pass a fixed executable and separate arguments to the process API.",
            ),
            Self::IgnoredIo => (
                Severity::P2,
                Category::Correctness,
                "I/O failure discarded",
                "This write, flush or sync discards its error result.",
                "Handle or propagate the I/O error before reporting success.",
            ),
        }
    }
}

const SECRET: &str = r#"(?ix)(?:\b(?:gh[pousr]_[A-Za-z0-9]{20,}|github_pat_[A-Za-z0-9_]{32,}|sk-[A-Za-z0-9_-]{20,}|AKIA[A-Z0-9]{16})\b|\b(?:[a-z0-9_]*(?:api[_-]?key|secret|password|access[_-]?token)|token)\b["']?\s*(?::\s*[^=;"']{1,40})?\s*[:=]\s*["'][^"'\s$]{8,}["'])"#;
const SQL: &str = r#"(?ix)(?:(?:format!\s*\(\s*"|\bf["']|`)(?:\s*)(?:SELECT\b|UPDATE\b|DELETE\s+FROM\b|INSERT\s+INTO\b)[^\n;]*\{[^}]*\}|["']\s*(?:SELECT\b|UPDATE\b|DELETE\s+FROM\b|INSERT\s+INTO\b)[^\n;]*(?:["']\s*\+\s*\w|%s["']\s*%))"#;
const SHELL: &str = r#"(?x)(?:os\.system|child_process\.exec|subprocess\.getoutput)\s*\(\s*(?:[a-zA-Z_]\w*\s*[,)]|f["']|`[^\x60]*\$\{|["'][^"']*["']\s*\+)|\.arg\("-c"\)\s*\.arg\(\s*(?:format!|[a-zA-Z_]\w*\s*\))"#;
const IGNORED_IO: &str =
    r"\blet\s+_\s*=\s*[^;]*(?:\.write_all|\.flush|\.sync_all)\s*\([^;]*\)\s*(?:\.await)?\s*;";

pub(super) struct Scanner {
    patterns: RegexSet,
    secret: Regex,
}
impl Scanner {
    pub fn new() -> Result<Self, regex::Error> {
        Ok(Self {
            patterns: RegexSet::new([SECRET, SQL, SHELL, IGNORED_IO])?,
            secret: Regex::new(SECRET)?,
        })
    }
    pub fn redact(&self, text: &str) -> String {
        self.secret
            .replace_all(text, "[redacted credential]")
            .into_owned()
    }
    pub fn rules(&self, path: &str, line: &str) -> Vec<Rule> {
        let ext = Path::new(path)
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("");
        let trimmed = line.trim_start();
        if ["//", "/*", "* ", "# ", "-- "]
            .iter()
            .any(|prefix| trimmed.starts_with(prefix))
        {
            return vec![];
        }
        let mut found = Vec::new();
        for idx in self.patterns.matches(line).iter() {
            let rule = match idx {
                0 => Rule::Secret,
                1 => Rule::Sql,
                2 => Rule::Shell,
                _ => Rule::IgnoredIo,
            };
            if (rule == Rule::Secret
                || matches!(
                    ext,
                    "rs" | "py" | "go" | "js" | "ts" | "tsx" | "jsx" | "rb" | "php" | "java" | "cs"
                ))
                && (rule != Rule::IgnoredIo || ext == "rs")
            {
                found.push(rule);
            }
        }
        found
    }
}

pub(super) fn deny_unsafe(manifest: &str) -> bool {
    let Ok(value) = toml::from_str::<toml::Value>(manifest) else {
        return false;
    };
    [Some(&value), value.get("workspace")]
        .into_iter()
        .flatten()
        .any(|root| {
            let lint = root
                .get("lints")
                .and_then(|l| l.get("rust"))
                .and_then(|r| r.get("unsafe_code"));
            matches!(
                lint.and_then(|v| v.as_str().or_else(|| v.get("level")?.as_str())),
                Some("deny" | "forbid")
            )
        })
}

struct Token<'a> {
    text: &'a str,
    line: u32,
    end: usize,
    parent: Option<usize>,
}

fn split_top_level_commas(input: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    let mut start = 0usize;
    for (idx, ch) in input.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }
        match ch {
            '"' => in_string = true,
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                parts.push(&input[start..idx]);
                start = idx + 1;
            }
            _ => {}
        }
    }
    parts.push(&input[start..]);
    parts
}

fn is_test_cfg_expr(expr: &str) -> bool {
    if expr == "test" {
        return true;
    }
    if let Some(inner) = expr.strip_prefix("all(").and_then(|s| s.strip_suffix(')')) {
        return split_top_level_commas(inner)
            .into_iter()
            .any(is_test_cfg_expr);
    }
    if let Some(inner) = expr.strip_prefix("any(").and_then(|s| s.strip_suffix(')')) {
        return split_top_level_commas(inner)
            .into_iter()
            .any(is_test_cfg_expr);
    }
    false
}

fn is_test_attribute(attr: &str) -> bool {
    if attr == "test" || attr.ends_with("::test") {
        return true;
    }
    if let Some((path, rest)) = attr.split_once('(') {
        let Some(inner) = rest.strip_suffix(')') else {
            return false;
        };
        if path == "test" || path.ends_with("::test") {
            return true;
        }
        if path == "cfg" {
            return is_test_cfg_expr(inner);
        }
    }
    false
}

pub(super) fn rust_context(source: &str, mut deny: bool, file: &mut File) {
    use rustc_lexer::TokenKind;
    let (mut line, mut pos) = (1, 0);
    let mut stack = Vec::new();
    let mut tokens: Vec<Token> = Vec::new();
    for tok in rustc_lexer::tokenize(source) {
        let text = &source[pos..pos + tok.len];
        if !matches!(
            tok.kind,
            TokenKind::Whitespace | TokenKind::LineComment | TokenKind::BlockComment { .. }
        ) {
            let i = tokens.len();
            let parent = stack.last().copied();
            if matches!(text, "(" | "[" | "{") {
                stack.push(i);
            } else if matches!(text, ")" | "]" | "}")
                && let Some(begin) = stack.pop()
            {
                tokens[begin].end = i;
            }
            tokens.push(Token {
                text,
                line,
                end: i,
                parent,
            });
        }
        pos += tok.len;
        line += text.bytes().filter(|b| *b == b'\n').count() as u32;
    }
    // Precompute item ends once, including arbitrarily long chains of attributes.
    let mut item_ends = vec![None; tokens.len() + 1];
    for (i, t) in tokens.iter().enumerate().rev() {
        item_ends[i] = if matches!(t.text, "{" | ";") {
            Some(tokens[t.end].line)
        } else {
            item_ends[t.end + 1]
        };
    }
    let at = |i| tokens.get(i).map(|t: &Token| t.text);
    for (i, t) in tokens.iter().enumerate().filter(|(_, t)| t.text == "#") {
        let inner = at(i + 1) == Some("!");
        let bracket = i + 1 + usize::from(inner);
        if at(bracket) != Some("[") {
            continue;
        }
        let end = tokens[bracket].end;
        if end <= bracket || end - bracket > 96 {
            continue;
        }
        let attr: String = tokens[bracket + 1..end].iter().map(|t| t.text).collect();
        if inner && matches!(attr.as_str(), "deny(unsafe_code)" | "forbid(unsafe_code)") {
            deny = true;
        }
        if is_test_attribute(&attr) {
            let end_line = if inner {
                Some(
                    t.parent
                        .and_then(|parent| tokens.get(tokens[parent].end))
                        .map_or(line, |close| close.line),
                )
            } else {
                item_ends[end + 1]
            };
            if let Some(last) = end_line {
                if let Some(range) = file.test_ranges.last_mut()
                    && t.line <= *range.end()
                {
                    *range = *range.start()..=last.max(*range.end());
                } else {
                    file.test_ranges.push(t.line..=last);
                }
            }
        }
    }
    for (i, t) in tokens.iter().enumerate() {
        let is_unsafe_attr = at(i + 1) == Some("(")
            && ((i >= 2 && at(i - 1) == Some("[") && at(i - 2) == Some("#"))
                || (i >= 3
                    && at(i - 1) == Some("[")
                    && at(i - 2) == Some("!")
                    && at(i - 3) == Some("#")));
        if deny
            && t.text == "unsafe"
            && (matches!(at(i + 1), Some("{" | "fn" | "impl" | "trait" | "extern"))
                || is_unsafe_attr)
        {
            file.rust_signals.insert((t.line, Rule::Unsafe));
        }
        if i > 0
            && at(i - 1) == Some(".")
            && matches!(t.text, "unwrap" | "expect")
            && at(i + 1) == Some("(")
        {
            file.rust_signals.insert((t.line, Rule::Panic));
        }
    }
}

pub(super) fn is_test(path: &str, file: &File, start: u32, end: u32) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    path.split('/')
        .any(|s| matches!(s, "tests" | "__tests__" | "benches"))
        || name == "tests.rs"
        || name.starts_with("test_")
        || name.ends_with("_test.go")
        || name.contains(".test.")
        || name.contains(".spec.")
        || file
            .test_ranges
            .get(file.test_ranges.partition_point(|r| *r.end() < start))
            .is_some_and(|r| *r.start() <= end)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn rust_context_detects_all_unsafe_forms_without_false_positives() {
        let source = "\
fn a() { unsafe { raw(); } }
pub unsafe fn b() {}
unsafe impl Send for Foo {}
pub unsafe trait Bar {}
unsafe extern \"C\" { fn c_fn(); }
#[unsafe(no_mangle)]
pub extern \"C\" fn d() {}
fn safe_fn() { let s = \"unsafe fn fake() {}\"; let unsafe_name = 1; }
";
        let mut file = File::default();
        rust_context(source, true, &mut file);
        let unsafe_lines: BTreeSet<u32> = file
            .rust_signals
            .iter()
            .filter_map(|(line, rule)| (*rule == Rule::Unsafe).then_some(*line))
            .collect();
        assert_eq!(unsafe_lines, BTreeSet::from([1, 2, 3, 4, 5, 6]));
    }

    #[test]
    fn rust_context_accurately_detects_parameterized_and_boolean_test_attributes() {
        let source = "\
#[test]
fn plain_test() { v.unwrap(); }
#[tokio::test]
async fn tokio_plain() { v.unwrap(); }
#[tokio::test(flavor = \"multi_thread\", worker_threads = 2)]
async fn tokio_param() { v.unwrap(); }
#[cfg(test)]
mod unit_tests { fn helper() { v.unwrap(); } }
#[cfg(all(test, unix))]
fn unix_test_helper() { v.unwrap(); }
#[cfg(all(unix, test))]
fn reordered_all_helper() { v.unwrap(); }
#[cfg(any(test, feature = \"bench\"))]
fn any_test_helper() { v.unwrap(); }
#[cfg(not(test))]
fn prod_only() { v.unwrap(); }
#[cfg(feature = \"test\")]
fn feature_not_test() { v.unwrap(); }
#[cfg_attr(test, derive(Debug))]
fn cfg_attr_prod() { v.unwrap(); }
";
        let mut file = File::default();
        rust_context(source, true, &mut file);

        // Lines 2, 4, 6, 8, 10, 12, 14 are inside test contexts.
        for test_line in [2u32, 4, 6, 8, 10, 12, 14] {
            assert!(
                is_test("src/lib.rs", &file, test_line, test_line),
                "expected line {test_line} to be classified as test code"
            );
        }

        // Lines 16 (`#[cfg(not(test))]`), 18 (`#[cfg(feature = "test")]`),
        // and 20 (`#[cfg_attr(test, ...)]`) are production code and MUST NOT be skipped!
        for prod_line in [16u32, 18, 20] {
            assert!(
                !is_test("src/lib.rs", &file, prod_line, prod_line),
                "expected line {prod_line} to remain production code"
            );
            assert!(
                file.rust_signals.contains(&(prod_line, Rule::Panic)),
                "expected panic signal on production line {prod_line}"
            );
        }
    }
}
