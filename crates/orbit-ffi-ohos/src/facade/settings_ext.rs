use super::OhosCoreFacade;
pub(crate) use super::memory_cwd_cache::BoundedMemoryCwdCache;
use orbit_protocol::ProjectMemoryItem;

fn normalize_required_id(value: &str, field: &str) -> Result<String, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(format!("{field} cannot be empty"));
    }
    Ok(trimmed.to_string())
}

/// Validates a required filesystem path argument without stripping meaningful leading/trailing spaces.
fn validate_required_path(value: &str, field: &str) -> Result<String, String> {
    if value.trim().is_empty() {
        return Err(format!("{field} cannot be empty"));
    }
    Ok(value.to_string())
}

/// Validates an optional filesystem path argument without stripping meaningful leading/trailing spaces.
fn validate_optional_path(value: Option<String>) -> Option<String> {
    value.filter(|raw| !raw.trim().is_empty())
}

impl OhosCoreFacade {
    pub(crate) fn record_project_memory_cwds(
        &self,
        host_id: &str,
        raw_request_cwd: &str,
        items: &[ProjectMemoryItem],
    ) {
        let Ok(mut cache) = self.memory_cwd_by_host_and_id.lock() else {
            return;
        };
        let host_key = host_id.trim();
        if host_key.is_empty() {
            return;
        }
        let has_request_cwd = !raw_request_cwd.trim().is_empty();
        for item in items {
            let id = item.id.trim();
            if id.is_empty() {
                continue;
            }
            let has_canonical_root = !item.workspace_root.trim().is_empty();
            if has_request_cwd && has_canonical_root {
                cache.record_workspace_alias(
                    host_key,
                    raw_request_cwd,
                    item.workspace_root.as_str(),
                );
            }
            let authoritative_cwd = if has_canonical_root {
                item.workspace_root.as_str()
            } else if has_request_cwd {
                raw_request_cwd
            } else {
                continue;
            };
            cache.record(host_key, id, authoritative_cwd);
        }
    }

    pub(crate) fn resolve_project_memory_cwd(
        &self,
        host_id: &str,
        memory_id: &str,
        explicit_cwd: Option<String>,
    ) -> Result<String, String> {
        if let Some(exact_cwd) = validate_optional_path(explicit_cwd) {
            return Ok(exact_cwd);
        }
        let cache = self
            .memory_cwd_by_host_and_id
            .lock()
            .map_err(|_| "memory cwd cache lock poisoned".to_string())?;
        cache.resolve_legacy_two_arg(host_id.trim(), memory_id.trim())
    }

    pub(crate) fn record_project_memory_deleted_for_workspace(
        &self,
        host_id: &str,
        memory_id: &str,
        raw_request_cwd: &str,
        was_explicit_cwd: bool,
    ) {
        if let Ok(mut cache) = self.memory_cwd_by_host_and_id.lock() {
            cache.record_deleted_for_workspace(
                host_id.trim(),
                memory_id.trim(),
                raw_request_cwd,
                was_explicit_cwd,
            );
        }
    }

    #[cfg(test)]
    pub(crate) fn evict_project_memory_cwd_for_workspace(
        &self,
        host_id: &str,
        memory_id: &str,
        exact_cwd: &str,
    ) {
        if let Ok(mut cache) = self.memory_cwd_by_host_and_id.lock() {
            cache.evict_for_workspace(host_id.trim(), memory_id.trim(), exact_cwd);
        }
    }

    #[cfg(test)]
    pub(crate) fn memory_cwd_cache_len(&self) -> usize {
        self.memory_cwd_by_host_and_id
            .lock()
            .map(|cache| cache.len())
            .unwrap_or(0)
    }

    pub fn export_host_pairing_uri(&self, host_id: &str) -> Result<String, String> {
        let host_id = normalize_required_id(host_id, "host_id")?;
        self.inner
            .export_host_pairing_uri(&host_id)
            .map_err(|e| e.to_string())
    }

    pub fn get_usage_insights_json(&self, host_id: &str, days: u32) -> Result<String, String> {
        let host_id = normalize_required_id(host_id, "host_id")?;
        let clamped_days = days.clamp(1, 90);
        let snapshot = self.inner.run_mobile_rpc(
            self.inner
                .get_usage_insights_on_host(&host_id, clamped_days),
        )?;
        serde_json::to_string(&snapshot).map_err(|e| e.to_string())
    }

    pub fn list_external_mcp_servers_json(
        &self,
        host_id: &str,
        cwd: Option<String>,
    ) -> Result<String, String> {
        let host_id = normalize_required_id(host_id, "host_id")?;
        let exact_cwd = validate_optional_path(cwd);
        let servers = self.inner.run_mobile_rpc(
            self.inner
                .list_external_mcp_servers_async(&host_id, exact_cwd),
        )?;
        serde_json::to_string(&servers).map_err(|e| e.to_string())
    }

    pub fn toggle_external_mcp_server_json(
        &self,
        host_id: &str,
        server_id: &str,
        enabled: bool,
        cwd: Option<String>,
    ) -> Result<(), String> {
        let host_id = normalize_required_id(host_id, "host_id")?;
        let server_id = normalize_required_id(server_id, "server_id")?;
        let exact_cwd = validate_optional_path(cwd);
        self.inner.run_mobile_rpc(
            self.inner
                .toggle_external_mcp_server_async(&host_id, &server_id, enabled, exact_cwd),
        )?;
        Ok(())
    }

    pub fn list_managed_skills_json(
        &self,
        host_id: &str,
        cwd: Option<String>,
    ) -> Result<String, String> {
        let host_id = normalize_required_id(host_id, "host_id")?;
        let exact_cwd = validate_optional_path(cwd);
        let skills = self
            .inner
            .run_mobile_rpc(self.inner.list_managed_skills_async(&host_id, exact_cwd))?;
        serde_json::to_string(&skills).map_err(|e| e.to_string())
    }

    pub fn list_project_memories_json(
        &self,
        host_id: &str,
        cwd: &str,
        limit: u32,
    ) -> Result<String, String> {
        let host_id = normalize_required_id(host_id, "host_id")?;
        let exact_cwd = validate_required_path(cwd, "cwd")?;
        let normalized_limit = Some(limit.clamp(1, 500));
        let items = self.inner.run_mobile_rpc(self.inner.list_project_memories(
            &host_id,
            &exact_cwd,
            None,
            None,
            None,
            normalized_limit,
        ))?;
        self.record_project_memory_cwds(&host_id, &exact_cwd, &items);
        serde_json::to_string(&items).map_err(|e| e.to_string())
    }

    pub fn confirm_project_memory_json(
        &self,
        host_id: &str,
        memory_id: &str,
        cwd: Option<String>,
    ) -> Result<(), String> {
        let host_id = normalize_required_id(host_id, "host_id")?;
        let memory_id = normalize_required_id(memory_id, "memory_id")?;
        let resolved_cwd = self.resolve_project_memory_cwd(&host_id, &memory_id, cwd)?;
        let saved = self
            .inner
            .run_mobile_rpc(self.inner.confirm_project_memory(
                &host_id,
                &resolved_cwd,
                &memory_id,
            ))?;
        self.record_project_memory_cwds(&host_id, &resolved_cwd, std::slice::from_ref(&saved));
        Ok(())
    }

    pub fn delete_project_memory_json(
        &self,
        host_id: &str,
        memory_id: &str,
        cwd: Option<String>,
    ) -> Result<(), String> {
        let host_id = normalize_required_id(host_id, "host_id")?;
        let memory_id = normalize_required_id(memory_id, "memory_id")?;
        let explicit_cwd = validate_optional_path(cwd);
        let was_explicit_cwd = explicit_cwd.is_some();
        let resolved_cwd = self.resolve_project_memory_cwd(&host_id, &memory_id, explicit_cwd)?;
        let deleted = self.inner.run_mobile_rpc(self.inner.delete_project_memory(
            &host_id,
            &resolved_cwd,
            &memory_id,
        ))?;
        if !deleted {
            return Err(format!("project memory `{memory_id}` not found"));
        }
        self.record_project_memory_deleted_for_workspace(
            &host_id,
            &memory_id,
            &resolved_cwd,
            was_explicit_cwd,
        );
        Ok(())
    }

    pub fn list_plugins_json(&self, host_id: &str) -> Result<String, String> {
        let host_id = normalize_required_id(host_id, "host_id")?;
        let plugins = self
            .inner
            .run_mobile_rpc(self.inner.list_plugins(&host_id))?;
        serde_json::to_string(&plugins).map_err(|e| e.to_string())
    }

    pub fn set_plugin_enabled_json(
        &self,
        host_id: &str,
        plugin_id: &str,
        enabled: bool,
    ) -> Result<(), String> {
        let host_id = normalize_required_id(host_id, "host_id")?;
        let plugin_id = normalize_required_id(plugin_id, "plugin_id")?;
        self.inner
            .run_mobile_rpc(self.inner.set_plugin_enabled(&host_id, &plugin_id, enabled))?;
        Ok(())
    }

    pub fn get_sandbox_config_json(&self, host_id: &str) -> Result<String, String> {
        let host_id = normalize_required_id(host_id, "host_id")?;
        let policy = self
            .inner
            .run_mobile_rpc(self.inner.get_sandbox_config(&host_id))?;
        serde_json::to_string(&policy).map_err(|e| e.to_string())
    }
}
