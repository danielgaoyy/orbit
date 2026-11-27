use super::OhosCoreFacade;

impl OhosCoreFacade {
    pub fn git_review_json(
        &self,
        host_id: &str,
        cwd: &str,
        path: Option<&str>,
        staged: Option<bool>,
    ) -> Result<String, String> {
        let view = match staged {
            Some(mode) => self.inner.git_review_for_mode(host_id, cwd, path, mode),
            None => self.inner.git_review(host_id, cwd, path),
        };
        serde_json::to_string(&view).map_err(|e| e.to_string())
    }

    pub fn terminal_screen_json(&self, host_id: &str) -> Result<String, String> {
        serde_json::to_string(&self.inner.terminal_screen(host_id)).map_err(|e| e.to_string())
    }

    pub fn terminal_screen_slot_json(&self, host_id: &str, slot: u8) -> Result<String, String> {
        serde_json::to_string(&self.inner.terminal_screen_slot(host_id, slot))
            .map_err(|e| e.to_string())
    }

    pub fn fetch_git_status(&self, host_id: &str, cwd: &str) {
        self.inner.fetch_git_status(host_id, cwd);
    }

    pub fn fetch_git_diff(&self, host_id: &str, cwd: &str, path: Option<&str>, staged: bool) {
        self.inner.fetch_git_diff(host_id, cwd, path, staged);
    }

    pub fn start_terminal(&self, host_id: &str, cwd: &str, cols: u16, rows: u16) {
        self.inner.start_terminal(host_id, cwd, cols, rows);
    }

    pub fn attach_terminal(&self, host_id: &str, terminal_id: &str, slot: u8) {
        self.inner.attach_terminal(host_id, terminal_id, slot);
    }

    pub fn kill_terminal(&self, host_id: &str, terminal_id: &str) {
        self.inner.kill_terminal(host_id, terminal_id);
    }

    pub fn send_terminal_keys(&self, host_id: &str, keys: &str) {
        self.inner
            .send_terminal_input(host_id, keys.as_bytes().to_vec());
    }

    pub fn send_terminal_keys_slot(&self, host_id: &str, slot: u8, keys: &str) {
        self.inner
            .send_terminal_input_slot(host_id, slot, keys.as_bytes().to_vec());
    }

    pub fn resize_terminal(&self, host_id: &str, cols: u16, rows: u16) {
        self.inner.resize_terminal(host_id, cols, rows);
    }

    pub fn resize_terminal_slot(&self, host_id: &str, slot: u8, cols: u16, rows: u16) {
        self.inner.resize_terminal_slot(host_id, slot, cols, rows);
    }

    pub fn request_git_repos_inspection(
        &self,
        host_id: String,
        path: String,
        extra_dirs: Vec<String>,
    ) {
        self.inner
            .request_git_repos_inspection(&host_id, &path, extra_dirs);
    }

    pub fn send_terminal_bytes(&self, host_id: String, slot: u8, bytes: Vec<u8>) {
        self.inner.send_terminal_input_slot(&host_id, slot, bytes);
    }

    pub fn get_tool_call_diff_json(
        &self,
        host_id: String,
        agent_id: String,
        seq: u64,
    ) -> Result<String, String> {
        serde_json::to_string(&self.inner.get_tool_call_diff(&host_id, &agent_id, seq))
            .map_err(|e| e.to_string())
    }

    pub fn git_repos_inspection_json(
        &self,
        host_id: String,
        path: String,
    ) -> Result<String, String> {
        serde_json::to_string(&self.inner.git_repos_inspection(&host_id, &path))
            .map_err(|e| e.to_string())
    }

    pub fn git_commit_json(
        &self,
        host_id: String,
        cwd: String,
        message: String,
        amend: bool,
    ) -> Result<String, String> {
        serde_json::to_string(
            &self.inner.run_mobile_rpc(
                self.inner
                    .git_commit_on_host(&host_id, &cwd, message, amend),
            )?,
        )
        .map_err(|e| e.to_string())
    }

    pub fn git_generate_commit_message_json(
        &self,
        host_id: String,
        cwd: String,
    ) -> Result<String, String> {
        serde_json::to_string(
            &self.inner.run_mobile_rpc(
                self.inner
                    .git_generate_commit_message_on_host(&host_id, &cwd),
            )?,
        )
        .map_err(|e| e.to_string())
    }

    pub fn git_stash_list_json(&self, host_id: String, cwd: String) -> Result<String, String> {
        serde_json::to_string(
            &self
                .inner
                .run_mobile_rpc(self.inner.git_stash_list_on_host(&host_id, &cwd))?,
        )
        .map_err(|e| e.to_string())
    }

    pub fn git_stash_apply_json(
        &self,
        host_id: String,
        cwd: String,
        stash_ref: String,
        expected_oid: Option<String>,
    ) -> Result<String, String> {
        serde_json::to_string(
            &self
                .inner
                .run_mobile_rpc(self.inner.git_stash_apply_on_host(
                    &host_id,
                    &cwd,
                    stash_ref,
                    expected_oid,
                ))?,
        )
        .map_err(|e| e.to_string())
    }

    pub fn git_stash_drop_json(
        &self,
        host_id: String,
        cwd: String,
        stash_ref: String,
        expected_oid: Option<String>,
    ) -> Result<String, String> {
        serde_json::to_string(
            &self
                .inner
                .run_mobile_rpc(self.inner.git_stash_drop_on_host(
                    &host_id,
                    &cwd,
                    stash_ref,
                    expected_oid,
                ))?,
        )
        .map_err(|e| e.to_string())
    }

    pub fn git_stash_pop_json(
        &self,
        host_id: String,
        cwd: String,
        stash_ref: String,
        expected_oid: Option<String>,
    ) -> Result<String, String> {
        serde_json::to_string(&self.inner.run_mobile_rpc(self.inner.git_stash_pop_on_host(
            &host_id,
            &cwd,
            stash_ref,
            expected_oid,
        ))?)
        .map_err(|e| e.to_string())
    }

    pub fn git_create_pr_json(
        &self,
        host_id: String,
        cwd: String,
        title: String,
        body: String,
        base: Option<String>,
        draft: bool,
    ) -> Result<String, String> {
        serde_json::to_string(
            &self.inner.run_mobile_rpc(
                self.inner
                    .git_create_pr_on_host(&host_id, &cwd, title, body, base, draft),
            )?,
        )
        .map_err(|e| e.to_string())
    }

    pub fn git_pr_status_json(&self, host_id: String, cwd: String) -> Result<String, String> {
        serde_json::to_string(
            &self
                .inner
                .run_mobile_rpc(self.inner.git_pr_status_on_host(&host_id, &cwd))?,
        )
        .map_err(|e| e.to_string())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn git_resolve_conflict_json(
        &self,
        host_id: String,
        cwd: String,
        path: String,
        block_index: Option<u32>,
        resolution_json: String,
        auto_stage: bool,
        expected_sha256: Option<String>,
    ) -> Result<String, String> {
        serde_json::to_string(&self.inner.run_mobile_rpc(
            self.inner.git_resolve_conflict_on_host(
                &host_id,
                &cwd,
                path,
                block_index,
                serde_json::from_str(&resolution_json).map_err(|e| e.to_string())?,
                auto_stage,
                expected_sha256,
            ),
        )?)
        .map_err(|e| e.to_string())
    }

    pub fn git_rebase_status_json(&self, host_id: String, cwd: String) -> Result<String, String> {
        serde_json::to_string(
            &self
                .inner
                .run_mobile_rpc(self.inner.git_rebase_status_on_host(&host_id, &cwd))?,
        )
        .map_err(|e| e.to_string())
    }

    pub fn git_rebase_start_json(
        &self,
        host_id: String,
        cwd: String,
        onto_ref: String,
    ) -> Result<String, String> {
        serde_json::to_string(
            &self.inner.run_mobile_rpc(
                self.inner
                    .git_rebase_start_on_host(&host_id, &cwd, onto_ref),
            )?,
        )
        .map_err(|e| e.to_string())
    }

    pub fn git_rebase_continue_json(&self, host_id: String, cwd: String) -> Result<String, String> {
        serde_json::to_string(
            &self
                .inner
                .run_mobile_rpc(self.inner.git_rebase_continue_on_host(&host_id, &cwd))?,
        )
        .map_err(|e| e.to_string())
    }

    pub fn git_rebase_abort_json(&self, host_id: String, cwd: String) -> Result<String, String> {
        serde_json::to_string(
            &self
                .inner
                .run_mobile_rpc(self.inner.git_rebase_abort_on_host(&host_id, &cwd))?,
        )
        .map_err(|e| e.to_string())
    }

    pub fn git_rebase_skip_json(
        &self,
        host_id: String,
        cwd: String,
        expected_step: Option<u32>,
    ) -> Result<String, String> {
        serde_json::to_string(
            &self
                .inner
                .run_mobile_rpc(self.inner.git_rebase_skip_on_host(
                    &host_id,
                    &cwd,
                    expected_step,
                ))?,
        )
        .map_err(|e| e.to_string())
    }

    pub fn git_stage_paths_json(
        &self,
        host_id: String,
        cwd: String,
        paths: Vec<String>,
        all: bool,
    ) -> Result<String, String> {
        serde_json::to_string(
            &self.inner.run_mobile_rpc(
                self.inner
                    .git_stage_files_on_host(&host_id, &cwd, paths, all),
            )?,
        )
        .map_err(|e| e.to_string())
    }

    pub fn git_unstage_paths_json(
        &self,
        host_id: String,
        cwd: String,
        paths: Vec<String>,
        all: bool,
    ) -> Result<String, String> {
        serde_json::to_string(
            &self.inner.run_mobile_rpc(
                self.inner
                    .git_unstage_files_on_host(&host_id, &cwd, paths, all),
            )?,
        )
        .map_err(|e| e.to_string())
    }

    pub fn git_stash_save_json(
        &self,
        host_id: String,
        cwd: String,
        message: Option<String>,
        include_untracked: bool,
    ) -> Result<String, String> {
        serde_json::to_string(
            &self
                .inner
                .run_mobile_rpc(self.inner.git_stash_push_on_host(
                    &host_id,
                    &cwd,
                    message,
                    include_untracked,
                ))?,
        )
        .map_err(|e| e.to_string())
    }

    pub fn git_conflicts_json(&self, host_id: String, cwd: String) -> Result<String, String> {
        serde_json::to_string(
            &self
                .inner
                .run_mobile_rpc(self.inner.git_list_conflicts_on_host(&host_id, &cwd))?,
        )
        .map_err(|e| e.to_string())
    }
}
