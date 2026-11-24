//! Assemble the guest's GitHub endpoint and the literal parts of its portable report.
use super::GuestSource;
use serde_json::json;

pub(super) fn source(src: &mut GuestSource) -> anyhow::Result<(&'static str, String)> {
    src.literal(
        "url",
        "https://api.github.com/repos/danielgaoyy/orbit/issues?state=open&per_page=5",
    )?;
    src.literal("ui_begin",r#"{"root":{"type":"card","title":"GitHub PR & issue board","children":[{"type":"badge","text":"Open · danielgaoyy/orbit","tone":"info"},{"type":"table","columns":["Type","Title"],"rows":["#)?;
    src.literal(
        "ui_end",
        r#"]},{"type":"button","label":"Refresh","action_id":"refresh","variant":"primary"}]}}"#,
    )?;
    src.literal("refresh", r#""refresh""#)?;
    src.literal("next", ",")?;
    src.literal("end_row", "]")?;
    src.literal("pr", r#"["Pull request","#)?;
    src.literal("issue", r#"["Issue","#)?;
    for key in ["title", "pull_request", "action_id"] {
        src.literal(key, &json!(key).to_string())?;
    }
    Ok((
        r#"(import "orbit" "host_http_get" (func $get (param i32 i32) (result i64)))"#,
        include_str!("board.wat").into(),
    ))
}
