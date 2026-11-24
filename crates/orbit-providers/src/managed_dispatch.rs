//! Orbit MCP dispatches return a managed thread ID in the result payload.
use orbit_protocol::ToolCallDetail;
use serde_json::Value;

pub(super) fn is_launch(name: &str) -> bool {
    let normal = name.trim().to_ascii_lowercase().replace('_', " ");
    let words = normal.split_whitespace().collect::<Vec<_>>().join(" ");
    let leaf = words.strip_prefix("run ").unwrap_or(&words);
    matches!(
        leaf.strip_prefix("mcp orbit ").unwrap_or(leaf),
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

fn result_field(output: &Value, keys: &[&str]) -> Option<String> {
    match output {
        Value::Object(_) => field(output, keys).map(str::to_owned).or_else(|| {
            ["structuredContent", "content", "result", "output", "text"]
                .iter()
                .find_map(|key| result_field(output.get(key)?, keys))
        }),
        Value::Array(blocks) => blocks.iter().find_map(|block| result_field(block, keys)),
        Value::String(text) => {
            if let Ok(value) = serde_json::from_str::<Value>(text)
                && !value.is_string()
            {
                return result_field(&value, keys);
            }
            keys.iter().find_map(|key| {
                let (_, tail) = text.split_once(&format!("\"{key}\""))?;
                let tail = tail.trim_start().strip_prefix(':')?.trim_start();
                serde_json::Deserializer::from_str(tail)
                    .into_iter::<String>()
                    .next()?
                    .ok()
            })
        }
        _ => None,
    }
}

pub(super) fn child_id(output: &Value) -> Option<String> {
    result_field(output, &["child_agent_id", "agent_id"]).filter(|id| id.starts_with("ag_"))
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
        .or_else(|| result_field(output, &["title", "handle", "toolSummary", "Role", "role"]))
        .unwrap_or_else(|| "Subagent".into()),
        prompt_preview: field(input, &["initial_prompt", "Prompt", "prompt"])
            .unwrap_or(old_prompt)
            .chars()
            .take(240)
            .collect(),
    }
}
