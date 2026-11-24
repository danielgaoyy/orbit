//! Headless & Interactive CLI Cloud Sync (`orbit login`, `orbit sync`, `orbit logout`).
//!
//! Uses GitHub OAuth Device Flow (`scope=gist`, zero Orbit servers) or a GitHub Personal Access
//! Token (`gist` scope) to synchronize Orbit hosts, projects, and settings through an Argon2id +
//! AES-256-GCM end-to-end encrypted GitHub Secret Gist.

use std::io::{IsTerminal, Write};
use std::path::Path;
use std::time::Duration;

use orbit_client_core::cloud_sync::{
    CloudVaultPayload, DevicePollOutcome, GitHubCloudClient, LocalHostSyncRecord,
    PersistedCloudSyncConfig, SyncedHostEntry, decrypt_remote_with_cached_key,
    delete_cli_sync_config, filter_shareable_endpoints, generate_sync_passphrase,
    load_cli_sync_config, now_unix_ms, push_vault_payload, resolve_github_client_id,
    save_cli_sync_config, save_sync_config_to_store, unlock_remote_vault_with_passphrase,
};
use orbit_client_core::desktop_ops::open_external_url;
use orbit_crypto::PairingOffer;
use orbit_daemon::{DaemonPersistedConfig, DaemonRuntime};
use orbit_protocol::WireMessage;
use orbit_relay_transport::redact_relay_url;
use orbit_store::{OrbitStore, StoredHostMeta, StoredHostProfile};
use uuid::Uuid;

use crate::service::{
    PortOwnership, default_daemon_listen, find_live_daemon_port_for_host, probe_daemon_port,
};

/// Secret-free JSON summary emitted by `orbit login --json` and `orbit sync --json`.
/// Never serializes `pairing_secret` or raw SSH relay credentials to routine stdout.
fn hosts_to_cli_json_views(hosts: &[SyncedHostEntry]) -> Vec<serde_json::Value> {
    hosts
        .iter()
        .map(|h| {
            serde_json::json!({
                "host_id": h.host_id,
                "hostname": h.hostname,
                "daemon_pubkey_b64": h.daemon_pubkey_b64,
                "direct_endpoints": h.direct_endpoints,
                "relay_url": h.relay_url.as_deref().map(redact_relay_url),
                "custom_name": h.custom_name,
                "ssh_alias": h.ssh_alias,
                "route_mode": h.route_mode,
                "remote_daemon_port": h.remote_daemon_port,
                "custom_direct_endpoint": h.custom_direct_endpoint,
                "os_arch": h.os_arch,
                "updated_at_ms": h.updated_at_ms,
            })
        })
        .collect()
}

pub struct LoginOptions {
    pub token: Option<String>,
    pub client_id: Option<String>,
    pub passphrase: Option<String>,
    pub no_register_host: bool,
    pub sync_mesh: bool,
    pub json_mode: bool,
}

pub struct SyncOptions {
    pub no_register_host: bool,
    pub sync_mesh: bool,
    pub json_mode: bool,
}

pub async fn cmd_login(home_dir: &Path, opts: LoginOptions) -> anyhow::Result<()> {
    let client = GitHubCloudClient::new();
    let mut cfg = load_combined_sync_config(home_dir);

    if let Some(ref cid) = opts.client_id {
        let trimmed = cid.trim();
        if !trimmed.is_empty() {
            cfg.github_client_id = Some(trimmed.to_string());
        }
    }

    // 1. Obtain GitHub access token (PAT override, OAuth Device Flow, or interactive token fallback)
    let token = if let Some(tok) = opts
        .token
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        tok.to_string()
    } else if let Some(cid) =
        resolve_github_client_id(opts.client_id.as_deref(), cfg.github_client_id.as_deref())
    {
        run_interactive_device_flow(&client, &cid).await?
    } else {
        if !std::io::stdin().is_terminal() {
            anyhow::bail!(
                "No GitHub OAuth Client ID is configured. Pass `--token <GITHUB_PAT>` (with `gist` scope) or `--client-id <OAUTH_CLIENT_ID>`."
            );
        }
        eprintln!("=== Orbit Cloud Sync (Zero-Server GitHub E2EE) ===");
        eprintln!("No default GitHub OAuth Client ID is configured in this build.");
        eprintln!(
            "You can sign in immediately with a GitHub Personal Access Token (`gist` scope):"
        );
        eprintln!(
            "  Create one at: https://github.com/settings/tokens/new?scopes=gist&description=Orbit%20Cloud%20Sync"
        );
        eprintln!(
            "  (Or pass `--client-id <YOUR_GITHUB_OAUTH_CLIENT_ID>` for GitHub Device Flow.)\n"
        );
        prompt_secret("Paste GitHub Token (`gist` scope): ")?
    };

    if token.trim().is_empty() {
        anyhow::bail!("GitHub token cannot be empty");
    }

    let user = client.verify_token(&token).await?;
    let remote_gist = client
        .fetch_vault_gist(&token, cfg.gist_id.as_deref())
        .await?;

    let account_changed = cfg.github_login.as_deref() != Some(user.login.as_str());
    if account_changed {
        cfg.clear_derived_key();
        cfg.gist_id = None;
        cfg.gist_html_url = None;
        cfg.last_synced_at_ms = None;
        cfg.last_vault_revision = 0;
    }
    cfg.github_access_token = Some(token.clone());
    cfg.github_login = Some(user.login.clone());
    cfg.github_name = user.name.clone();
    cfg.github_avatar_url = user.avatar_url.clone();
    cfg.last_error = None;
    if let Some(ref gist) = remote_gist {
        cfg.gist_id = Some(gist.gist_id.clone());
        cfg.gist_html_url = Some(gist.html_url.clone());
        cfg.remote_vault_exists = true;
    } else {
        cfg.remote_vault_exists = false;
    }

    if !opts.json_mode {
        eprintln!(
            "[✓] Signed in to GitHub as @{}{}",
            user.login,
            user.name
                .as_deref()
                .map(|n| format!(" ({n})"))
                .unwrap_or_default()
        );
    }

    // 2. Obtain Sync Passphrase (decrypt existing remote vault or initialize a brand-new encrypted vault)
    let mut generated_recovery: Option<String> = None;
    let passphrase = if let Some(p) = opts
        .passphrase
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        p.to_string()
    } else if remote_gist.is_some() {
        if !std::io::stdin().is_terminal() {
            anyhow::bail!(
                "Existing encrypted Orbit Vault found on GitHub; pass `--passphrase <PASSPHRASE>` (or `ORBIT_SYNC_PASSPHRASE`)."
            );
        }
        eprintln!("\nExisting encrypted Orbit Cloud Vault found on GitHub.");
        prompt_secret("Enter your Sync Passphrase to unlock: ")?
    } else {
        if !std::io::stdin().is_terminal() {
            anyhow::bail!(
                "Pass `--passphrase <PASSPHRASE>` (or `ORBIT_SYNC_PASSPHRASE`) to initialize your encrypted Orbit Vault."
            );
        }
        eprintln!("\nNo existing Orbit Cloud Vault found — creating a new E2EE Secret Gist.");
        eprintln!(
            "Your hosts, projects, and settings are encrypted locally with Argon2id + AES-256-GCM before upload."
        );
        let entered = prompt_secret(
            "Create Sync Passphrase (leave blank to auto-generate a Recovery Key): ",
        )?;
        if entered.trim().is_empty() {
            let rec = generate_sync_passphrase();
            generated_recovery = Some(rec.clone());
            rec
        } else {
            let confirm = prompt_secret("Confirm Sync Passphrase: ")?;
            if entered.trim() != confirm.trim() {
                anyhow::bail!("Sync Passphrases did not match");
            }
            entered
        }
    };

    // 3. Unlock / derive master key & perform initial sync
    let (key, unlocked_gist, remote_payload) =
        unlock_remote_vault_with_passphrase(&client, &token, cfg.gist_id.as_deref(), &passphrase)
            .await?;

    cfg.set_derived_key(&key);
    if let Some(ref gist) = unlocked_gist {
        cfg.gist_id = Some(gist.gist_id.clone());
        cfg.gist_html_url = Some(gist.html_url.clone());
        cfg.remote_vault_exists = true;
    }

    let summary = execute_cli_sync(
        home_dir,
        &client,
        &mut cfg,
        &key,
        remote_payload.as_ref(),
        !opts.no_register_host,
        opts.sync_mesh,
    )
    .await?;

    if opts.json_mode {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "status": "signed_in",
                "github_login": cfg.github_login,
                "gist_html_url": cfg.gist_html_url,
                "recovery_passphrase": generated_recovery,
                "registered_this_host": summary.registered_host_id,
                "vault_revision": summary.vault_revision,
                "hosts": hosts_to_cli_json_views(&summary.active_hosts),
                "projects_count": summary.active_projects_count,
                "mesh_registered": summary.mesh_registered,
            }))?
        );
    } else {
        if let Some(ref rec) = generated_recovery {
            println!("\n╔══════════════════════════════════════════════════════════╗");
            println!("║  SAVE YOUR ORBIT SYNC RECOVERY KEY (shown once!)        ║");
            println!("║                                                          ║");
            println!("║    {rec:<50}    ║");
            println!("║                                                          ║");
            println!("║  Store it in your password manager to unlock new devices ║");
            println!("╚══════════════════════════════════════════════════════════╝");
        }
        print_sync_summary(&cfg, &summary);
    }

    Ok(())
}

pub async fn cmd_sync(home_dir: &Path, opts: SyncOptions) -> anyhow::Result<()> {
    let mut cfg = load_combined_sync_config(home_dir);
    let token = cfg
        .github_access_token
        .clone()
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| {
            anyhow::anyhow!("Not signed in to Orbit Cloud Sync. Run `orbit login` first.")
        })?;
    let key = cfg.derived_key().ok_or_else(|| {
        anyhow::anyhow!(
            "Orbit Cloud Vault is locked. Run `orbit login` to unlock with your Sync Passphrase."
        )
    })?;

    let client = GitHubCloudClient::new();
    let remote_gist = client
        .fetch_vault_gist(&token, cfg.gist_id.as_deref())
        .await?;

    let remote_payload = if let Some(ref gist) = remote_gist {
        cfg.gist_id = Some(gist.gist_id.clone());
        cfg.gist_html_url = Some(gist.html_url.clone());
        cfg.remote_vault_exists = true;
        Some(decrypt_remote_with_cached_key(&key, gist)?)
    } else {
        cfg.remote_vault_exists = false;
        None
    };

    let summary = execute_cli_sync(
        home_dir,
        &client,
        &mut cfg,
        &key,
        remote_payload.as_ref(),
        !opts.no_register_host,
        opts.sync_mesh,
    )
    .await?;

    if opts.json_mode {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "status": "synced",
                "github_login": cfg.github_login,
                "gist_html_url": cfg.gist_html_url,
                "registered_this_host": summary.registered_host_id,
                "pushed_remote": summary.pushed_remote,
                "vault_revision": summary.vault_revision,
                "hosts": hosts_to_cli_json_views(&summary.active_hosts),
                "projects_count": summary.active_projects_count,
                "mesh_registered": summary.mesh_registered,
            }))?
        );
    } else {
        print_sync_summary(&cfg, &summary);
    }

    Ok(())
}

pub async fn cmd_logout(
    home_dir: &Path,
    clear_local_hosts: bool,
    json_mode: bool,
) -> anyhow::Result<()> {
    delete_cli_sync_config(home_dir)?;

    let desktop_db = home_dir.join("desktop-client.sqlite");
    if desktop_db.is_file()
        && let Ok(core) = orbit_client_core::app_core::OrbitAppCore::open(&desktop_db, false)
    {
        let _ = core.cloud_sync_logout(clear_local_hosts);
    }

    if json_mode {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "status": "signed_out",
                "cleared_local_hosts": clear_local_hosts,
            }))?
        );
    } else {
        println!("[✓] Signed out of Orbit Cloud Sync on this machine.");
        println!(
            "    (Your encrypted GitHub Secret Gist was NOT deleted so other devices stay synced.)"
        );
    }
    Ok(())
}

struct CliSyncSummary {
    registered_host_id: Option<String>,
    pushed_remote: bool,
    vault_revision: u64,
    active_hosts: Vec<SyncedHostEntry>,
    active_projects_count: usize,
    mesh_registered: usize,
}

async fn execute_cli_sync(
    home_dir: &Path,
    client: &GitHubCloudClient,
    cfg: &mut PersistedCloudSyncConfig,
    key: &orbit_crypto::DerivedVaultKey,
    remote_payload: Option<&CloudVaultPayload>,
    register_this_host: bool,
    sync_mesh: bool,
) -> anyhow::Result<CliSyncSummary> {
    let token = cfg
        .github_access_token
        .clone()
        .ok_or_else(|| anyhow::anyhow!("missing GitHub token"))?;
    let now_ms = now_unix_ms();

    // Load or create desktop client SQLite store if present (or initialize cleanly in `~/.orbit/desktop-client.sqlite`)
    let desktop_db_path = home_dir.join("desktop-client.sqlite");
    let store = OrbitStore::open(&desktop_db_path)?;

    let mut local_hosts_map: std::collections::HashMap<String, LocalHostSyncRecord> =
        std::collections::HashMap::new();
    let mut local_meta = orbit_client_core::cloud_sync::load_sync_meta_from_store(&store);
    let fallback_ts = if remote_payload.is_none() { now_ms } else { 1 };

    for h in store.list_client_hosts().unwrap_or_default() {
        let ts = local_meta
            .host_updated_at_ms
            .get(&h.host_id)
            .copied()
            .unwrap_or(fallback_ts);
        local_hosts_map.insert(
            h.host_id.clone(),
            LocalHostSyncRecord {
                profile: h,
                os_arch: None,
                updated_at_ms: ts,
            },
        );
    }

    // Optionally snapshot this machine's Orbit Daemon identity & active endpoints into the sync payload!
    let mut registered_host_id = None;
    let local_identity = crate::read_local_host_identity(home_dir);
    let local_daemon_secret = orbit_daemon::read_pairing_secret(home_dir);
    if register_this_host {
        let local_rec =
            build_this_machine_sync_record(home_dir, now_ms, &local_hosts_map, remote_payload)
                .await?;
        registered_host_id = Some(local_rec.profile.host_id.clone());
        local_meta.record_host_mutated(&local_rec.profile.host_id, now_ms);
        local_hosts_map.insert(local_rec.profile.host_id.clone(), local_rec);
    } else if let (Some((local_hid, local_pub)), Some(local_secret)) =
        (local_identity.as_ref(), local_daemon_secret.as_ref())
        && let Some(existing) = local_hosts_map.get_mut(local_hid)
        && existing.profile.daemon_pubkey_b64 == *local_pub
    {
        let remote_secret_ts = remote_payload
            .and_then(|r| r.hosts.get(local_hid))
            .map(|r| r.pairing_secret_updated_at_ms)
            .unwrap_or(0);
        let remote_secret_differs = remote_payload
            .and_then(|r| r.hosts.get(local_hid))
            .is_some_and(|r| r.pairing_secret != *local_secret);
        if existing.profile.pairing_secret != *local_secret
            || (remote_secret_differs
                && existing.profile.meta.pairing_secret_updated_at_ms <= remote_secret_ts)
        {
            existing.profile.pairing_secret = local_secret.clone();
            existing.profile.meta.pairing_secret_updated_at_ms = now_ms
                .max(
                    existing
                        .profile
                        .meta
                        .pairing_secret_updated_at_ms
                        .saturating_add(1),
                )
                .max(remote_secret_ts.saturating_add(1));
        }
    }

    let local_hosts: Vec<LocalHostSyncRecord> = local_hosts_map.into_values().collect();
    let local_projects = store.list_client_projects().unwrap_or_default();
    let local_project_order: Vec<String> = store
        .get_client_kv("project_order")
        .ok()
        .flatten()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default();
    let mut local_prefs = orbit_client_core::cloud_sync::load_sync_prefs_from_store(&store);
    let local_fleet_network = orbit_client_core::cloud_sync::load_fleet_network_from_store(&store);

    let mut outcome = orbit_client_core::cloud_sync::merge_cloud_vault_with_network(
        remote_payload,
        &local_hosts,
        &local_projects,
        &local_project_order,
        &local_prefs,
        &local_meta,
        local_fleet_network.as_ref(),
        now_ms,
    );

    // Authoritatively prefer this machine's persisted `$ORBIT_HOME/pairing-secret` over any
    // stale remote synced value even when pulling this host into a fresh local store.
    if let (Some((local_hid, local_pub)), Some(local_secret)) =
        (local_identity.as_ref(), local_daemon_secret.as_ref())
        && let Some(entry) = outcome.merged.hosts.get_mut(local_hid)
        && entry.deleted_at_ms.is_none()
        && entry.daemon_pubkey_b64 == *local_pub
        && entry.pairing_secret != *local_secret
    {
        entry.pairing_secret = local_secret.clone();
        entry.pairing_secret_updated_at_ms =
            now_ms.max(entry.pairing_secret_updated_at_ms.saturating_add(1));
        outcome.needs_push = true;
    }

    // Persist merged hosts/projects/order/prefs into `~/.orbit/desktop-client.sqlite` so Desktop sees them instantly!
    for host in outcome.merged.hosts.values() {
        if let Some(del_ts) = host.deleted_at_ms {
            let _ = store.delete_client_host(&host.host_id);
            local_meta.record_host_deleted(&host.host_id, del_ts);
        } else {
            let mut stored = orbit_client_core::cloud_sync::stored_host_from_synced(host);
            let is_this_machine = registered_host_id.as_deref() == Some(host.host_id.as_str())
                || local_identity.as_ref().is_some_and(|(lid, lpub)| {
                    *lid == host.host_id && *lpub == host.daemon_pubkey_b64
                });
            if is_this_machine {
                let port = stored.meta.remote_daemon_port.unwrap_or(6868);
                let loopback = format!("ws://127.0.0.1:{port}/ws");
                if !stored.direct_endpoints.contains(&loopback) {
                    stored.direct_endpoints.insert(0, loopback);
                }
            }
            let _ = store.upsert_client_host(&stored);
            local_meta.record_host_mutated(&host.host_id, host.updated_at_ms);
        }
    }

    for proj in outcome.merged.projects.values() {
        if let Some(del_ts) = proj.deleted_at_ms {
            let _ = store.delete_client_project(&proj.project_id);
            local_meta.record_project_deleted(&proj.project_id, del_ts);
        } else {
            let _ = store.upsert_client_project(&orbit_store::StoredClientProject {
                project_id: proj.project_id.clone(),
                host_id: proj.host_id.clone(),
                name: proj.name.clone(),
                directories: proj.directories.clone(),
                updated_at_ms: proj.updated_at_ms,
            });
            local_meta.record_project_mutated(&proj.project_id);
        }
    }

    if let Some((ref new_order, ts)) = outcome.project_order_changed {
        if let Ok(json) = serde_json::to_string(new_order) {
            let _ = store.set_client_kv("project_order", &json, ts);
        }
        local_meta.record_project_order_mutated(ts);
    }

    if !outcome.preferences_to_apply.is_empty() {
        for (k, v) in &outcome.preferences_to_apply {
            local_prefs.insert(k.clone(), v.clone());
        }
        let _ = orbit_client_core::cloud_sync::save_sync_prefs_to_store(&store, &local_prefs);
    }
    if let Some(ref net) = outcome.fleet_network_changed {
        let _ = orbit_client_core::cloud_sync::save_fleet_network_to_store(&store, net);
    }
    let _ = orbit_client_core::cloud_sync::save_sync_meta_to_store(&store, &local_meta);

    if outcome.needs_push {
        let (gid, html_url) =
            push_vault_payload(client, &token, cfg.gist_id.as_deref(), key, &outcome.merged)
                .await?;
        cfg.gist_id = Some(gid);
        cfg.gist_html_url = Some(html_url);
        cfg.remote_vault_exists = true;
    }

    let mut active_hosts: Vec<SyncedHostEntry> = outcome
        .merged
        .hosts
        .values()
        .filter(|h| h.deleted_at_ms.is_none())
        .cloned()
        .collect();
    active_hosts.sort_by(|a, b| a.hostname.cmp(&b.hostname));

    let active_projects_count = outcome
        .merged
        .projects
        .values()
        .filter(|p| p.deleted_at_ms.is_none())
        .count();

    cfg.last_synced_at_ms = Some(now_ms);
    cfg.last_vault_revision = outcome.merged.revision;
    cfg.synced_host_count = active_hosts.len() as u32;
    cfg.synced_project_count = active_projects_count as u32;
    cfg.last_error = None;

    // Persist identical config into both `~/.orbit/cloud-sync.json` (0600) AND Desktop SQLite `client_kv`
    save_cli_sync_config(home_dir, cfg)?;
    save_sync_config_to_store(&store, cfg)?;

    // Optionally register all remote synced hosts into this machine's running Daemon connected-host mesh!
    // Also adopt the account fleet relay URL on this daemon if it is unset.
    let mut mesh_registered = 0usize;
    if sync_mesh {
        let fleet_relay = outcome
            .merged
            .fleet_network
            .as_ref()
            .and_then(|n| n.primary_relay_url.as_deref());
        mesh_registered = register_synced_hosts_into_local_daemon_mesh(
            home_dir,
            registered_host_id.as_deref(),
            &active_hosts,
            fleet_relay,
        )
        .await;
    }

    Ok(CliSyncSummary {
        registered_host_id,
        pushed_remote: outcome.needs_push,
        vault_revision: outcome.merged.revision,
        active_hosts,
        active_projects_count,
        mesh_registered,
    })
}

async fn build_this_machine_sync_record(
    home_dir: &Path,
    now_ms: i64,
    existing_local_hosts: &std::collections::HashMap<String, LocalHostSyncRecord>,
    remote_payload: Option<&CloudVaultPayload>,
) -> anyhow::Result<LocalHostSyncRecord> {
    let runtime = DaemonRuntime::bootstrap(home_dir).await?;
    let persisted = DaemonPersistedConfig::load(home_dir);
    let configured_listen = persisted.listen_addr.unwrap_or_else(default_daemon_listen);
    let live_addr = orbit_daemon::read_live_daemon_addr(home_dir);
    let preferred_port = live_addr
        .map(|a| a.port())
        .unwrap_or_else(|| configured_listen.port());
    let port = find_live_daemon_port_for_host(&runtime.host_info.host_id, preferred_port).await;
    let bind_ip = if let Some(la) = live_addr
        && la.port() == port
        && probe_daemon_port(port, &runtime.host_info.host_id).await == PortOwnership::Mine
    {
        la.ip()
    } else {
        configured_listen.ip()
    };
    let raw_endpoints = crate::advertised_direct_endpoints(bind_ip, port);
    let endpoints = filter_shareable_endpoints(&raw_endpoints);
    let os_arch = format!("{}/{}", runtime.host_info.os, runtime.host_info.arch);
    let pairing_secret = runtime.active_pairing_secret().ok_or_else(|| {
        anyhow::anyhow!(
            "missing or invalid pairing secret at {}",
            home_dir
                .join(orbit_daemon::PAIRING_SECRET_FILENAME)
                .display()
        )
    })?;

    let existing = existing_local_hosts.get(&runtime.host_info.host_id);
    let remote_host = remote_payload.and_then(|r| r.hosts.get(&runtime.host_info.host_id));
    let existing_ts = existing
        .map(|e| e.profile.meta.pairing_secret_updated_at_ms)
        .unwrap_or(0);
    let remote_ts = remote_host
        .map(|r| r.pairing_secret_updated_at_ms)
        .unwrap_or(0);
    let remote_differs = remote_host.is_some_and(|r| r.pairing_secret != pairing_secret);
    let pairing_secret_updated_at_ms = match existing {
        Some(e)
            if e.profile.pairing_secret == pairing_secret
                && existing_ts > 0
                && (!remote_differs || existing_ts > remote_ts) =>
        {
            existing_ts
        }
        _ => now_ms
            .max(existing_ts.saturating_add(1))
            .max(remote_ts.saturating_add(1)),
    };

    Ok(LocalHostSyncRecord {
        profile: StoredHostProfile {
            host_id: runtime.host_info.host_id.clone(),
            display_name: runtime.host_info.hostname.clone(),
            daemon_pubkey_b64: runtime.keypair.public_key_b64.clone(),
            direct_endpoints: endpoints,
            relay_url: persisted.relay_url,
            pairing_secret,
            meta: StoredHostMeta {
                remote_daemon_port: Some(port),
                route_mode: Some("auto".to_string()),
                pairing_secret_updated_at_ms,
                ..StoredHostMeta::default()
            },
        },
        os_arch: Some(os_arch),
        updated_at_ms: now_ms,
    })
}

async fn register_synced_hosts_into_local_daemon_mesh(
    home_dir: &Path,
    self_host_id: Option<&str>,
    hosts: &[SyncedHostEntry],
    fleet_primary_relay: Option<&str>,
) -> usize {
    let owned_local_id = self_host_id
        .map(ToOwned::to_owned)
        .or_else(|| crate::read_local_host_identity(home_dir).map(|(hid, _)| hid));
    let Some(local_host_id) = owned_local_id else {
        return 0;
    };
    let port = find_live_daemon_port_for_host(
        &local_host_id,
        orbit_daemon::read_live_daemon_port(home_dir),
    )
    .await;
    if probe_daemon_port(port, &local_host_id).await != PortOwnership::Mine {
        return 0;
    }
    let Some(local_secret) = orbit_daemon::read_pairing_secret(home_dir) else {
        return 0;
    };
    let ws_url = format!("ws://127.0.0.1:{port}/ws");
    let Ok(conn) =
        orbit_client_core::connect_direct_verified(&local_host_id, &ws_url, &local_secret, None)
            .await
    else {
        return 0;
    };
    if let Some(relay_url) = fleet_primary_relay
        && orbit_daemon::DaemonPersistedConfig::load(home_dir)
            .relay_url
            .is_none()
    {
        let req_id = Uuid::new_v4();
        let _ = conn
            .request(
                WireMessage::ConfigureHostRelay {
                    request_id: req_id,
                    relay_url: Some(relay_url.to_string()),
                },
                Duration::from_secs(5),
                |w| matches!(w, WireMessage::HostRelayConfigured { request_id: r, .. } if *r == req_id),
            )
            .await;
    }
    let mut registered = 0usize;
    for host in hosts {
        if host.host_id == local_host_id {
            continue;
        }
        let offer = PairingOffer {
            host_id: host.host_id.clone(),
            hostname: host.hostname.clone(),
            daemon_pubkey_b64: host.daemon_pubkey_b64.clone(),
            direct_endpoints: host.direct_endpoints.clone(),
            relay_url: host.relay_url.clone(),
            pairing_secret: host.pairing_secret.clone(),
        };
        let Ok(uri) = offer.to_qr_uri() else {
            continue;
        };
        let req_id = Uuid::new_v4();
        let label = host
            .custom_name
            .clone()
            .or_else(|| Some(host.hostname.clone()));
        let res = conn
            .request(
                WireMessage::AddConnectedHost {
                    request_id: req_id,
                    label,
                    pairing_uri: uri,
                    ssh_alias: host.ssh_alias.clone(),
                    remote_daemon_port: host.remote_daemon_port,
                },
                Duration::from_secs(5),
                |w| matches!(w, WireMessage::ConnectedHostsSnapshot { request_id: r, .. } if *r == req_id),
            )
            .await;
        if res.is_ok() {
            registered += 1;
        }
    }
    registered
}

async fn run_interactive_device_flow(
    client: &GitHubCloudClient,
    client_id: &str,
) -> anyhow::Result<String> {
    let started = client.start_device_flow(client_id).await?;
    eprintln!("=== Orbit Cloud Sync (GitHub E2EE) ===");
    eprintln!("  1. Open : {}", started.verification_uri);
    eprintln!("  2. Code : {}", started.user_code);
    eprintln!("  (Scope  : `gist` only — Orbit cannot access your repositories)\n");

    let has_gui = cfg!(target_os = "macos")
        || cfg!(target_os = "windows")
        || std::env::var_os("DISPLAY").is_some()
        || std::env::var_os("WAYLAND_DISPLAY").is_some();
    if has_gui {
        let _ = open_external_url(&started.verification_uri);
    }

    eprint!("Waiting for GitHub authorization");
    let _ = std::io::stderr().flush();

    let mut interval = started.interval_secs.max(2);
    let deadline = tokio::time::Instant::now() + Duration::from_secs(started.expires_in_secs);

    loop {
        if tokio::time::Instant::now() >= deadline {
            eprintln!();
            anyhow::bail!("GitHub authorization timed out — please run `orbit login` again.");
        }
        tokio::time::sleep(Duration::from_secs(interval)).await;
        match client
            .poll_device_flow_once(client_id, &started.device_code)
            .await?
        {
            DevicePollOutcome::Pending => {
                eprint!(".");
                let _ = std::io::stderr().flush();
            }
            DevicePollOutcome::SlowDown { interval_secs } => {
                interval = interval_secs.max(interval + 5);
            }
            DevicePollOutcome::Authorized { access_token } => {
                eprintln!();
                return Ok(access_token);
            }
            DevicePollOutcome::Expired => {
                eprintln!();
                anyhow::bail!("GitHub verification code expired — please run `orbit login` again.");
            }
            DevicePollOutcome::Denied => {
                eprintln!();
                anyhow::bail!("GitHub authorization was cancelled.");
            }
        }
    }
}

fn load_combined_sync_config(home_dir: &Path) -> PersistedCloudSyncConfig {
    let cli_cfg = load_cli_sync_config(home_dir);
    if cli_cfg.is_signed_in() {
        return cli_cfg;
    }
    let desktop_db = home_dir.join("desktop-client.sqlite");
    if desktop_db.is_file()
        && let Ok(store) = OrbitStore::open(&desktop_db)
    {
        let db_cfg = orbit_client_core::cloud_sync::load_sync_config_from_store(&store);
        if db_cfg.is_signed_in() {
            return db_cfg;
        }
    }
    cli_cfg
}

fn prompt_secret(prompt: &str) -> anyhow::Result<String> {
    eprint!("{prompt}");
    let _ = std::io::stderr().flush();

    #[cfg(unix)]
    let echoed_off = if std::io::stdin().is_terminal() {
        std::process::Command::new("stty")
            .arg("-echo")
            .status()
            .is_ok_and(|s| s.success())
    } else {
        false
    };

    let mut line = String::new();
    let read_res = std::io::stdin().read_line(&mut line);

    #[cfg(unix)]
    if echoed_off {
        let _ = std::process::Command::new("stty").arg("echo").status();
        eprintln!();
    }

    read_res?;
    Ok(line.trim().to_string())
}

fn print_sync_summary(cfg: &PersistedCloudSyncConfig, summary: &CliSyncSummary) {
    println!(
        "\n[✓] Orbit Cloud Sync complete (vault rev #{}, GitHub @{})",
        summary.vault_revision,
        cfg.github_login.as_deref().unwrap_or("unknown")
    );
    if let Some(ref url) = cfg.gist_html_url
        && !url.is_empty()
    {
        println!("    Secret Gist : {url} (AES-256-GCM E2EE)");
    }
    if let Some(ref hid) = summary.registered_host_id {
        println!("    This Host   : {hid} (registered & up to date)");
    }
    if summary.mesh_registered > 0 {
        println!(
            "    Fleet Mesh  : registered {} peer host(s) into local Daemon",
            summary.mesh_registered
        );
    }

    println!(
        "\nSynced Fleet ({} host(s), {} project(s)):",
        summary.active_hosts.len(),
        summary.active_projects_count
    );
    for h in &summary.active_hosts {
        let label = h.custom_name.as_deref().unwrap_or(&h.hostname);
        let relay = h
            .relay_url
            .as_deref()
            .map(redact_relay_url)
            .unwrap_or_else(|| "-".to_string());
        let direct = if h.direct_endpoints.is_empty() {
            "-".to_string()
        } else {
            h.direct_endpoints.join(", ")
        };
        let marker = if summary.registered_host_id.as_deref() == Some(h.host_id.as_str()) {
            " (this machine)"
        } else {
            ""
        };
        println!(
            "  - {:<20} {:<22} direct={}  relay={}{}",
            label, h.host_id, direct, relay, marker
        );
    }
    println!(
        "\nNext: Sign in with GitHub in Orbit Desktop / iOS / Android to sync all hosts automatically."
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_sync_json_view_omits_pairing_secret_and_redacts_relay() {
        let secret = "super-sensitive-pairing-secret-0123456789abcdef";
        let host = SyncedHostEntry {
            host_id: "host-abc".into(),
            hostname: "workstation".into(),
            daemon_pubkey_b64: "pk-abc".into(),
            direct_endpoints: vec!["ws://10.0.0.5:6868/ws".into()],
            relay_url: Some("ssh+relay://user@relay.example:22/7878?key=RAW_SSH_SECRET_KEY".into()),
            pairing_secret: secret.into(),
            pairing_secret_updated_at_ms: 12345,
            custom_name: Some("Lab Box".into()),
            ssh_alias: None,
            route_mode: Some("auto".into()),
            remote_daemon_port: Some(6868),
            custom_direct_endpoint: None,
            os_arch: Some("linux/x86_64".into()),
            bridge_host_ids: Vec::new(),
            updated_at_ms: 99999,
            deleted_at_ms: None,
        };
        let views = hosts_to_cli_json_views(&[host]);
        let json = serde_json::to_string(&views).expect("serialize cli host views");
        assert!(json.contains("host-abc"));
        assert!(json.contains("Lab Box"));
        assert!(
            !json.contains("pairing_secret"),
            "CLI JSON must never include pairing_secret field: {json}"
        );
        assert!(
            !json.contains(secret),
            "CLI JSON must never contain raw pairing secret value: {json}"
        );
        assert!(
            !json.contains("RAW_SSH_SECRET_KEY"),
            "CLI JSON must redact embedded SSH relay credentials: {json}"
        );
    }
}
