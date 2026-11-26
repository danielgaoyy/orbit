use super::{
    OhosCoreFacade, effort_to_slug, parse_badges_json, parse_client_platform, parse_effort_opt,
    parse_mode_str, parse_provider_str, parse_string_vec_json,
};
use crate::dto::OhosAskSideQuestionPayload;
use orbit_client_core::app_core::{AskSideQuestionRequest, CreateAgentRequest};
use orbit_client_core::catalog::{
    default_effort_description as core_default_effort_description,
    fallback_provider_catalog as core_fallback_provider_catalog,
    format_catalog_model_label as core_format_catalog_model_label,
    format_effort_label as core_format_effort_label,
    resolve_supported_effort_for_model as core_resolve_supported_effort_for_model,
};
use orbit_client_core::composer::ComposePromptRequest;
use orbit_client_core::contract::{
    CreateSessionRequest, MentionRankRequest, ModelSelectionRequest, SendPromptRequest,
};
use orbit_protocol::{ProviderModelCatalog, ProviderModelSpec};

impl OhosCoreFacade {
    pub fn get_composer_draft_json(&self, draft_key: &str) -> Result<Option<String>, String> {
        self.inner.get_composer_draft(draft_key)
    }

    pub fn set_composer_draft(&self, draft_key: &str, draft_json: &str) -> Result<(), String> {
        self.inner.set_composer_draft(draft_key, draft_json)
    }

    pub fn delete_composer_draft(&self, draft_key: &str) -> Result<(), String> {
        self.inner.delete_composer_draft(draft_key)
    }

    pub fn create_session_json(&self, req_json: &str) -> Result<String, String> {
        let req: CreateSessionRequest =
            serde_json::from_str(req_json).map_err(|e| e.to_string())?;
        Ok(self.inner.create_session(req))
    }

    pub fn send_prompt_json(&self, req_json: &str) -> Result<(), String> {
        let req: SendPromptRequest = serde_json::from_str(req_json).map_err(|e| e.to_string())?;
        self.inner.send_prompt_request(req);
        Ok(())
    }

    pub fn resolve_model_selection_json(&self, req_json: &str) -> Result<String, String> {
        let req: ModelSelectionRequest =
            serde_json::from_str(req_json).map_err(|e| e.to_string())?;
        serde_json::to_string(&self.inner.resolve_model_selection(&req)).map_err(|e| e.to_string())
    }

    pub fn slash_commands_json(
        &self,
        platform: Option<&str>,
        query: Option<&str>,
    ) -> Result<String, String> {
        serde_json::to_string(
            &self
                .inner
                .slash_commands(parse_client_platform(platform), query),
        )
        .map_err(|e| e.to_string())
    }

    pub fn compose_prompt_json(&self, req_json: &str) -> Result<String, String> {
        let req: ComposePromptRequest =
            serde_json::from_str(req_json).map_err(|e| e.to_string())?;
        serde_json::to_string(&self.inner.compose_prompt_for_host(&req)).map_err(|e| e.to_string())
    }

    pub fn rank_mention_candidates_json(&self, req_json: &str) -> Result<String, String> {
        let req: MentionRankRequest = serde_json::from_str(req_json).map_err(|e| e.to_string())?;
        serde_json::to_string(&self.inner.rank_mention_candidates(&req)).map_err(|e| e.to_string())
    }

    pub fn fallback_provider_catalog_json(&self, provider: &str) -> Result<String, String> {
        serde_json::to_string(&core_fallback_provider_catalog(parse_provider_str(
            provider,
        )))
        .map_err(|e| e.to_string())
    }

    pub fn fallback_provider_catalogs_json(&self) -> Result<String, String> {
        serde_json::to_string(&self.inner.fallback_provider_catalogs()).map_err(|e| e.to_string())
    }

    pub fn format_catalog_model_label(
        &self,
        catalog_json: &str,
        raw_model_id: Option<&str>,
    ) -> Result<String, String> {
        let catalog: ProviderModelCatalog =
            serde_json::from_str(catalog_json).map_err(|e| e.to_string())?;
        Ok(core_format_catalog_model_label(&catalog, raw_model_id))
    }

    pub fn format_effort_label(&self, effort: &str) -> String {
        match parse_effort_opt(Some(effort)) {
            Some(eff) => core_format_effort_label(eff).to_string(),
            None => effort.trim().to_string(),
        }
    }

    pub fn default_effort_description(&self, effort: &str) -> String {
        match parse_effort_opt(Some(effort)) {
            Some(eff) => core_default_effort_description(eff).to_string(),
            None => String::new(),
        }
    }

    pub fn resolve_supported_effort_for_model(
        &self,
        spec_json: Option<&str>,
        desired: Option<&str>,
        catalog_default: Option<&str>,
    ) -> Result<Option<String>, String> {
        let spec: Option<ProviderModelSpec> = match spec_json.map(str::trim) {
            Some(s) if !s.is_empty() && s != "null" => {
                Some(serde_json::from_str(s).map_err(|e| e.to_string())?)
            }
            _ => None,
        };
        Ok(core_resolve_supported_effort_for_model(
            spec.as_ref(),
            parse_effort_opt(desired),
            parse_effort_opt(catalog_default),
        )
        .map(|e| effort_to_slug(e).to_string()))
    }

    pub fn create_agent(
        &self,
        host_id: &str,
        cwd: &str,
        provider_str: &str,
        mode_str: &str,
        initial_prompt: &str,
        worktree_branch: Option<&str>,
    ) {
        let provider = parse_provider_str(provider_str);
        let mode = parse_mode_str(mode_str);
        self.inner.create_agent(
            host_id,
            cwd,
            provider,
            mode,
            initial_prompt,
            worktree_branch,
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub fn create_agent_full(
        &self,
        host_id: &str,
        cwd: &str,
        extra_dirs_json: Option<&str>,
        provider_str: &str,
        acp_agent_id: Option<&str>,
        model: Option<String>,
        effort_str: Option<&str>,
        mode_str: &str,
        initial_prompt: &str,
        initial_display_prompt: Option<String>,
        initial_badges_json: Option<&str>,
        worktree_branch: Option<&str>,
    ) -> Result<String, String> {
        let extra_dirs = parse_string_vec_json(extra_dirs_json)?;
        let initial_badges = parse_badges_json(initial_badges_json)?;
        let opt_id = self.inner.create_agent_with_config(CreateAgentRequest {
            host_id,
            cwd,
            extra_dirs,
            provider: parse_provider_str(provider_str),
            acp_agent_id: acp_agent_id.map(ToOwned::to_owned),
            model,
            effort: parse_effort_opt(effort_str),
            fast_mode: None,
            mode: parse_mode_str(mode_str),
            initial_prompt,
            initial_display_prompt,
            initial_badges,
            worktree_branch,
            workspace_id: None,
            handle: None,
            auto_cluster: None,
        });
        Ok(opt_id)
    }

    pub fn send_prompt(&self, host_id: &str, agent_id: &str, prompt: &str) {
        self.inner.send_prompt(host_id, agent_id, prompt);
    }

    pub fn send_prompt_with_meta(
        &self,
        host_id: &str,
        agent_id: &str,
        prompt: &str,
        display_prompt: Option<String>,
        badges_json: Option<&str>,
    ) -> Result<(), String> {
        let badges = parse_badges_json(badges_json)?;
        self.inner
            .send_prompt_with_meta(host_id, agent_id, prompt, display_prompt, badges);
        Ok(())
    }

    pub fn enqueue_prompt(
        &self,
        host_id: &str,
        agent_id: &str,
        prompt: &str,
        display_prompt: Option<String>,
        badges_json: Option<&str>,
        queue_id: Option<String>,
    ) -> Result<String, String> {
        let badges = parse_badges_json(badges_json)?;
        Ok(self
            .inner
            .enqueue_prompt(host_id, agent_id, prompt, display_prompt, badges, queue_id))
    }

    pub fn remove_queued_prompt(&self, host_id: &str, agent_id: &str, queue_id: &str) {
        self.inner.remove_queued_prompt(host_id, agent_id, queue_id);
    }

    pub fn flush_queued_prompt_now(&self, host_id: &str, agent_id: &str, queue_id: &str) {
        self.inner
            .flush_queued_prompt_now(host_id, agent_id, queue_id);
    }

    pub fn reorder_queued_prompts(
        &self,
        host_id: &str,
        agent_id: &str,
        queue_ids_json: &str,
    ) -> Result<(), String> {
        let queue_ids: Vec<String> = serde_json::from_str(queue_ids_json)
            .map_err(|e| format!("invalid queue_ids_json: {e}"))?;
        self.inner
            .reorder_queued_prompts(host_id, agent_id, queue_ids);
        Ok(())
    }

    pub fn build_side_question_context(
        &self,
        host_id: &str,
        agent_id: &str,
        max_chars: Option<usize>,
    ) -> String {
        self.inner
            .build_side_question_context(host_id, agent_id, max_chars)
    }

    pub fn ask_side_question(
        &self,
        req_json: &str,
        on_event: impl Fn(String) + Send + Sync + 'static,
    ) -> Result<String, String> {
        let payload: OhosAskSideQuestionPayload =
            serde_json::from_str(req_json).map_err(|e| e.to_string())?;
        let context_summary = match payload.context_summary.filter(|s| !s.trim().is_empty()) {
            Some(ctx) => ctx,
            None => payload
                .agent_id
                .as_deref()
                .map(|ag_id| {
                    self.inner
                        .build_side_question_context(&payload.host_id, ag_id, Some(3000))
                })
                .unwrap_or_default(),
        };
        self.inner.ask_side_question_with_callback(
            AskSideQuestionRequest {
                host_id: payload.host_id,
                agent_id: payload.agent_id,
                cwd: payload.cwd,
                provider: parse_provider_str(&payload.provider),
                model: payload.model,
                question: payload.question,
                context_summary,
            },
            move |ev| {
                if let Ok(json) = serde_json::to_string(&ev) {
                    on_event(json);
                }
            },
        )
    }
}
