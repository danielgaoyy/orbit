use super::{OhosCoreFacade, parse_effort_opt, parse_mode_str, parse_provider_str};
use orbit_client_core::HostRouteMode;
use orbit_client_core::contract::UpdateHostConfigRequest;
use orbit_protocol::AcpAgentSpec;

impl OhosCoreFacade {
    pub fn update_host_config_request_json(&self, req_json: &str) -> Result<String, String> {
        let req: UpdateHostConfigRequest =
            serde_json::from_str(req_json).map_err(|e| e.to_string())?;
        let card = self
            .inner
            .update_host_config_request(req)
            .map_err(|e| e.to_string())?;
        serde_json::to_string(&card).map_err(|e| e.to_string())
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

    pub fn apply_relay_to_fleet(&self, raw_relay_url: &str) -> Result<u32, String> {
        self.inner
            .apply_relay_to_fleet(raw_relay_url)
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

    pub fn refresh_provider_models(&self, host_id: &str, force_refresh: bool) {
        self.inner.refresh_provider_models(host_id, force_refresh);
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

    pub fn set_agent_fast_mode(&self, host_id: &str, agent_id: &str, fast_mode: bool) {
        self.inner.set_agent_fast_mode(host_id, agent_id, fast_mode);
    }

    pub fn switch_agent_provider(
        &self,
        host_id: &str,
        agent_id: &str,
        provider_str: &str,
        model: Option<String>,
        effort_str: Option<&str>,
        acp_agent_id: Option<String>,
    ) {
        self.inner.switch_agent_provider_with_config(
            host_id,
            agent_id,
            parse_provider_str(provider_str),
            model,
            parse_effort_opt(effort_str),
            acp_agent_id,
        );
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

    // --- Universal ACP Agent Registry & Probing ---

    pub fn list_acp_agents_json(&self, host_id: &str) -> Result<String, String> {
        serde_json::to_string(&self.inner.list_acp_agents(host_id)).map_err(|e| e.to_string())
    }

    pub fn refresh_acp_agents(&self, host_id: &str) {
        self.inner.refresh_acp_agents(host_id);
    }

    pub fn upsert_custom_acp_agent_json(
        &self,
        host_id: &str,
        spec_json: &str,
    ) -> Result<String, String> {
        let spec: AcpAgentSpec = serde_json::from_str(spec_json).map_err(|e| e.to_string())?;
        let agents = self.inner.upsert_custom_acp_agent_blocking(host_id, spec)?;
        serde_json::to_string(&agents).map_err(|e| e.to_string())
    }

    pub fn delete_custom_acp_agent_json(
        &self,
        host_id: &str,
        agent_id: &str,
    ) -> Result<String, String> {
        let agents = self
            .inner
            .delete_custom_acp_agent_blocking(host_id, agent_id)?;
        serde_json::to_string(&agents).map_err(|e| e.to_string())
    }

    pub fn test_acp_agent_json(
        &self,
        host_id: &str,
        agent_id: &str,
        cwd: Option<String>,
    ) -> Result<String, String> {
        let res = self.inner.test_acp_agent_blocking(host_id, agent_id, cwd)?;
        serde_json::to_string(&res).map_err(|e| e.to_string())
    }

    pub fn refresh_acp_registry_json(
        &self,
        host_id: &str,
        registry_url: Option<String>,
    ) -> Result<String, String> {
        let agents = self
            .inner
            .refresh_acp_registry_blocking(host_id, registry_url)?;
        serde_json::to_string(&agents).map_err(|e| e.to_string())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn update_agent_identity(
        &self,
        host_id: String,
        agent_id: String,
        handle: Option<String>,
        title: Option<String>,
        task_summary: Option<String>,
        lock_handle: bool,
        lock_title: bool,
    ) {
        self.inner.update_agent_identity(
            &host_id,
            &agent_id,
            handle,
            title,
            task_summary,
            lock_handle,
            lock_title,
        );
    }

    pub fn upgrade_host_daemon_over_wire(
        &self,
        host_id: String,
        target_version: Option<String>,
    ) -> Result<String, String> {
        self.inner
            .upgrade_host_daemon_over_wire_blocking(&host_id, target_version)
    }

    pub fn get_guardian_config_json(&self, host_id: String) -> Result<String, String> {
        serde_json::to_string(
            &self
                .inner
                .run_mobile_rpc(self.inner.get_guardian_config_on_host(&host_id))?,
        )
        .map_err(|e| e.to_string())
    }

    pub fn update_guardian_config_json(
        &self,
        host_id: String,
        config_json: String,
    ) -> Result<String, String> {
        serde_json::to_string(&self.inner.run_mobile_rpc(
            self.inner.update_guardian_config_on_host(
                &host_id,
                serde_json::from_str(&config_json).map_err(|e| e.to_string())?,
            ),
        )?)
        .map_err(|e| e.to_string())
    }

    pub fn list_permission_rules_json(
        &self,
        host_id: String,
        project_cwd: Option<String>,
    ) -> Result<String, String> {
        serde_json::to_string(
            &self.inner.run_mobile_rpc(
                self.inner
                    .list_permission_rules_on_host(&host_id, project_cwd),
            )?,
        )
        .map_err(|e| e.to_string())
    }

    pub fn upsert_permission_rule_json(
        &self,
        host_id: String,
        rule_json: String,
    ) -> Result<String, String> {
        serde_json::to_string(&self.inner.run_mobile_rpc(
            self.inner.upsert_permission_rule_on_host(
                &host_id,
                serde_json::from_str(&rule_json).map_err(|e| e.to_string())?,
            ),
        )?)
        .map_err(|e| e.to_string())
    }

    pub fn delete_permission_rule_json(
        &self,
        host_id: String,
        rule_id: String,
    ) -> Result<String, String> {
        serde_json::to_string(
            &self
                .inner
                .run_mobile_rpc(self.inner.delete_permission_rule_on_host(&host_id, rule_id))?,
        )
        .map_err(|e| e.to_string())
    }

    pub fn resolve_permissions_batch_json(
        &self,
        host_id: String,
        items_json: String,
        decision: String,
    ) -> Result<String, String> {
        serde_json::to_string(
            &self.inner.run_mobile_rpc(
                self.inner.resolve_permissions_batch_on_host(
                    &host_id,
                    serde_json::from_str::<Vec<orbit_client_core::contract::PermissionRequestId>>(
                        &items_json,
                    )
                    .map_err(|e| e.to_string())?
                    .into_iter()
                    .map(Into::into)
                    .collect(),
                    decision,
                ),
            )?,
        )
        .map_err(|e| e.to_string())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn resolve_plan_approval_json(
        &self,
        host_id: String,
        agent_id: String,
        permission_request_id: String,
        action_json: String,
        edited_plan_markdown: Option<String>,
        feedback: Option<String>,
        target_mode_json: String,
    ) -> Result<(), String> {
        self.inner
            .run_mobile_rpc(self.inner.resolve_plan_approval_on_host(
                &host_id,
                &agent_id,
                &permission_request_id,
                serde_json::from_str(&action_json).map_err(|e| e.to_string())?,
                edited_plan_markdown,
                feedback,
                serde_json::from_str(&target_mode_json).map_err(|e| e.to_string())?,
            ))
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn get_router_config_json(&self, host_id: String) -> Result<String, String> {
        serde_json::to_string(
            &self
                .inner
                .run_mobile_rpc(self.inner.get_personal_router_status_on_host(&host_id))?,
        )
        .map_err(|e| e.to_string())
    }

    pub fn update_router_config_json(
        &self,
        host_id: String,
        config_json: String,
        external_webhook_secret: Option<String>,
        activate: bool,
    ) -> Result<String, String> {
        serde_json::to_string(&self.inner.run_mobile_rpc(
            self.inner.configure_personal_router_on_host(
                &host_id,
                serde_json::from_str(&config_json).map_err(|e| e.to_string())?,
                external_webhook_secret,
                activate,
            ),
        )?)
        .map_err(|e| e.to_string())
    }

    pub fn update_mesh_peer_json(
        &self,
        host_id: String,
        registration_id: String,
        label: Option<String>,
        enabled: Option<bool>,
    ) -> Result<(), String> {
        self.inner
            .update_connected_host(&host_id, &registration_id, label, enabled);
        Ok(())
    }
}
