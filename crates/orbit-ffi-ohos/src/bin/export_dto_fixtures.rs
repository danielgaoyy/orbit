//! Regenerate the ArkTS contract examples and the device-free streaming recording.
#[cfg(not(target_env = "ohos"))]
#[path = "fixtures/mod.rs"]
mod fixtures;

#[cfg(not(target_env = "ohos"))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use std::{env, fs, path::Path};
    let check = match env::args().skip(1).collect::<Vec<_>>().as_slice() {
        [] => false,
        [flag] if flag == "--check" => true,
        _ => return Err("usage: export_dto_fixtures [--check]".into()),
    };
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/harmony");
    let emit = |name: &str, data: String| -> Result<(), Box<dyn std::error::Error>> {
        let path = root.join(name);
        if check {
            if fs::read(&path)? != data.as_bytes() {
                return Err(format!("stale fixture: {}", path.display()).into());
            }
        } else {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&path, data)?;
        }
        Ok(())
    };
    for (name, view) in fixtures::contracts()? {
        emit(
            &format!("contract/fixtures/{name}.json"),
            format!("{}\n", serde_json::to_string_pretty(&view)?),
        )?;
    }
    emit(
        "scripts/perf/fixtures/stream-500.json",
        fixtures::recording()?,
    )?;
    Ok(())
}

#[cfg(target_env = "ohos")]
fn main() {}
