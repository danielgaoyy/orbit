use super::core::OrbitHarmonyCore;
use napi_derive_ohos::napi;
use napi_ohos::bindgen_prelude::*;
use napi_ohos::threadsafe_function::{ThreadsafeFunction, ThreadsafeFunctionCallMode};
use std::sync::Arc;

#[napi]
impl OrbitHarmonyCore {
    #[napi]
    pub fn create_session_json(&self, req_json: String) -> Result<String> {
        self.facade
            .create_session_json(&req_json)
            .map_err(Error::from_reason)
    }

    #[napi]
    pub fn send_prompt_json(&self, req_json: String) -> Result<()> {
        self.facade
            .send_prompt_json(&req_json)
            .map_err(Error::from_reason)
    }

    #[napi]
    pub fn resolve_model_selection_json(&self, req_json: String) -> Result<String> {
        self.facade
            .resolve_model_selection_json(&req_json)
            .map_err(Error::from_reason)
    }

    #[napi]
    pub fn slash_commands_json(
        &self,
        platform: Option<String>,
        query: Option<String>,
    ) -> Result<String> {
        self.facade
            .slash_commands_json(platform.as_deref(), query.as_deref())
            .map_err(Error::from_reason)
    }

    #[napi]
    pub fn compose_prompt_json(&self, req_json: String) -> Result<String> {
        self.facade
            .compose_prompt_json(&req_json)
            .map_err(Error::from_reason)
    }

    #[napi]
    pub fn rank_mention_candidates_json(&self, req_json: String) -> Result<String> {
        self.facade
            .rank_mention_candidates_json(&req_json)
            .map_err(Error::from_reason)
    }

    #[napi]
    pub fn fallback_provider_catalog_json(&self, provider: String) -> Result<String> {
        self.facade
            .fallback_provider_catalog_json(&provider)
            .map_err(Error::from_reason)
    }

    #[napi]
    pub fn fallback_provider_catalogs_json(&self) -> Result<String> {
        self.facade
            .fallback_provider_catalogs_json()
            .map_err(Error::from_reason)
    }

    #[napi]
    pub fn format_catalog_model_label(
        &self,
        catalog_json: String,
        raw_model_id: Option<String>,
    ) -> Result<String> {
        self.facade
            .format_catalog_model_label(&catalog_json, raw_model_id.as_deref())
            .map_err(Error::from_reason)
    }

    #[napi]
    pub fn format_effort_label(&self, effort: String) -> String {
        self.facade.format_effort_label(&effort)
    }

    #[napi]
    pub fn default_effort_description(&self, effort: String) -> String {
        self.facade.default_effort_description(&effort)
    }

    #[napi]
    pub fn resolve_supported_effort_for_model(
        &self,
        spec_json: Option<String>,
        desired: Option<String>,
        catalog_default: Option<String>,
    ) -> Result<Option<String>> {
        self.facade
            .resolve_supported_effort_for_model(
                spec_json.as_deref(),
                desired.as_deref(),
                catalog_default.as_deref(),
            )
            .map_err(Error::from_reason)
    }

    #[napi]
    pub fn create_agent(
        &self,
        host_id: String,
        cwd: String,
        provider: String,
        mode: String,
        initial_prompt: String,
        worktree_branch: Option<String>,
    ) {
        self.facade.create_agent(
            &host_id,
            &cwd,
            &provider,
            &mode,
            &initial_prompt,
            worktree_branch.as_deref(),
        );
    }

    #[napi]
    #[allow(clippy::too_many_arguments)]
    pub fn create_agent_full(
        &self,
        host_id: String,
        cwd: String,
        extra_dirs_json: Option<String>,
        provider: String,
        acp_agent_id: Option<String>,
        model: Option<String>,
        effort: Option<String>,
        mode: String,
        initial_prompt: String,
        initial_display_prompt: Option<String>,
        initial_badges_json: Option<String>,
        worktree_branch: Option<String>,
    ) -> Result<String> {
        self.facade
            .create_agent_full(
                &host_id,
                &cwd,
                extra_dirs_json.as_deref(),
                &provider,
                acp_agent_id.as_deref(),
                model,
                effort.as_deref(),
                &mode,
                &initial_prompt,
                initial_display_prompt,
                initial_badges_json.as_deref(),
                worktree_branch.as_deref(),
            )
            .map_err(Error::from_reason)
    }

    #[napi]
    pub fn send_prompt(&self, host_id: String, agent_id: String, prompt: String) {
        self.facade.send_prompt(&host_id, &agent_id, &prompt);
    }

    #[napi]
    pub fn set_account_default_mode(&self, mode: String, updated_at_ms: Option<i64>) {
        self.facade
            .set_account_default_mode(&mode, updated_at_ms.unwrap_or(0));
    }

    #[napi]
    pub fn send_prompt_with_meta(
        &self,
        host_id: String,
        agent_id: String,
        prompt: String,
        display_prompt: Option<String>,
        badges_json: Option<String>,
    ) -> Result<()> {
        self.facade
            .send_prompt_with_meta(
                &host_id,
                &agent_id,
                &prompt,
                display_prompt,
                badges_json.as_deref(),
            )
            .map_err(Error::from_reason)
    }

    #[napi]
    pub fn enqueue_prompt(
        &self,
        host_id: String,
        agent_id: String,
        prompt: String,
        display_prompt: Option<String>,
        badges_json: Option<String>,
        queue_id: Option<String>,
    ) -> Result<String> {
        self.facade
            .enqueue_prompt(
                &host_id,
                &agent_id,
                &prompt,
                display_prompt,
                badges_json.as_deref(),
                queue_id,
            )
            .map_err(Error::from_reason)
    }

    #[napi]
    pub fn remove_queued_prompt(&self, host_id: String, agent_id: String, queue_id: String) {
        self.facade
            .remove_queued_prompt(&host_id, &agent_id, &queue_id);
    }

    #[napi]
    pub fn flush_queued_prompt_now(&self, host_id: String, agent_id: String, queue_id: String) {
        self.facade
            .flush_queued_prompt_now(&host_id, &agent_id, &queue_id);
    }

    #[napi]
    pub fn reorder_queued_prompts(
        &self,
        host_id: String,
        agent_id: String,
        queue_ids_json: String,
    ) -> Result<()> {
        self.facade
            .reorder_queued_prompts(&host_id, &agent_id, &queue_ids_json)
            .map_err(Error::from_reason)
    }

    #[napi]
    pub fn build_side_question_context(
        &self,
        host_id: String,
        agent_id: String,
        max_chars: Option<u32>,
    ) -> String {
        self.facade
            .build_side_question_context(&host_id, &agent_id, max_chars.map(|v| v as usize))
    }

    #[napi]
    pub fn ask_side_question(
        &self,
        req_json: String,
        callback: ThreadsafeFunction<String, ()>,
    ) -> Result<String> {
        let tsfn = Arc::new(callback);
        self.facade
            .ask_side_question(&req_json, move |payload_json| {
                let _ = tsfn.call(Ok(payload_json), ThreadsafeFunctionCallMode::NonBlocking);
            })
            .map_err(Error::from_reason)
    }
}
