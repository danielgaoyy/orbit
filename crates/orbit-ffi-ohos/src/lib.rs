//! HarmonyOS 7+ (`API 26` / `aarch64-unknown-linux-ohos`) Native N-API Bridge (`liborbit_ohos.so`).
//!
//! Uses `ohos-rs` (`napi-ohos` + `ThreadsafeFunction`) on HarmonyOS targets to drive ArkTS `@ObservedV2`
//! state models, while remaining 100% buildable and testable on macOS/Linux host toolchains.

use std::sync::Arc;

use orbit_client_core::HostRouteMode;
use orbit_client_core::app_core::{
    CoreEventSubscriber, CoreNotification, CreateAgentRequest, OrbitAppCore, TimelineItemView,
};
use orbit_client_core::stage::describe_tool_step;
use orbit_protocol::{PromptBadge, ProviderKind, ReasoningEffort, SessionMode};

fn parse_provider_str(raw: &str) -> ProviderKind {
    match raw.trim().to_ascii_lowercase().as_str() {
        "codex" => ProviderKind::Codex,
        "claude_code" | "claude" => ProviderKind::ClaudeCode,
        _ => ProviderKind::Antigravity,
    }
}

fn parse_mode_str(raw: &str) -> SessionMode {
    match raw.trim() {
        "default" | "" => SessionMode::Default,
        "plan" => SessionMode::Plan,
        "accept_edits" => SessionMode::AcceptEdits,
        "full_access" => SessionMode::FullAccess,
        custom => SessionMode::Custom(custom.to_string()),
    }
}

fn parse_effort_opt(raw: Option<&str>) -> Option<ReasoningEffort> {
    raw.and_then(ReasoningEffort::from_slug)
}

fn parse_string_vec_json(raw: Option<&str>) -> Result<Vec<String>, String> {
    let Some(trimmed) = raw.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(Vec::new());
    };
    serde_json::from_str::<Vec<String>>(trimmed).map_err(|e| e.to_string())
}

fn parse_badges_json(raw: Option<&str>) -> Result<Vec<PromptBadge>, String> {
    let Some(trimmed) = raw.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(Vec::new());
    };
    serde_json::from_str::<Vec<PromptBadge>>(trimmed).map_err(|e| e.to_string())
}

/// Pure-Rust JSON facade shared by both host unit tests and the HarmonyOS N-API wrapper.
pub struct OhosCoreFacade {
    inner: Arc<OrbitAppCore>,
}

impl OhosCoreFacade {
    pub fn open(db_path: &str) -> Result<Self, String> {
        Self::open_with_options(db_path, false)
    }

    pub fn open_with_options(db_path: &str, auto_discover_local: bool) -> Result<Self, String> {
        let inner = OrbitAppCore::open(db_path, auto_discover_local).map_err(|e| e.to_string())?;
        Ok(Self { inner })
    }

    pub fn open_in_memory() -> Result<Self, String> {
        let inner = OrbitAppCore::open_in_memory().map_err(|e| e.to_string())?;
        Ok(Self { inner })
    }

    pub fn subscribe(&self, callback: impl Fn(String) + Send + Sync + 'static) {
        self.subscribe_coalesced(0, callback);
    }

    pub fn subscribe_coalesced(
        &self,
        min_interval_ms: u64,
        callback: impl Fn(String) + Send + Sync + 'static,
    ) {
        struct FnSub<F>(F);
        impl<F: Fn(String) + Send + Sync> CoreEventSubscriber for FnSub<F> {
            fn on_notification(&self, event: CoreNotification) {
                if let Ok(json) = serde_json::to_string(&event) {
                    (self.0)(json);
                }
            }
        }
        self.inner
            .subscribe_coalesced(min_interval_ms, Arc::new(FnSub(callback)));
    }

    pub fn discover_local_daemon_uri(&self) -> Option<String> {
        self.inner.discover_local_daemon_uri()
    }

    pub fn fleet_overview_json(&self) -> Result<String, String> {
        serde_json::to_string(&self.inner.fleet_overview()).map_err(|e| e.to_string())
    }

    pub fn host_detail_json(&self, host_id: &str) -> Result<String, String> {
        serde_json::to_string(&self.inner.host_detail(host_id)).map_err(|e| e.to_string())
    }

    pub fn agent_stage_json(&self, host_id: &str, agent_id: &str) -> Result<String, String> {
        serde_json::to_string(&self.inner.agent_stage(host_id, agent_id)).map_err(|e| e.to_string())
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

    pub fn git_review_json(
        &self,
        host_id: &str,
        cwd: &str,
        path: Option<&str>,
    ) -> Result<String, String> {
        serde_json::to_string(&self.inner.git_review(host_id, cwd, path)).map_err(|e| e.to_string())
    }

    pub fn terminal_screen_json(&self, host_id: &str) -> Result<String, String> {
        serde_json::to_string(&self.inner.terminal_screen(host_id)).map_err(|e| e.to_string())
    }

    pub fn pair_host_json(&self, pairing_uri: &str) -> Result<String, String> {
        let card = self
            .inner
            .pair_host(pairing_uri)
            .map_err(|e| e.to_string())?;
        serde_json::to_string(&card).map_err(|e| e.to_string())
    }

    pub fn remove_host(&self, host_id: &str) -> Result<(), String> {
        self.inner.remove_host(host_id).map_err(|e| e.to_string())
    }

    pub fn update_host_relay(&self, host_id: &str, relay_url: Option<&str>) -> Result<(), String> {
        self.inner
            .update_host_relay(host_id, relay_url)
            .map_err(|e| e.to_string())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn update_host_config_json(
        &self,
        host_id: &str,
        custom_name: Option<String>,
        route_mode: Option<&str>,
        ssh_alias: Option<String>,
        remote_daemon_port: Option<u16>,
        custom_direct_endpoint: Option<String>,
        relay_url: Option<String>,
        reconnect: bool,
    ) -> Result<String, String> {
        let parsed_route = route_mode.map(|rm| HostRouteMode::from_opt_str(Some(rm)));
        let card = self
            .inner
            .update_host_config(
                host_id,
                custom_name,
                parsed_route,
                ssh_alias,
                remote_daemon_port,
                custom_direct_endpoint,
                relay_url,
                reconnect,
            )
            .map_err(|e| e.to_string())?;
        serde_json::to_string(&card).map_err(|e| e.to_string())
    }

    pub fn upsert_project_json(
        &self,
        project_id: Option<&str>,
        host_id: &str,
        name: &str,
        directories_json: &str,
    ) -> Result<String, String> {
        let dirs = parse_string_vec_json(Some(directories_json))?;
        let proj = self
            .inner
            .upsert_project(project_id, host_id, name, dirs)
            .map_err(|e| e.to_string())?;
        serde_json::to_string(&proj).map_err(|e| e.to_string())
    }

    pub fn remove_project(&self, project_id: &str) -> Result<(), String> {
        self.inner
            .remove_project(project_id)
            .map_err(|e| e.to_string())
    }

    pub fn list_directory(&self, host_id: &str, path: Option<String>) {
        self.inner.list_directory(host_id, path);
    }

    pub fn connect_all_hosts(&self) {
        self.inner.connect_all_hosts();
    }

    pub fn connect_host(&self, host_id: &str) {
        self.inner.connect_host_async(host_id.to_string());
    }

    pub fn refresh_host(&self, host_id: &str) {
        self.inner.refresh_host(host_id);
    }

    pub fn refresh_quotas(&self, host_id: &str) {
        self.inner.refresh_quotas(host_id);
    }

    pub fn refresh_provider_models(&self, host_id: &str, force_refresh: bool) {
        self.inner.refresh_provider_models(host_id, force_refresh);
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
            model,
            effort: parse_effort_opt(effort_str),
            mode: parse_mode_str(mode_str),
            initial_prompt,
            initial_display_prompt,
            initial_badges,
            worktree_branch,
        });
        Ok(opt_id)
    }

    pub fn open_agent_timeline(&self, host_id: &str, agent_id: &str) {
        self.inner.open_agent_timeline(host_id, agent_id);
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

    pub fn interrupt_agent(&self, host_id: &str, agent_id: &str) {
        self.inner.interrupt_agent(host_id, agent_id);
    }

    pub fn set_session_mode(&self, host_id: &str, agent_id: &str, mode_str: &str) {
        self.inner
            .set_session_mode(host_id, agent_id, parse_mode_str(mode_str));
    }

    pub fn set_agent_config(
        &self,
        host_id: &str,
        agent_id: &str,
        model: Option<String>,
        effort_str: Option<&str>,
    ) {
        self.inner
            .set_agent_config(host_id, agent_id, model, parse_effort_opt(effort_str));
    }

    pub fn switch_agent_provider(
        &self,
        host_id: &str,
        agent_id: &str,
        provider_str: &str,
        model: Option<String>,
        effort_str: Option<&str>,
    ) {
        self.inner.switch_agent_provider_with_config(
            host_id,
            agent_id,
            parse_provider_str(provider_str),
            model,
            parse_effort_opt(effort_str),
        );
    }

    pub fn archive_agent(&self, host_id: &str, agent_id: &str) {
        self.inner.archive_agent(host_id, agent_id);
    }

    pub fn resolve_permission(
        &self,
        host_id: &str,
        agent_id: &str,
        permission_request_id: &str,
        decision: &str,
    ) {
        self.inner
            .resolve_permission(host_id, agent_id, permission_request_id, decision);
    }

    pub fn answer_question(
        &self,
        host_id: &str,
        agent_id: &str,
        question_request_id: &str,
        answers_json: &str,
    ) -> Result<(), String> {
        let answers: Vec<Vec<String>> =
            serde_json::from_str(answers_json).map_err(|e| e.to_string())?;
        self.inner
            .answer_question(host_id, agent_id, question_request_id, answers);
        Ok(())
    }

    pub fn fetch_git_status(&self, host_id: &str, cwd: &str) {
        self.inner.fetch_git_status(host_id, cwd);
    }

    pub fn fetch_git_diff(&self, host_id: &str, cwd: &str, path: Option<&str>, staged: bool) {
        self.inner.fetch_git_diff(host_id, cwd, path, staged);
    }

    pub fn fetch_workspace_files(&self, host_id: &str, cwd: &str) {
        self.inner.fetch_workspace_files(host_id, cwd);
    }

    pub fn workspace_files_json(&self, host_id: &str, cwd: &str) -> Result<String, String> {
        serde_json::to_string(&self.inner.workspace_files(host_id, cwd)).map_err(|e| e.to_string())
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

    pub fn resize_terminal(&self, host_id: &str, cols: u16, rows: u16) {
        self.inner.resize_terminal(host_id, cols, rows);
    }

    pub fn on_app_foreground(&self) {
        self.inner.on_app_foreground();
    }

    pub fn on_app_background(&self) {
        self.inner.on_app_background();
    }

    pub fn on_network_changed(&self) {
        self.inner.on_network_changed();
    }
}

#[cfg(target_env = "ohos")]
mod ohos_napi {
    use std::sync::Arc;

    use napi_derive_ohos::napi;
    use napi_ohos::bindgen_prelude::*;
    use napi_ohos::threadsafe_function::{ThreadsafeFunction, ThreadsafeFunctionCallMode};

    use super::OhosCoreFacade;

    #[napi]
    pub struct OrbitHarmonyCore {
        facade: OhosCoreFacade,
    }

    #[napi]
    impl OrbitHarmonyCore {
        #[napi(constructor)]
        pub fn new(db_path: String, auto_discover_local: Option<bool>) -> Result<Self> {
            let facade =
                OhosCoreFacade::open_with_options(&db_path, auto_discover_local.unwrap_or(false))
                    .map_err(Error::from_reason)?;
            Ok(Self { facade })
        }

        #[napi(factory)]
        pub fn open_with_options(db_path: String, auto_discover_local: bool) -> Result<Self> {
            let facade = OhosCoreFacade::open_with_options(&db_path, auto_discover_local)
                .map_err(Error::from_reason)?;
            Ok(Self { facade })
        }

        #[napi]
        pub fn subscribe_events(&self, callback: ThreadsafeFunction<String, ()>) -> Result<()> {
            let tsfn = Arc::new(callback);
            self.facade.subscribe_coalesced(24, move |payload_json| {
                let _ = tsfn.call(Ok(payload_json), ThreadsafeFunctionCallMode::NonBlocking);
            });
            Ok(())
        }

        #[napi]
        pub fn discover_local_daemon_uri(&self) -> Option<String> {
            self.facade.discover_local_daemon_uri()
        }

        #[napi]
        pub fn fleet_overview_json(&self) -> Result<String> {
            self.facade
                .fleet_overview_json()
                .map_err(Error::from_reason)
        }

        #[napi]
        pub fn host_detail_json(&self, host_id: String) -> Result<String> {
            self.facade
                .host_detail_json(&host_id)
                .map_err(Error::from_reason)
        }

        #[napi]
        pub fn agent_stage_json(&self, host_id: String, agent_id: String) -> Result<String> {
            self.facade
                .agent_stage_json(&host_id, &agent_id)
                .map_err(Error::from_reason)
        }

        #[napi]
        pub fn agent_timeline_json(&self, host_id: String, agent_id: String) -> Result<String> {
            self.facade
                .agent_timeline_json(&host_id, &agent_id)
                .map_err(Error::from_reason)
        }

        #[napi]
        pub fn agent_timeline_delta_json(
            &self,
            host_id: String,
            agent_id: String,
            since_seq: i64,
        ) -> Result<String> {
            self.facade
                .agent_timeline_delta_json(&host_id, &agent_id, since_seq.max(0) as u64)
                .map_err(Error::from_reason)
        }

        #[napi]
        pub fn describe_tool_item_json(
            &self,
            item_json: String,
            cwd: Option<String>,
        ) -> Result<Option<String>> {
            self.facade
                .describe_tool_item_json(&item_json, cwd.as_deref())
                .map_err(Error::from_reason)
        }

        #[napi]
        pub fn git_review_json(
            &self,
            host_id: String,
            cwd: String,
            path: Option<String>,
        ) -> Result<String> {
            self.facade
                .git_review_json(&host_id, &cwd, path.as_deref())
                .map_err(Error::from_reason)
        }

        #[napi]
        pub fn terminal_screen_json(&self, host_id: String) -> Result<String> {
            self.facade
                .terminal_screen_json(&host_id)
                .map_err(Error::from_reason)
        }

        #[napi]
        pub fn pair_host_json(&self, pairing_uri: String) -> Result<String> {
            self.facade
                .pair_host_json(&pairing_uri)
                .map_err(Error::from_reason)
        }

        #[napi]
        pub fn remove_host(&self, host_id: String) -> Result<()> {
            self.facade
                .remove_host(&host_id)
                .map_err(Error::from_reason)
        }

        #[napi]
        pub fn update_host_relay(&self, host_id: String, relay_url: Option<String>) -> Result<()> {
            self.facade
                .update_host_relay(&host_id, relay_url.as_deref())
                .map_err(Error::from_reason)
        }

        #[napi]
        #[allow(clippy::too_many_arguments)]
        pub fn update_host_config_json(
            &self,
            host_id: String,
            custom_name: Option<String>,
            route_mode: Option<String>,
            ssh_alias: Option<String>,
            remote_daemon_port: Option<u32>,
            custom_direct_endpoint: Option<String>,
            relay_url: Option<String>,
            reconnect: Option<bool>,
        ) -> Result<String> {
            self.facade
                .update_host_config_json(
                    &host_id,
                    custom_name,
                    route_mode.as_deref(),
                    ssh_alias,
                    remote_daemon_port.map(|p| p as u16),
                    custom_direct_endpoint,
                    relay_url,
                    reconnect.unwrap_or(true),
                )
                .map_err(Error::from_reason)
        }

        #[napi]
        pub fn upsert_project_json(
            &self,
            project_id: Option<String>,
            host_id: String,
            name: String,
            directories_json: String,
        ) -> Result<String> {
            self.facade
                .upsert_project_json(project_id.as_deref(), &host_id, &name, &directories_json)
                .map_err(Error::from_reason)
        }

        #[napi]
        pub fn remove_project(&self, project_id: String) -> Result<()> {
            self.facade
                .remove_project(&project_id)
                .map_err(Error::from_reason)
        }

        #[napi]
        pub fn list_directory(&self, host_id: String, path: Option<String>) {
            self.facade.list_directory(&host_id, path);
        }

        #[napi]
        pub fn connect_all_hosts(&self) {
            self.facade.connect_all_hosts();
        }

        #[napi]
        pub fn connect_host(&self, host_id: String) {
            self.facade.connect_host(&host_id);
        }

        #[napi]
        pub fn refresh_host(&self, host_id: String) {
            self.facade.refresh_host(&host_id);
        }

        #[napi]
        pub fn refresh_quotas(&self, host_id: String) {
            self.facade.refresh_quotas(&host_id);
        }

        #[napi]
        pub fn refresh_provider_models(&self, host_id: String, force_refresh: bool) {
            self.facade.refresh_provider_models(&host_id, force_refresh);
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
        pub fn open_agent_timeline(&self, host_id: String, agent_id: String) {
            self.facade.open_agent_timeline(&host_id, &agent_id);
        }

        #[napi]
        pub fn send_prompt(&self, host_id: String, agent_id: String, prompt: String) {
            self.facade.send_prompt(&host_id, &agent_id, &prompt);
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
        pub fn interrupt_agent(&self, host_id: String, agent_id: String) {
            self.facade.interrupt_agent(&host_id, &agent_id);
        }

        #[napi]
        pub fn set_session_mode(&self, host_id: String, agent_id: String, mode: String) {
            self.facade.set_session_mode(&host_id, &agent_id, &mode);
        }

        #[napi]
        pub fn set_agent_config(
            &self,
            host_id: String,
            agent_id: String,
            model: Option<String>,
            effort: Option<String>,
        ) {
            self.facade
                .set_agent_config(&host_id, &agent_id, model, effort.as_deref());
        }

        #[napi]
        pub fn switch_agent_provider(
            &self,
            host_id: String,
            agent_id: String,
            provider: String,
            model: Option<String>,
            effort: Option<String>,
        ) {
            self.facade.switch_agent_provider(
                &host_id,
                &agent_id,
                &provider,
                model,
                effort.as_deref(),
            );
        }

        #[napi]
        pub fn archive_agent(&self, host_id: String, agent_id: String) {
            self.facade.archive_agent(&host_id, &agent_id);
        }

        #[napi]
        pub fn resolve_permission(
            &self,
            host_id: String,
            agent_id: String,
            permission_request_id: String,
            decision: String,
        ) {
            self.facade
                .resolve_permission(&host_id, &agent_id, &permission_request_id, &decision);
        }

        #[napi]
        pub fn answer_question(
            &self,
            host_id: String,
            agent_id: String,
            question_request_id: String,
            answers_json: String,
        ) -> Result<()> {
            self.facade
                .answer_question(&host_id, &agent_id, &question_request_id, &answers_json)
                .map_err(Error::from_reason)
        }

        #[napi]
        pub fn fetch_git_status(&self, host_id: String, cwd: String) {
            self.facade.fetch_git_status(&host_id, &cwd);
        }

        #[napi]
        pub fn fetch_git_diff(
            &self,
            host_id: String,
            cwd: String,
            path: Option<String>,
            staged: bool,
        ) {
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
        pub fn resize_terminal(&self, host_id: String, cols: u32, rows: u32) {
            self.facade
                .resize_terminal(&host_id, cols as u16, rows as u16);
        }

        #[napi]
        pub fn on_app_foreground(&self) {
            self.facade.on_app_foreground();
        }

        #[napi]
        pub fn on_app_background(&self) {
            self.facade.on_app_background();
        }

        #[napi]
        pub fn on_network_changed(&self) {
            self.facade.on_network_changed();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ohos_facade_exports_valid_fleet_json() {
        let facade = OhosCoreFacade::open_in_memory().expect("open");
        let json = facade.fleet_overview_json().expect("json");
        assert!(json.contains("\"hosts\":[]"));
        assert!(json.contains("\"projects\":[]"));
    }

    #[test]
    fn ohos_facade_agent_stage_project_and_lifecycle_roundtrip() {
        let facade = OhosCoreFacade::open_in_memory().expect("open");
        let kp = orbit_crypto::HostIdentityKeypair::generate().expect("keypair");
        let offer = orbit_crypto::PairingOffer {
            host_id: "host_ohos".into(),
            hostname: "matebook-fold".into(),
            daemon_pubkey_b64: kp.public_key_b64,
            direct_endpoints: Vec::new(),
            relay_url: None,
            pairing_secret: "secret".into(),
        };
        let uri = offer.to_qr_uri().expect("uri");
        let paired_json = facade.pair_host_json(&uri).expect("pair");
        assert!(paired_json.contains("host_ohos"));

        let proj_json = facade
            .upsert_project_json(
                None,
                "host_ohos",
                "orbit-harmony",
                r#"["/workspace/orbit","/workspace/shared"]"#,
            )
            .expect("upsert project");
        assert!(proj_json.contains("orbit-harmony"));

        let opt_id = facade
            .create_agent_full(
                "host_ohos",
                "/workspace/orbit",
                Some(r#"["/workspace/shared"]"#),
                "claude_code",
                Some("claude-opus-5-5".into()),
                Some("high"),
                "accept_edits",
                "Build HarmonyOS NEXT stage UI",
                Some("/plan Build HarmonyOS NEXT stage UI".into()),
                Some(r#"[{"kind":"slash","label":"/plan"}]"#),
                None,
            )
            .expect("create_agent_full");
        assert!(opt_id.starts_with("opt_"));

        let stage_json = facade
            .agent_stage_json("host_ohos", &opt_id)
            .expect("stage json");
        let stage_val: serde_json::Value = serde_json::from_str(&stage_json).expect("valid json");
        assert_eq!(stage_val["host_id"], "host_ohos");
        assert_eq!(stage_val["turns"].as_array().map(Vec::len), Some(1));

        let delta_json = facade
            .agent_timeline_delta_json("host_ohos", &opt_id, 0)
            .expect("delta json");
        assert!(delta_json.contains("Build HarmonyOS NEXT stage UI"));

        facade.on_app_background();
        facade.on_app_foreground();
        facade.on_network_changed();

        facade
            .remove_project("host_ohos::/workspace/orbit")
            .expect("remove project");
    }
}
