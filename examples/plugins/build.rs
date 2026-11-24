//! Build the example packages on the host architecture. CI also checks exact reproducibility.
use std::{fmt::Write as _, fs, path::Path};

#[path = "pr-dashboard/guest.rs"]
mod board;
#[path = "project-health/guest.rs"]
mod health;

#[derive(Default)]
struct GuestSource {
    fields: String,
    offset: usize,
}
impl GuestSource {
    /// Emit a named WAT function returning the (pointer, length) of a literal.
    fn literal(&mut self, name: &str, bytes: &str) -> anyhow::Result<()> {
        let offset = self.offset;
        self.offset += bytes.len();
        anyhow::ensure!(self.offset <= 65536, "static data exceeds its ABI region");
        write!(self.fields, "(data (i32.const {offset}) \"")?;
        for b in bytes.bytes() {
            write!(self.fields, "\\{b:02x}")?;
        }
        writeln!(
            self.fields,
            "\")\n(func ${name} (result i32 i32) i32.const {offset} i32.const {})",
            bytes.len()
        )?;
        Ok(())
    }
}

fn build(package: &str) -> anyhow::Result<Vec<u8>> {
    let mut src = GuestSource::default();
    let (imports, body) = match package {
        "project-health" => health::source(&mut src)?,
        "pr-dashboard" => board::source(&mut src)?,
        _ => anyhow::bail!("unknown example"),
    };
    let common = include_str!("abi.wat");
    Ok(wat::parse_str(format!(
        "(module {imports}\n{common}\n{}\n{body})",
        src.fields
    ))?)
}

fn main() -> anyhow::Result<()> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/plugins");
    for name in ["project-health", "pr-dashboard"] {
        fs::write(dir.join(name).join("plugin.wasm"), build(name)?)?;
    }
    Ok(())
}

#[test]
fn packaged_guests_are_reproducible() -> anyhow::Result<()> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/plugins");
    for name in ["project-health", "pr-dashboard"] {
        assert_eq!(
            fs::read(dir.join(name).join("plugin.wasm"))?,
            build(name)?,
            "{name}"
        );
    }
    Ok(())
}
