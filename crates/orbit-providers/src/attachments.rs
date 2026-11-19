//! Authorized staged attachment extraction and multimodal image block builders.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use base64::{Engine as _, engine::general_purpose::STANDARD as B64};
use serde_json::{Value, json};

pub const AGY_INLINE_IMAGE_MAX_BYTES: usize = 8 * 1024 * 1024;
pub const CLAUDE_INLINE_IMAGE_MAX_BYTES: usize = 4 * 1024 * 1024;
pub const CODEX_LOCAL_IMAGE_MAX_BYTES: usize = 25 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedImageAttachment {
    pub canonical_path: PathBuf,
    pub mime_type: &'static str,
    pub bytes: Vec<u8>,
}

/// Extract deduplicated `Attached image file: \`<path>\`` entries in prompt order.
pub fn extract_staged_image_paths(prompt: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for line in prompt.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("Attached image file:") {
            let clean = rest.trim().trim_matches('`').trim();
            if !clean.is_empty() && seen.insert(clean.to_string()) {
                out.push(PathBuf::from(clean));
            }
        }
    }
    out
}

/// Check whether `path` resolves (`canonicalize`) to a regular file strictly inside `dir`.
pub fn is_path_within_dir(path: &Path, dir: &Path) -> bool {
    let Ok(canon_dir) = fs::canonicalize(dir) else {
        return false;
    };
    let Ok(canon_file) = fs::canonicalize(path) else {
        return false;
    };
    canon_file.is_file() && canon_file.starts_with(&canon_dir)
}

/// Sniff image MIME type from magic header bytes (no reliance on file extensions).
pub fn sniff_supported_image_mime(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("image/jpeg")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some("image/webp")
    } else {
        None
    }
}

/// Resolve only staged image paths that live inside `attachments_dir`, pass size limits, and match image magic bytes.
pub fn resolve_authorized_staged_images(
    prompt: &str,
    attachments_dir: Option<&Path>,
    max_bytes: usize,
) -> Vec<ResolvedImageAttachment> {
    let Some(dir) = attachments_dir else {
        return Vec::new();
    };
    let Ok(canon_dir) = fs::canonicalize(dir) else {
        return Vec::new();
    };

    let mut out = Vec::new();
    let mut seen_canon = HashSet::new();
    for candidate in extract_staged_image_paths(prompt) {
        let Ok(canon_file) = fs::canonicalize(&candidate) else {
            continue;
        };
        if !canon_file.starts_with(&canon_dir) || !canon_file.is_file() {
            continue;
        }
        if !seen_canon.insert(canon_file.clone()) {
            continue;
        }
        let Ok(meta) = fs::metadata(&canon_file) else {
            continue;
        };
        if meta.len() as usize > max_bytes {
            continue;
        }
        let Ok(bytes) = fs::read(&canon_file) else {
            continue;
        };
        if bytes.len() > max_bytes {
            continue;
        }
        let Some(mime_type) = sniff_supported_image_mime(&bytes) else {
            continue;
        };
        out.push(ResolvedImageAttachment {
            canonical_path: canon_file,
            mime_type,
            bytes,
        });
    }
    out
}

pub fn build_agy_prompt_blocks(prompt: &str, attachments_dir: Option<&Path>) -> Vec<Value> {
    let mut blocks = vec![json!({ "type": "text", "text": prompt })];
    for img in resolve_authorized_staged_images(prompt, attachments_dir, AGY_INLINE_IMAGE_MAX_BYTES)
    {
        blocks.push(json!({
            "type": "image",
            "data": B64.encode(&img.bytes),
            "mimeType": img.mime_type,
        }));
    }
    blocks
}

pub fn build_claude_content_blocks(prompt: &str, attachments_dir: Option<&Path>) -> Vec<Value> {
    let mut blocks = vec![json!({ "type": "text", "text": prompt })];
    for img in
        resolve_authorized_staged_images(prompt, attachments_dir, CLAUDE_INLINE_IMAGE_MAX_BYTES)
    {
        blocks.push(json!({
            "type": "image",
            "source": {
                "type": "base64",
                "media_type": img.mime_type,
                "data": B64.encode(&img.bytes),
            }
        }));
    }
    blocks
}

pub fn build_codex_input_items(prompt: &str, attachments_dir: Option<&Path>) -> Vec<Value> {
    let mut items = vec![json!({
        "type": "text",
        "text": prompt,
        "text_elements": []
    })];
    for img in
        resolve_authorized_staged_images(prompt, attachments_dir, CODEX_LOCAL_IMAGE_MAX_BYTES)
    {
        items.push(json!({
            "type": "localImage",
            "path": img.canonical_path.to_string_lossy(),
        }));
    }
    items
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn make_temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "orbit_prov_att_{tag}_{}",
            &Uuid::new_v4().simple().to_string()[..8]
        ));
        let _ = fs::create_dir_all(&dir);
        dir
    }

    #[test]
    fn enforces_attachments_dir_allowlist_and_sniffs_formats() {
        let root = make_temp_dir("allow");
        let att_dir = root.join("attachments");
        let outside_dir = root.join("outside");
        fs::create_dir_all(&att_dir).expect("mkdir att");
        fs::create_dir_all(&outside_dir).expect("mkdir out");

        let png_path = att_dir.join("shot.png");
        let png_bytes = b"\x89PNG\r\n\x1a\nfake-png";
        fs::write(&png_path, png_bytes).expect("write png");

        let jpg_path = att_dir.join("photo.bin");
        let jpg_bytes = b"\xFF\xD8\xFF\xE0fake-jpg";
        fs::write(&jpg_path, jpg_bytes).expect("write jpg");

        let txt_path = att_dir.join("notes.png");
        fs::write(&txt_path, b"not really a png").expect("write fake");

        let outside_png = outside_dir.join("secret.png");
        fs::write(&outside_png, png_bytes).expect("write outside");

        #[cfg(unix)]
        {
            let link_escape = att_dir.join("escape.png");
            let _ = std::os::unix::fs::symlink(&outside_png, &link_escape);
        }

        let prompt = format!(
            "<ATTACHED_CONTEXT>\n\
             Attached image file: `{}`\n\
             Attached image file: `{}`\n\
             Attached image file: `{}`\n\
             Attached image file: `{}`\n\
             Attached image file: `{}`\n\
             </ATTACHED_CONTEXT>",
            png_path.display(),
            jpg_path.display(),
            txt_path.display(),
            outside_png.display(),
            att_dir.join("escape.png").display()
        );

        let resolved = resolve_authorized_staged_images(&prompt, Some(&att_dir), 1024 * 1024);
        assert_eq!(resolved.len(), 2);
        assert_eq!(resolved[0].mime_type, "image/png");
        assert_eq!(resolved[0].bytes, png_bytes);
        assert_eq!(resolved[1].mime_type, "image/jpeg");
        assert_eq!(resolved[1].bytes, jpg_bytes);

        // Size cap enforcement
        let capped = resolve_authorized_staged_images(&prompt, Some(&att_dir), 4);
        assert!(capped.is_empty());

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn builds_provider_multimodal_payloads() {
        let root = make_temp_dir("payloads");
        let att_dir = root.join("attachments");
        fs::create_dir_all(&att_dir).expect("mkdir");
        let webp_path = att_dir.join("img.webp");
        let webp_bytes = b"RIFF\x00\x00\x00\x00WEBPdata";
        fs::write(&webp_path, webp_bytes).expect("write webp");

        let prompt = format!(
            "<ATTACHED_CONTEXT>\nAttached image file: `{}`\n</ATTACHED_CONTEXT>\n\n<USER_REQUEST>\nCheck\n</USER_REQUEST>",
            webp_path.display()
        );

        let agy = build_agy_prompt_blocks(&prompt, Some(&att_dir));
        assert_eq!(agy.len(), 2);
        assert_eq!(agy[0]["type"], "text");
        assert_eq!(agy[1]["type"], "image");
        assert_eq!(agy[1]["mimeType"], "image/webp");
        assert_eq!(agy[1]["data"], B64.encode(webp_bytes));

        let claude = build_claude_content_blocks(&prompt, Some(&att_dir));
        assert_eq!(claude.len(), 2);
        assert_eq!(claude[1]["type"], "image");
        assert_eq!(claude[1]["source"]["type"], "base64");
        assert_eq!(claude[1]["source"]["media_type"], "image/webp");
        assert_eq!(claude[1]["source"]["data"], B64.encode(webp_bytes));

        let codex = build_codex_input_items(&prompt, Some(&att_dir));
        assert_eq!(codex.len(), 2);
        assert_eq!(codex[0]["type"], "text");
        assert_eq!(codex[1]["type"], "localImage");
        assert_eq!(
            codex[1]["path"],
            fs::canonicalize(&webp_path)
                .expect("canon")
                .to_string_lossy()
                .as_ref()
        );

        let _ = fs::remove_dir_all(&root);
    }
}
