//! C-ABI Native FFI Bridge (`liborbit_ffi.so` on Android / `liborbit_ffi.a` on iOS).
//!
//! Exposes `Noise_NK_25519_ChaChaPoly_SHA256` E2EE session handles, Zstd wire compression,
//! and QR pairing URI parsing to Flutter (`dart:ffi`) with zero-copy byte buffers.

use std::collections::HashMap;
use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::sync::{Mutex, OnceLock};

use base64::{Engine as _, engine::general_purpose::STANDARD as B64};
use orbit_crypto::{ClientHandshake, PairingOffer, SecureChannel, decode_key_32};
use serde::Serialize;

struct FfiRegistry {
    next_id: u64,
    handshakes: HashMap<u64, ClientHandshake>,
    channels: HashMap<u64, SecureChannel>,
}

fn registry() -> &'static Mutex<FfiRegistry> {
    static REG: OnceLock<Mutex<FfiRegistry>> = OnceLock::new();
    REG.get_or_init(|| {
        Mutex::new(FfiRegistry {
            next_id: 1,
            handshakes: HashMap::new(),
            channels: HashMap::new(),
        })
    })
}

#[derive(Serialize)]
struct FfiResult<T: Serialize> {
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

fn into_json_cstr<T: Serialize>(res: Result<T, String>) -> *mut c_char {
    let payload = match res {
        Ok(data) => FfiResult {
            ok: true,
            data: Some(data),
            error: None,
        },
        Err(err) => FfiResult {
            ok: false,
            data: None,
            error: Some(err),
        },
    };
    let json = serde_json::to_string(&payload)
        .unwrap_or_else(|_| r#"{"ok":false,"error":"serialization failed"}"#.to_string());
    CString::new(json)
        .unwrap_or_else(|_| CString::default())
        .into_raw()
}

/// Free any string returned by an `orbit_ffi_*` function.
///
/// # Safety
/// `ptr` must be either null or a pointer previously returned by `orbit_ffi_*`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn orbit_ffi_free_string(ptr: *mut c_char) {
    if !ptr.is_null() {
        unsafe {
            drop(CString::from_raw(ptr));
        }
    }
}

/// Parse an `orbit://pair/...` QR code URI into JSON `PairingOffer`.
///
/// # Safety
/// `uri_ptr` must point to a valid NUL-terminated UTF-8 string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn orbit_ffi_parse_pairing_uri(uri_ptr: *const c_char) -> *mut c_char {
    if uri_ptr.is_null() {
        return into_json_cstr::<PairingOffer>(Err("null pointer".into()));
    }
    let uri = unsafe { CStr::from_ptr(uri_ptr) }.to_string_lossy();
    into_json_cstr(PairingOffer::from_qr_uri(&uri).map_err(|e| e.to_string()))
}

#[derive(Serialize)]
struct NoiseInitOut {
    handshake_handle: u64,
    msg1_b64: String,
}

/// Begin a `Noise_NK_25519_ChaChaPoly_SHA256` initiator handshake for a pinned Daemon public key.
///
/// # Safety
/// `pubkey_b64_ptr` must point to a valid NUL-terminated UTF-8 string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn orbit_ffi_noise_client_init(pubkey_b64_ptr: *const c_char) -> *mut c_char {
    if pubkey_b64_ptr.is_null() {
        return into_json_cstr::<NoiseInitOut>(Err("null pubkey pointer".into()));
    }
    let pubkey_str = unsafe { CStr::from_ptr(pubkey_b64_ptr) }.to_string_lossy();
    let res = (|| -> Result<NoiseInitOut, String> {
        let key = decode_key_32(&pubkey_str).map_err(|e| e.to_string())?;
        let (hs, msg1) = ClientHandshake::new(&key).map_err(|e| e.to_string())?;
        let mut reg = registry().lock().map_err(|_| "lock poisoned".to_string())?;
        let id = reg.next_id;
        reg.next_id += 1;
        reg.handshakes.insert(id, hs);
        Ok(NoiseInitOut {
            handshake_handle: id,
            msg1_b64: B64.encode(msg1),
        })
    })();
    into_json_cstr(res)
}

/// Finalize Noise NK handshake with Daemon's `msg2` and return an established `channel_handle`.
///
/// # Safety
/// `msg2_b64_ptr` must point to a valid NUL-terminated UTF-8 string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn orbit_ffi_noise_client_finalize(
    handshake_handle: u64,
    msg2_b64_ptr: *const c_char,
) -> *mut c_char {
    if msg2_b64_ptr.is_null() {
        return into_json_cstr::<u64>(Err("null msg2 pointer".into()));
    }
    let msg2_b64 = unsafe { CStr::from_ptr(msg2_b64_ptr) }.to_string_lossy();
    let res = (|| -> Result<u64, String> {
        let msg2 = B64.decode(msg2_b64.trim()).map_err(|e| e.to_string())?;
        let hs = {
            let mut reg = registry().lock().map_err(|_| "lock poisoned".to_string())?;
            reg.handshakes
                .remove(&handshake_handle)
                .ok_or_else(|| "invalid handshake handle".to_string())?
        };
        let chan = hs.finalize(&msg2).map_err(|e| e.to_string())?;
        let mut reg = registry().lock().map_err(|_| "lock poisoned".to_string())?;
        let cid = reg.next_id;
        reg.next_id += 1;
        reg.channels.insert(cid, chan);
        Ok(cid)
    })();
    into_json_cstr(res)
}

/// Seal (encrypt + Zstd compress if wire JSON) a plaintext byte buffer (passed as base64) into Noise record base64.
///
/// # Safety
/// `plain_b64_ptr` must point to a valid NUL-terminated UTF-8 string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn orbit_ffi_noise_seal(
    channel_handle: u64,
    plain_b64_ptr: *const c_char,
) -> *mut c_char {
    if plain_b64_ptr.is_null() {
        return into_json_cstr::<String>(Err("null plain pointer".into()));
    }
    let plain_b64 = unsafe { CStr::from_ptr(plain_b64_ptr) }.to_string_lossy();
    let res = (|| -> Result<String, String> {
        let plain = B64.decode(plain_b64.trim()).map_err(|e| e.to_string())?;
        let mut reg = registry().lock().map_err(|_| "lock poisoned".to_string())?;
        let chan = reg
            .channels
            .get_mut(&channel_handle)
            .ok_or_else(|| "invalid channel handle".to_string())?;
        let ct = chan.seal_record(&plain).map_err(|e| e.to_string())?;
        Ok(B64.encode(ct))
    })();
    into_json_cstr(res)
}

/// Open (decrypt & verify anti-replay tag) a Noise ciphertext record (base64) into plaintext base64.
///
/// # Safety
/// `record_b64_ptr` must point to a valid NUL-terminated UTF-8 string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn orbit_ffi_noise_open(
    channel_handle: u64,
    record_b64_ptr: *const c_char,
) -> *mut c_char {
    if record_b64_ptr.is_null() {
        return into_json_cstr::<String>(Err("null record pointer".into()));
    }
    let record_b64 = unsafe { CStr::from_ptr(record_b64_ptr) }.to_string_lossy();
    let res = (|| -> Result<String, String> {
        let ct = B64.decode(record_b64.trim()).map_err(|e| e.to_string())?;
        let mut reg = registry().lock().map_err(|_| "lock poisoned".to_string())?;
        let chan = reg
            .channels
            .get_mut(&channel_handle)
            .ok_or_else(|| "invalid channel handle".to_string())?;
        let plain = chan.open_record(&ct).map_err(|e| e.to_string())?;
        Ok(B64.encode(plain))
    })();
    into_json_cstr(res)
}

/// Close and drop an established Noise channel handle.
#[unsafe(no_mangle)]
pub extern "C" fn orbit_ffi_noise_close(channel_handle: u64) {
    if let Ok(mut reg) = registry().lock() {
        reg.channels.remove(&channel_handle);
    }
}

/// Zstd-compress UTF-8 JSON string and return base64 bytes (`0x00` + zstd frame).
///
/// # Safety
/// `json_ptr` must point to a valid NUL-terminated UTF-8 string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn orbit_ffi_zstd_compress_wire(json_ptr: *const c_char) -> *mut c_char {
    if json_ptr.is_null() {
        return into_json_cstr::<String>(Err("null json pointer".into()));
    }
    let json_str = unsafe { CStr::from_ptr(json_ptr) }.to_string_lossy();
    let res = zstd::encode_all(json_str.as_bytes(), 3)
        .map(|z| {
            let mut framed = Vec::with_capacity(1 + z.len());
            framed.push(0x00u8);
            framed.extend_from_slice(&z);
            B64.encode(framed)
        })
        .map_err(|e| e.to_string());
    into_json_cstr(res)
}

/// Decompress Zstd payload (base64 without discriminator byte) back into UTF-8 JSON string.
///
/// # Safety
/// `zstd_b64_ptr` must point to a valid NUL-terminated UTF-8 string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn orbit_ffi_zstd_decompress_wire(
    zstd_b64_ptr: *const c_char,
) -> *mut c_char {
    if zstd_b64_ptr.is_null() {
        return into_json_cstr::<String>(Err("null zstd pointer".into()));
    }
    let b64_str = unsafe { CStr::from_ptr(zstd_b64_ptr) }.to_string_lossy();
    let res = (|| -> Result<String, String> {
        use std::io::Read;
        let compressed = B64.decode(b64_str.trim()).map_err(|e| e.to_string())?;
        let decoder = zstd::Decoder::new(compressed.as_slice()).map_err(|e| e.to_string())?;
        let mut out = Vec::new();
        decoder
            .take(16 * 1024 * 1024)
            .read_to_end(&mut out)
            .map_err(|e| e.to_string())?;
        String::from_utf8(out).map_err(|e| e.to_string())
    })();
    into_json_cstr(res)
}
