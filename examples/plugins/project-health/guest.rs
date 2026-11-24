//! Generate the eight possible reports. Guest bytecode selects one after probing the workspace.
use super::GuestSource;
use serde_json::json;
use std::fmt::Write as _;

pub(super) fn source(src: &mut GuestSource) -> anyhow::Result<(&'static str, String)> {
    let files = ["Cargo.toml", "package.json", "README.md"];
    for (i, file) in files.iter().enumerate() {
        src.literal(&format!("path{i}"), file)?;
    }
    let mut body = String::from(include_str!("health.wat"));
    for mask in 0..8 {
        let checks: Vec<_> = files
            .iter()
            .enumerate()
            .map(|(i, path)| {
                json!({
                    "path": path, "nonempty": mask & (1 << i) != 0
                })
            })
            .collect();
        src.literal(
            &format!("result{mask}"),
            &json!({"checks":checks}).to_string(),
        )?;
        let children: Vec<_> = files
            .iter()
            .enumerate()
            .map(|(i, file)| {
                let present = mask & (1 << i) != 0;
                let (value, tone) = if present {
                    ("Present", "success")
                } else {
                    ("Missing or empty", "neutral")
                };
                json!({ "type": "key_value", "label": file, "value": value, "tone": tone })
            })
            .collect();
        src.literal(
            &format!("card{mask}"),
            &json!({"root":{"type":"card","title":"Project health",
            "subtitle":"Workspace manifests and documentation","children":children}})
            .to_string(),
        )?;
        writeln!(
            body,
            "(if (i32.eq (local.get $mask) (i32.const {mask})) (then
            (if (call $is_call (local.get $req)) (then call $result{mask} call $append)
                (else call $card{mask} call $append)) (return (call $finish))))"
        )?;
    }
    body.push_str("unreachable)");
    Ok((
        r#"(import "orbit" "host_read_file" (func $read (param i32 i32) (result i64)))"#,
        body,
    ))
}
