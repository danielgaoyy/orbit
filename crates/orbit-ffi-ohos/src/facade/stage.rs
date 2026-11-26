use super::OhosCoreFacade;
use orbit_client_core::app_core::TimelineItemView;
use orbit_client_core::contract::{AgentStageDeltaRequest, ForkAgentRequest, RenderDiagramRequest};
use orbit_client_core::diagram::{
    DiagramKind, detect_diagram_kind as core_detect_diagram_kind,
    render_diagram as core_render_diagram,
};
use orbit_client_core::export::ExportOptions;
use orbit_client_core::stage::describe_tool_step;

impl OhosCoreFacade {
    pub fn agent_stage_turns_slice_json(
        &self,
        host_id: &str,
        agent_id: &str,
        start_turn_index: u32,
        count: u32,
    ) -> Result<String, String> {
        let turns = self.inner.agent_stage_turns_slice(
            host_id,
            agent_id,
            start_turn_index as usize,
            count as usize,
        );
        serde_json::to_string(&turns).map_err(|error| error.to_string())
    }

    pub fn export_agent_transcript_json(
        &self,
        host_id: &str,
        agent_id: &str,
        options_json: &str,
    ) -> Result<String, String> {
        let opts: ExportOptions = if options_json.trim().is_empty() {
            ExportOptions::default()
        } else {
            serde_json::from_str(options_json).map_err(|e| e.to_string())?
        };
        let exported = self
            .inner
            .export_agent_transcript(host_id, agent_id, &opts)?;
        serde_json::to_string(&exported).map_err(|e| e.to_string())
    }

    pub fn list_agent_code_blocks_json(
        &self,
        host_id: &str,
        agent_id: &str,
    ) -> Result<String, String> {
        let blocks = self.inner.list_agent_code_blocks(host_id, agent_id)?;
        serde_json::to_string(&blocks).map_err(|e| e.to_string())
    }

    /// Play a performance recording through the existing event and delta-pull API.
    pub fn debug_replay_stage_deltas_json(
        &self,
        host_id: &str,
        agent_id: &str,
        frames_json: &str,
    ) -> Result<(), String> {
        #[cfg(feature = "perf-hooks")]
        {
            crate::replay::Replay::start(&self.replay, host_id, agent_id, frames_json)
        }
        #[cfg(not(feature = "perf-hooks"))]
        {
            let _ = (host_id, agent_id, frames_json);
            Err("perf hooks disabled".into())
        }
    }

    #[cfg(feature = "perf-hooks")]
    fn replay_delta_json(&self, req: &AgentStageDeltaRequest) -> Result<Option<String>, String> {
        let delta = self.replay.lock().map_err(|e| e.to_string())?.delta(req);
        delta
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .map_err(|e| e.to_string())
    }

    pub fn agent_stage_delta_by_request_json(&self, req_json: &str) -> Result<String, String> {
        let req: AgentStageDeltaRequest =
            serde_json::from_str(req_json).map_err(|e| e.to_string())?;
        #[cfg(feature = "perf-hooks")]
        if let Some(json) = self.replay_delta_json(&req)? {
            return Ok(json);
        }
        serde_json::to_string(&self.inner.agent_stage_delta_by_request(&req))
            .map_err(|e| e.to_string())
    }

    pub fn agent_stage_json(&self, host_id: &str, agent_id: &str) -> Result<String, String> {
        serde_json::to_string(&self.inner.agent_stage(host_id, agent_id)).map_err(|e| e.to_string())
    }

    pub fn agent_stage_delta_json(
        &self,
        host_id: &str,
        agent_id: &str,
        since_revision: u64,
    ) -> Result<String, String> {
        #[cfg(feature = "perf-hooks")]
        if let Some(json) = self.replay_delta_json(&AgentStageDeltaRequest {
            host_id: host_id.into(),
            agent_id: agent_id.into(),
            since_revision,
            ..Default::default()
        })? {
            return Ok(json);
        }
        serde_json::to_string(
            &self
                .inner
                .agent_stage_delta(host_id, agent_id, since_revision),
        )
        .map_err(|e| e.to_string())
    }

    pub fn agent_timeline_json(&self, host_id: &str, agent_id: &str) -> Result<String, String> {
        serde_json::to_string(&self.inner.agent_timeline(host_id, agent_id))
            .map_err(|e| e.to_string())
    }

    pub fn agent_timeline_delta_json(
        &self,
        host_id: &str,
        agent_id: &str,
        since_seq: u64,
    ) -> Result<String, String> {
        serde_json::to_string(
            &self
                .inner
                .agent_timeline_delta(host_id, agent_id, since_seq),
        )
        .map_err(|e| e.to_string())
    }

    pub fn describe_tool_item_json(
        &self,
        item_json: &str,
        cwd: Option<&str>,
    ) -> Result<Option<String>, String> {
        let item: TimelineItemView = serde_json::from_str(item_json).map_err(|e| e.to_string())?;
        match describe_tool_step(&item, cwd) {
            Some(desc) => serde_json::to_string(&desc)
                .map(Some)
                .map_err(|e| e.to_string()),
            None => Ok(None),
        }
    }

    pub fn open_agent_timeline(&self, host_id: &str, agent_id: &str) {
        self.inner.open_agent_timeline(host_id, agent_id);
    }

    pub fn load_older_agent_timeline(
        &self,
        host_id: &str,
        agent_id: &str,
        limit_events: u32,
    ) -> bool {
        self.inner
            .load_older_agent_timeline(host_id, agent_id, limit_events)
    }

    pub fn interrupt_agent(&self, host_id: &str, agent_id: &str) {
        self.inner.interrupt_agent(host_id, agent_id);
    }

    pub fn interrupt_agent_and_continue(&self, host_id: &str, agent_id: &str) {
        self.inner.interrupt_agent_and_continue(host_id, agent_id);
    }

    pub fn archive_agent(&self, host_id: &str, agent_id: &str) {
        self.inner.archive_agent(host_id, agent_id);
    }

    pub fn unarchive_agent(&self, host_id: &str, agent_id: &str) {
        self.inner.unarchive_agent(host_id, agent_id);
    }

    pub fn delete_agent(&self, host_id: &str, agent_id: &str) {
        self.inner.delete_agent(host_id, agent_id);
    }

    pub fn rewind_agent_json(
        &self,
        host_id: &str,
        agent_id: &str,
        to_before_seq: u64,
        restore_files: bool,
    ) -> Result<String, String> {
        let res =
            self.inner
                .rewind_agent_blocking(host_id, agent_id, to_before_seq, restore_files)?;
        serde_json::to_string(&res).map_err(|e| e.to_string())
    }

    pub fn fork_agent_json(&self, req_json: &str) -> Result<String, String> {
        let req: ForkAgentRequest =
            serde_json::from_str(req_json).map_err(|e| format!("invalid ForkAgentRequest: {e}"))?;
        let res = self.inner.fork_agent_blocking(req)?;
        serde_json::to_string(&res).map_err(|e| e.to_string())
    }

    pub fn detect_diagram_kind(&self, language: &str, source: &str) -> Option<String> {
        core_detect_diagram_kind(language, source).map(|kind| match kind {
            DiagramKind::Mermaid => "mermaid".to_string(),
            DiagramKind::Svg => "svg".to_string(),
            DiagramKind::Graphviz => "graphviz".to_string(),
        })
    }

    pub fn render_diagram_json(&self, req_json: &str) -> Result<String, String> {
        let req: RenderDiagramRequest =
            serde_json::from_str(req_json).map_err(|e| e.to_string())?;
        let rendered = core_render_diagram(&req);
        serde_json::to_string(&rendered).map_err(|e| e.to_string())
    }

    pub fn parse_markdown_preview_json(
        &self,
        markdown: &str,
        streaming: bool,
    ) -> Result<String, String> {
        serde_json::to_string(&orbit_client_core::render::parse_markdown_blocks(
            markdown, streaming,
        ))
        .map_err(|e| e.to_string())
    }

    pub fn detach_agent(&self, host_id: String, agent_id: String) {
        self.inner.detach_agent(&host_id, &agent_id);
    }

    pub fn summarize_agent(&self, host_id: String, agent_id: String) {
        self.inner.summarize_agent(&host_id, &agent_id);
    }

    pub fn list_provider_subagents(&self, host_id: String, parent_agent_id: String) {
        self.inner
            .list_provider_subagents(&host_id, &parent_agent_id);
    }

    pub fn open_provider_subagent_timeline(
        &self,
        host_id: String,
        parent_agent_id: String,
        subagent_id: String,
    ) {
        self.inner
            .open_provider_subagent_timeline(&host_id, &parent_agent_id, &subagent_id);
    }

    pub fn provider_subagent_timeline_json(
        &self,
        host_id: String,
        parent_agent_id: String,
        subagent_id: String,
    ) -> Result<String, String> {
        serde_json::to_string(&self.inner.provider_subagent_timeline(
            &host_id,
            &parent_agent_id,
            &subagent_id,
        ))
        .map_err(|e| e.to_string())
    }

    pub fn rewind_agent_request_json(&self, req_json: String) -> Result<String, String> {
        let r: orbit_client_core::contract::RewindAgentRequest =
            serde_json::from_str(&req_json).map_err(|e| e.to_string())?;
        self.rewind_agent_json(&r.host_id, &r.agent_id, r.to_before_seq, r.restore_files)
    }
}
