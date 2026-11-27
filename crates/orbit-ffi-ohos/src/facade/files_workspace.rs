use super::OhosCoreFacade;
use crate::dto::{
    OhosCreateWorkspaceV2Payload, OhosHighlightWorkspaceCodePayload,
    OhosUpdateWorkspaceBoardCasPayload,
};
use orbit_client_core::contract::{
    CreateWorkspaceEntryRequest, DeleteWorkspaceEntryRequest, ExecuteWorktreeHandoffRequest,
    PreviewWorktreeHandoffRequest, RenameWorkspaceEntryRequest, SpawnWorkspacePeerRequest,
    UpsertArtifactRequest, WriteWorkspaceFileRequest,
};
use orbit_client_core::render::{
    MAX_WORKSPACE_HIGHLIGHT_LINES, highlight_workspace_code_capped_with_plain_lines,
};
use orbit_protocol::{WorkspaceIsolation, WorktreeCreateMode};

impl OhosCoreFacade {
    pub fn list_directory(&self, host_id: &str, path: Option<String>) {
        self.inner.list_directory(host_id, path);
    }

    pub fn fetch_workspace_files(&self, host_id: &str, cwd: &str) {
        self.inner.fetch_workspace_files(host_id, cwd);
    }

    pub fn workspace_files_json(&self, host_id: &str, cwd: &str) -> Result<String, String> {
        serde_json::to_string(&self.inner.workspace_files(host_id, cwd)).map_err(|e| e.to_string())
    }

    pub fn stage_attachment_json(
        &self,
        host_id: &str,
        filename: &str,
        mime_type: Option<&str>,
        bytes: Vec<u8>,
    ) -> Result<String, String> {
        let staged = self
            .inner
            .stage_attachment(host_id, filename, mime_type, bytes)
            .map_err(|e| e.to_string())?;
        serde_json::to_string(&staged).map_err(|e| e.to_string())
    }

    pub fn preview_worktree_handoff_json(&self, req_json: &str) -> Result<String, String> {
        let req: PreviewWorktreeHandoffRequest = serde_json::from_str(req_json)
            .map_err(|e| format!("invalid PreviewWorktreeHandoffRequest: {e}"))?;
        let res = self.inner.preview_worktree_handoff_blocking(req)?;
        serde_json::to_string(&res).map_err(|e| e.to_string())
    }

    pub fn execute_worktree_handoff_json(&self, req_json: &str) -> Result<String, String> {
        let req: ExecuteWorktreeHandoffRequest = serde_json::from_str(req_json)
            .map_err(|e| format!("invalid ExecuteWorktreeHandoffRequest: {e}"))?;
        let res = self.inner.execute_worktree_handoff_blocking(req)?;
        serde_json::to_string(&res).map_err(|e| e.to_string())
    }

    pub fn read_workspace_file_json(
        &self,
        host_id: &str,
        cwd: &str,
        path: &str,
    ) -> Result<String, String> {
        let file = self
            .inner
            .read_workspace_file_prefix_blocking(host_id, cwd, path)?;
        serde_json::to_string(&file).map_err(|e| e.to_string())
    }

    pub fn write_workspace_file_json(&self, req_json: &str) -> Result<String, String> {
        let req: WriteWorkspaceFileRequest = serde_json::from_str(req_json)
            .map_err(|e| format!("invalid WriteWorkspaceFileRequest: {e}"))?;
        let res = self.inner.write_workspace_file_blocking(req)?;
        serde_json::to_string(&res).map_err(|e| e.to_string())
    }

    pub fn create_workspace_entry_json(&self, req_json: &str) -> Result<String, String> {
        let req: CreateWorkspaceEntryRequest = serde_json::from_str(req_json)
            .map_err(|e| format!("invalid CreateWorkspaceEntryRequest: {e}"))?;
        let res = self.inner.create_workspace_entry_blocking(req)?;
        serde_json::to_string(&res).map_err(|e| e.to_string())
    }

    pub fn rename_workspace_entry_json(&self, req_json: &str) -> Result<String, String> {
        let req: RenameWorkspaceEntryRequest = serde_json::from_str(req_json)
            .map_err(|e| format!("invalid RenameWorkspaceEntryRequest: {e}"))?;
        let res = self.inner.rename_workspace_entry_blocking(req)?;
        serde_json::to_string(&res).map_err(|e| e.to_string())
    }

    pub fn delete_workspace_entry_json(&self, req_json: &str) -> Result<String, String> {
        let req: DeleteWorkspaceEntryRequest = serde_json::from_str(req_json)
            .map_err(|e| format!("invalid DeleteWorkspaceEntryRequest: {e}"))?;
        let res = self.inner.delete_workspace_entry_blocking(req)?;
        serde_json::to_string(&res).map_err(|e| e.to_string())
    }

    pub fn move_agent_to_workspace(
        &self,
        host_id: &str,
        agent_id: &str,
        target_workspace_id: Option<String>,
    ) {
        self.inner
            .move_agent_to_workspace(host_id, agent_id, target_workspace_id);
    }

    pub fn merge_workspaces(
        &self,
        host_id: &str,
        source_workspace_id: &str,
        target_workspace_id: &str,
    ) {
        self.inner
            .merge_workspaces(host_id, source_workspace_id, target_workspace_id);
    }

    pub fn dismiss_cluster_suggestion(&self, host_id: &str, agent_id: &str) {
        self.inner.dismiss_cluster_suggestion(host_id, agent_id);
    }

    // --- First-Class Artifacts & SEP-1865 MCP Apps ---

    pub fn list_artifacts_json(
        &self,
        host_id: &str,
        workspace_id: Option<String>,
        agent_id: Option<String>,
    ) -> Result<String, String> {
        let artifacts = self
            .inner
            .list_artifacts_blocking(host_id, workspace_id, agent_id)?;
        serde_json::to_string(&artifacts).map_err(|e| e.to_string())
    }

    pub fn get_artifact_json(
        &self,
        host_id: &str,
        artifact_id: &str,
        version: Option<u32>,
    ) -> Result<String, String> {
        let detail = self
            .inner
            .get_artifact_blocking(host_id, artifact_id, version)?;
        serde_json::to_string(&detail).map_err(|e| e.to_string())
    }

    pub fn upsert_artifact_json(&self, req_json: &str) -> Result<String, String> {
        let req: UpsertArtifactRequest =
            serde_json::from_str(req_json).map_err(|e| e.to_string())?;
        let summary = self.inner.upsert_artifact_blocking(req)?;
        serde_json::to_string(&summary).map_err(|e| e.to_string())
    }

    pub fn delete_artifact(&self, host_id: &str, artifact_id: &str) -> Result<String, String> {
        self.inner.delete_artifact_blocking(host_id, artifact_id)
    }

    // --- Mission Workspace Shared Board (`BOARD.md`), Events & Peers ---

    pub fn fetch_workspace_board(&self, host_id: &str, workspace_id: &str) {
        self.inner.fetch_workspace_board(host_id, workspace_id);
    }

    pub fn workspace_board_json(
        &self,
        host_id: &str,
        workspace_id: &str,
    ) -> Result<String, String> {
        serde_json::to_string(&self.inner.workspace_board(host_id, workspace_id))
            .map_err(|e| e.to_string())
    }

    pub fn update_workspace_board(
        &self,
        host_id: &str,
        workspace_id: &str,
        content: String,
        section: Option<String>,
        updated_by: Option<String>,
    ) {
        self.inner
            .update_workspace_board(host_id, workspace_id, content, section, updated_by);
    }

    pub fn fetch_workspace_events(&self, host_id: &str, workspace_id: &str, limit: Option<u32>) {
        self.inner
            .fetch_workspace_events(host_id, workspace_id, limit);
    }

    pub fn workspace_events_json(
        &self,
        host_id: &str,
        workspace_id: &str,
    ) -> Result<String, String> {
        serde_json::to_string(&self.inner.workspace_events(host_id, workspace_id))
            .map_err(|e| e.to_string())
    }

    pub fn spawn_workspace_peer_json(&self, req_json: &str) -> Result<String, String> {
        let req: SpawnWorkspacePeerRequest = serde_json::from_str(req_json)
            .map_err(|e| format!("invalid SpawnWorkspacePeerRequest: {e}"))?;
        Ok(self.inner.spawn_workspace_peer(req))
    }

    #[allow(clippy::too_many_arguments)]
    pub fn create_mission_workspace(
        &self,
        host_id: String,
        source_cwd: String,
        title: String,
        mission_goal: Option<String>,
        use_worktree: bool,
        worktree_branch: Option<String>,
        base_ref: Option<String>,
    ) {
        let isolation = if use_worktree {
            orbit_protocol::WorkspaceIsolation::Worktree
        } else {
            orbit_protocol::WorkspaceIsolation::Local
        };
        self.inner.create_mission_workspace(
            &host_id,
            &source_cwd,
            title,
            mission_goal,
            isolation,
            worktree_branch,
            base_ref,
        );
    }

    pub fn rename_workspace(&self, host_id: String, workspace_id: String, title: String) {
        self.inner.rename_workspace(&host_id, &workspace_id, &title);
    }

    pub fn update_workspace_goal(&self, host_id: String, workspace_id: String, goal: String) {
        self.inner
            .update_workspace_goal(&host_id, &workspace_id, goal);
    }

    pub fn archive_workspace(
        &self,
        host_id: String,
        workspace_id: String,
        keep_agents_unclassified: bool,
    ) {
        self.inner
            .archive_workspace_v2(&host_id, &workspace_id, keep_agents_unclassified);
    }

    pub fn move_agent_to_workspace_v2(
        &self,
        host_id: String,
        agent_id: String,
        target_workspace_id: Option<String>,
        new_workspace_title: Option<String>,
        make_unclassified: bool,
    ) {
        self.inner.move_agent_to_workspace_v2(
            &host_id,
            &agent_id,
            target_workspace_id,
            new_workspace_title,
            make_unclassified,
        );
    }

    pub fn organize_project_workspaces(
        &self,
        host_id: String,
        project_root: String,
        agent_ids: Vec<String>,
        auto_apply: bool,
    ) {
        self.inner
            .organize_project_workspaces(&host_id, &project_root, agent_ids, auto_apply);
    }

    pub fn read_workspace_file_request_json(&self, req_json: String) -> Result<String, String> {
        let r: orbit_client_core::contract::ReadWorkspaceFileRequest =
            serde_json::from_str(&req_json).map_err(|e| e.to_string())?;
        let file = self.inner.read_workspace_file_request_blocking(r)?;
        serde_json::to_string(&file).map_err(|e| e.to_string())
    }

    pub fn highlight_workspace_code_json(&self, req_json: &str) -> Result<String, String> {
        let req: OhosHighlightWorkspaceCodePayload = serde_json::from_str(req_json)
            .map_err(|e| format!("invalid highlight_workspace_code request JSON: {e}"))?;
        let requested = req
            .max_lines
            .map(|n| n as usize)
            .filter(|&n| n > 0)
            .unwrap_or(MAX_WORKSPACE_HIGHLIGHT_LINES);
        let capped = requested.min(MAX_WORKSPACE_HIGHLIGHT_LINES);
        let view = highlight_workspace_code_capped_with_plain_lines(
            &req.language,
            &req.code,
            capped,
            &req.plain_line_numbers,
        );
        serde_json::to_string(&view).map_err(|e| e.to_string())
    }

    pub fn create_workspace_v2_json(&self, req_json: &str) -> Result<(), String> {
        let req: OhosCreateWorkspaceV2Payload = serde_json::from_str(req_json)
            .map_err(|e| format!("invalid create_workspace_v2 request JSON: {e}"))?;
        if req.host_id.trim().is_empty() {
            return Err("create_workspace_v2 requires non-empty host_id".into());
        }
        if req.source_cwd.trim().is_empty() {
            return Err("create_workspace_v2 requires non-empty source_cwd".into());
        }
        let isolation = match req.isolation.as_deref().map(str::trim) {
            None | Some("") | Some("local") => WorkspaceIsolation::Local,
            Some("worktree") => WorkspaceIsolation::Worktree,
            Some(other) => return Err(format!("unsupported workspace isolation `{other}`")),
        };
        let worktree_mode = match req.worktree_mode.as_deref().map(str::trim) {
            None | Some("") | Some("branch_off") => WorktreeCreateMode::BranchOff,
            Some("checkout_branch") => WorktreeCreateMode::CheckoutBranch,
            Some("checkout_pr") => WorktreeCreateMode::CheckoutPr,
            Some(other) => return Err(format!("unsupported worktree_mode `{other}`")),
        };
        let title = req
            .title
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned);
        let worktree_branch = req
            .worktree_branch
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned);
        let base_ref = req
            .base_ref
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned);
        self.inner.create_workspace_v2(
            &req.host_id,
            &req.source_cwd,
            title,
            isolation,
            worktree_mode,
            worktree_branch,
            base_ref,
            req.pr_number,
        );
        Ok(())
    }

    pub fn archive_workspace_v2_json(
        &self,
        host_id: &str,
        workspace_id: &str,
        keep_agents_unclassified: bool,
    ) -> Result<String, String> {
        if host_id.trim().is_empty() {
            return Err("archive_workspace_v2 requires non-empty host_id".into());
        }
        if workspace_id.trim().is_empty() {
            return Err("archive_workspace_v2 requires non-empty workspace_id".into());
        }
        self.inner
            .run_mobile_rpc(self.inner.archive_workspace_confirmed(
                host_id,
                workspace_id,
                keep_agents_unclassified,
            ))?;
        Ok(r#"{"ok":true}"#.to_string())
    }

    pub fn unarchive_workspace_json(
        &self,
        host_id: &str,
        workspace_id: &str,
    ) -> Result<String, String> {
        if host_id.trim().is_empty() {
            return Err("unarchive_workspace requires non-empty host_id".into());
        }
        if workspace_id.trim().is_empty() {
            return Err("unarchive_workspace requires non-empty workspace_id".into());
        }
        self.inner.run_mobile_rpc(
            self.inner
                .unarchive_workspace_confirmed(host_id, workspace_id),
        )?;
        Ok(r#"{"ok":true}"#.to_string())
    }

    pub fn rename_workspace_confirmed_json(
        &self,
        host_id: &str,
        workspace_id: &str,
        title: &str,
    ) -> Result<String, String> {
        if host_id.trim().is_empty() {
            return Err("rename_workspace_confirmed requires non-empty host_id".into());
        }
        if workspace_id.trim().is_empty() {
            return Err("rename_workspace_confirmed requires non-empty workspace_id".into());
        }
        let trimmed_title = title.trim();
        if trimmed_title.is_empty() {
            return Err("rename_workspace_confirmed requires non-empty title".into());
        }
        self.inner
            .run_mobile_rpc(self.inner.rename_workspace_confirmed(
                host_id,
                workspace_id,
                trimmed_title,
            ))?;
        Ok(r#"{"ok":true}"#.to_string())
    }

    pub fn request_workspace_scripts(
        &self,
        host_id: &str,
        workspace_id: &str,
    ) -> Result<(), String> {
        if host_id.trim().is_empty() {
            return Err("request_workspace_scripts requires non-empty host_id".into());
        }
        if workspace_id.trim().is_empty() {
            return Err("request_workspace_scripts requires non-empty workspace_id".into());
        }
        self.inner.list_workspace_scripts(host_id, workspace_id);
        Ok(())
    }

    pub fn start_workspace_script_json(
        &self,
        host_id: &str,
        workspace_id: &str,
        script_name: &str,
    ) -> Result<(), String> {
        if host_id.trim().is_empty() {
            return Err("start_workspace_script requires non-empty host_id".into());
        }
        if workspace_id.trim().is_empty() {
            return Err("start_workspace_script requires non-empty workspace_id".into());
        }
        if script_name.trim().is_empty() {
            return Err("start_workspace_script requires non-empty script_name".into());
        }
        self.inner
            .start_workspace_script(host_id, workspace_id, script_name);
        Ok(())
    }

    pub fn stop_workspace_script_json(
        &self,
        host_id: &str,
        workspace_id: &str,
        script_name: &str,
    ) -> Result<(), String> {
        if host_id.trim().is_empty() {
            return Err("stop_workspace_script requires non-empty host_id".into());
        }
        if workspace_id.trim().is_empty() {
            return Err("stop_workspace_script requires non-empty workspace_id".into());
        }
        if script_name.trim().is_empty() {
            return Err("stop_workspace_script requires non-empty script_name".into());
        }
        self.inner
            .stop_workspace_script(host_id, workspace_id, script_name);
        Ok(())
    }

    pub fn update_workspace_board_cas_json(&self, req_json: &str) -> Result<String, String> {
        let req: OhosUpdateWorkspaceBoardCasPayload = serde_json::from_str(req_json)
            .map_err(|e| format!("invalid update_workspace_board_cas request JSON: {e}"))?;
        if req.host_id.trim().is_empty() {
            return Err("update_workspace_board_cas requires non-empty host_id".into());
        }
        if req.workspace_id.trim().is_empty() {
            return Err("update_workspace_board_cas requires non-empty workspace_id".into());
        }
        let outcome = self
            .inner
            .run_mobile_rpc(self.inner.update_workspace_board_cas_on_host(
                &req.host_id,
                &req.workspace_id,
                req.content,
                req.section,
                req.expected_revision,
                req.updated_by,
            ))?;
        serde_json::to_string(&outcome).map_err(|e| e.to_string())
    }
}
