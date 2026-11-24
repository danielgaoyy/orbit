//! Orbit MCP dispatches return a managed thread ID in the result payload.
use orbit_protocol::ToolCallDetail;
use serde_json::Value;

pub(super) fn is_launch(name: &str) -> bool {
    let normal = name
        .trim()
        .to_ascii_lowercase()
        .replace(['_', '/', ':', '.'], " ");
    let words = normal.split_whitespace().collect::<Vec<_>>().join(" ");
    let leaf = words.strip_prefix("run ").unwrap_or(&words);
    matches!(
        leaf.strip_prefix("mcp orbit ")
            .or_else(|| leaf.strip_prefix("orbit "))
            .unwrap_or(leaf),
        "create agent" | "spawn workspace peer"
    )
}

fn field<'a>(object: &'a Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter().find_map(|key| {
        object
            .get(key)?
            .as_str()
            .map(str::trim)
            .filter(|s| !s.is_empty())
    })
}

fn result_field(output: &Value, keys: &[&str], accept: fn(&str) -> bool) -> Option<String> {
    const ENVELOPE_KEYS: &[&str] = &[
        "structuredContent",
        "content",
        "result",
        "output",
        "stdout",
        "text",
    ];
    match output {
        Value::Object(_) => keys
            .iter()
            .filter_map(|key| output.get(key)?.as_str().map(str::trim))
            .find(|value| accept(value))
            .map(str::to_owned)
            .or_else(|| {
                ENVELOPE_KEYS
                    .iter()
                    .find_map(|key| result_field(output.get(key)?, keys, accept))
            }),
        Value::Array(blocks) => blocks
            .iter()
            .find_map(|block| result_field(block, keys, accept)),
        Value::String(text) => {
            if let Ok(value) = serde_json::from_str::<Value>(text) {
                return result_field(&value, keys, accept);
            }
            // Prose-prefixed and truncated results may still contain a complete JSON string field.
            keys.iter().chain(ENVELOPE_KEYS).find_map(|key| {
                let quoted = format!("\"{key}\"");
                text.match_indices(&quoted).find_map(|(index, _)| {
                    let tail = &text[index + quoted.len()..];
                    let tail = tail.trim_start().strip_prefix(':')?.trim_start();
                    let value = serde_json::Deserializer::from_str(tail)
                        .into_iter::<String>()
                        .next()?
                        .ok()?;
                    if keys.contains(key) {
                        return accept(value.trim()).then(|| value.trim().to_owned());
                    }
                    result_field(&Value::String(value), keys, accept)
                })
            })
        }
        _ => None,
    }
}

pub(super) fn child_id(output: &Value) -> Option<String> {
    result_field(output, &["child_agent_id", "agent_id"], |id| {
        id.starts_with("ag_")
    })
}

pub(super) fn acp_detail(update: &Value, previous: Option<&ToolCallDetail>) -> ToolCallDetail {
    let mut dispatch = detail(&update["rawInput"], &update["rawOutput"], previous);
    if let ToolCallDetail::Subagent {
        child_agent_id,
        role,
        ..
    } = &mut dispatch
    {
        if child_agent_id.is_none() {
            *child_agent_id = child_id(&update["content"]);
        }
        if role == "Subagent"
            && let Some(summary) = field(update, &["toolSummary"])
        {
            *role = summary.into();
        }
    }
    dispatch
}

pub(super) fn detail(
    input: &Value,
    output: &Value,
    previous: Option<&ToolCallDetail>,
) -> ToolCallDetail {
    let (old_id, old_role, old_prompt) = match previous {
        Some(ToolCallDetail::Subagent {
            child_agent_id,
            role,
            prompt_preview,
        }) => (
            child_agent_id.as_ref(),
            role.as_str(),
            prompt_preview.as_str(),
        ),
        _ => (None, "Subagent", ""),
    };
    ToolCallDetail::Subagent {
        child_agent_id: child_id(output).or_else(|| old_id.cloned()),
        role: field(
            input,
            &["title", "handle", "toolSummary", "Role", "role", "TypeName"],
        )
        .or_else(|| (old_role != "Subagent").then_some(old_role))
        .map(str::to_owned)
        .or_else(|| {
            result_field(
                output,
                &["title", "handle", "toolSummary", "Role", "role"],
                |v| !v.is_empty(),
            )
        })
        .unwrap_or_else(|| "Subagent".into()),
        prompt_preview: field(input, &["initial_prompt", "Prompt", "prompt"])
            .unwrap_or(old_prompt)
            .chars()
            .take(240)
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn extracts_managed_ids_from_acp_result_envelopes_and_text_blocks() {
        let encoded = json!({"agent_id": "ag_peer"}).to_string();
        for update in [
            json!({"rawOutput": {"stdout": encoded}}),
            json!({"rawOutput": format!("Completed: [{{\"type\":\"text\",\"text\":{}, ...", json!(encoded))}),
            json!({"content": [{"type": "content", "content": {
                "type": "text", "text": json!(encoded).to_string()
            }}]}),
            json!({"rawOutput": {"structuredContent": {"agent_id": "native"},
                "content": [{"text": "Created: {\"agent_id\":\"ag_peer\",\"title\":"}]}}),
        ] {
            assert!(
                matches!(acp_detail(&update, None),
                ToolCallDetail::Subagent { child_agent_id: Some(id), .. } if id == "ag_peer"),
                "missing child identity in {update}"
            );
        }
        assert_eq!(child_id(&json!({"text": "{\"agent_id\": \"ag_part"})), None);
    }

    #[test]
    fn recognizes_namespaced_acp_launch_titles() {
        for title in [
            "Run orbit/create_agent",
            "orbit:spawn_workspace_peer",
            "Run mcp.orbit.create_agent",
        ] {
            assert!(is_launch(title), "unrecognized launch title {title}");
        }
        assert!(!is_launch("other/create_agent"));
    }
}
