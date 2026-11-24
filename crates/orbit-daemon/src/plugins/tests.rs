use super::*;
use orbit_protocol::{OrbitPluginCapabilities, OrbitPluginUiNode};
use serde_json::json;

fn demo(id: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/plugins")
        .join(id)
}

fn manifest() -> Result<OrbitPluginManifest> {
    let mut m = Package::local(&demo("project-health"))?.manifest;
    m.capabilities = OrbitPluginCapabilities {
        fs_read_globs: vec!["*.md".into()],
        http_domains: vec!["127.0.0.1".into(), "169.254.169.254".into()],
        max_fuel: Some(1000),
        max_memory_bytes: Some(65536),
        ..Default::default()
    };
    Ok(m)
}

fn fixture(import: &str, data: &str, body: &str, pages: u32) -> Result<Vec<u8>> {
    Ok(wat::parse_str(format!(
        r#"(module {import}
        (memory (export "memory") {pages})
        (global (export "orbit_host_buffer") i32 (i32.const 512))
        (data (i32.const 0) "{data}")
        (func (export "orbit_alloc") (param i32) (result i32) i32.const 128)
        (func (export "orbit_invoke") (param i32 i32) (result i64) {body}))"#
    ))?)
}

#[test]
fn fuel_memory_and_pointer_limits() -> Result<()> {
    for (body, expected) in [
        ("(loop $again br $again) unreachable", "fuel"),
        ("i32.const 1 memory.grow drop i64.const 0", "growth"),
        ("i64.const -1", "byte limit"),
        ("i64.const 281474976710657", "pointer"),
    ] {
        let guest = Guest::load(manifest()?, &fixture("", "", body, 1)?)?;
        let error = guest
            .run(None, b"{}", Instant::now() + DEADLINE)
            .err()
            .context("guest should fail")?;
        assert!(error.to_string().contains(expected), "{error}");
    }
    assert!(Guest::load(manifest()?, &fixture("", "", "i64.const 0", 2)?).is_err());
    Ok(())
}

#[tokio::test]
async fn host_imports_enforce_caps_and_workspace_confinement() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let root = temp.path().join("work");
    fs::create_dir(&root)?;
    fs::write(temp.path().join("other.md"), "true")?;
    #[cfg(unix)]
    std::os::unix::fs::symlink(temp.path().join("other.md"), root.join("alias.md"))?;
    fs::write(root.join("large.md"), vec![0; BUFFER_LIMIT + 1])?;
    for (host_func, param, expected) in [
        ("host_read_file", "../other.md", "capability denied"),
        ("host_read_file", "private.txt", "capability denied"),
        #[cfg(unix)]
        ("host_read_file", "alias.md", ""),
        ("host_read_file", "large.md", "bounded regular file"),
        ("host_http_get", "https://example.com", "domain denied"),
        ("host_http_get", "http://127.0.0.1", "requires HTTPS"),
        ("host_http_get", "https://127.0.0.1", "SSRF"),
        ("host_http_get", "https://169.254.169.254", "SSRF"),
    ] {
        let import =
            format!(r#"(import "orbit" "{host_func}" (func $io (param i32 i32) (result i64)))"#);
        let body = format!("i32.const 0 i32.const {} call $io", param.len());
        let guest = Guest::load(manifest()?, &fixture(&import, param, &body, 1)?)?;
        let path = root.canonicalize()?;
        let error = tokio::task::spawn_blocking(move || {
            guest.run(Some(&path), b"{}", Instant::now() + DEADLINE)
        })
        .await?
        .err()
        .context("host import should fail")?;
        assert!(
            error
                .to_string()
                .to_lowercase()
                .contains(&expected.to_lowercase()),
            "{param}: {error}"
        );
    }
    Ok(())
}

#[tokio::test]
async fn installs_survive_restart_and_snapshot_the_module() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let rt = PluginRuntime::open(temp.path())?;
    let source = demo("project-health").display().to_string();
    let (a, b) = tokio::join!(rt.install(source.clone(), true), rt.install(source, true));
    assert_ne!(a.is_ok(), b.is_ok());
    rt.set_enabled("project-health", false)?;
    let rt = PluginRuntime::open(temp.path())?;
    assert!(!rt.has_ops());
    assert!(
        rt.invoke("project-health", "check_health", None, &Value::Null)
            .await
            .is_err()
    );
    rt.set_enabled("project-health", true)?;
    let workspace = tempfile::tempdir()?;
    fs::write(workspace.path().join("README.md"), "Orbit")?;
    // P1-2: `$ORBIT_HOME` must never be readable as a plugin workspace.
    assert!(
        rt.invoke(
            "project-health",
            "check_health",
            Some(temp.path().canonicalize()?),
            &Value::Null,
        )
        .await
        .is_err()
    );
    let result = rt
        .invoke(
            "project-health",
            "check_health",
            Some(workspace.path().canonicalize()?),
            &Value::Null,
        )
        .await?;
    assert_eq!(result["checks"][2]["nonempty"], true);
    let spec = rt
        .render(
            "project-health",
            OrbitPluginUiSlot::StageCard,
            Some(workspace.path().canonicalize()?),
            None,
            None,
        )
        .await?;
    assert!(matches!(spec.root, OrbitPluginUiNode::Card { .. }));
    rt.uninstall("project-health")?;
    assert!(PluginRuntime::open(temp.path())?.list()?.is_empty());
    Ok(())
}

#[tokio::test]
async fn ui_only_plugin_does_not_enable_mcp_and_rejects_oversized_ui() -> Result<()> {
    let home = tempfile::tempdir()?;
    let source = tempfile::tempdir()?;
    let rt = PluginRuntime::open(home.path())?;
    let mut manifest = manifest()?;
    manifest.mcp_ops.clear();
    fs::write(source.path().join(MANIFEST), serde_json::to_vec(&manifest)?)?;
    let mut root = json!({"type":"markdown","content":"text"});
    for _ in 0..8 {
        root = json!({"type":"card","title":"nested","children":[root]});
    }
    let output = json!({"root":root}).to_string();
    let wasm = wat::parse_str(format!(
        r#"(module
        (memory (export "memory") 1)
        (data (i32.const 4096) "{}")
        (func (export "orbit_alloc") (param i32) (result i32) i32.const 0)
        (func (export "orbit_invoke") (param i32 i32) (result i64) i64.const {}))"#,
        output.escape_default(),
        (4096i64 << 32) | output.len() as i64
    ))?;
    fs::write(source.path().join(&manifest.wasm_entry), wasm)?;
    rt.install(source.path().display().to_string(), true)
        .await?;
    assert!(!rt.has_ops());
    let render = rt
        .render(&manifest.id, OrbitPluginUiSlot::StageCard, None, None, None)
        .await;
    assert!(
        render
            .err()
            .context("oversized guest UI accepted")?
            .to_string()
            .contains("tree limits")
    );
    Ok(())
}

#[test]
fn git_sources_pin_commits_and_reject_ambiguous_locations() {
    assert!(github_source("https://github.com/example/plugin.git").is_ok());
    assert!(
        github_source(
            "https://github.com/example/plugin.git#0123456789012345678901234567890123456789"
        )
        .is_ok()
    );
    for url in [
        "file:///tmp/p",
        "ssh://git@github.com/user/repo",
        "https://github.com@localhost/x/y",
        "https://github.com/u/r?token=x",
        "https://github.com/u/../r",
        "https://github.com/u/r#main",
    ] {
        assert!(github_source(url).is_err(), "{url}");
    }
}

// Replace only the external service in this deterministic guest contract test.
// The production network authorization path is exercised separately above.
#[test]
fn board_refresh_projects_github_items_to_inert_table_rows() -> Result<()> {
    use wasmi::{Caller, Engine, Linker, Module, Store};
    let engine = Engine::default();
    let module = Module::new(&engine, fs::read(demo("pr-dashboard").join("plugin.wasm"))?)?;
    let items =
        json!([{"number":1,"user":{"title":"nested"},"title":"Quoted \"fix\"","pull_request":{}},
        {"title":"Usage <guide>","labels":[{"name":"help"}]}])
        .to_string();
    let mut store = Store::new(&engine, ());
    let mut linker = Linker::new(&engine);
    linker.func_wrap(
        "orbit",
        "host_http_get",
        move |mut caller: Caller<'_, ()>,
              p: i32,
              n: i32|
              -> std::result::Result<i64, wasmi::Error> {
            let memory = caller
                .get_export("memory")
                .and_then(|m| m.into_memory())
                .context("missing memory")
                .map_err(|e| wasmi::Error::new(e.to_string()))?;
            let mut url = vec![0; n as usize];
            memory.read(&caller, p as usize, &mut url)?;
            assert_eq!(
                &url,
                b"https://api.github.com/repos/danielgaoyy/orbit/issues?state=open&per_page=5"
            );
            memory.write(&mut caller, 327680, items.as_bytes())?;
            Ok((327680i64 << 32) | items.len() as i64)
        },
    )?;
    let instance = linker.instantiate_and_start(&mut store, &module)?;
    let memory = instance
        .get_memory(&store, "memory")
        .context("missing memory")?;
    let invoke = instance.get_typed_func::<(i32, i32), i64>(&store, "orbit_invoke")?;
    let request =
        br#"{"kind":"render","slot":"side_panel_tab","action_id":"refresh","payload":null}"#;
    memory.write(&mut store, 65536, request)?;
    let packed = invoke.call(&mut store, (65536, request.len() as i32))? as u64;
    let mut bytes = vec![0; packed as u32 as usize];
    memory.read(&store, (packed >> 32) as usize, &mut bytes)?;
    let spec: OrbitPluginUiSpec = serde_json::from_slice(&bytes)?;
    spec.validate().map_err(anyhow::Error::msg)?;
    assert_eq!(
        serde_json::to_value(spec)?["root"]["children"][1]["rows"],
        json!([
            ["Pull request", "Quoted \"fix\""],
            ["Issue", "Usage <guide>"]
        ])
    );
    Ok(())
}
