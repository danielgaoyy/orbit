use super::*;
use orbit_protocol::{AgentStatus, McpToolPolicy, ProviderKind, SessionMode, WireMessage};
use serde_json::{Value, json};
use tempfile::TempDir;

fn engine() -> Result<MemoryEngine> {
    Ok(MemoryEngine::new(
        Arc::new(OrbitStore::open_in_memory()?),
        MemoryEmbeddingConfig::default(),
        None,
    ))
}

fn draft() -> MemoryDraft {
    MemoryDraft {
        id: Some("quorum".into()),
        title: "Quorum: \"核心\"".into(),
        content: "\n# Election\n---\nQuorum requires two peers.\n".into(),
        category: MemoryCategory::Architecture,
        status: Some(MemoryStatus::Confirmed),
        file_paths: vec![r".\src\quorum.rs".into()],
        tags: vec!["colon: tag".into(), "引号".into()],
    }
}

#[tokio::test]
async fn markdown_roundtrip_edit_rename_delete_and_rebuild() -> Result<()> {
    let tmp = TempDir::new()?;
    let root = tmp.path().to_str().context("root")?;
    let e = engine()?;
    let saved = e.save(root, draft(), None).await?;
    let dir = tmp.path().join(".orbit/memory");
    let path = dir.join("quorum.md");
    let text = std::fs::read_to_string(&path)?;
    assert_eq!(parse(&saved.workspace_root, "quorum.md", &text)?, saved);
    assert_eq!(std::fs::read_dir(&dir)?.count(), 1);
    let renamed = dir.join("节点选举 Rules.md");
    std::fs::rename(&path, &renamed)?;
    std::fs::write(&renamed, text.replace("two peers", "three peers"))?;
    let e = engine()?;
    let hits = e.recall(root, "three peers".into(), None, false).await?;
    assert_eq!(
        hits[0].item.source_path.as_deref(),
        Some(".orbit/memory/节点选举 Rules.md")
    );
    assert!(hits[0].item.content.contains("three peers"));
    e.changed(root, vec!["other.rs".into()]).await?;
    assert_eq!(
        e.list(root, Some(MemoryStatus::Confirmed), None, None, None)
            .await?
            .len(),
        1
    );
    e.changed(root, vec!["src/quorum.rs".into()]).await?;
    assert!(
        e.build_confirmed_memory_context(root, "election", 1200)
            .await?
            .is_empty()
    );
    assert_eq!(
        parse(
            root,
            "节点选举 Rules.md",
            &std::fs::read_to_string(&renamed)?
        )?
        .status,
        MemoryStatus::NeedsRevalidation
    );
    let restored = e.confirm(root, saved.id).await?;
    assert_eq!(restored.status, MemoryStatus::Confirmed);
    std::fs::remove_file(renamed)?;
    assert!(e.list(root, None, None, None, None).await?.is_empty());
    Ok(())
}

#[tokio::test]
async fn atomic_concurrent_remember_and_path_confinement() -> Result<()> {
    let tmp = TempDir::new()?;
    let root = tmp.path().to_str().context("root")?;
    let e = engine()?;
    for id in ["../escape", "/absolute", "a/b", r"..\escape", ".."] {
        let mut d = draft();
        d.id = Some(id.into());
        assert!(e.save(root, d, None).await.is_err());
    }
    let mut d = draft();
    d.file_paths = vec!["../src/quorum.rs".into()];
    assert!(e.save(root, d, None).await.is_err());
    let remember = || {
        let mut d = draft();
        d.id = None;
        e.save(root, d, None)
    };
    let (a, b) = tokio::try_join!(remember(), remember())?;
    assert_eq!(a.id, b.id);
    assert_eq!(
        std::fs::read_dir(tmp.path().join(".orbit/memory"))?.count(),
        1
    );
    let mut d = draft();
    d.id = Some(a.id.clone());
    d.content = "z".repeat(MAX_BYTES);
    assert!(e.save(root, d, None).await.is_err());
    assert_eq!(
        e.list(root, None, None, None, None).await?[0].content,
        a.content
    );
    #[cfg(unix)]
    {
        let outside = TempDir::new()?;
        for scope in [".orbit", ".orbit/memory"] {
            let root_dir = TempDir::new()?;
            if scope.contains('/') {
                std::fs::create_dir(root_dir.path().join(".orbit"))?;
            }
            std::os::unix::fs::symlink(outside.path(), root_dir.path().join(scope))?;
            assert!(
                e.save(root_dir.path().to_str().context("root")?, draft(), None)
                    .await
                    .is_err()
            );
        }
        assert_eq!(std::fs::read_dir(outside.path())?.count(), 0);
        let external = outside.path().join("note");
        std::fs::write(&external, "original")?;
        std::os::unix::fs::symlink(&external, tmp.path().join(".orbit/memory/quorum.md"))?;
        assert!(e.save(root, draft(), None).await.is_err());
        assert_eq!(std::fs::read_to_string(external)?, "original");
    }
    Ok(())
}

#[tokio::test]
async fn legacy_import_is_read_only_and_scoped() -> Result<()> {
    let home = TempDir::new()?;
    let hermes = crate::platform::canonicalize_clean(home.path())?.join(".hermes");
    let legacy = hermes.join("memories");
    std::fs::create_dir_all(&legacy)?;
    let path = legacy.join("USER.md");
    std::fs::write(&path, "Use green tea")?;
    let other = TempDir::new()?;
    let e = MemoryEngine::new(
        Arc::new(OrbitStore::open_in_memory()?),
        MemoryEmbeddingConfig::default(),
        Some(legacy),
    );
    let root = hermes.to_str().context("root")?;
    let h = e.recall(root, "green tea".into(), None, true).await?;
    assert_eq!(h[0].item.source_provider.as_deref(), Some("hermes"));
    assert!(e.forget(root, h[0].item.id.clone()).await.is_err());
    assert!(e.confirm(root, h[0].item.id.clone()).await.is_err());
    assert!(
        e.recall(
            other.path().to_str().context("other")?,
            "tea".into(),
            None,
            true
        )
        .await?
        .is_empty()
    );
    assert_eq!(std::fs::read_to_string(&path)?, "Use green tea");
    Ok(())
}

#[tokio::test]
async fn dense_endpoint_and_offline_fallback() -> Result<()> {
    let tmp = TempDir::new()?;
    let root = tmp.path().to_str().context("root")?;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let config = MemoryEmbeddingConfig {
        provider: MemoryEmbeddingProvider::OpenAiCompatible,
        endpoint_url: Some(format!("http://{}/v1/embeddings", listener.local_addr()?)),
        model: Some("fake".into()),
        ..Default::default()
    };
    let e = MemoryEngine::new(Arc::new(OrbitStore::open_in_memory()?), config, None);
    let app = axum::Router::new().route(
        "/v1/embeddings",
        axum::routing::post(|axum::Json(body): axum::Json<Value>| async move {
            assert_eq!(body["model"], "fake");
            assert!(body["input"].is_string());
            axum::Json(json!({"data":[{"embedding":[0.0,2.0,0.0]}]}))
        }),
    );
    let server = tokio::spawn(async move { axum::serve(listener, app).await });
    e.save(root, draft(), None).await?;
    e.confirm(root, "quorum".into()).await?;
    let hit = e.recall(root, "爱因斯坦".into(), None, false).await?;
    assert_eq!(hit[0].fts_rank, None);
    assert_eq!(hit[0].vector_rank, Some(1));
    let merged = e
        .save(
            root,
            MemoryDraft {
                id: None,
                title: "Election".into(),
                content: "同一个规则".into(),
                ..draft()
            },
            None,
        )
        .await?;
    assert_eq!(merged.id, "quorum");
    server.abort();
    let _ = server.await;
    let h = e.recall(root, "Election".into(), None, false).await?;
    assert_eq!(h[0].vector_rank, Some(1));
    e.save(root, draft(), None).await?;
    Ok(())
}

pub(crate) async fn fixture(cwd: &str, home: &Path) -> Result<Arc<crate::DaemonRuntime>> {
    let rt = crate::DaemonRuntime::bootstrap(home).await?;
    let now = Utc::now();
    rt.store.upsert_workspace(&serde_json::from_value(json!({"workspace_id":"memory-ws","cwd":cwd,"title":"Memory tests","updated_at":now,"is_worktree":false}))?)?;
    rt.store.upsert_agent(&serde_json::from_value(json!({"workspace_id":"memory-ws","agent_id":"memory-agent","cwd":cwd,"title":"Memory tests","latest_seq":0,"created_at":now,"updated_at":now,
        "mode":SessionMode::Default,"provider":ProviderKind::Codex,"status":AgentStatus::Idle,"labels":{"orbit.mcp_packs":"core"}}))?,None)?;
    Ok(rt)
}

#[tokio::test]
async fn composite_mcp_lifecycle_and_budgeted_context() -> Result<()> {
    let project = TempDir::new()?;
    let home = TempDir::new()?;
    let cwd = project.path().to_str().context("cwd")?;
    let rt = fixture(cwd, home.path()).await?;
    let defs = crate::tools::list_tool_definitions_for_caller(
        &rt,
        Some("memory-agent"),
        &McpToolPolicy::default(),
    );
    assert_eq!(defs.len(), 12);
    let mem: Vec<_> = defs.iter().filter(|d| d.name.contains("memory")).collect();
    assert_eq!(mem.len(), 1);
    assert_eq!(mem[0].name, "orbit_memory");
    assert_eq!(
        mem[0].input_schema["properties"]["action"]["enum"],
        json!(["recall", "remember", "confirm", "forget"])
    );
    let call = |args| crate::tools::execute_tool(&rt, Some("memory-agent"), "orbit_memory", args);
    let res =
        call(json!({"action":"remember","title":"quorum","content":"<guard> & 节点".repeat(200)}))
            .await;
    assert!(!res.is_error, "{}", res.text);
    assert_eq!(res.structured["item"]["status"], "hypothesis");
    assert_eq!(res.structured["item"]["source_agent_id"], "memory-agent");
    let id = &res.structured["item"]["id"];
    let e = &rt.orchestrator.memory;
    assert!(
        e.build_confirmed_memory_context(cwd, "quorum", 1200)
            .await?
            .is_empty()
    );
    let r = call(json!({"action":"recall","query":"quorum","include_unconfirmed":true})).await;
    assert_eq!(&r.structured["hits"][0]["item"]["id"], id);
    assert!(
        call(json!({"action":"remember","title":"T","content":"C","status":"confirmed"}))
            .await
            .is_error
    );
    assert!(!call(json!({"action":"confirm","id":id})).await.is_error);
    let xml = e.build_confirmed_memory_context(cwd, "quorum", 32).await?;
    assert!(xml.starts_with("<orbit_project_memory untrusted=\"true\">\n- quorum"));
    assert!(xml.contains("&lt;guard&gt;"));
    assert!(xml.ends_with("</orbit_project_memory>"));
    assert!(xml.chars().count() <= 128);
    assert!(
        e.build_confirmed_memory_context(cwd, "", 1)
            .await?
            .is_empty()
    );
    assert!(call(json!({"action":"forget","id":id})).await.structured["deleted"] == true);
    assert!(
        e.build_confirmed_memory_context(cwd, "quorum", 1200)
            .await?
            .is_empty()
    );
    Ok(())
}

async fn rpc(rt: &Arc<crate::DaemonRuntime>, request: WireMessage) -> Result<Value> {
    let (tx, mut rx) = tokio::sync::mpsc::channel(1);
    crate::handle_wire_rpc(
        rt,
        Uuid::new_v4(),
        request,
        &mut false,
        &mut Default::default(),
        &mut Default::default(),
        &mut Default::default(),
        &mut vec![],
        &tx,
    )
    .await;
    let Some(crate::SessionFrame::Wire { msg, .. }) =
        tokio::time::timeout(Duration::from_secs(5), rx.recv()).await?
    else {
        anyhow::bail!("missing response")
    };
    Ok(serde_json::to_value(msg)?)
}

#[tokio::test]
async fn five_wire_rpcs_share_the_markdown_index() -> Result<()> {
    let project = TempDir::new()?;
    let home = TempDir::new()?;
    let cwd = project.path().to_str().context("cwd")?;
    let rt = fixture(cwd, home.path()).await?;
    let request_id = Uuid::new_v4();
    for (op, args, reply) in [
        (
            "upsert_project_memory",
            json!({"id":"rpc","title":"search","content":"unique_search_code","category":"debugging"}),
            "project_memory_saved",
        ),
        (
            "list_project_memories",
            json!({"category":"debugging","status":"hypothesis","query":"unique_search_code"}),
            "project_memories_list",
        ),
        (
            "confirm_project_memory",
            json!({"id":"rpc"}),
            "project_memory_saved",
        ),
        (
            "recall_project_memories",
            json!({"query":"unique_search_code"}),
            "project_memory_recall_result",
        ),
        (
            "delete_project_memory",
            json!({"id":"rpc"}),
            "project_memory_deleted",
        ),
    ] {
        let mut args = args;
        args["cwd"] = json!(cwd);
        args["request_id"] = json!(request_id);
        args["op"] = json!(op);
        let res = rpc(&rt, serde_json::from_value(args)?).await?;
        assert_eq!(res["op"], reply, "{res}");
        assert_eq!(res["request_id"], json!(request_id));
        match op {
            "list_project_memories" => assert_eq!(res["items"][0]["id"], "rpc"),
            "recall_project_memories" => assert_eq!(res["hits"][0]["item"]["status"], "confirmed"),
            "delete_project_memory" => assert_eq!(res["deleted"], true),
            _ => assert_eq!(res["item"]["id"], "rpc"),
        }
    }
    Ok(())
}

#[tokio::test]
async fn embedding_endpoint_blocks_ssrf_metadata_and_preserves_azure_query() -> Result<()> {
    for bad in [
        "file:///etc/passwd",
        "http://user:pass@127.0.0.1:11434/v1",
        "http://metadata.google.internal/computeMetadata/v1",
        "http://169.254.169.254/latest/meta-data",
    ] {
        let store = Arc::new(OrbitStore::open_in_memory()?);
        let engine = MemoryEngine::new(
            store,
            MemoryEmbeddingConfig {
                provider: MemoryEmbeddingProvider::OpenAiCompatible,
                endpoint_url: Some(bad.into()),
                model: Some("text-embedding-3-small".into()),
                api_key_env: None,
                auto_inject_budget_tokens: 1200,
            },
            None,
        );
        assert!(engine.dense("hello").await.is_none(), "must reject {bad}");
    }
    let azure = validate_embedding_endpoint(
        "https://example.openai.azure.com/openai/deployments/emb?api-version=2024-02-01",
    )
    .expect("azure URL valid");
    assert_eq!(azure.path(), "/openai/deployments/emb/embeddings");
    assert_eq!(azure.query(), Some("api-version=2024-02-01"));
    Ok(())
}

#[tokio::test]
async fn duplicate_markdown_id_and_cjk_budget_degrade_gracefully() -> Result<()> {
    let project = TempDir::new()?;
    let cwd = project.path().to_str().context("cwd")?;
    let mem_dir = project.path().join(".orbit/memory");
    std::fs::create_dir_all(&mem_dir)?;
    let md = "---\nid: dup1\ntitle: 第一条\ncategory: architecture\nstatus: confirmed\nconfidence: 1.0\nfile_paths:\n  - src/a.rs\ntags: []\ncreated_at_ms: 1\nupdated_at_ms: 1\n---\n重要架构约定内容非常长需要严格按字节截断避免超出中文令牌预算\n";
    std::fs::write(mem_dir.join("a.md"), md)?;
    std::fs::write(mem_dir.join("b.md"), md)?;

    let engine = MemoryEngine::new(
        Arc::new(OrbitStore::open_in_memory()?),
        MemoryEmbeddingConfig::default(),
        None,
    );
    let listed = engine.list(cwd, None, None, None, Some(10)).await?;
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, "dup1");

    // Windows backslash revalidation flips confirmed -> needs_revalidation cleanly.
    engine.changed(cwd, vec![r".\src\a.rs".into()]).await?;
    let after = engine.list(cwd, None, None, None, Some(10)).await?;
    assert_eq!(after[0].status, MemoryStatus::NeedsRevalidation);

    // UTF-8 byte budget enforcement on CJK content.
    let ctx = format_context(&listed, 40);
    assert!(
        ctx.len() <= 40 * 4,
        "ctx bytes {} exceeded budget {}",
        ctx.len(),
        40 * 4
    );
    assert!(!ctx.ends_with("- \n</orbit_project_memory>"));
    Ok(())
}
