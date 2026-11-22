//! Daemon Artifacts service, live broadcast helpers, and safe O(N) auto-extraction
//! of `<orbit-artifact>`, `<agent-embed>`, and SEP-1865 MCP App (`ui://`) payloads.

use std::collections::HashMap;

use orbit_protocol::{ArtifactDetail, ArtifactKind, ArtifactSummary};
use orbit_store::{MAX_ARTIFACT_CONTENT_BYTES, UpsertArtifactInput, validate_artifact_id};
use serde_json::Value;

use crate::manager::{AgentOrchestrator, DaemonBroadcast};

/// Upper bound on artifacts auto-extracted from a single message or tool output.
const MAX_AUTO_EXTRACTED_PER_MESSAGE: usize = 16;

/// Upper bound on bytes scanned while looking for the closing `>` of an opening directive tag.
const MAX_OPENING_TAG_BYTES: usize = 4096;

/// Returns `true` when `tool_name` refers to one of Orbit's own Artifact MCP tools
/// (bare or prefixed, e.g. `publish_artifact`, `mcp_orbit_publish_artifact`, `mcp__orbit__read_artifact`).
pub fn is_orbit_artifact_mcp_tool(tool_name: &str) -> bool {
    let norm = tool_name.trim().to_ascii_lowercase();
    let leaf = norm.rsplit([':', '/', '.']).next().unwrap_or(norm.as_str());
    const OWN_TOOLS: [&str; 4] = [
        "publish_artifact",
        "read_artifact",
        "list_artifacts",
        "delete_artifact",
    ];
    OWN_TOOLS
        .iter()
        .any(|t| leaf == *t || leaf.ends_with(&format!("_{t}")))
}

/// Parsed candidate discovered during assistant-message or tool-payload scanning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractedArtifactCandidate {
    pub artifact_id: Option<String>,
    pub title: String,
    pub kind: ArtifactKind,
    pub language: Option<String>,
    pub mime_type: Option<String>,
    pub resource_uri: Option<String>,
    pub content: String,
    pub summary_note: Option<String>,
}

/// Safely extract explicit `<orbit-artifact ...>...</orbit-artifact>` and
/// `<agent-embed ...>...</agent-embed>` directives from a completed assistant message in O(N) time.
///
/// Directives inside fenced Markdown code blocks (`` ``` `` or `~~~`) or inline backtick code spans
/// are skipped so documentation examples are never mistakenly registered as live artifacts.
pub fn extract_artifacts_from_assistant_text(text: &str) -> Vec<ExtractedArtifactCandidate> {
    let lower = text.to_ascii_lowercase();
    if !lower.contains("<orbit-artifact") && !lower.contains("<agent-embed") {
        return Vec::new();
    }

    let fenced_ranges = compute_fenced_code_ranges(text);
    let inline_ranges = compute_inline_code_ranges(text, &fenced_ranges);
    let mut no_more_closing_tag = [false; 2];
    let mut results = Vec::new();
    let mut cursor = 0usize;

    while cursor < text.len() && results.len() < MAX_AUTO_EXTRACTED_PER_MESSAGE {
        let Some((tag_start_rel, tag_idx, tag_name)) = find_next_directive_tag(&lower[cursor..])
        else {
            break;
        };
        let tag_start = cursor + tag_start_rel;

        if is_inside_ranges(tag_start, &fenced_ranges)
            || is_inside_ranges(tag_start, &inline_ranges)
        {
            cursor = tag_start + tag_name.len() + 1;
            continue;
        }

        let attr_start = tag_start + 1 + tag_name.len();
        let Some(open_end) = find_opening_tag_end(text, attr_start, MAX_OPENING_TAG_BYTES) else {
            // Malformed or oversized opening tag — advance safely in O(1).
            cursor = attr_start;
            continue;
        };

        let raw_attrs = &text[attr_start..open_end];
        let is_self_closing = raw_attrs.trim_end().ends_with('/');
        let attrs_clean = if is_self_closing {
            raw_attrs.trim_end().trim_end_matches('/').trim()
        } else {
            raw_attrs.trim()
        };
        let attrs = parse_tag_attributes(attrs_clean);

        if is_self_closing {
            cursor = open_end + 1;
            if let Some(candidate) = build_candidate_from_directive(tag_name, &attrs, "") {
                results.push(candidate);
            }
            continue;
        }

        let content_start = open_end + 1;
        if no_more_closing_tag[tag_idx] {
            cursor = content_start;
            continue;
        }

        let close_needle = format!("</{tag_name}>");
        let Some(close_rel) = lower[content_start..].find(&close_needle) else {
            // Memoize absence of any remaining closing tag for this tag name to guarantee O(N).
            no_more_closing_tag[tag_idx] = true;
            cursor = content_start;
            continue;
        };
        let content_end = content_start + close_rel;
        let raw_body = text[content_start..content_end].trim();
        cursor = content_end + close_needle.len();

        if let Some(candidate) = build_candidate_from_directive(tag_name, &attrs, raw_body) {
            results.push(candidate);
        }
    }

    results
}

/// Safely scan a completed tool call payload/output for SEP-1865 MCP App declarations
/// (`_meta.ui.resourceUri` / `ui://...` HTML payloads).
pub fn extract_mcp_app_from_tool_payload(
    tool_name: &str,
    payload: &str,
) -> Vec<ExtractedArtifactCandidate> {
    if !payload.contains("ui://") {
        return Vec::new();
    }
    let Ok(val) = serde_json::from_str::<Value>(payload) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    collect_mcp_app_candidates(&val, tool_name, &mut out);
    out
}

fn collect_mcp_app_candidates(
    val: &Value,
    tool_name: &str,
    out: &mut Vec<ExtractedArtifactCandidate>,
) {
    if out.len() >= MAX_AUTO_EXTRACTED_PER_MESSAGE {
        return;
    }

    if let Some(obj) = val.as_object() {
        let meta_uri = obj
            .get("_meta")
            .or_else(|| obj.get("meta"))
            .and_then(extract_meta_ui_uri);

        let direct_uri = obj
            .get("resource_uri")
            .or_else(|| obj.get("resourceUri"))
            .or_else(|| obj.get("uri"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| s.starts_with("ui://"))
            .map(ToOwned::to_owned);

        let effective_uri = meta_uri.or(direct_uri);

        let inline_html = obj
            .get("html")
            .or_else(|| obj.get("text"))
            .or_else(|| obj.get("content"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty());

        let mime_hint = obj
            .get("mimeType")
            .or_else(|| obj.get("mime_type"))
            .and_then(Value::as_str)
            .map(str::trim);

        if let Some(uri) = effective_uri {
            let body = inline_html
                .map(ToOwned::to_owned)
                .or_else(|| find_html_in_content_array(obj.get("content"), Some(&uri)))
                .or_else(|| {
                    obj.get("resource")
                        .and_then(|r| r.get("text"))
                        .and_then(Value::as_str)
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                });

            if let Some(content) = body
                && content.len() <= MAX_ARTIFACT_CONTENT_BYTES
            {
                let title = obj
                    .get("title")
                    .or_else(|| obj.get("name"))
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(ToOwned::to_owned)
                    .unwrap_or_else(|| derive_title_from_uri_or_tool(&uri, tool_name));
                let mime_type = mime_hint
                    .filter(|m| !m.is_empty())
                    .map(ToOwned::to_owned)
                    .or_else(|| Some(ArtifactKind::McpApp.default_mime_type().to_string()));
                if !out.iter().any(|c| {
                    c.resource_uri.as_deref() == Some(uri.as_str()) && c.content == content
                }) {
                    out.push(ExtractedArtifactCandidate {
                        artifact_id: obj
                            .get("artifact_id")
                            .and_then(Value::as_str)
                            .map(str::trim)
                            .filter(|s| !s.is_empty())
                            .map(ToOwned::to_owned),
                        title,
                        kind: ArtifactKind::McpApp,
                        language: Some("html".into()),
                        mime_type,
                        resource_uri: Some(uri),
                        content,
                        summary_note: Some(format!("Extracted from tool `{tool_name}`")),
                    });
                }
                return;
            }
        }

        for key in ["result", "output", "resource", "content", "structured"] {
            if let Some(child) = obj.get(key) {
                collect_mcp_app_candidates(child, tool_name, out);
            }
        }
    } else if let Some(arr) = val.as_array() {
        for item in arr {
            collect_mcp_app_candidates(item, tool_name, out);
            if out.len() >= MAX_AUTO_EXTRACTED_PER_MESSAGE {
                break;
            }
        }
    }
}

fn extract_meta_ui_uri(meta: &Value) -> Option<String> {
    let obj = meta.as_object()?;
    let uri = obj
        .get("ui")
        .and_then(|ui| ui.get("resourceUri").or_else(|| ui.get("resource_uri")))
        .or_else(|| obj.get("ui/resourceUri"))
        .and_then(Value::as_str)?;
    let trimmed = uri.trim();
    if trimmed.starts_with("ui://") {
        Some(trimmed.to_string())
    } else {
        None
    }
}

fn find_html_in_content_array(content: Option<&Value>, target_uri: Option<&str>) -> Option<String> {
    let arr = content?.as_array()?;
    for item in arr {
        let Some(obj) = item.as_object() else {
            continue;
        };
        let resource = obj.get("resource").unwrap_or(item);
        let Some(res_obj) = resource.as_object() else {
            continue;
        };
        let item_uri = res_obj.get("uri").and_then(Value::as_str).map(str::trim);
        let mime = res_obj
            .get("mimeType")
            .or_else(|| res_obj.get("mime_type"))
            .and_then(Value::as_str)
            .unwrap_or("");
        let matches_uri = target_uri.is_none_or(|t| item_uri == Some(t));
        let looks_like_app = item_uri.is_some_and(|u| u.starts_with("ui://"))
            || mime.to_ascii_lowercase().contains("html");
        if matches_uri
            && looks_like_app
            && let Some(text) = res_obj
                .get("text")
                .or_else(|| res_obj.get("html"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
        {
            return Some(text.to_string());
        }
    }
    None
}

fn derive_title_from_uri_or_tool(uri: &str, tool_name: &str) -> String {
    let stripped = uri.trim().trim_start_matches("ui://").trim_matches('/');
    let segment = stripped
        .rsplit('/')
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or(tool_name);
    let clean = segment
        .replace(['-', '_', '.'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if clean.is_empty() {
        "MCP App".into()
    } else {
        clean
    }
}

fn compute_fenced_code_ranges(text: &str) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    let mut in_fence: Option<(char, usize, usize)> = None;
    let mut offset = 0usize;

    for raw_line in text.split_inclusive('\n') {
        let trimmed = raw_line.trim_start();
        if let Some((marker, marker_len, start)) = in_fence {
            let count = trimmed.chars().take_while(|&c| c == marker).count();
            if count >= marker_len && trimmed[count * marker.len_utf8()..].trim().is_empty() {
                ranges.push((start, offset + raw_line.len()));
                in_fence = None;
            }
        } else {
            let first = trimmed.chars().next();
            if matches!(first, Some('`') | Some('~')) {
                let marker = first.unwrap_or('`');
                let count = trimmed.chars().take_while(|&c| c == marker).count();
                // CommonMark rule: info string of a backtick code fence cannot contain backticks.
                if count >= 3 && !(marker == '`' && trimmed[count..].contains('`')) {
                    in_fence = Some((marker, count, offset));
                }
            }
        }
        offset += raw_line.len();
    }
    if let Some((_, _, start)) = in_fence {
        ranges.push((start, text.len()));
    }
    ranges
}

/// Compute CommonMark-style inline backtick code span byte ranges outside fenced code blocks in O(N).
fn compute_inline_code_ranges(text: &str, fenced_ranges: &[(usize, usize)]) -> Vec<(usize, usize)> {
    let bytes = text.as_bytes();
    let mut ranges = Vec::new();
    let mut i = 0usize;

    while i < bytes.len() {
        if let Some(&(_, end)) = fenced_ranges.iter().find(|&&(s, e)| i >= s && i < e) {
            i = end;
            continue;
        }
        if bytes[i] != b'`' {
            i += 1;
            continue;
        }
        let open_start = i;
        while i < bytes.len() && bytes[i] == b'`' {
            i += 1;
        }
        let tick_len = i - open_start;
        // Scan forward for a matching closing backtick run of exact length `tick_len`
        let mut scan = i;
        let mut found_end = None;
        while scan < bytes.len() {
            if is_inside_ranges(scan, fenced_ranges) {
                break;
            }
            if bytes[scan] == b'`' {
                let run_start = scan;
                while scan < bytes.len() && bytes[scan] == b'`' {
                    scan += 1;
                }
                if scan - run_start == tick_len {
                    found_end = Some(scan);
                    break;
                }
            } else {
                scan += 1;
            }
        }
        if let Some(close_end) = found_end {
            ranges.push((open_start, close_end));
            i = close_end;
        }
    }
    ranges
}

fn is_inside_ranges(pos: usize, ranges: &[(usize, usize)]) -> bool {
    ranges.iter().any(|&(start, end)| pos >= start && pos < end)
}

fn find_next_directive_tag(lower_slice: &str) -> Option<(usize, usize, &'static str)> {
    const TAGS: [&str; 2] = ["orbit-artifact", "agent-embed"];
    let mut best: Option<(usize, usize, &'static str)> = None;

    for (tag_idx, tag) in TAGS.iter().enumerate() {
        let needle = format!("<{tag}");
        let mut search_from = 0usize;
        while let Some(rel) = lower_slice[search_from..].find(&needle) {
            let idx = search_from + rel;
            let after_idx = idx + needle.len();
            let boundary_ok = lower_slice[after_idx..]
                .chars()
                .next()
                .is_none_or(|c| c.is_whitespace() || c == '>' || c == '/');
            if boundary_ok {
                if best.is_none_or(|(best_idx, _, _)| idx < best_idx) {
                    best = Some((idx, tag_idx, *tag));
                }
                break;
            }
            search_from = after_idx;
        }
    }

    best
}

fn find_opening_tag_end(text: &str, start: usize, max_bytes: usize) -> Option<usize> {
    let mut quote: Option<char> = None;
    let mut limit = start.saturating_add(max_bytes).min(text.len());
    while limit > start && !text.is_char_boundary(limit) {
        limit -= 1;
    }
    for (rel_idx, ch) in text[start..limit].char_indices() {
        match quote {
            Some(q) => {
                if ch == q {
                    quote = None;
                }
            }
            None => match ch {
                '"' | '\'' => quote = Some(ch),
                '>' => return Some(start + rel_idx),
                _ => {}
            },
        }
    }
    None
}

fn parse_tag_attributes(raw: &str) -> HashMap<String, String> {
    let mut attrs = HashMap::new();
    let bytes = raw.as_bytes();
    let mut i = 0usize;

    while i < bytes.len() {
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }

        let key_start = i;
        while i < bytes.len()
            && !bytes[i].is_ascii_whitespace()
            && bytes[i] != b'='
            && bytes[i] != b'/'
        {
            i += 1;
        }
        let key = raw[key_start..i].trim().to_ascii_lowercase();
        if key.is_empty() {
            i += 1;
            continue;
        }

        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() || bytes[i] != b'=' {
            attrs.insert(key, "true".into());
            continue;
        }
        i += 1;
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }

        let val = if bytes[i] == b'"' || bytes[i] == b'\'' {
            let quote = bytes[i];
            i += 1;
            let val_start = i;
            while i < bytes.len() && bytes[i] != quote {
                i += 1;
            }
            let slice = raw[val_start..i].to_string();
            if i < bytes.len() {
                i += 1;
            }
            slice
        } else {
            let val_start = i;
            while i < bytes.len() && !bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            raw[val_start..i].to_string()
        };

        attrs.insert(key, decode_basic_html_entities(&val));
    }

    attrs
}

fn decode_basic_html_entities(raw: &str) -> String {
    raw.replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&#39;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

fn build_candidate_from_directive(
    tag_name: &str,
    attrs: &HashMap<String, String>,
    body: &str,
) -> Option<ExtractedArtifactCandidate> {
    let content = if !body.is_empty() {
        body.to_string()
    } else if let Some(inline_content) = attrs.get("content").map(|s| s.trim())
        && !inline_content.is_empty()
    {
        inline_content.to_string()
    } else {
        return None;
    };

    if content.len() > MAX_ARTIFACT_CONTENT_BYTES {
        return None;
    }

    let resource_uri = attrs
        .get("resource_uri")
        .or_else(|| attrs.get("resourceuri"))
        .or_else(|| attrs.get("uri"))
        .or_else(|| attrs.get("src"))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    let mime_type = attrs
        .get("mime_type")
        .or_else(|| attrs.get("mimetype"))
        .or_else(|| attrs.get("mime"))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    let language = attrs
        .get("language")
        .or_else(|| attrs.get("lang"))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    let kind = attrs
        .get("kind")
        .or_else(|| attrs.get("type"))
        .and_then(|s| ArtifactKind::from_slug(s))
        .or_else(|| mime_type.as_deref().and_then(ArtifactKind::from_slug))
        .or_else(|| {
            resource_uri
                .as_deref()
                .filter(|u| u.starts_with("ui://"))
                .map(|_| ArtifactKind::McpApp)
        })
        .unwrap_or_else(|| {
            infer_kind_from_tag_and_content(tag_name, language.as_deref(), &content)
        });

    let title = attrs
        .get("title")
        .or_else(|| attrs.get("name"))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| match kind {
            ArtifactKind::Html => "Interactive Preview".into(),
            ArtifactKind::McpApp => resource_uri
                .as_deref()
                .map(|u| derive_title_from_uri_or_tool(u, "MCP App"))
                .unwrap_or_else(|| "MCP App".into()),
            ArtifactKind::Svg => "SVG Diagram".into(),
            ArtifactKind::Mermaid => "Mermaid Diagram".into(),
            ArtifactKind::Code => language
                .as_deref()
                .map(|l| format!("{l} Snippet"))
                .unwrap_or_else(|| "Code Artifact".into()),
            ArtifactKind::Markdown => "Document Artifact".into(),
        });

    let artifact_id = attrs
        .get("id")
        .or_else(|| attrs.get("artifact_id"))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    let summary_note = attrs
        .get("summary_note")
        .or_else(|| attrs.get("note"))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    Some(ExtractedArtifactCandidate {
        artifact_id,
        title,
        kind,
        language,
        mime_type,
        resource_uri,
        content,
        summary_note,
    })
}

fn infer_kind_from_tag_and_content(
    tag_name: &str,
    language: Option<&str>,
    content: &str,
) -> ArtifactKind {
    let trimmed = content.trim_start();
    let lower_head: String = trimmed
        .chars()
        .take(64)
        .collect::<String>()
        .to_ascii_lowercase();
    if lower_head.starts_with("<svg")
        || lower_head.starts_with("<?xml") && lower_head.contains("<svg")
    {
        return ArtifactKind::Svg;
    }
    if lower_head.starts_with("<!doctype html")
        || lower_head.starts_with("<html")
        || tag_name == "agent-embed"
    {
        return ArtifactKind::Html;
    }
    if let Some(lang) = language {
        if lang.eq_ignore_ascii_case("mermaid") || lang.eq_ignore_ascii_case("mmd") {
            return ArtifactKind::Mermaid;
        }
        if lang.eq_ignore_ascii_case("svg") {
            return ArtifactKind::Svg;
        }
        if lang.eq_ignore_ascii_case("html") || lang.eq_ignore_ascii_case("htm") {
            return ArtifactKind::Html;
        }
        if lang.eq_ignore_ascii_case("markdown") || lang.eq_ignore_ascii_case("md") {
            return ArtifactKind::Markdown;
        }
        return ArtifactKind::Code;
    }
    ArtifactKind::Markdown
}

impl AgentOrchestrator {
    pub fn list_artifacts(
        &self,
        workspace_id: Option<&str>,
        agent_id: Option<&str>,
    ) -> anyhow::Result<Vec<ArtifactSummary>> {
        Ok(self.store().list_artifacts(workspace_id, agent_id)?)
    }

    pub fn get_artifact(
        &self,
        artifact_id: &str,
        version: Option<u32>,
    ) -> anyhow::Result<Option<ArtifactDetail>> {
        Ok(self.store().get_artifact(artifact_id, version)?)
    }

    pub fn upsert_artifact(&self, input: UpsertArtifactInput) -> anyhow::Result<ArtifactSummary> {
        let summary = self.store().upsert_artifact(input)?;
        let _ = self
            .broadcast_tx
            .send(DaemonBroadcast::ArtifactUpdated(summary.clone()));
        Ok(summary)
    }

    pub fn delete_artifact(&self, artifact_id: &str) -> anyhow::Result<bool> {
        let deleted = self.store().delete_artifact(artifact_id)?;
        if deleted {
            let _ = self
                .broadcast_tx
                .send(DaemonBroadcast::ArtifactDeleted(artifact_id.to_string()));
        }
        Ok(deleted)
    }

    /// Scan a completed assistant message for explicit artifact directives and persist/broadcast them.
    pub fn auto_extract_artifacts_from_message(
        &self,
        agent_id: &str,
        text: &str,
    ) -> Vec<ArtifactSummary> {
        let candidates = extract_artifacts_from_assistant_text(text);
        if candidates.is_empty() {
            return Vec::new();
        }
        self.persist_extracted_candidates(agent_id, candidates)
    }

    /// Scan a completed tool call payload for SEP-1865 MCP App (`ui://`) resources and persist/broadcast them.
    pub fn auto_extract_artifacts_from_tool_call(
        &self,
        agent_id: &str,
        tool_name: &str,
        payload: &str,
    ) -> Vec<ArtifactSummary> {
        if is_orbit_artifact_mcp_tool(tool_name) {
            return Vec::new();
        }
        let candidates = extract_mcp_app_from_tool_payload(tool_name, payload);
        if candidates.is_empty() {
            return Vec::new();
        }
        self.persist_extracted_candidates(agent_id, candidates)
    }

    fn persist_extracted_candidates(
        &self,
        agent_id: &str,
        candidates: Vec<ExtractedArtifactCandidate>,
    ) -> Vec<ArtifactSummary> {
        let Some(agent) = self.store().get_agent(agent_id).ok().flatten() else {
            return Vec::new();
        };
        let workspace_id = agent.workspace_id;
        let mut published = Vec::new();

        for candidate in candidates {
            // Only reuse an explicit `artifact_id` when it is syntactically valid AND either does
            // not yet exist or already belongs to `workspace_id`. Never hijack another workspace's ID.
            let valid_explicit_id = candidate
                .artifact_id
                .as_deref()
                .filter(|id| validate_artifact_id(id).is_ok())
                .map(ToOwned::to_owned);

            let mut allow_explicit_id = valid_explicit_id.clone();
            let existing = if let Some(ref explicit_id) = valid_explicit_id {
                match self.store().get_artifact(explicit_id, None).ok().flatten() {
                    Some(d) if d.summary.workspace_id == workspace_id => {
                        Some((d.summary, Some(d.content)))
                    }
                    Some(_) => {
                        // Cross-workspace collision: regenerate a fresh ID scoped to this workspace.
                        allow_explicit_id = None;
                        None
                    }
                    None => None,
                }
            } else if let Some(ref uri) = candidate.resource_uri {
                self.store()
                    .find_artifact_by_workspace_and_resource_uri(&workspace_id, uri)
                    .ok()
                    .flatten()
                    .map(|s| {
                        let content = self
                            .store()
                            .get_artifact_revision(&s.artifact_id, s.version)
                            .ok()
                            .flatten()
                            .map(|r| r.content);
                        (s, content)
                    })
            } else {
                self.store()
                    .find_artifact_by_workspace_title_kind(
                        &workspace_id,
                        &candidate.title,
                        candidate.kind,
                    )
                    .ok()
                    .flatten()
                    .map(|s| {
                        let content = self
                            .store()
                            .get_artifact_revision(&s.artifact_id, s.version)
                            .ok()
                            .flatten()
                            .map(|r| r.content);
                        (s, content)
                    })
            };

            if let Some((existing_summary, Some(existing_content))) = existing.as_ref()
                && existing_content == &candidate.content
                && existing_summary.title == candidate.title
                && existing_summary.kind == candidate.kind
            {
                published.push(existing_summary.clone());
                continue;
            }

            let resolved_id = allow_explicit_id.or_else(|| existing.map(|(s, _)| s.artifact_id));

            match self.upsert_artifact(UpsertArtifactInput {
                artifact_id: resolved_id,
                workspace_id: workspace_id.clone(),
                agent_id: Some(agent_id.to_string()),
                title: candidate.title,
                kind: candidate.kind,
                language: candidate.language,
                mime_type: candidate.mime_type,
                resource_uri: candidate.resource_uri,
                content: candidate.content,
                summary_note: candidate.summary_note,
            }) {
                Ok(summary) => published.push(summary),
                Err(err) => {
                    tracing::warn!(
                        agent_id = %agent_id,
                        error = %err,
                        "Failed to auto-extract artifact"
                    );
                }
            }
        }

        published
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DaemonRuntime;
    use crate::tools::execute_tool;
    use chrono::Utc;
    use orbit_protocol::{
        AgentStatus, AgentSummary, AttentionState, ProviderKind, SessionMode, WorkspaceSetupStatus,
        WorkspaceSummary,
    };
    use serde_json::json;
    use std::collections::HashMap;
    use uuid::Uuid;

    fn seed_ws_and_agent(rt: &DaemonRuntime, ws_id: &str, ag_id: &str, cwd: &str) {
        let now = Utc::now();
        rt.store
            .upsert_workspace(&WorkspaceSummary {
                workspace_id: ws_id.into(),
                title: format!("WS {ws_id}"),
                cwd: cwd.into(),
                git_branch: None,
                is_worktree: false,
                updated_at: now,
                source_cwd: None,
                base_ref: None,
                parent_workspace_id: None,
                archived_at: None,
                background: false,
                setup_status: WorkspaceSetupStatus::Idle,
                mission_goal: None,
                project_root: None,
                board_revision: 0,
                title_locked: false,
                auto_clustered: false,
                is_curated: false,
            })
            .expect("ws");
        rt.store
            .upsert_agent(
                &AgentSummary {
                    agent_id: ag_id.into(),
                    workspace_id: ws_id.into(),
                    cwd: cwd.into(),
                    title: format!("Agent {ag_id}"),
                    provider: ProviderKind::ClaudeCode,
                    model: None,
                    effort: None,
                    fast_mode: false,
                    mode: SessionMode::AcceptEdits,
                    status: AgentStatus::Idle,
                    parent_agent_id: None,
                    parent_host_id: None,
                    latest_seq: 0,
                    timeline_epoch: 0,
                    created_at: now,
                    updated_at: now,
                    archived_at: None,
                    background: false,
                    auto_archive: false,
                    profile_id: None,
                    labels: HashMap::new(),
                    attention: AttentionState::None,
                    last_activity_at: None,
                    queued_prompts: Vec::new(),
                    read_seq: 0,
                    is_pinned: false,
                    pinned_order: 0,
                    handle: None,
                    cluster_suggestion: None,
                    task_summary: None,
                    forked_from_agent_id: None,
                    forked_at_seq: None,
                    acp_agent_id: None,
                },
                None,
            )
            .expect("agent");
    }

    #[test]
    fn extracts_orbit_artifact_and_agent_embed_directives() {
        let msg = r#"
Here is the interactive dashboard:

<orbit-artifact id="art_chart" title="Latency Chart" kind="html" note="v1 chart">
<!doctype html>
<html><body><canvas id="c"></canvas></body></html>
</orbit-artifact>

And a SEP-1865 widget:

<agent-embed title='Quota Monitor' uri='ui://orbit/quota' mime='text/html;profile=mcp-app'>
<div id="app">Live Quota</div>
</agent-embed>

```html
<!-- Inside code fence — must be ignored -->
<orbit-artifact title="Doc Example" kind="html"><div>Ignored</div></orbit-artifact>
```
"#;

        let found = extract_artifacts_from_assistant_text(msg);
        assert_eq!(found.len(), 2);

        assert_eq!(found[0].artifact_id.as_deref(), Some("art_chart"));
        assert_eq!(found[0].title, "Latency Chart");
        assert_eq!(found[0].kind, ArtifactKind::Html);
        assert_eq!(found[0].summary_note.as_deref(), Some("v1 chart"));
        assert!(found[0].content.contains("<canvas id=\"c\">"));

        assert_eq!(found[1].title, "Quota Monitor");
        assert_eq!(found[1].kind, ArtifactKind::McpApp);
        assert_eq!(found[1].resource_uri.as_deref(), Some("ui://orbit/quota"));
        assert_eq!(found[1].content, "<div id=\"app\">Live Quota</div>");
    }

    #[test]
    fn skips_inline_code_spans_and_handles_commonmark_backtick_info_strings() {
        let msg = concat!(
            "Use `<orbit-artifact title=\"InlineDoc\" kind=\"html\"><div>no</div></orbit-artifact>` in docs.\n",
            "Also ``<agent-embed title=\"DoubleTick\"><div>no</div></agent-embed>`` should be skipped.\n",
            "```inline`not-a-fence\n",
            "<orbit-artifact title=\"RealAfterInlineTriple\" kind=\"markdown\"># Real</orbit-artifact>\n",
        );

        let found = extract_artifacts_from_assistant_text(msg);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].title, "RealAfterInlineTriple");
        assert_eq!(found[0].content, "# Real");
    }

    #[test]
    fn malformed_and_unclosed_tags_fail_safely_in_linear_time() {
        let bad_inputs = [
            "<orbit-artifact title=\"Unclosed\"><div>no closing tag",
            "<orbit-artifact title=\"Broken attr",
            "<agent-embed></agent-embed>",
            "<orbit-artifact-other title=\"Wrong\">body</orbit-artifact-other>",
            "Plain text with < and > and </orbit-artifact>",
        ];
        for input in bad_inputs {
            let extracted = extract_artifacts_from_assistant_text(input);
            assert!(
                extracted.is_empty(),
                "expected empty extraction for malformed input: {input}"
            );
        }

        // Valid tag following a malformed unclosed opening tag still extracts cleanly
        let mixed = "<orbit-artifact title=\"Unclosed\">oops\n<agent-embed title=\"Good\" kind=\"svg\"><svg></svg></agent-embed>";
        let extracted = extract_artifacts_from_assistant_text(mixed);
        assert_eq!(extracted.len(), 1);
        assert_eq!(extracted[0].title, "Good");
        assert_eq!(extracted[0].kind, ArtifactKind::Svg);

        // Many unclosed tags remain strictly O(N) via per-tag no_more_closing_tag memoization
        let many_unclosed = format!(
            "{}{}",
            "<orbit-artifact title=\"x\">".repeat(2000),
            "<agent-embed ".repeat(500),
        );
        assert!(extract_artifacts_from_assistant_text(&many_unclosed).is_empty());

        // Oversized opening tag (>4 KiB) is skipped safely without extracting
        let oversized_open = format!(
            "<orbit-artifact title=\"{}\">body</orbit-artifact>",
            "a".repeat(MAX_OPENING_TAG_BYTES + 100),
        );
        assert!(extract_artifacts_from_assistant_text(&oversized_open).is_empty());
    }

    #[test]
    fn is_orbit_artifact_mcp_tool_detects_bare_and_prefixed_names() {
        for own in [
            "publish_artifact",
            "read_artifact",
            "list_artifacts",
            "delete_artifact",
            "mcp_orbit_publish_artifact",
            "mcp__orbit__read_artifact",
            "orbit:list_artifacts",
            "orbit/delete_artifact",
        ] {
            assert!(
                is_orbit_artifact_mcp_tool(own),
                "expected `{own}` to be recognized as an Orbit artifact MCP tool"
            );
        }
        for other in ["analytics_query", "shell", "mcp_custom_render_ui"] {
            assert!(!is_orbit_artifact_mcp_tool(other));
        }
    }

    #[test]
    fn extracts_sep_1865_mcp_app_from_tool_payload() {
        let payload = json!({
            "_meta": {
                "ui": {
                    "resourceUri": "ui://analytics/funnel-view"
                }
            },
            "title": "Conversion Funnel",
            "content": [
                {
                    "type": "resource",
                    "resource": {
                        "uri": "ui://analytics/funnel-view",
                        "mimeType": "text/html;profile=mcp-app",
                        "text": "<!doctype html><html><body><h1>Funnel</h1></body></html>"
                    }
                }
            ]
        })
        .to_string();

        let extracted = extract_mcp_app_from_tool_payload("analytics_query", &payload);
        assert_eq!(extracted.len(), 1);
        assert_eq!(extracted[0].kind, ArtifactKind::McpApp);
        assert_eq!(extracted[0].title, "Conversion Funnel");
        assert_eq!(
            extracted[0].resource_uri.as_deref(),
            Some("ui://analytics/funnel-view")
        );
        assert!(extracted[0].content.contains("<h1>Funnel</h1>"));
    }

    #[tokio::test]
    async fn mcp_artifact_tools_end_to_end_broadcast_and_cross_workspace_isolation() {
        let tmp = std::env::temp_dir().join(format!("orbit-art-test-{}", Uuid::new_v4().simple()));
        let rt = DaemonRuntime::bootstrap(&tmp).await.expect("bootstrap");
        let cwd = tmp.to_string_lossy().to_string();
        seed_ws_and_agent(&rt, "ws_art", "ag_art", &cwd);
        seed_ws_and_agent(&rt, "ws_other", "ag_other", &cwd);

        let mut bcast_rx = rt.orchestrator.broadcast_tx.subscribe();

        // 1. publish_artifact (v1)
        let pub_res = execute_tool(
            &rt,
            Some("ag_art"),
            "publish_artifact",
            json!({
                "title": "System Architecture",
                "kind": "mermaid",
                "content": "flowchart LR\nA-->B",
                "summary_note": "Initial diagram"
            }),
        )
        .await;
        assert!(!pub_res.is_error, "publish error: {}", pub_res.text);
        let art_id = pub_res.structured["artifact"]["artifact_id"]
            .as_str()
            .expect("artifact_id")
            .to_string();
        assert_eq!(pub_res.structured["artifact"]["version"], 1);
        assert_eq!(pub_res.structured["artifact"]["workspace_id"], "ws_art");

        let ev = bcast_rx.try_recv().expect("broadcast ArtifactUpdated");
        assert!(matches!(ev, DaemonBroadcast::ArtifactUpdated(s) if s.artifact_id == art_id));

        // 2. list_artifacts
        let list_res = execute_tool(
            &rt,
            Some("ag_art"),
            "list_artifacts",
            json!({ "workspace_id": "ws_art" }),
        )
        .await;
        assert!(!list_res.is_error);
        assert_eq!(
            list_res.structured["artifacts"].as_array().map(Vec::len),
            Some(1)
        );

        // 3. read_artifact (valid + invalid versions)
        let read_res = execute_tool(
            &rt,
            Some("ag_art"),
            "read_artifact",
            json!({ "artifact_id": art_id }),
        )
        .await;
        assert!(!read_res.is_error);
        assert_eq!(
            read_res.structured["artifact"]["content"],
            "flowchart LR\nA-->B"
        );

        for bad_ver in [json!(0), json!(-1), json!("v1")] {
            let err_res = execute_tool(
                &rt,
                Some("ag_art"),
                "read_artifact",
                json!({ "artifact_id": art_id, "version": bad_ver }),
            )
            .await;
            assert!(
                err_res.is_error,
                "invalid version {bad_ver} must return error"
            );
        }

        // 4. Auto-extract deduplication & cross-workspace hijack protection
        let msg = format!(
            "<orbit-artifact id=\"{art_id}\" title=\"HijackAttempt\" kind=\"html\"><div>evil</div></orbit-artifact>"
        );
        let extracted_other = rt
            .orchestrator
            .auto_extract_artifacts_from_message("ag_other", &msg);
        assert_eq!(extracted_other.len(), 1);
        assert_ne!(
            extracted_other[0].artifact_id, art_id,
            "cross-workspace auto-extract must regenerate fresh ID instead of overwriting"
        );
        assert_eq!(extracted_other[0].workspace_id, "ws_other");

        let original_intact = rt
            .orchestrator
            .get_artifact(&art_id, None)
            .expect("get orig")
            .expect("still exists");
        assert_eq!(original_intact.summary.workspace_id, "ws_art");
        assert_eq!(original_intact.summary.version, 1);

        // Skipping Orbit's own artifact MCP tools in auto-extract prevents duplicate revisions
        let self_tool_payload = json!({
            "resource_uri": "ui://orbit/self",
            "content": "<div>ignored</div>"
        })
        .to_string();
        assert!(
            rt.orchestrator
                .auto_extract_artifacts_from_tool_call(
                    "ag_art",
                    "mcp_orbit_publish_artifact",
                    &self_tool_payload,
                )
                .is_empty()
        );

        // 5. Same-workspace auto-extract deduplication
        let wmsg = "<orbit-artifact title=\"Widget\" kind=\"html\"><div>hi</div></orbit-artifact>";
        let first = rt
            .orchestrator
            .auto_extract_artifacts_from_message("ag_art", wmsg);
        let second = rt
            .orchestrator
            .auto_extract_artifacts_from_message("ag_art", wmsg);
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].version, 1);
        assert_eq!(second.len(), 1);
        assert_eq!(second[0].artifact_id, first[0].artifact_id);
        assert_eq!(second[0].version, 1);

        // 6. delete_artifact
        let del_res = execute_tool(
            &rt,
            Some("ag_art"),
            "delete_artifact",
            json!({ "artifact_id": art_id }),
        )
        .await;
        assert!(!del_res.is_error);
        assert_eq!(del_res.structured["deleted"], true);

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
