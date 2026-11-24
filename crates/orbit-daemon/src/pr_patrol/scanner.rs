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
        if attr == "cfg(test)" || attr == "test" || attr.ends_with("::test") {
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
        if deny && t.text == "unsafe" && at(i + 1) == Some("{") {
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
