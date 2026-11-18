//! HarmonyOS 7+ (`API 26` / `aarch64-unknown-linux-ohos`) Native N-API Bridge (`liborbit_ohos.so`).
//!
//! Uses `ohos-rs` (`napi-ohos` + `ThreadsafeFunction`) on HarmonyOS targets to drive ArkTS `@ObservedV2`
//! state models, while remaining 100% buildable and testable on macOS/Linux host toolchains.

use std::sync::Arc;

use orbit_client_core::app_core::{CoreEventSubscriber, CoreNotification, OrbitAppCore};
use orbit_protocol::{ProviderKind, SessionMode};

/// Pure-Rust JSON facade shared by both host unit tests and the HarmonyOS N-API wrapper.
pub struct OhosCoreFacade {
    inner: Arc<OrbitAppCore>,
}

impl OhosCoreFacade {
    pub fn open(db_path: &str) -> Result<Self, String> {
        let inner = OrbitAppCore::open(db_path, false).map_err(|e| e.to_string())?;
        Ok(Self { inner })
    }

    pub fn open_in_memory() -> Result<Self, String> {
        let inner = OrbitAppCore::open_in_memory().map_err(|e| e.to_string())?;
        Ok(Self { inner })
    }

    pub fn subscribe(&self, callback: impl Fn(String) + Send + Sync + 'static) {
        struct FnSub<F>(F);
        impl<F: Fn(String) + Send + Sync> CoreEventSubscriber for FnSub<F> {
            fn on_notification(&self, event: CoreNotification) {
                if let Ok(json) = serde_json::to_string(&event) {
                    (self.0)(json);
                }
            }
        }
        self.inner.subscribe(Arc::new(FnSub(callback)));
    }

    pub fn fleet_overview_json(&self) -> Result<String, String> {
        serde_json::to_string(&self.inner.fleet_overview()).map_err(|e| e.to_string())
    }

    pub fn host_detail_json(&self, host_id: &str) -> Result<String, String> {
        serde_json::to_string(&self.inner.host_detail(host_id)).map_err(|e| e.to_string())
    }

    pub fn agent_timeline_json(&self, host_id: &str, agent_id: &str) -> Result<String, String> {
        serde_json::to_string(&self.inner.agent_timeline(host_id, agent_id))
            .map_err(|e| e.to_string())
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

    pub fn create_agent(
        &self,
        host_id: &str,
        cwd: &str,
        provider_str: &str,
        mode_str: &str,
        initial_prompt: &str,
        worktree_branch: Option<&str>,
    ) {
        let provider = match provider_str {
            "codex" => ProviderKind::Codex,
            "claude_code" | "claude" => ProviderKind::ClaudeCode,
            _ => ProviderKind::Antigravity,
        };
        let mode = match mode_str {
            "plan" => SessionMode::Plan,
            "accept_edits" => SessionMode::AcceptEdits,
            "full_access" => SessionMode::FullAccess,
            _ => SessionMode::Default,
        };
        self.inner.create_agent(
            host_id,
            cwd,
            provider,
            mode,
            initial_prompt,
            worktree_branch,
        );
    }

    pub fn open_agent_timeline(&self, host_id: &str, agent_id: &str) {
        self.inner.open_agent_timeline(host_id, agent_id);
    }

    pub fn send_prompt(&self, host_id: &str, agent_id: &str, prompt: &str) {
        self.inner.send_prompt(host_id, agent_id, prompt);
    }

    pub fn interrupt_agent(&self, host_id: &str, agent_id: &str) {
        self.inner.interrupt_agent(host_id, agent_id);
    }

    pub fn set_session_mode(&self, host_id: &str, agent_id: &str, mode_str: &str) {
        let mode = match mode_str {
            "plan" => SessionMode::Plan,
            "accept_edits" => SessionMode::AcceptEdits,
            "full_access" => SessionMode::FullAccess,
            _ => SessionMode::Default,
        };
        self.inner.set_session_mode(host_id, agent_id, mode);
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

    pub fn start_terminal(&self, host_id: &str, cwd: &str, cols: u16, rows: u16) {
        self.inner.start_terminal(host_id, cwd, cols, rows);
    }

    pub fn send_terminal_keys(&self, host_id: &str, keys: &str) {
        self.inner
            .send_terminal_input(host_id, keys.as_bytes().to_vec());
    }

    pub fn resize_terminal(&self, host_id: &str, cols: u16, rows: u16) {
        self.inner.resize_terminal(host_id, cols, rows);
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
        pub fn new(db_path: String) -> Result<Self> {
            let facade = OhosCoreFacade::open(&db_path).map_err(Error::from_reason)?;
            Ok(Self { facade })
        }

        #[napi]
        pub fn subscribe_events(&self, callback: ThreadsafeFunction<String, ()>) -> Result<()> {
            let tsfn = Arc::new(callback);
            self.facade.subscribe(move |payload_json| {
                let _ = tsfn.call(Ok(payload_json), ThreadsafeFunctionCallMode::NonBlocking);
            });
            Ok(())
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
        pub fn agent_timeline_json(&self, host_id: String, agent_id: String) -> Result<String> {
            self.facade
                .agent_timeline_json(&host_id, &agent_id)
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
        pub fn open_agent_timeline(&self, host_id: String, agent_id: String) {
            self.facade.open_agent_timeline(&host_id, &agent_id);
        }

        #[napi]
        pub fn send_prompt(&self, host_id: String, agent_id: String, prompt: String) {
            self.facade.send_prompt(&host_id, &agent_id, &prompt);
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
        pub fn start_terminal(&self, host_id: String, cwd: String, cols: u32, rows: u32) {
            self.facade
                .start_terminal(&host_id, &cwd, cols as u16, rows as u16);
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
    }
}
