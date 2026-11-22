//! Authorized staged attachment extraction, pure-Rust PDF metadata inspection,
//! and optimal provider-specific multimodal payload builders.

use std::collections::HashSet;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use base64::{Engine as _, engine::general_purpose::STANDARD as B64};
pub use orbit_protocol::media::{
    AttachmentModality, SniffedMedia, sniff_media_mime, sniff_supported_image_mime,
};
use serde_json::{Value, json};

pub const AGY_INLINE_IMAGE_MAX_BYTES: usize = 8 * 1024 * 1024;
pub const AGY_INLINE_AUDIO_MAX_BYTES: usize = 8 * 1024 * 1024;
pub const ACP_TOTAL_INLINE_MAX_BYTES: usize = 16 * 1024 * 1024;
pub const CLAUDE_INLINE_IMAGE_MAX_BYTES: usize = 4 * 1024 * 1024;
pub const CLAUDE_INLINE_PDF_MAX_BYTES: usize = 20 * 1024 * 1024;
pub const CLAUDE_TOTAL_INLINE_MAX_BYTES: usize = 20 * 1024 * 1024;
pub const CLAUDE_UNKNOWN_PAGES_PDF_MAX_BYTES: usize = 5 * 1024 * 1024;
pub const CLAUDE_INLINE_PDF_MAX_PAGES: u32 = 100;
pub const CODEX_LOCAL_IMAGE_MAX_BYTES: usize = 25 * 1024 * 1024;
pub const MAX_STAGED_MEDIA_BYTES: usize = 25 * 1024 * 1024;

const SNIFF_HEADER_BYTES: usize = 4096;
const MAX_PDF_INSPECT_BYTES: usize = 32 * 1024 * 1024;
const MAX_SANE_PDF_PAGES: u32 = 100_000;
const PDF_PAGES_DICT_WINDOW_BYTES: usize = 8192;
const AGY_PDF_LARGE_PAGE_HINT_THRESHOLD: u32 = 10;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PdfMetadata {
    pub pdf_version: Option<String>,
    pub page_count: Option<u32>,
    pub title: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AcpPromptMediaCaps {
    pub image: bool,
    pub audio: bool,
}

impl Default for AcpPromptMediaCaps {
    fn default() -> Self {
        Self {
            image: true,
            audio: false,
        }
    }
}

impl AcpPromptMediaCaps {
    /// Antigravity ACP defaults: inline images + audio when advertised (or omitted).
    pub fn antigravity() -> Self {
        Self {
            image: true,
            audio: true,
        }
    }

    /// Parse capabilities from an ACP `initialize` result payload for third-party ACP servers.
    /// Per ACP spec, `audio` defaults to `false` when omitted while `image` defaults to `true`
    /// for compatibility with servers that omit `promptCapabilities`.
    pub fn from_acp_initialize(init_result: &Value) -> Self {
        let prompt_caps = init_result.pointer("/agentCapabilities/promptCapabilities");
        let image = prompt_caps
            .and_then(|v| v.get("image"))
            .and_then(Value::as_bool)
            .unwrap_or(true);
        let audio = prompt_caps
            .and_then(|v| v.get("audio"))
            .and_then(Value::as_bool)
            .unwrap_or(false);
        Self { image, audio }
    }

    /// Parse capabilities from Antigravity's ACP `initialize` response.
    pub fn from_antigravity_initialize(init_result: &Value) -> Self {
        let prompt_caps = init_result.pointer("/agentCapabilities/promptCapabilities");
        let image = prompt_caps
            .and_then(|v| v.get("image"))
            .and_then(Value::as_bool)
            .unwrap_or(true);
        let audio = prompt_caps
            .and_then(|v| v.get("audio"))
            .and_then(Value::as_bool)
            .unwrap_or(true);
        Self { image, audio }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedImageAttachment {
    pub canonical_path: PathBuf,
    pub mime_type: &'static str,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedMultimodalAttachment {
    pub canonical_path: PathBuf,
    pub filename: String,
    pub modality: AttachmentModality,
    pub mime_type: &'static str,
    pub size_bytes: u64,
    pub pdf_metadata: Option<PdfMetadata>,
    /// Cached full bytes when already read (e.g., PDF inspection <= 20 MiB) to avoid double disk reads.
    pub inline_bytes: Option<Vec<u8>>,
}

const ATTACHMENT_LINE_PREFIXES: [&str; 5] = [
    "Attached image file:",
    "Attached PDF file:",
    "Attached audio file:",
    "Attached video file:",
    "Attached file:",
];

fn extract_attached_context_sections(prompt: &str) -> Option<Vec<&str>> {
    const OPEN: &str = "<ATTACHED_CONTEXT>";
    const CLOSE: &str = "</ATTACHED_CONTEXT>";
    let mut sections = Vec::new();
    let mut rest = prompt;
    while let Some(start_idx) = rest.find(OPEN) {
        let after_open = &rest[start_idx + OPEN.len()..];
        let Some(end_idx) = after_open.find(CLOSE) else {
            break;
        };
        sections.push(&after_open[..end_idx]);
        rest = &after_open[end_idx + CLOSE.len()..];
    }
    (!sections.is_empty()).then_some(sections)
}

fn collect_prefixed_paths(
    text: &str,
    prefixes: &[&str],
    out: &mut Vec<PathBuf>,
    seen: &mut HashSet<String>,
) {
    for line in text.lines() {
        let trimmed = line.trim();
        for &prefix in prefixes {
            if let Some(rest) = trimmed.strip_prefix(prefix) {
                let clean = rest.trim().trim_matches('`').trim();
                if !clean.is_empty() && seen.insert(clean.to_string()) {
                    out.push(PathBuf::from(clean));
                }
                break;
            }
        }
    }
}

/// Extract deduplicated `Attached image file: \`<path>\`` entries in prompt order.
/// When `<ATTACHED_CONTEXT>...</ATTACHED_CONTEXT>` is present, extraction is strictly scoped to
/// those sections so pasted prose quotes in `<USER_REQUEST>` cannot re-attach prior files.
pub fn extract_staged_image_paths(prompt: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    if let Some(sections) = extract_attached_context_sections(prompt) {
        for sec in sections {
            collect_prefixed_paths(sec, &["Attached image file:"], &mut out, &mut seen);
        }
    } else {
        collect_prefixed_paths(prompt, &["Attached image file:"], &mut out, &mut seen);
    }
    out
}

/// Extract all deduplicated staged attachment file paths (`Attached file:`, `Attached image file:`,
/// `Attached PDF file:`, `Attached audio file:`, `Attached video file:`) in prompt order.
/// Strictly scoped to `<ATTACHED_CONTEXT>...</ATTACHED_CONTEXT>` when present.
pub fn extract_staged_attachment_paths(prompt: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    if let Some(sections) = extract_attached_context_sections(prompt) {
        for sec in sections {
            collect_prefixed_paths(sec, &ATTACHMENT_LINE_PREFIXES, &mut out, &mut seen);
        }
    } else {
        collect_prefixed_paths(prompt, &ATTACHMENT_LINE_PREFIXES, &mut out, &mut seen);
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

fn is_pdf_whitespace(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\r' | b'\n' | 0x0C | 0x00)
}

fn is_pdf_delimiter(b: u8) -> bool {
    matches!(
        b,
        b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%'
    )
}

fn is_pdf_token_boundary(bytes: &[u8], idx: usize) -> bool {
    idx >= bytes.len() || is_pdf_whitespace(bytes[idx]) || is_pdf_delimiter(bytes[idx])
}

fn parse_pdf_version(data: &[u8]) -> Option<String> {
    let rest = data.strip_prefix(b"%PDF-")?;
    let mut ver = String::new();
    for &b in rest.iter().take(8) {
        if b.is_ascii_digit() || b == b'.' {
            ver.push(char::from(b));
        } else {
            break;
        }
    }
    if ver.is_empty() || !ver.bytes().any(|b| b.is_ascii_digit()) {
        None
    } else {
        Some(ver)
    }
}

/// Locate the enclosing `<< ... >>` dictionary around `/Type /Pages` across an 8 KiB window while
/// tracking nested `<<`/`>>` depth so >100-page `/Kids [...]` arrays and nested `/Resources << >>`
/// dictionaries never misattribute or truncate `/Count N`.
fn parse_pages_dict_count(data: &[u8], pages_pos: usize) -> Option<u32> {
    let min_start = pages_pos.saturating_sub(PDF_PAGES_DICT_WINDOW_BYTES);
    let mut dict_start = min_start;
    let mut back_depth: usize = 0;
    let mut k = pages_pos;
    while k >= min_start + 2 {
        let pair = &data[k - 2..k];
        if pair == b">>" {
            back_depth = back_depth.saturating_add(1);
            k -= 2;
        } else if pair == b"<<" {
            if back_depth == 0 {
                dict_start = k;
                break;
            }
            back_depth -= 1;
            k -= 2;
        } else {
            k -= 1;
        }
    }

    let max_end = (pages_pos + PDF_PAGES_DICT_WINDOW_BYTES).min(data.len());
    let mut dict_end = max_end;
    let mut fwd_depth: usize = 0;
    let mut m = pages_pos;
    while m + 1 < max_end {
        let pair = &data[m..m + 2];
        if pair == b"<<" {
            fwd_depth = fwd_depth.saturating_add(1);
            m += 2;
        } else if pair == b">>" {
            if fwd_depth == 0 {
                dict_end = m;
                break;
            }
            fwd_depth -= 1;
            m += 2;
        } else {
            m += 1;
        }
    }

    let slice = &data[dict_start..dict_end];
    let mut depth: usize = 0;
    let mut i = 0usize;
    while i < slice.len() {
        if i + 1 < slice.len() && &slice[i..i + 2] == b"<<" {
            depth = depth.saturating_add(1);
            i += 2;
            continue;
        }
        if i + 1 < slice.len() && &slice[i..i + 2] == b">>" {
            depth = depth.saturating_sub(1);
            i += 2;
            continue;
        }
        if depth == 0
            && i + 6 <= slice.len()
            && &slice[i..i + 6] == b"/Count"
            && is_pdf_token_boundary(slice, i + 6)
        {
            let mut j = i + 6;
            while j < slice.len() && is_pdf_whitespace(slice[j]) {
                j += 1;
            }
            let num_start = j;
            while j < slice.len() && slice[j].is_ascii_digit() && (j - num_start) < 9 {
                j += 1;
            }
            if j > num_start
                && is_pdf_token_boundary(slice, j)
                && let Ok(s) = std::str::from_utf8(&slice[num_start..j])
                && let Ok(n) = s.parse::<u32>()
                && n > 0
                && n <= MAX_SANE_PDF_PAGES
            {
                return Some(n);
            }
            i = j;
            continue;
        }
        i += 1;
    }
    None
}

fn decode_pdf_title_bytes(raw: &[u8]) -> Option<String> {
    if raw.is_empty() {
        return None;
    }
    let decoded = if raw.len() >= 2 && raw[0] == 0xFE && raw[1] == 0xFF {
        let units: Vec<u16> = raw[2..]
            .chunks_exact(2)
            .map(|c| u16::from_be_bytes([c[0], c[1]]))
            .collect();
        String::from_utf16(&units).ok()?
    } else if raw.len() >= 2 && raw[0] == 0xFF && raw[1] == 0xFE {
        let units: Vec<u16> = raw[2..]
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        String::from_utf16(&units).ok()?
    } else if !raw.contains(&0)
        && let Ok(utf8) = std::str::from_utf8(raw)
    {
        utf8.to_string()
    } else {
        if raw.contains(&0) {
            return None;
        }
        raw.iter().map(|&b| char::from(b)).collect()
    };

    let mut clean = String::with_capacity(decoded.len());
    let mut bad_chars = 0usize;
    let mut total_chars = 0usize;
    for ch in decoded.chars() {
        total_chars += 1;
        if ch == '\0' || ch == '\u{FFFD}' {
            bad_chars += 1;
        } else if ch.is_control() {
            if matches!(ch, '\t' | '\n' | '\r') {
                clean.push(' ');
            } else {
                bad_chars += 1;
            }
        } else {
            clean.push(ch);
        }
    }
    let trimmed = clean.trim();
    if trimmed.is_empty() || total_chars == 0 || bad_chars * 5 > total_chars {
        return None;
    }
    Some(trimmed.chars().take(160).collect())
}

fn parse_pdf_title_at(data: &[u8], value_start: usize) -> Option<String> {
    let mut j = value_start;
    while j < data.len() && is_pdf_whitespace(data[j]) {
        j += 1;
    }
    if j >= data.len() {
        return None;
    }

    if data[j] == b'(' {
        j += 1;
        let mut depth = 1u32;
        let mut out = Vec::with_capacity(64);
        let limit = (j + 512).min(data.len());
        while j < limit && depth > 0 {
            let b = data[j];
            if b == b'\\' && j + 1 < limit {
                j += 1;
                let esc = data[j];
                match esc {
                    b'n' => out.push(b'\n'),
                    b'r' => out.push(b'\r'),
                    b't' => out.push(b'\t'),
                    b'b' => out.push(0x08),
                    b'f' => out.push(0x0C),
                    b'(' | b')' | b'\\' => out.push(esc),
                    b'0'..=b'7' => {
                        let mut oct = u32::from(esc - b'0');
                        for _ in 0..2 {
                            if j + 1 < limit && matches!(data[j + 1], b'0'..=b'7') {
                                j += 1;
                                oct = (oct << 3) | u32::from(data[j] - b'0');
                            } else {
                                break;
                            }
                        }
                        out.push((oct & 0xFF) as u8);
                    }
                    b'\r' | b'\n' => {}
                    other => out.push(other),
                }
                j += 1;
            } else if b == b'(' {
                depth = depth.saturating_add(1);
                out.push(b);
                j += 1;
            } else if b == b')' {
                depth -= 1;
                if depth > 0 {
                    out.push(b);
                }
                j += 1;
            } else {
                out.push(b);
                j += 1;
            }
        }
        if depth != 0 {
            return None;
        }
        return decode_pdf_title_bytes(&out);
    }

    if data[j] == b'<' && data.get(j + 1) != Some(&b'<') {
        j += 1;
        let limit = (j + 512).min(data.len());
        let mut nibbles = Vec::with_capacity(128);
        while j < limit && data[j] != b'>' {
            let b = data[j];
            if !is_pdf_whitespace(b) {
                let val = char::from(b).to_digit(16)?;
                nibbles.push(val as u8);
            }
            j += 1;
        }
        if j >= data.len() || data[j] != b'>' || nibbles.is_empty() {
            return None;
        }
        if nibbles.len() % 2 == 1 {
            nibbles.push(0);
        }
        let raw: Vec<u8> = nibbles
            .chunks_exact(2)
            .map(|pair| (pair[0] << 4) | pair[1])
            .collect();
        return decode_pdf_title_bytes(&raw);
    }

    None
}

/// Fast, bounded, pure-Rust PDF metadata inspector (O(N), zero recursion, never panics on
/// malformed, truncated, or encrypted PDFs).
pub fn inspect_pdf_metadata(bytes: &[u8]) -> PdfMetadata {
    let data = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    if !data.starts_with(b"%PDF-") {
        return PdfMetadata::default();
    }
    let bounded = &data[..data.len().min(MAX_PDF_INSPECT_BYTES)];
    let pdf_version = parse_pdf_version(bounded);

    let mut leaf_page_count: u32 = 0;
    let mut max_pages_dict_count: Option<u32> = None;
    let mut title: Option<String> = None;

    let mut i = 5usize;
    let len = bounded.len();
    while i < len {
        if bounded[i] != b'/' {
            i += 1;
            continue;
        }

        if i + 5 <= len && &bounded[i..i + 5] == b"/Type" && is_pdf_token_boundary(bounded, i + 5) {
            let mut j = i + 5;
            while j < len && is_pdf_whitespace(bounded[j]) {
                j += 1;
            }
            if j + 6 <= len
                && &bounded[j..j + 6] == b"/Pages"
                && is_pdf_token_boundary(bounded, j + 6)
            {
                if let Some(count) = parse_pages_dict_count(bounded, j) {
                    max_pages_dict_count =
                        Some(max_pages_dict_count.map_or(count, |prev| prev.max(count)));
                }
                i = j + 6;
                continue;
            }
            if j + 5 <= len
                && &bounded[j..j + 5] == b"/Page"
                && is_pdf_token_boundary(bounded, j + 5)
            {
                leaf_page_count = leaf_page_count.saturating_add(1);
                i = j + 5;
                continue;
            }
            i += 5;
            continue;
        }

        if title.is_none()
            && i + 6 <= len
            && &bounded[i..i + 6] == b"/Title"
            && is_pdf_token_boundary(bounded, i + 6)
        {
            if let Some(parsed) = parse_pdf_title_at(bounded, i + 6) {
                title = Some(parsed);
            }
            i += 6;
            continue;
        }

        i += 1;
    }

    // Prefer root `/Pages /Count N` over raw `/Type /Page` count so incrementally updated PDFs
    // with superseded page objects are not double-counted, falling back to leaf page count.
    let page_count = max_pages_dict_count.or((leaf_page_count > 0).then_some(leaf_page_count));

    PdfMetadata {
        pdf_version,
        page_count,
        title,
    }
}

fn read_file_prefix(path: &Path, max_len: usize) -> Option<Vec<u8>> {
    let file = fs::File::open(path).ok()?;
    let mut buf = Vec::with_capacity(max_len.min(SNIFF_HEADER_BYTES));
    file.take(max_len as u64).read_to_end(&mut buf).ok()?;
    Some(buf)
}

fn read_file_capped(path: &Path, max_bytes: usize) -> Option<Vec<u8>> {
    let meta = fs::metadata(path).ok()?;
    if (meta.len() as usize) > max_bytes {
        return None;
    }
    let file = fs::File::open(path).ok()?;
    let mut buf = Vec::with_capacity((meta.len() as usize).min(max_bytes));
    file.take((max_bytes as u64).saturating_add(1))
        .read_to_end(&mut buf)
        .ok()?;
    if buf.len() > max_bytes {
        return None;
    }
    Some(buf)
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
    for candidate in extract_staged_attachment_paths(prompt) {
        let Ok(canon_file) = fs::canonicalize(&candidate) else {
            continue;
        };
        if !canon_file.starts_with(&canon_dir) || !canon_file.is_file() {
            continue;
        }
        if !seen_canon.insert(canon_file.clone()) {
            continue;
        }
        let Some(bytes) = read_file_capped(&canon_file, max_bytes) else {
            continue;
        };
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

fn resolve_authorized_staged_media_inner(
    prompt: &str,
    attachments_dir: Option<&Path>,
    retain_pdf_bytes_limit: usize,
) -> Vec<ResolvedMultimodalAttachment> {
    let Some(dir) = attachments_dir else {
        return Vec::new();
    };
    let Ok(canon_dir) = fs::canonicalize(dir) else {
        return Vec::new();
    };

    let mut out = Vec::new();
    let mut seen_canon = HashSet::new();
    for candidate in extract_staged_attachment_paths(prompt) {
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
        let size_bytes = meta.len();
        if size_bytes == 0 || (size_bytes as usize) > MAX_STAGED_MEDIA_BYTES {
            continue;
        }
        let filename = canon_file
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("attachment")
            .to_string();
        let Some(header) = read_file_prefix(&canon_file, SNIFF_HEADER_BYTES) else {
            continue;
        };
        let Some(sniffed) = sniff_media_mime(&header, Some(&filename)) else {
            continue;
        };
        let (pdf_metadata, inline_bytes) = if sniffed.modality == AttachmentModality::Pdf {
            if let Some(full) = read_file_capped(&canon_file, MAX_STAGED_MEDIA_BYTES) {
                let meta = inspect_pdf_metadata(&full);
                let retained = (full.len() <= retain_pdf_bytes_limit).then_some(full);
                (Some(meta), retained)
            } else {
                (None, None)
            }
        } else {
            (None, None)
        };

        out.push(ResolvedMultimodalAttachment {
            canonical_path: canon_file,
            filename,
            modality: sniffed.modality,
            mime_type: sniffed.mime_type,
            size_bytes,
            pdf_metadata,
            inline_bytes,
        });
    }
    out
}

/// Resolve all authorized staged multimodal files (`Image`, `Pdf`, `Audio`, `Video`) strictly inside
/// `attachments_dir`, verifying regular-file containment, size cap (<= 25 MiB), and magic bytes.
pub fn resolve_authorized_staged_media(
    prompt: &str,
    attachments_dir: Option<&Path>,
) -> Vec<ResolvedMultimodalAttachment> {
    resolve_authorized_staged_media_inner(prompt, attachments_dir, 0)
}

fn format_size_human(bytes: u64) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.1} MB", (bytes as f64) / (1024.0 * 1024.0))
    } else if bytes >= 1024 {
        format!("{} KB", (bytes + 511) / 1024)
    } else {
        format!("{bytes} B")
    }
}

fn file_uri_from_path(path: &Path) -> String {
    let raw_norm = path.to_string_lossy().replace('\\', "/");
    // Strip Windows extended-length path prefix (`\\?\C:\...` -> `C:/...`, `\\?\UNC\...` -> `//...`)
    let raw = if let Some(unc) = raw_norm.strip_prefix("//?/UNC/") {
        format!("//{unc}")
    } else if let Some(stripped) = raw_norm.strip_prefix("//?/") {
        stripped.to_string()
    } else {
        raw_norm
    };

    let bytes = raw.as_bytes();
    let has_windows_drive = bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':';

    let mut encoded = String::with_capacity(raw.len() + 16);
    encoded.push_str("file://");
    if !raw.starts_with('/') {
        encoded.push('/');
    }
    for (idx, &b) in bytes.iter().enumerate() {
        let is_drive_colon = has_windows_drive && idx == 1 && b == b':';
        let keep = b.is_ascii_alphanumeric()
            || matches!(b, b'/' | b'-' | b'_' | b'.' | b'~')
            || is_drive_colon;
        if keep {
            encoded.push(char::from(b));
        } else {
            encoded.push_str(&format!("%{b:02X}"));
        }
    }
    encoded
}

fn format_pdf_summary_prefix(att: &ResolvedMultimodalAttachment) -> String {
    let size = format_size_human(att.size_bytes);
    let mut parts = vec![att.filename.clone()];
    if let Some(meta) = &att.pdf_metadata {
        if let Some(pages) = meta.page_count {
            parts.push(if pages == 1 {
                "1 page".to_string()
            } else {
                format!("{pages} pages")
            });
        }
        if let Some(ver) = &meta.pdf_version {
            parts.push(format!("PDF {ver}"));
        }
    }
    parts.push(size);
    parts.join(", ")
}

fn append_media_hints(prompt: &str, hints: &[String]) -> String {
    if hints.is_empty() {
        return prompt.to_string();
    }
    let mut out = String::with_capacity(prompt.len() + hints.len() * 96 + 32);
    out.push_str(prompt);
    if !prompt.ends_with('\n') {
        out.push('\n');
    }
    out.push('\n');
    for (idx, hint) in hints.iter().enumerate() {
        if idx > 0 {
            out.push('\n');
        }
        out.push_str(hint);
    }
    out
}

/// Build ACP prompt blocks (`Antigravity` and generic ACP servers) with optimal per-modality transport:
/// - Images (<= 8 MiB, within cumulative 16 MiB turn budget): native ACP `image` base64 block.
/// - Audio (<= 8 MiB, when supported & within cumulative 16 MiB budget): native ACP `audio` base64 block + metadata hint.
/// - PDF & Video (and overflow/non-inlined Audio/Images): ACP `resource_link` (`file://...`) +
///   enriched metadata hint (including PDF page count & `>10 pages` targeted reading guidance).
pub fn build_acp_prompt_blocks(
    prompt: &str,
    attachments_dir: Option<&Path>,
    caps: AcpPromptMediaCaps,
) -> Vec<Value> {
    let media = resolve_authorized_staged_media(prompt, attachments_dir);
    let mut media_blocks = Vec::new();
    let mut hints = Vec::new();
    let mut inlined_raw_bytes: usize = 0;

    for att in media {
        let canon_str = att.canonical_path.to_string_lossy();
        match att.modality {
            AttachmentModality::Image => {
                let within_budget = inlined_raw_bytes.saturating_add(att.size_bytes as usize)
                    <= ACP_TOTAL_INLINE_MAX_BYTES;
                if caps.image
                    && within_budget
                    && let Some(bytes) =
                        read_file_capped(&att.canonical_path, AGY_INLINE_IMAGE_MAX_BYTES)
                {
                    inlined_raw_bytes = inlined_raw_bytes.saturating_add(bytes.len());
                    media_blocks.push(json!({
                        "type": "image",
                        "data": B64.encode(&bytes),
                        "mimeType": att.mime_type,
                    }));
                } else {
                    hints.push(format!(
                        "[Image: {}, {}, {} — staged at `{canon_str}`]",
                        att.filename,
                        att.mime_type,
                        format_size_human(att.size_bytes)
                    ));
                    media_blocks.push(json!({
                        "type": "resource_link",
                        "uri": file_uri_from_path(&att.canonical_path),
                        "name": att.filename,
                        "mimeType": att.mime_type,
                    }));
                }
            }
            AttachmentModality::Audio => {
                let within_budget = inlined_raw_bytes.saturating_add(att.size_bytes as usize)
                    <= ACP_TOTAL_INLINE_MAX_BYTES;
                if caps.audio
                    && within_budget
                    && let Some(bytes) =
                        read_file_capped(&att.canonical_path, AGY_INLINE_AUDIO_MAX_BYTES)
                {
                    inlined_raw_bytes = inlined_raw_bytes.saturating_add(bytes.len());
                    media_blocks.push(json!({
                        "type": "audio",
                        "data": B64.encode(&bytes),
                        "mimeType": att.mime_type,
                    }));
                    hints.push(format!(
                        "[Audio: {}, {}, {} — staged at `{canon_str}`]",
                        att.filename,
                        att.mime_type,
                        format_size_human(att.size_bytes)
                    ));
                } else {
                    hints.push(format!(
                        "[Audio: {}, {}, {} — staged at `{canon_str}`]",
                        att.filename,
                        att.mime_type,
                        format_size_human(att.size_bytes)
                    ));
                    media_blocks.push(json!({
                        "type": "resource_link",
                        "uri": file_uri_from_path(&att.canonical_path),
                        "name": att.filename,
                        "mimeType": att.mime_type,
                    }));
                }
            }
            AttachmentModality::Pdf => {
                let summary = format_pdf_summary_prefix(&att);
                let page_count = att.pdf_metadata.as_ref().and_then(|m| m.page_count);
                if page_count.is_some_and(|p| p > AGY_PDF_LARGE_PAGE_HINT_THRESHOLD) {
                    hints.push(format!(
                        "[PDF: {summary} — exceeds 10 pages; read specific page ranges from `{canon_str}`]"
                    ));
                } else {
                    hints.push(format!("[PDF: {summary} — staged at `{canon_str}`]"));
                }
                media_blocks.push(json!({
                    "type": "resource_link",
                    "uri": file_uri_from_path(&att.canonical_path),
                    "name": att.filename,
                    "mimeType": att.mime_type,
                }));
            }
            AttachmentModality::Video => {
                hints.push(format!(
                    "[Video: {}, {}, {} — staged at `{canon_str}`]",
                    att.filename,
                    att.mime_type,
                    format_size_human(att.size_bytes)
                ));
                media_blocks.push(json!({
                    "type": "resource_link",
                    "uri": file_uri_from_path(&att.canonical_path),
                    "name": att.filename,
                    "mimeType": att.mime_type,
                }));
            }
        }
    }

    let text = append_media_hints(prompt, &hints);
    let mut blocks = Vec::with_capacity(1 + media_blocks.len());
    blocks.push(json!({ "type": "text", "text": text }));
    blocks.extend(media_blocks);
    blocks
}

/// Build Antigravity ACP prompt blocks (native image & audio inline blocks <= 8 MiB within
/// cumulative 16 MiB budget, `resource_link` + metadata hints for PDF and video).
pub fn build_agy_prompt_blocks(prompt: &str, attachments_dir: Option<&Path>) -> Vec<Value> {
    build_acp_prompt_blocks(prompt, attachments_dir, AcpPromptMediaCaps::antigravity())
}

/// Build Claude Code Anthropic content blocks:
/// - Images (<= 4 MiB, within cumulative 20 MiB turn budget): native Anthropic `image` base64 block.
/// - PDFs (<= 20 MiB and <= 100 pages, or <= 5 MiB when compressed PDF page count is unknown,
///   within cumulative 20 MiB turn budget): native Anthropic `document` base64 block (reusing bytes
///   read during PDF inspection with zero double disk read).
/// - Oversized/overflow PDFs, Audio, Video, oversized/overflow Images: staged path + concise hint.
pub fn build_claude_content_blocks(prompt: &str, attachments_dir: Option<&Path>) -> Vec<Value> {
    let media =
        resolve_authorized_staged_media_inner(prompt, attachments_dir, CLAUDE_INLINE_PDF_MAX_BYTES);
    let mut media_blocks = Vec::new();
    let mut hints = Vec::new();
    let mut inlined_raw_bytes: usize = 0;

    for mut att in media {
        let canon_str = att.canonical_path.to_string_lossy();
        match att.modality {
            AttachmentModality::Image => {
                let within_budget = inlined_raw_bytes.saturating_add(att.size_bytes as usize)
                    <= CLAUDE_TOTAL_INLINE_MAX_BYTES;
                if within_budget
                    && let Some(bytes) =
                        read_file_capped(&att.canonical_path, CLAUDE_INLINE_IMAGE_MAX_BYTES)
                {
                    inlined_raw_bytes = inlined_raw_bytes.saturating_add(bytes.len());
                    media_blocks.push(json!({
                        "type": "image",
                        "source": {
                            "type": "base64",
                            "media_type": att.mime_type,
                            "data": B64.encode(&bytes),
                        }
                    }));
                } else {
                    hints.push(format!(
                        "[Image: {}, {}, {} — exceeds inline cap; staged at `{canon_str}`]",
                        att.filename,
                        att.mime_type,
                        format_size_human(att.size_bytes)
                    ));
                }
            }
            AttachmentModality::Pdf => {
                let page_count = att.pdf_metadata.as_ref().and_then(|m| m.page_count);
                let size_usize = att.size_bytes as usize;
                let within_pdf_cap = match page_count {
                    Some(pages) => {
                        pages <= CLAUDE_INLINE_PDF_MAX_PAGES
                            && size_usize <= CLAUDE_INLINE_PDF_MAX_BYTES
                    }
                    None => size_usize <= CLAUDE_UNKNOWN_PAGES_PDF_MAX_BYTES,
                };
                let within_turn_budget =
                    inlined_raw_bytes.saturating_add(size_usize) <= CLAUDE_TOTAL_INLINE_MAX_BYTES;

                if within_pdf_cap && within_turn_budget {
                    let bytes_opt = att.inline_bytes.take().or_else(|| {
                        read_file_capped(&att.canonical_path, CLAUDE_INLINE_PDF_MAX_BYTES)
                    });
                    if let Some(bytes) = bytes_opt {
                        inlined_raw_bytes = inlined_raw_bytes.saturating_add(bytes.len());
                        media_blocks.push(json!({
                            "type": "document",
                            "source": {
                                "type": "base64",
                                "media_type": "application/pdf",
                                "data": B64.encode(&bytes),
                            }
                        }));
                        continue;
                    }
                }
                let summary = format_pdf_summary_prefix(&att);
                hints.push(format!(
                    "[PDF: {summary} — read specific page ranges from `{canon_str}`]"
                ));
            }
            AttachmentModality::Audio => {
                hints.push(format!(
                    "[Audio: {}, {}, {} — staged at `{canon_str}`]",
                    att.filename,
                    att.mime_type,
                    format_size_human(att.size_bytes)
                ));
            }
            AttachmentModality::Video => {
                hints.push(format!(
                    "[Video: {}, {}, {} — staged at `{canon_str}`]",
                    att.filename,
                    att.mime_type,
                    format_size_human(att.size_bytes)
                ));
            }
        }
    }

    let text = append_media_hints(prompt, &hints);
    let mut blocks = Vec::with_capacity(1 + media_blocks.len());
    blocks.push(json!({ "type": "text", "text": text }));
    blocks.extend(media_blocks);
    blocks
}

/// Enrich a prompt with concise metadata lines (MIME, size, PDF page count, canonical path) for
/// authorized staged PDF, Audio, and Video attachments (used by Codex and Pi).
pub fn enrich_prompt_with_media_metadata(prompt: &str, attachments_dir: Option<&Path>) -> String {
    let media = resolve_authorized_staged_media(prompt, attachments_dir);
    let mut hints = Vec::new();
    for att in media {
        let canon_str = att.canonical_path.to_string_lossy();
        match att.modality {
            AttachmentModality::Image => {}
            AttachmentModality::Pdf => {
                let summary = format_pdf_summary_prefix(&att);
                hints.push(format!(
                    "[PDF: {summary}, {} — staged at `{canon_str}`]",
                    att.mime_type
                ));
            }
            AttachmentModality::Audio => {
                hints.push(format!(
                    "[Audio: {}, {}, {} — staged at `{canon_str}`]",
                    att.filename,
                    att.mime_type,
                    format_size_human(att.size_bytes)
                ));
            }
            AttachmentModality::Video => {
                hints.push(format!(
                    "[Video: {}, {}, {} — staged at `{canon_str}`]",
                    att.filename,
                    att.mime_type,
                    format_size_human(att.size_bytes)
                ));
            }
        }
    }
    append_media_hints(prompt, &hints)
}

/// Build Codex input items: native `localImage` for images + enriched metadata lines for PDF/audio/video.
pub fn build_codex_input_items(prompt: &str, attachments_dir: Option<&Path>) -> Vec<Value> {
    let media = resolve_authorized_staged_media(prompt, attachments_dir);
    let mut local_images = Vec::new();
    let mut hints = Vec::new();

    for att in media {
        let canon_str = att.canonical_path.to_string_lossy().to_string();
        match att.modality {
            AttachmentModality::Image => {
                if (att.size_bytes as usize) <= CODEX_LOCAL_IMAGE_MAX_BYTES {
                    local_images.push(json!({
                        "type": "localImage",
                        "path": canon_str,
                    }));
                }
            }
            AttachmentModality::Pdf => {
                let summary = format_pdf_summary_prefix(&att);
                hints.push(format!(
                    "[PDF: {summary}, {} — staged at `{canon_str}`]",
                    att.mime_type
                ));
            }
            AttachmentModality::Audio => {
                hints.push(format!(
                    "[Audio: {}, {}, {} — staged at `{canon_str}`]",
                    att.filename,
                    att.mime_type,
                    format_size_human(att.size_bytes)
                ));
            }
            AttachmentModality::Video => {
                hints.push(format!(
                    "[Video: {}, {}, {} — staged at `{canon_str}`]",
                    att.filename,
                    att.mime_type,
                    format_size_human(att.size_bytes)
                ));
            }
        }
    }

    let text = append_media_hints(prompt, &hints);
    let mut items = Vec::with_capacity(1 + local_images.len());
    items.push(json!({
        "type": "text",
        "text": text,
        "text_elements": []
    }));
    items.extend(local_images);
    items
}

fn has_potential_staged_attachments(prompt: &str, attachments_dir: Option<&Path>) -> bool {
    attachments_dir.is_some() && !extract_staged_attachment_paths(prompt).is_empty()
}

/// Async wrapper offloading blocking media disk I/O + base64 encoding off Tokio workers.
pub async fn build_claude_content_blocks_async(
    prompt: String,
    attachments_dir: Option<PathBuf>,
) -> Vec<Value> {
    if !has_potential_staged_attachments(&prompt, attachments_dir.as_deref()) {
        return vec![json!({ "type": "text", "text": prompt })];
    }
    let fallback_prompt = prompt.clone();
    tokio::task::spawn_blocking(move || {
        build_claude_content_blocks(&prompt, attachments_dir.as_deref())
    })
    .await
    .unwrap_or_else(|_| vec![json!({ "type": "text", "text": fallback_prompt })])
}

/// Async wrapper offloading ACP media disk I/O + base64 encoding off Tokio workers.
pub async fn build_acp_prompt_blocks_async(
    prompt: String,
    attachments_dir: Option<PathBuf>,
    caps: AcpPromptMediaCaps,
) -> Vec<Value> {
    if !has_potential_staged_attachments(&prompt, attachments_dir.as_deref()) {
        return vec![json!({ "type": "text", "text": prompt })];
    }
    let fallback_prompt = prompt.clone();
    tokio::task::spawn_blocking(move || {
        build_acp_prompt_blocks(&prompt, attachments_dir.as_deref(), caps)
    })
    .await
    .unwrap_or_else(|_| vec![json!({ "type": "text", "text": fallback_prompt })])
}

/// Async wrapper offloading Antigravity media disk I/O + base64 encoding off Tokio workers.
pub async fn build_agy_prompt_blocks_async(
    prompt: String,
    attachments_dir: Option<PathBuf>,
) -> Vec<Value> {
    build_acp_prompt_blocks_async(prompt, attachments_dir, AcpPromptMediaCaps::antigravity()).await
}

/// Async wrapper offloading Codex media inspection off Tokio workers.
pub async fn build_codex_input_items_async(
    prompt: String,
    attachments_dir: Option<PathBuf>,
) -> Vec<Value> {
    if !has_potential_staged_attachments(&prompt, attachments_dir.as_deref()) {
        return vec![json!({ "type": "text", "text": prompt, "text_elements": [] })];
    }
    let fallback_prompt = prompt.clone();
    tokio::task::spawn_blocking(move || {
        build_codex_input_items(&prompt, attachments_dir.as_deref())
    })
    .await
    .unwrap_or_else(|_| {
        vec![json!({ "type": "text", "text": fallback_prompt, "text_elements": [] })]
    })
}

/// Async wrapper offloading Pi image base64 encoding & media metadata inspection off Tokio workers.
pub async fn build_pi_prompt_payload_async(
    prompt: String,
    attachments_dir: Option<PathBuf>,
) -> (String, Vec<Value>) {
    if !has_potential_staged_attachments(&prompt, attachments_dir.as_deref()) {
        return (prompt, Vec::new());
    }
    let fallback_prompt = prompt.clone();
    tokio::task::spawn_blocking(move || {
        let images: Vec<Value> = resolve_authorized_staged_images(
            &prompt,
            attachments_dir.as_deref(),
            CLAUDE_INLINE_IMAGE_MAX_BYTES,
        )
        .into_iter()
        .map(|img| {
            json!({
                "type": "image",
                "data": B64.encode(&img.bytes),
                "mimeType": img.mime_type,
            })
        })
        .collect();
        let enriched = enrich_prompt_with_media_metadata(&prompt, attachments_dir.as_deref());
        (enriched, images)
    })
    .await
    .unwrap_or_else(|_| (fallback_prompt, Vec::new()))
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

    fn make_synthetic_pdf(version: &str, pages: u32, title: Option<&str>) -> Vec<u8> {
        let mut s = format!("%PDF-{version}\n");
        if let Some(t) = title {
            s.push_str(&format!("1 0 obj\n<< /Title ({t}) >>\nendobj\n"));
        }
        s.push_str(&format!(
            "2 0 obj\n<< /Type /Pages /Count {pages} /Kids [] >>\nendobj\n"
        ));
        for p in 0..pages {
            s.push_str(&format!(
                "{} 0 obj\n<< /Type /Page /Parent 2 0 R >>\nendobj\n",
                p + 3
            ));
        }
        s.push_str("%%EOF\n");
        s.into_bytes()
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
    fn sniffs_pdf_audio_video_magic_bytes_and_rejects_spoofed_extensions() {
        assert_eq!(
            sniff_media_mime(b"%PDF-1.7\n...", Some("spec.pdf")),
            Some(SniffedMedia {
                modality: AttachmentModality::Pdf,
                mime_type: "application/pdf",
            })
        );
        assert_eq!(
            sniff_media_mime(b"ID3\x04\x00\x00\x00\x00\x00\x00", Some("clip.mp3")),
            Some(SniffedMedia {
                modality: AttachmentModality::Audio,
                mime_type: "audio/mpeg",
            })
        );
        assert_eq!(
            sniff_media_mime(&[0xFF, 0xFB, 0x90, 0x64], Some("raw.mp3")),
            Some(SniffedMedia {
                modality: AttachmentModality::Audio,
                mime_type: "audio/mpeg",
            })
        );
        // UTF-16LE BOM text MUST NOT be misclassified as audio/mpeg
        assert_eq!(
            sniff_media_mime(&[0xFF, 0xFE, 0x41, 0x00, 0x42, 0x00], Some("notes.txt")),
            None
        );
        assert_eq!(
            sniff_media_mime(&[0xFF, 0xFE, 0x41, 0x00, 0x42, 0x00], Some("notes.mp3")),
            None
        );
        assert_eq!(
            sniff_media_mime(b"RIFF\x24\x00\x00\x00WAVEfmt ", Some("voice.wav")),
            Some(SniffedMedia {
                modality: AttachmentModality::Audio,
                mime_type: "audio/wav",
            })
        );
        assert_eq!(
            sniff_media_mime(b"OggS\x00\x02\x00\x00", Some("note.ogg")),
            Some(SniffedMedia {
                modality: AttachmentModality::Audio,
                mime_type: "audio/ogg",
            })
        );
        assert_eq!(
            sniff_media_mime(b"fLaC\x00\x00\x00\x22", Some("lossless.flac")),
            Some(SniffedMedia {
                modality: AttachmentModality::Audio,
                mime_type: "audio/flac",
            })
        );
        assert_eq!(
            sniff_media_mime(
                b"\x00\x00\x00\x18ftypM4A \x00\x00\x00\x00M4A ",
                Some("a.m4a")
            ),
            Some(SniffedMedia {
                modality: AttachmentModality::Audio,
                mime_type: "audio/mp4",
            })
        );
        assert_eq!(
            sniff_media_mime(
                b"\x00\x00\x00\x14ftypisom\x00\x00\x02\x00",
                Some("memo.m4a")
            ),
            Some(SniffedMedia {
                modality: AttachmentModality::Audio,
                mime_type: "audio/mp4",
            })
        );
        assert_eq!(
            sniff_media_mime(
                b"\x00\x00\x00\x18ftypmp42\x00\x00\x00\x00isom",
                Some("demo.mp4")
            ),
            Some(SniffedMedia {
                modality: AttachmentModality::Video,
                mime_type: "video/mp4",
            })
        );
        assert_eq!(
            sniff_media_mime(
                b"\x00\x00\x00\x14ftypavc1\x00\x00\x00\x00",
                Some("clip.bin")
            ),
            Some(SniffedMedia {
                modality: AttachmentModality::Video,
                mime_type: "video/mp4",
            })
        );
        assert_eq!(
            sniff_media_mime(
                b"\x00\x00\x00\x14ftypqt  \x20\x05\x03\x00",
                Some("screen.mov")
            ),
            Some(SniffedMedia {
                modality: AttachmentModality::Video,
                mime_type: "video/quicktime",
            })
        );
        assert_eq!(
            sniff_media_mime(
                b"\x1A\x45\xDF\xA3\x9F\x42\x82\x84webmV_VP9",
                Some("clip.webm")
            ),
            Some(SniffedMedia {
                modality: AttachmentModality::Video,
                mime_type: "video/webm",
            })
        );
        assert_eq!(
            sniff_media_mime(
                b"\x1A\x45\xDF\xA3\x9F\x42\x82\x84webmA_OPUS",
                Some("voice.webm")
            ),
            Some(SniffedMedia {
                modality: AttachmentModality::Audio,
                mime_type: "audio/webm",
            })
        );
        assert_eq!(
            sniff_media_mime(b"\x1A\x45\xDF\xA3\x9Fmatroska", Some("capture.mkv")),
            Some(SniffedMedia {
                modality: AttachmentModality::Video,
                mime_type: "video/x-matroska",
            })
        );

        // Spoofed extensions MUST be rejected
        assert_eq!(
            sniff_media_mime(b"Hello plain text", Some("fake.pdf")),
            None
        );
        assert_eq!(
            sniff_media_mime(b"#!/bin/sh\nrm -rf /", Some("fake.mp4")),
            None
        );
        assert_eq!(
            sniff_media_mime(b"not audio at all", Some("fake.mp3")),
            None
        );
        assert_eq!(
            sniff_media_mime(&[0xFF, 0xFF, 0xFF, 0xFF], Some("junk.mp3")),
            None
        );
    }

    #[test]
    fn inspects_pdf_metadata_nested_resources_incremental_updates_and_malformed_inputs() {
        let single = make_synthetic_pdf("1.7", 1, Some("Quarterly Report"));
        let m1 = inspect_pdf_metadata(&single);
        assert_eq!(m1.pdf_version.as_deref(), Some("1.7"));
        assert_eq!(m1.page_count, Some(1));
        assert_eq!(m1.title.as_deref(), Some("Quarterly Report"));

        // Multi-page PDF (>10 pages) without /Count in /Pages (must NOT count /Pages as a page!)
        let mut no_count = b"%PDF-2.0\n1 0 obj << /Type /Pages /Kids [] >> endobj\n".to_vec();
        for i in 0..14 {
            no_count.extend_from_slice(
                format!(
                    "{idx} 0 obj << /Type/Page /PageMode /UseNone >> endobj\n",
                    idx = i + 2
                )
                .as_bytes(),
            );
        }
        let m2 = inspect_pdf_metadata(&no_count);
        assert_eq!(m2.pdf_version.as_deref(), Some("2.0"));
        assert_eq!(m2.page_count, Some(14));

        // Nested `/Resources << /Count 999 >>` + long `/Kids [...]` (>1024 bytes) before `/Count 142`
        let kids: String = (3..220).map(|n| format!("{n} 0 R ")).collect();
        let nested_pdf = format!(
            "%PDF-1.7\n1 0 obj << /Type /Pages /Resources << /Count 999 /Font << >> >> /Kids [{kids}] /Count 142 >> endobj\n%%EOF\n"
        );
        let m_nested = inspect_pdf_metadata(nested_pdf.as_bytes());
        assert_eq!(m_nested.page_count, Some(142));

        // Incrementally saved PDF: 2 active pages, but 4 `/Type /Page` entries due to rewritten pages
        let incremental_pdf = concat!(
            "%PDF-1.7\n",
            "1 0 obj << /Type /Pages /Count 2 >> endobj\n",
            "2 0 obj << /Type /Page >> endobj\n",
            "3 0 obj << /Type /Page >> endobj\n",
            "2 0 obj << /Type /Page >> endobj\n",
            "3 0 obj << /Type /Page >> endobj\n",
            "%%EOF\n"
        );
        assert_eq!(
            inspect_pdf_metadata(incremental_pdf.as_bytes()).page_count,
            Some(2)
        );

        // Malformed / truncated / garbage inputs NEVER panic
        assert_eq!(inspect_pdf_metadata(b""), PdfMetadata::default());
        assert_eq!(inspect_pdf_metadata(b"not a pdf"), PdfMetadata::default());
        let broken =
            b"%PDF-1.4\n/Type /Pages << /Count 99999999999999 /Title (\x00\x01\x02unterminated";
        let m4 = inspect_pdf_metadata(broken);
        assert_eq!(m4.pdf_version.as_deref(), Some("1.4"));
        assert_eq!(m4.page_count, None);
        assert_eq!(m4.title, None);
    }

    #[test]
    fn extraction_ignores_pasted_attachment_lines_inside_user_request_when_context_present() {
        let prompt = "<ATTACHED_CONTEXT>\n\
            Attached PDF file: `/tmp/att/current.pdf`\n\
            </ATTACHED_CONTEXT>\n\n\
            <USER_REQUEST>\n\
            Earlier I saw:\n\
            Attached PDF file: `/tmp/att/old_secret.pdf`\n\
            Attached image file: `/tmp/att/old_img.png`\n\
            </USER_REQUEST>";

        let extracted = extract_staged_attachment_paths(prompt);
        assert_eq!(extracted, vec![PathBuf::from("/tmp/att/current.pdf")]);
        assert!(extract_staged_image_paths(prompt).is_empty());
    }

    #[test]
    fn file_uri_strips_windows_extended_prefix_and_encodes_non_drive_colons() {
        assert_eq!(
            file_uri_from_path(Path::new("/Users/me/meeting 10:30.mp4")),
            "file:///Users/me/meeting%2010%3A30.mp4"
        );
        assert_eq!(
            file_uri_from_path(Path::new(r"\\?\C:\Users\me\doc 1.pdf")),
            "file:///C:/Users/me/doc%201.pdf"
        );
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

    #[test]
    fn claude_enforces_pdf_caps_unknown_page_cap_and_cumulative_turn_budget() {
        let root = make_temp_dir("claude_pdf");
        let att_dir = root.join("attachments");
        fs::create_dir_all(&att_dir).expect("mkdir");

        let small_pdf_path = att_dir.join("brief.pdf");
        let small_pdf = make_synthetic_pdf("1.7", 4, Some("Brief"));
        fs::write(&small_pdf_path, &small_pdf).expect("write small pdf");

        let huge_pages_pdf_path = att_dir.join("manual.pdf");
        let huge_pages_pdf = b"%PDF-1.7\n1 0 obj << /Type /Pages /Count 142 >> endobj\n%%EOF\n";
        fs::write(&huge_pages_pdf_path, huge_pages_pdf).expect("write 142p pdf");

        // Compressed PDF 1.5+ with unknown page count > 5 MiB must degrade to staged-path hint
        let unknown_pages_big_path = att_dir.join("compressed_objstm.pdf");
        let mut unknown_big = b"%PDF-1.7\n1 0 obj << /Type /ObjStm >> endobj\n".to_vec();
        unknown_big.resize(CLAUDE_UNKNOWN_PAGES_PDF_MAX_BYTES + 1024, b' ');
        fs::write(&unknown_pages_big_path, &unknown_big).expect("write unknown big pdf");

        let prompt_small = format!(
            "<ATTACHED_CONTEXT>\nAttached PDF file: `{}`\n</ATTACHED_CONTEXT>",
            small_pdf_path.display()
        );
        let blocks_small = build_claude_content_blocks(&prompt_small, Some(&att_dir));
        assert_eq!(blocks_small.len(), 2);
        assert_eq!(blocks_small[1]["type"], "document");
        assert_eq!(blocks_small[1]["source"]["type"], "base64");
        assert_eq!(blocks_small[1]["source"]["media_type"], "application/pdf");
        assert_eq!(blocks_small[1]["source"]["data"], B64.encode(&small_pdf));

        let prompt_big_pages = format!(
            "<ATTACHED_CONTEXT>\nAttached PDF file: `{}`\n</ATTACHED_CONTEXT>",
            huge_pages_pdf_path.display()
        );
        let blocks_big = build_claude_content_blocks(&prompt_big_pages, Some(&att_dir));
        assert_eq!(
            blocks_big.len(),
            1,
            "142-page PDF must NOT be base64 inlined"
        );
        let hint_text = blocks_big[0]["text"].as_str().expect("text");
        assert!(hint_text.contains("142 pages"));
        assert!(hint_text.contains("read specific page ranges"));

        let prompt_unknown_big = format!(
            "<ATTACHED_CONTEXT>\nAttached PDF file: `{}`\n</ATTACHED_CONTEXT>",
            unknown_pages_big_path.display()
        );
        let blocks_unknown = build_claude_content_blocks(&prompt_unknown_big, Some(&att_dir));
        assert_eq!(
            blocks_unknown.len(),
            1,
            ">5 MiB unknown-page compressed PDF must degrade to staged path"
        );

        // Cumulative 20 MiB budget: two 12 MiB valid PDFs -> first inlined, second degrades to hint
        let pdf_a = att_dir.join("part_a.pdf");
        let pdf_b = att_dir.join("part_b.pdf");
        let mut twelve_mb = make_synthetic_pdf("1.7", 3, None);
        twelve_mb.resize(12 * 1024 * 1024, b' ');
        fs::write(&pdf_a, &twelve_mb).expect("write part_a");
        fs::write(&pdf_b, &twelve_mb).expect("write part_b");

        let prompt_multi = format!(
            "<ATTACHED_CONTEXT>\nAttached PDF file: `{}`\nAttached PDF file: `{}`\n</ATTACHED_CONTEXT>",
            pdf_a.display(),
            pdf_b.display()
        );
        let blocks_multi = build_claude_content_blocks(&prompt_multi, Some(&att_dir));
        assert_eq!(
            blocks_multi.len(),
            2,
            "second 12 MiB PDF must overflow cumulative 20 MiB turn budget into hint"
        );
        assert_eq!(blocks_multi[1]["type"], "document");
        assert!(
            blocks_multi[0]["text"]
                .as_str()
                .expect("text")
                .contains("part_b.pdf")
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn builds_agy_acp_and_codex_multimodal_payloads_async_and_blocks_escapes() {
        let root = make_temp_dir("multimodal_all");
        let att_dir = root.join("attachments");
        let outside_dir = root.join("outside");
        fs::create_dir_all(&att_dir).expect("mkdir att");
        fs::create_dir_all(&outside_dir).expect("mkdir outside");

        let pdf_path = att_dir.join("design doc.pdf");
        let pdf_bytes = make_synthetic_pdf("1.7", 18, Some("Design"));
        fs::write(&pdf_path, &pdf_bytes).expect("write pdf");

        let mp3_path = att_dir.join("voice.mp3");
        let mp3_bytes = b"ID3\x04\x00\x00\x00\x00\x00\x00audio-frames";
        fs::write(&mp3_path, mp3_bytes).expect("write mp3");

        let mp4_path = att_dir.join("demo.mp4");
        let mp4_bytes = b"\x00\x00\x00\x18ftypmp42\x00\x00\x00\x00isomvideo";
        fs::write(&mp4_path, mp4_bytes).expect("write mp4");

        let outside_pdf = outside_dir.join("secret.pdf");
        fs::write(&outside_pdf, &pdf_bytes).expect("write outside pdf");
        #[cfg(unix)]
        {
            let _ = std::os::unix::fs::symlink(&outside_pdf, att_dir.join("symlink_secret.pdf"));
        }

        let prompt = format!(
            "<ATTACHED_CONTEXT>\n\
             Attached PDF file: `{}`\n\
             Attached audio file: `{}`\n\
             Attached video file: `{}`\n\
             Attached PDF file: `{}`\n\
             Attached PDF file: `{}`\n\
             Attached PDF file: `{}`\n\
             </ATTACHED_CONTEXT>",
            pdf_path.display(),
            mp3_path.display(),
            mp4_path.display(),
            outside_pdf.display(),
            att_dir.join("../outside/secret.pdf").display(),
            att_dir.join("symlink_secret.pdf").display(),
        );

        let agy = build_agy_prompt_blocks_async(prompt.clone(), Some(att_dir.clone())).await;
        assert_eq!(agy.len(), 4);
        let agy_text = agy[0]["text"].as_str().expect("agy text");
        assert!(agy_text.contains("18 pages"));
        assert!(agy_text.contains("exceeds 10 pages"));
        assert!(!agy.iter().any(|b| b["name"] == "secret.pdf"));

        assert_eq!(agy[1]["type"], "resource_link");
        assert_eq!(agy[1]["mimeType"], "application/pdf");
        assert!(
            agy[1]["uri"]
                .as_str()
                .expect("uri")
                .contains("design%20doc.pdf")
        );

        assert_eq!(agy[2]["type"], "audio");
        assert_eq!(agy[2]["mimeType"], "audio/mpeg");
        assert_eq!(agy[2]["data"], B64.encode(mp3_bytes));

        assert_eq!(agy[3]["type"], "resource_link");
        assert_eq!(agy[3]["mimeType"], "video/mp4");

        // Generic ACP without `promptCapabilities.audio`: audio degrades cleanly to `resource_link`
        let generic_acp = build_acp_prompt_blocks_async(
            prompt.clone(),
            Some(att_dir.clone()),
            AcpPromptMediaCaps::default(),
        )
        .await;
        assert_eq!(generic_acp.len(), 4);
        assert_eq!(generic_acp[2]["type"], "resource_link");
        assert_eq!(generic_acp[2]["mimeType"], "audio/mpeg");

        // Codex async builder
        let codex = build_codex_input_items_async(prompt, Some(att_dir)).await;
        assert_eq!(codex.len(), 1);
        let codex_text = codex[0]["text"].as_str().expect("codex text");
        assert!(codex_text.contains("18 pages"));
        assert!(codex_text.contains("audio/mpeg"));
        assert!(codex_text.contains("video/mp4"));
        assert!(!codex_text.contains("[PDF: secret.pdf"));

        let _ = fs::remove_dir_all(&root);
    }
}
