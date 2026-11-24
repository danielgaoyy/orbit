//! Browser bindings to the same authenticated records and Rust projections as native clients.
mod host;
use orbit_client_core::{
    catalog, composer, contract, diagram, export, math, render, sidebar, stage, viewmodel,
};
use orbit_crypto::{ClientHandshake, EncryptedVaultEnvelope, PairingOffer, SecureChannel};
use orbit_protocol::{E2EE_FRAME_WIRE_JSON, MAX_WS_FRAME_BYTES, WireMessage};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use wasm_bindgen::prelude::*;

fn error(e: impl std::fmt::Display) -> JsError {
    JsError::new(&e.to_string())
}
fn from_js<T: DeserializeOwned>(value: JsValue) -> Result<T, JsError> {
    serde_wasm_bindgen::from_value(value).map_err(error)
}
fn js(value: &impl Serialize) -> Result<JsValue, JsError> {
    value
        .serialize(&serde_wasm_bindgen::Serializer::json_compatible())
        .map_err(error)
}

enum NoiseState {
    Ready(ClientHandshake, Vec<u8>),
    Waiting(ClientHandshake),
    Transport(SecureChannel),
    Closed,
}

#[wasm_bindgen]
pub struct WasmNoiseSession {
    state: NoiseState,
}

#[wasm_bindgen]
impl WasmNoiseSession {
    pub fn create_initiator(public_key_b64: &str, pairing_secret: &str) -> Result<Self, JsError> {
        let key = orbit_crypto::decode_key_32(public_key_b64).map_err(error)?;
        let (hs, msg) = ClientHandshake::new(&key, pairing_secret).map_err(error)?;
        Ok(Self {
            state: NoiseState::Ready(hs, msg),
        })
    }

    pub fn write_handshake_e(&mut self) -> Result<Vec<u8>, JsError> {
        match std::mem::replace(&mut self.state, NoiseState::Closed) {
            NoiseState::Ready(hs, msg) => {
                self.state = NoiseState::Waiting(hs);
                Ok(msg)
            }
            other => {
                self.state = other;
                Err(error("handshake already started"))
            }
        }
    }

    pub fn read_handshake_ee_es(&mut self, msg: &[u8]) -> Result<(), JsError> {
        let NoiseState::Waiting(hs) = std::mem::replace(&mut self.state, NoiseState::Closed) else {
            return Err(error("unexpected handshake response"));
        };
        self.state = NoiseState::Transport(hs.finalize(msg).map_err(error)?);
        Ok(())
    }

    pub fn encrypt_wire_json(&mut self, json: &str) -> Result<Vec<u8>, JsError> {
        if !matches!(self.state, NoiseState::Transport(_)) {
            return Err(error("handshake incomplete"));
        }
        let wire: WireMessage = serde_json::from_str(json).map_err(error)?;
        let encoded = wire.encode_e2ee(false).map_err(error)?;
        let NoiseState::Transport(chan) = &mut self.state else {
            return Err(error("handshake incomplete"));
        };
        match chan.seal_record(&encoded) {
            Ok(ct) => Ok(ct),
            Err(e) => {
                self.state = NoiseState::Closed;
                Err(error(e))
            }
        }
    }

    pub fn decrypt_wire_json(&mut self, record: &[u8]) -> Result<String, JsError> {
        if !matches!(self.state, NoiseState::Transport(_)) {
            return Err(error("handshake incomplete"));
        }
        if record.len() > MAX_WS_FRAME_BYTES {
            self.state = NoiseState::Closed;
            return Err(error("oversized Noise record"));
        }
        let NoiseState::Transport(chan) = &mut self.state else {
            return Err(error("handshake incomplete"));
        };
        let frame = match chan.open_record(record) {
            Ok(f) => f,
            Err(e) => {
                self.state = NoiseState::Closed;
                return Err(error(e));
            }
        };
        if frame.first() != Some(&E2EE_FRAME_WIRE_JSON) {
            return Err(error("expected encrypted JSON frame (0x02)"));
        }
        serde_json::to_string(&WireMessage::decode_e2ee(&frame).map_err(error)?).map_err(error)
    }
}

#[wasm_bindgen]
pub fn parse_pairing_uri(uri: &str) -> Result<JsValue, JsError> {
    js(&PairingOffer::from_qr_uri(uri.trim()).map_err(error)?)
}

#[wasm_bindgen]
pub fn vault_encrypt(
    passphrase: &str,
    payload: &[u8],
    updated_at_ms: f64,
) -> Result<JsValue, JsError> {
    let key = orbit_crypto::derive_new_vault_key(passphrase).map_err(error)?;
    js(&orbit_crypto::encrypt_vault_bytes(&key, payload, updated_at_ms as i64).map_err(error)?)
}

#[wasm_bindgen]
pub fn vault_decrypt(passphrase: &str, value: JsValue) -> Result<Vec<u8>, JsError> {
    let env: EncryptedVaultEnvelope = from_js(value)?;
    orbit_crypto::validate_envelope_header(&env).map_err(error)?;
    let key =
        orbit_crypto::derive_vault_key(passphrase, &env.salt_b64, env.kdf_params).map_err(error)?;
    orbit_crypto::decrypt_vault_bytes(&key, &env).map_err(error)
}

#[wasm_bindgen]
pub fn render_markdown_blocks(text: &str, streaming: bool) -> Result<JsValue, JsError> {
    js(&render::parse_markdown_blocks(text, streaming))
}
#[wasm_bindgen]
pub fn render_diagram_svg(request: JsValue) -> Result<JsValue, JsError> {
    js(&diagram::render_diagram(&from_js(request)?))
}
#[wasm_bindgen]
pub fn render_mathml(text: &str, block: bool) -> String {
    math::render_latex_to_mathml(
        text,
        if block {
            math::MathDisplay::Block
        } else {
            math::MathDisplay::Inline
        },
    )
}
#[wasm_bindgen]
pub fn parse_unified_diff(text: &str) -> Result<JsValue, JsError> {
    js(&render::parse_unified_diff(text))
}
#[wasm_bindgen]
pub fn project_stage_turns(items: JsValue, cwd: Option<String>) -> Result<JsValue, JsError> {
    let items: Vec<viewmodel::TimelineItemView> = from_js(items)?;
    js(&stage::process_timeline_turns_slice(&items, 0, 0, cwd.as_deref()).turns)
}
#[derive(Deserialize)]
struct SidebarData {
    revision: u64,
    hosts: Vec<viewmodel::HostDetailView>,
    #[serde(default)]
    projects: Vec<viewmodel::ClientProjectView>,
    #[serde(default)]
    project_order: Vec<String>,
}
#[wasm_bindgen]
pub fn project_fleet_sidebar(data: JsValue, request: JsValue) -> Result<JsValue, JsError> {
    let data: SidebarData = from_js(data)?;
    let req: contract::FleetSidebarRequest = from_js(request)?;
    let conflicts = Default::default();
    let hosts = data
        .hosts
        .iter()
        .map(|h| sidebar::SidebarHostInput {
            host_id: &h.host.host_id,
            host_name: &h.host.display_name,
            workspaces: &h.workspaces,
            discovered_projects: &h.discovered_projects,
            agents: h.agents.iter().map(|a| (a.clone(), 0)).collect(),
            active_git_branch: None,
            git_repos_by_cwd: &h.git_repos_by_cwd,
            workspace_conflict_ids: &conflicts,
        })
        .collect();
    js(&sidebar::build_fleet_sidebar(sidebar::FleetSidebarParams {
        revision: data.revision,
        hosts,
        pinned_projects: &data.projects,
        project_order: &data.project_order,
        sort: req.sort,
        filter: req.filter,
        query: &req.query,
        active_agent_id: req.active_agent_id.as_deref(),
        active_host_id: req.active_host_id.as_deref(),
        active_cwd: req.active_cwd.as_deref(),
    }))
}
#[wasm_bindgen]
pub fn export_transcript(
    meta: JsValue,
    events: JsValue,
    options: JsValue,
) -> Result<JsValue, JsError> {
    js(&export::export_timeline_transcript(
        &from_js(meta)?,
        &from_js::<Vec<_>>(events)?,
        &from_js(options)?,
    ))
}
#[wasm_bindgen]
pub fn compose_prompt(request: JsValue) -> Result<JsValue, JsError> {
    js(&composer::compose_prompt(&from_js(request)?, None, None))
}
#[wasm_bindgen]
pub fn slash_commands(query: &str) -> Result<JsValue, JsError> {
    js(&composer::filter_slash_commands(
        query,
        composer::ClientPlatform::Web,
    ))
}
#[wasm_bindgen]
pub fn provider_catalogs(live: JsValue) -> Result<JsValue, JsError> {
    js(&catalog::effective_provider_catalogs(&from_js::<Vec<_>>(
        live,
    )?))
}

#[wasm_bindgen]
pub fn protocol_version() -> u32 {
    orbit_protocol::PROTOCOL_VERSION
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn browser_session_exchanges_authenticated_json() -> Result<(), Box<dyn std::error::Error>> {
        use orbit_crypto::{DaemonHandshake, HostIdentityKeypair};
        let key = HostIdentityKeypair::generate()?;
        let mut browser = WasmNoiseSession::create_initiator(&key.public_key_b64, "paired")
            .map_err(|_| "initiator")?;
        let first = browser.write_handshake_e().map_err(|_| "first flight")?;
        let (mut peer, second) = DaemonHandshake::accept(&key.private_bytes()?, "paired", &first)?;
        browser
            .read_handshake_ee_es(&second)
            .map_err(|_| "second flight")?;
        let sealed = browser
            .encrypt_wire_json(r#"{"op":"ping","ts_ms":42}"#)
            .map_err(|_| "encrypt")?;
        let plain = peer.open_record(&sealed)?;
        assert_eq!(plain[0], E2EE_FRAME_WIRE_JSON);
        assert!(matches!(
            WireMessage::decode_e2ee(&plain)?,
            WireMessage::Ping { ts_ms: 42 }
        ));
        let reply = peer.seal_record(&WireMessage::Pong { ts_ms: 42 }.encode_e2ee(false)?)?;
        let received = browser.decrypt_wire_json(&reply).map_err(|_| "decrypt")?;
        assert!(matches!(
            serde_json::from_str::<WireMessage>(&received)?,
            WireMessage::Pong { ts_ms: 42 }
        ));
        Ok(())
    }
}

#[wasm_bindgen]
pub fn fleet_overview(revision: u32, cards: JsValue) -> Result<JsValue, JsError> {
    js(&viewmodel::project_fleet_overview(
        revision.into(),
        from_js(cards)?,
        Vec::new(),
        Vec::new(),
    ))
}

#[wasm_bindgen]
pub fn bridge_info() -> Result<JsValue, JsError> {
    js(&contract::BridgeInfoView {
        protocol_version: contract::CLIENT_BRIDGE_PROTOCOL_VERSION,
        core_version: env!("CARGO_PKG_VERSION").into(),
        capabilities: vec!["web".into(), "e2ee".into()],
    })
}
