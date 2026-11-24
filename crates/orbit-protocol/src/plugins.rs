//! Portable manifest and inert UI data shared by all Orbit clients.
use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const PLUGIN_MAX_FUEL: u64 = 50_000_000;
pub const PLUGIN_MAX_MEMORY: u32 = 32 * 1024 * 1024;

pub fn valid_plugin_id(id: &str) -> bool {
    let alnum = |c: u8| c.is_ascii_lowercase() || c.is_ascii_digit();
    !id.is_empty()
        && id.len() <= 64
        && alnum(id.as_bytes()[0])
        && id.bytes().all(|c| alnum(c) || matches!(c, b'-' | b'_'))
}

pub fn valid_plugin_relative_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 1024
        && !path.contains(['\\', ':'])
        && !path.chars().any(char::is_control)
        && path.split('/').all(|part| !matches!(part, "" | "." | ".."))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct OrbitPluginCapabilities {
    pub fs_read_globs: Vec<String>,
    pub http_domains: Vec<String>,
    pub hook_events: Vec<String>,
    pub max_fuel: Option<u64>,
    pub max_memory_bytes: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OrbitPluginMcpOp {
    pub name: String,
    pub description: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrbitPluginUiSlot {
    SidePanelTab,
    StageCard,
    SettingsPanel,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OrbitPluginUiSlotDecl {
    pub slot: OrbitPluginUiSlot,
    pub title: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OrbitPluginManifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub author: Option<String>,
    pub wasm_entry: String,
    #[serde(default)]
    pub capabilities: OrbitPluginCapabilities,
    #[serde(default)]
    pub mcp_ops: Vec<OrbitPluginMcpOp>,
    #[serde(default)]
    pub ui_slots: Vec<OrbitPluginUiSlotDecl>,
}

fn is_clean_single_line(s: &str) -> bool {
    !s.chars().any(char::is_control)
}

fn is_clean_multiline(s: &str) -> bool {
    !s.chars()
        .any(|c| c.is_control() && !matches!(c, '\n' | '\t'))
}

fn is_safe_markdown(content: &str) -> bool {
    if content.len() > 4096 || !is_clean_multiline(content) {
        return false;
    }
    let lower = content.to_ascii_lowercase();
    ![
        "<script",
        "</script",
        "<iframe",
        "<img",
        "<svg",
        "<object",
        "<embed",
        "<style",
        "<a ",
        "javascript:",
        "vbscript:",
        "data:text/html",
    ]
    .iter()
    .any(|bad| lower.contains(bad))
}

impl OrbitPluginManifest {
    pub fn validate(&self) -> Result<(), &'static str> {
        if !valid_plugin_id(&self.id)
            || self.name.trim().is_empty()
            || self.name.len() > 128
            || !is_clean_single_line(&self.name)
            || self.version.trim().is_empty()
            || self.version.len() > 64
            || !is_clean_single_line(&self.version)
            || self.description.len() > 2048
            || !is_clean_multiline(&self.description)
            || self
                .author
                .as_ref()
                .is_some_and(|s| s.len() > 256 || !is_clean_single_line(s))
        {
            return Err("invalid plugin metadata");
        }
        if !valid_plugin_relative_path(&self.wasm_entry)
            || !self.wasm_entry.ends_with(".wasm")
            || self
                .wasm_entry
                .split('/')
                .any(|s| s.starts_with('.') || s.eq_ignore_ascii_case("orbit-plugin.json"))
        {
            return Err("expected a relative .wasm entry inside the package");
        }
        let c = &self.capabilities;
        if c.fs_read_globs.len() > 32
            || c.fs_read_globs
                .iter()
                .any(|g| !valid_plugin_relative_path(g) || g.len() > 256)
            || c.http_domains.len() > 32
            || c.http_domains.iter().any(|h| {
                h.len() > 253
                    || h.split('.').any(|l| {
                        l.is_empty()
                            || l.len() > 63
                            || l.starts_with('-')
                            || l.ends_with('-')
                            || !l
                                .bytes()
                                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
                    })
            })
            || c.hook_events.len() > 32
            || c.hook_events.iter().any(|e| !valid_plugin_id(e))
            || c.max_fuel
                .is_some_and(|f| !(1..=PLUGIN_MAX_FUEL).contains(&f))
            || c.max_memory_bytes
                .is_some_and(|m| !(65_536..=PLUGIN_MAX_MEMORY).contains(&m))
        {
            return Err("invalid plugin capability declaration");
        }
        let mut names = HashSet::new();
        let mut slots = HashSet::new();
        if self.mcp_ops.len() > 32
            || self.mcp_ops.iter().any(|op| {
                !valid_plugin_id(&op.name)
                    || op.description.len() > 512
                    || !is_clean_multiline(&op.description)
                    || !names.insert(&op.name)
            })
            || self.ui_slots.iter().any(|d| {
                d.title.len() > 128 || !is_clean_single_line(&d.title) || !slots.insert(d.slot)
            })
        {
            return Err("invalid or duplicate plugin operation or UI slot");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstalledPluginView {
    pub manifest: OrbitPluginManifest,
    pub enabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrbitPluginDirection {
    Horizontal,
    Vertical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum OrbitPluginTone {
    #[default]
    Neutral,
    Info,
    Success,
    Warning,
    Danger,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum OrbitPluginButtonVariant {
    #[default]
    Secondary,
    Primary,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum OrbitPluginUiNode {
    Stack {
        direction: OrbitPluginDirection,
        #[serde(default)]
        gap: u8,
        children: Vec<Self>,
    },
    Card {
        title: String,
        subtitle: Option<String>,
        children: Vec<Self>,
    },
    KeyValue {
        label: String,
        value: String,
        #[serde(default)]
        tone: OrbitPluginTone,
    },
    Badge {
        text: String,
        #[serde(default)]
        tone: OrbitPluginTone,
    },
    Table {
        columns: Vec<String>,
        rows: Vec<Vec<String>>,
    },
    /// Clients must render as text or sanitized, inert Markdown. No HTML or remote resources.
    Markdown { content: String },
    Button {
        label: String,
        action_id: String,
        #[serde(default)]
        variant: OrbitPluginButtonVariant,
        #[serde(default)]
        payload: Value,
    },
    FormInput {
        field_id: String,
        label: String,
        placeholder: Option<String>,
        value: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "RawUiSpec")]
pub struct OrbitPluginUiSpec {
    pub root: OrbitPluginUiNode,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawUiSpec {
    root: Value,
}

impl TryFrom<RawUiSpec> for OrbitPluginUiSpec {
    type Error = String;

    fn try_from(raw: RawUiSpec) -> Result<Self, Self::Error> {
        // Bound the guest-controlled tree before serde builds the recursive tagged enum.
        fn visit(node: &Value, depth: usize, remaining: &mut usize) -> bool {
            if depth > 8 || *remaining == 0 {
                return false;
            }
            *remaining -= 1;
            node.get("children")
                .and_then(Value::as_array)
                .is_none_or(|children| children.iter().all(|c| visit(c, depth + 1, remaining)))
        }
        let mut remaining = 256;
        if !visit(&raw.root, 1, &mut remaining) {
            return Err("plugin UI exceeds tree limits".into());
        }
        let spec = Self {
            root: serde_json::from_value(raw.root).map_err(|e| e.to_string())?,
        };
        spec.validate()?;
        Ok(spec)
    }
}

impl OrbitPluginUiSpec {
    /// Reject malformed or oversized guest output before sending it to a client.
    pub fn validate(&self) -> Result<(), &'static str> {
        fn payload_ok(value: &Value, depth: usize, budget: &mut usize) -> bool {
            if depth > 8 || *budget == 0 {
                return false;
            }
            *budget -= 1;
            match value {
                Value::String(s) => s.len() <= 4096,
                Value::Array(a) => {
                    a.len() <= 64 && a.iter().all(|v| payload_ok(v, depth + 1, budget))
                }
                Value::Object(o) => {
                    o.len() <= 64
                        && o.iter()
                            .all(|(k, v)| k.len() <= 128 && payload_ok(v, depth + 1, budget))
                }
                _ => true,
            }
        }
        let mut pending = vec![(&self.root, 1)];
        let mut count = 0usize;
        while let Some((node, depth)) = pending.pop() {
            count += 1;
            if depth > 8 || count > 256 {
                return Err("plugin UI exceeds tree limits");
            }
            use OrbitPluginUiNode::*;
            if let Stack { children, .. } | Card { children, .. } = node {
                if children
                    .len()
                    .saturating_add(pending.len())
                    .saturating_add(count)
                    > 256
                {
                    return Err("too many UI nodes");
                }
                pending.extend(children.iter().map(|child| (child, depth + 1)));
            }
            let valid = match node {
                Stack { gap, .. } => *gap <= 64,
                Card {
                    title, subtitle, ..
                } => {
                    title.len() <= 256
                        && is_clean_single_line(title)
                        && subtitle
                            .as_ref()
                            .is_none_or(|s| s.len() <= 512 && is_clean_single_line(s))
                }
                KeyValue { label, value, .. } => {
                    label.len() <= 256
                        && is_clean_single_line(label)
                        && value.len() <= 4096
                        && is_clean_multiline(value)
                }
                Badge { text, .. } => text.len() <= 256 && is_clean_single_line(text),
                Table { columns, rows } => {
                    !columns.is_empty()
                        && columns.len() <= 16
                        && rows.len() <= 128
                        && columns
                            .iter()
                            .all(|s| s.len() <= 256 && is_clean_single_line(s))
                        && rows.iter().all(|row| {
                            row.len() == columns.len()
                                && row.iter().all(|s| s.len() <= 1024 && is_clean_multiline(s))
                        })
                }
                Markdown { content } => is_safe_markdown(content),
                Button {
                    label,
                    action_id,
                    payload,
                    ..
                } => {
                    let mut budget = 512;
                    label.len() <= 256
                        && is_clean_single_line(label)
                        && valid_plugin_id(action_id)
                        && payload_ok(payload, 1, &mut budget)
                }
                FormInput {
                    field_id,
                    label,
                    placeholder,
                    value,
                } => {
                    valid_plugin_id(field_id)
                        && label.len() <= 256
                        && is_clean_single_line(label)
                        && placeholder
                            .as_ref()
                            .is_none_or(|s| s.len() <= 256 && is_clean_single_line(s))
                        && value
                            .as_ref()
                            .is_none_or(|s| s.len() <= 4096 && is_clean_multiline(s))
                }
            };
            if !valid {
                return Err("invalid plugin UI field, dimensions or payload");
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn slug_relative_path_and_ui_bounds() -> Result<(), serde_json::Error> {
        assert!(valid_plugin_id("0a-good_id"));
        for s in ["", "_plugin", "CAPS", "a/b", "..", "é", &"a".repeat(65)] {
            assert!(!valid_plugin_id(s));
        }
        for s in ["../plugin.wasm", "a/../b", "a//b", "/abs", "c:\\x"] {
            assert!(!valid_plugin_relative_path(s));
        }
        let leaf = OrbitPluginUiNode::Badge {
            text: "ok".into(),
            tone: OrbitPluginTone::Success,
        };
        let stack = |children| OrbitPluginUiNode::Stack {
            direction: OrbitPluginDirection::Vertical,
            gap: 0,
            children,
        };
        let mut root = leaf.clone();
        for _ in 1..8 {
            root = stack(vec![root]);
        }
        assert!(OrbitPluginUiSpec { root: root.clone() }.validate().is_ok());
        for root in [
            stack(vec![root]),
            stack(vec![leaf; 256]),
            OrbitPluginUiNode::Markdown {
                content: "x".repeat(4097),
            },
        ] {
            assert!(OrbitPluginUiSpec { root }.validate().is_err());
        }
        assert!(
            serde_json::from_value::<OrbitPluginUiNode>(json!({"type":"script","content":"x"}))
                .is_err()
        );
        assert!(
            serde_json::from_value::<OrbitPluginUiNode>(
                json!({"type":"badge","text":"x","onclick":"x"})
            )
            .is_err()
        );
        Ok(())
    }
}
