use super::core::OrbitHarmonyCore;
use super::{MobileRpcTask, run_mobile_job};
use napi_derive_ohos::napi;
use napi_ohos::bindgen_prelude::*;
mod tasks;
use tasks::{
    CreateWorkspaceEntryTask, DeleteArtifactTask, DeleteWorkspaceEntryTask, GetArtifactTask,
    ListArtifactsTask, ReadWorkspaceFileTask, RenameWorkspaceEntryTask, StageAttachmentTask,
    UpsertArtifactTask, WriteWorkspaceFileTask,
};

#[napi]
impl OrbitHarmonyCore {
    #[napi]
    pub fn git_review_json(
        &self,
        host_id: String,
        cwd: String,
        path: Option<String>,
        staged: Option<bool>,
    ) -> Result<String> {
        self.facade
            .git_review_json(&host_id, &cwd, path.as_deref(), staged)
            .map_err(Error::from_reason)
    }

    #[napi]
    pub fn terminal_screen_json(&self, host_id: String) -> Result<String> {
        self.facade
            .terminal_screen_json(&host_id)
            .map_err(Error::from_reason)
    }

    #[napi]
    pub fn terminal_screen_slot_json(&self, host_id: String, slot: u32) -> Result<String> {
        self.facade
            .terminal_screen_slot_json(&host_id, slot.max(1) as u8)
            .map_err(Error::from_reason)
    }

    #[napi]
    pub fn list_directory(&self, host_id: String, path: Option<String>) {
        self.facade.list_directory(&host_id, path);
    }

    #[napi]
    pub fn fetch_git_status(&self, host_id: String, cwd: String) {
        self.facade.fetch_git_status(&host_id, &cwd);
    }

    #[napi]
    pub fn fetch_git_diff(&self, host_id: String, cwd: String, path: Option<String>, staged: bool) {
        self.facade
            .fetch_git_diff(&host_id, &cwd, path.as_deref(), staged);
    }

    #[napi]
    pub fn fetch_workspace_files(&self, host_id: String, cwd: String) {
        self.facade.fetch_workspace_files(&host_id, &cwd);
    }

    #[napi]
    pub fn workspace_files_json(&self, host_id: String, cwd: String) -> Result<String> {
        self.facade
            .workspace_files_json(&host_id, &cwd)
            .map_err(Error::from_reason)
    }

    #[napi]
    pub fn stage_attachment_json(
        &self,
        host_id: String,
        filename: String,
        mime_type: Option<String>,
        bytes: Uint8Array,
    ) -> AsyncTask<StageAttachmentTask> {
        AsyncTask::new(StageAttachmentTask {
            facade: self.facade.clone(),
            host_id,
            filename,
            mime_type,
            bytes: bytes.to_vec(),
        })
    }

    #[napi]
    pub fn start_terminal(&self, host_id: String, cwd: String, cols: u32, rows: u32) {
        self.facade
            .start_terminal(&host_id, &cwd, cols as u16, rows as u16);
    }

    #[napi]
    pub fn attach_terminal(&self, host_id: String, terminal_id: String, slot: u32) {
        self.facade
            .attach_terminal(&host_id, &terminal_id, slot as u8);
    }

    #[napi]
    pub fn kill_terminal(&self, host_id: String, terminal_id: String) {
        self.facade.kill_terminal(&host_id, &terminal_id);
    }

    #[napi]
    pub fn send_terminal_keys(&self, host_id: String, keys: String) {
        self.facade.send_terminal_keys(&host_id, &keys);
    }

    #[napi]
    pub fn send_terminal_keys_slot(&self, host_id: String, slot: u32, keys: String) {
        self.facade
            .send_terminal_keys_slot(&host_id, slot.max(1) as u8, &keys);
    }

    #[napi]
    pub fn resize_terminal(&self, host_id: String, cols: u32, rows: u32) {
        self.facade
            .resize_terminal(&host_id, cols as u16, rows as u16);
    }

    #[napi]
    pub fn resize_terminal_slot(&self, host_id: String, slot: u32, cols: u32, rows: u32) {
        self.facade
            .resize_terminal_slot(&host_id, slot.max(1) as u8, cols as u16, rows as u16);
    }

    #[napi]
    pub fn read_workspace_file_json(
        &self,
        host_id: String,
        cwd: String,
        path: String,
    ) -> AsyncTask<ReadWorkspaceFileTask> {
        AsyncTask::new(ReadWorkspaceFileTask {
            facade: self.facade.clone(),
            host_id,
            cwd,
            path,
        })
    }

    #[napi]
    pub fn write_workspace_file_json(&self, req_json: String) -> AsyncTask<WriteWorkspaceFileTask> {
        AsyncTask::new(WriteWorkspaceFileTask {
            facade: self.facade.clone(),
            req_json,
        })
    }

    #[napi]
    pub fn create_workspace_entry_json(
        &self,
        req_json: String,
    ) -> AsyncTask<CreateWorkspaceEntryTask> {
        AsyncTask::new(CreateWorkspaceEntryTask {
            facade: self.facade.clone(),
            req_json,
        })
    }

    #[napi]
    pub fn rename_workspace_entry_json(
        &self,
        req_json: String,
    ) -> AsyncTask<RenameWorkspaceEntryTask> {
        AsyncTask::new(RenameWorkspaceEntryTask {
            facade: self.facade.clone(),
            req_json,
        })
    }

    #[napi]
    pub fn delete_workspace_entry_json(
        &self,
        req_json: String,
    ) -> AsyncTask<DeleteWorkspaceEntryTask> {
        AsyncTask::new(DeleteWorkspaceEntryTask {
            facade: self.facade.clone(),
            req_json,
        })
    }

    #[napi]
    pub fn list_artifacts_json(
        &self,
        host_id: String,
        workspace_id: Option<String>,
        agent_id: Option<String>,
    ) -> AsyncTask<ListArtifactsTask> {
        AsyncTask::new(ListArtifactsTask {
            facade: self.facade.clone(),
            host_id,
            workspace_id,
            agent_id,
        })
    }

    #[napi]
    pub fn get_artifact_json(
        &self,
        host_id: String,
        artifact_id: String,
        version: Option<u32>,
    ) -> AsyncTask<GetArtifactTask> {
        AsyncTask::new(GetArtifactTask {
            facade: self.facade.clone(),
            host_id,
            artifact_id,
            version,
        })
    }

    #[napi]
    pub fn upsert_artifact_json(&self, req_json: String) -> AsyncTask<UpsertArtifactTask> {
        AsyncTask::new(UpsertArtifactTask {
            facade: self.facade.clone(),
            req_json,
        })
    }

    #[napi]
    pub fn delete_artifact(
        &self,
        host_id: String,
        artifact_id: String,
    ) -> AsyncTask<DeleteArtifactTask> {
        AsyncTask::new(DeleteArtifactTask {
            facade: self.facade.clone(),
            host_id,
            artifact_id,
        })
    }

    #[napi]
    pub fn get_tool_call_diff_json(
        &self,
        host_id: String,
        agent_id: String,
        seq: i64,
    ) -> Result<String> {
        self.facade
            .get_tool_call_diff_json(host_id, agent_id, seq.max(0) as u64)
            .map_err(Error::from_reason)
    }

    #[napi]
    pub fn git_commit_json(
        &self,
        host_id: String,
        cwd: String,
        message: String,
        amend: bool,
    ) -> AsyncTask<MobileRpcTask<String>> {
        run_mobile_job(&self.facade, move |facade| {
            facade.git_commit_json(host_id, cwd, message, amend)
        })
    }

    #[napi]
    pub fn git_conflicts_json(
        &self,
        host_id: String,
        cwd: String,
    ) -> AsyncTask<MobileRpcTask<String>> {
        run_mobile_job(&self.facade, move |facade| {
            facade.git_conflicts_json(host_id, cwd)
        })
    }

    #[napi]
    pub fn git_create_pr_json(
        &self,
        host_id: String,
        cwd: String,
        title: String,
        body: String,
        base: Option<String>,
        draft: bool,
    ) -> AsyncTask<MobileRpcTask<String>> {
        run_mobile_job(&self.facade, move |facade| {
            facade.git_create_pr_json(host_id, cwd, title, body, base, draft)
        })
    }

    #[napi]
    pub fn git_generate_commit_message_json(
        &self,
        host_id: String,
        cwd: String,
    ) -> AsyncTask<MobileRpcTask<String>> {
        run_mobile_job(&self.facade, move |facade| {
            facade.git_generate_commit_message_json(host_id, cwd)
        })
    }

    #[napi]
    pub fn git_pr_status_json(
        &self,
        host_id: String,
        cwd: String,
    ) -> AsyncTask<MobileRpcTask<String>> {
        run_mobile_job(&self.facade, move |facade| {
            facade.git_pr_status_json(host_id, cwd)
        })
    }

    #[napi]
    pub fn git_rebase_abort_json(
        &self,
        host_id: String,
        cwd: String,
    ) -> AsyncTask<MobileRpcTask<String>> {
        run_mobile_job(&self.facade, move |facade| {
            facade.git_rebase_abort_json(host_id, cwd)
        })
    }

    #[napi]
    pub fn git_rebase_continue_json(
        &self,
        host_id: String,
        cwd: String,
    ) -> AsyncTask<MobileRpcTask<String>> {
        run_mobile_job(&self.facade, move |facade| {
            facade.git_rebase_continue_json(host_id, cwd)
        })
    }

    #[napi]
    pub fn git_rebase_skip_json(
        &self,
        host_id: String,
        cwd: String,
        expected_step: Option<u32>,
    ) -> AsyncTask<MobileRpcTask<String>> {
        run_mobile_job(&self.facade, move |facade| {
            facade.git_rebase_skip_json(host_id, cwd, expected_step)
        })
    }

    #[napi]
    pub fn git_rebase_start_json(
        &self,
        host_id: String,
        cwd: String,
        onto_ref: String,
    ) -> AsyncTask<MobileRpcTask<String>> {
        run_mobile_job(&self.facade, move |facade| {
            facade.git_rebase_start_json(host_id, cwd, onto_ref)
        })
    }

    #[napi]
    pub fn git_rebase_status_json(
        &self,
        host_id: String,
        cwd: String,
    ) -> AsyncTask<MobileRpcTask<String>> {
        run_mobile_job(&self.facade, move |facade| {
            facade.git_rebase_status_json(host_id, cwd)
        })
    }

    #[napi]
    pub fn git_repos_inspection_json(&self, host_id: String, path: String) -> Result<String> {
        self.facade
            .git_repos_inspection_json(host_id, path)
            .map_err(Error::from_reason)
    }

    #[allow(clippy::too_many_arguments)]
    #[napi]
    pub fn git_resolve_conflict_json(
        &self,
        host_id: String,
        cwd: String,
        path: String,
        block_index: Option<u32>,
        resolution_json: String,
        auto_stage: bool,
        expected_sha256: Option<String>,
    ) -> AsyncTask<MobileRpcTask<String>> {
        run_mobile_job(&self.facade, move |facade| {
            facade.git_resolve_conflict_json(
                host_id,
                cwd,
                path,
                block_index,
                resolution_json,
                auto_stage,
                expected_sha256,
            )
        })
    }

    #[napi]
    pub fn git_stage_paths_json(
        &self,
        host_id: String,
        cwd: String,
        paths: Vec<String>,
        all: bool,
    ) -> AsyncTask<MobileRpcTask<String>> {
        run_mobile_job(&self.facade, move |facade| {
            facade.git_stage_paths_json(host_id, cwd, paths, all)
        })
    }

    #[napi]
    pub fn git_stash_apply_json(
        &self,
        host_id: String,
        cwd: String,
        stash_ref: String,
        expected_oid: Option<String>,
    ) -> AsyncTask<MobileRpcTask<String>> {
        run_mobile_job(&self.facade, move |facade| {
            facade.git_stash_apply_json(host_id, cwd, stash_ref, expected_oid)
        })
    }

    #[napi]
    pub fn git_stash_drop_json(
        &self,
        host_id: String,
        cwd: String,
        stash_ref: String,
        expected_oid: Option<String>,
    ) -> AsyncTask<MobileRpcTask<String>> {
        run_mobile_job(&self.facade, move |facade| {
            facade.git_stash_drop_json(host_id, cwd, stash_ref, expected_oid)
        })
    }

    #[napi]
    pub fn git_stash_list_json(
        &self,
        host_id: String,
        cwd: String,
    ) -> AsyncTask<MobileRpcTask<String>> {
        run_mobile_job(&self.facade, move |facade| {
            facade.git_stash_list_json(host_id, cwd)
        })
    }

    #[napi]
    pub fn git_stash_pop_json(
        &self,
        host_id: String,
        cwd: String,
        stash_ref: String,
        expected_oid: Option<String>,
    ) -> AsyncTask<MobileRpcTask<String>> {
        run_mobile_job(&self.facade, move |facade| {
            facade.git_stash_pop_json(host_id, cwd, stash_ref, expected_oid)
        })
    }

    #[napi]
    pub fn git_stash_save_json(
        &self,
        host_id: String,
        cwd: String,
        message: Option<String>,
        include_untracked: bool,
    ) -> AsyncTask<MobileRpcTask<String>> {
        run_mobile_job(&self.facade, move |facade| {
            facade.git_stash_save_json(host_id, cwd, message, include_untracked)
        })
    }

    #[napi]
    pub fn git_unstage_paths_json(
        &self,
        host_id: String,
        cwd: String,
        paths: Vec<String>,
        all: bool,
    ) -> AsyncTask<MobileRpcTask<String>> {
        run_mobile_job(&self.facade, move |facade| {
            facade.git_unstage_paths_json(host_id, cwd, paths, all)
        })
    }

    #[napi]
    pub fn read_workspace_file_request_json(
        &self,
        req_json: String,
    ) -> AsyncTask<MobileRpcTask<String>> {
        run_mobile_job(&self.facade, move |facade| {
            facade.read_workspace_file_request_json(req_json)
        })
    }

    #[napi]
    pub fn request_git_repos_inspection(
        &self,
        host_id: String,
        path: String,
        extra_dirs: Vec<String>,
    ) {
        self.facade
            .request_git_repos_inspection(host_id, path, extra_dirs)
    }

    #[napi]
    pub fn send_terminal_bytes(&self, host_id: String, slot: u32, bytes: Uint8Array) {
        self.facade
            .send_terminal_bytes(host_id, slot.clamp(1, 255) as u8, bytes.to_vec())
    }
}
