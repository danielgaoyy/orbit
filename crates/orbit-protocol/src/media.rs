//! Pure-Rust multimodal media sniffing shared across `orbit-providers`, `orbit-daemon`,
//! and `orbit-client-core`.
//!
//! All classification is anchored to magic header bytes (never trusting extensions alone).

use std::path::Path;

const SNIFF_HEADER_BYTES: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttachmentModality {
    Image,
    Pdf,
    Audio,
    Video,
}

impl AttachmentModality {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Image => "image",
            Self::Pdf => "pdf",
            Self::Audio => "audio",
            Self::Video => "video",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SniffedMedia {
    pub modality: AttachmentModality,
    pub mime_type: &'static str,
}

/// Sniff LLM-supported image MIME type strictly from magic header bytes (PNG, JPEG, GIF, WebP).
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

/// Sniff viewer-displayable raster image MIME types strictly from magic header bytes
/// (PNG, JPEG, GIF, WebP, BMP, ICO).
pub fn sniff_viewer_image_mime(bytes: &[u8]) -> Option<&'static str> {
    if let Some(mime) = sniff_supported_image_mime(bytes) {
        return Some(mime);
    }
    if bytes.len() >= 14 && &bytes[0..2] == b"BM" {
        let declared_size = u32::from_le_bytes([bytes[2], bytes[3], bytes[4], bytes[5]]);
        let pixel_offset = u32::from_le_bytes([bytes[10], bytes[11], bytes[12], bytes[13]]);
        if declared_size >= 14 && pixel_offset >= 14 && pixel_offset <= declared_size {
            return Some("image/bmp");
        }
    }
    if bytes.len() >= 6 && bytes[0..4] == [0x00, 0x00, 0x01, 0x00] {
        let count = u16::from_le_bytes([bytes[4], bytes[5]]);
        if count > 0 {
            return Some("image/x-icon");
        }
    }
    None
}

fn extension_lower_from_hint(hint: Option<&str>) -> Option<String> {
    let raw = hint?;
    Path::new(raw)
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_ascii_lowercase())
}

fn contains_subslice(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty()
        && needle.len() <= haystack.len()
        && haystack.windows(needle.len()).any(|w| w == needle)
}

#[derive(Debug, Clone, Copy)]
struct MpegHeaderInfo {
    version_bits: u8,
    layer_bits: u8,
    frame_len: usize,
}

fn parse_mpeg_audio_header(bytes: &[u8]) -> Option<MpegHeaderInfo> {
    if bytes.len() < 3 || bytes[0] != 0xFF || (bytes[1] & 0xE0) != 0xE0 {
        return None;
    }
    // Explicitly reject UTF-16LE BOM (`0xFF 0xFE`), which otherwise collides with MPEG-1 Layer I
    if bytes[1] == 0xFE {
        return None;
    }
    let version_bits = (bytes[1] >> 3) & 0x03;
    let layer_bits = (bytes[1] >> 1) & 0x03;
    let bitrate_idx = ((bytes[2] >> 4) & 0x0F) as usize;
    let sample_rate_idx = ((bytes[2] >> 2) & 0x03) as usize;
    let padding = ((bytes[2] >> 1) & 0x01) as usize;

    // Version 01 reserved; Layer 00 reserved (ADTS AAC handled separately);
    // Bitrate 0 (free) or 15 (invalid); Sample rate 3 reserved.
    if version_bits == 0x01
        || layer_bits == 0x00
        || bitrate_idx == 0
        || bitrate_idx == 15
        || sample_rate_idx == 3
    {
        return None;
    }

    const MPEG1_L1_KBPS: [usize; 16] = [
        0, 32, 64, 96, 128, 160, 192, 224, 256, 288, 320, 352, 384, 416, 448, 0,
    ];
    const MPEG1_L2_KBPS: [usize; 16] = [
        0, 32, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 384, 0,
    ];
    const MPEG1_L3_KBPS: [usize; 16] = [
        0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 0,
    ];
    const MPEG2_L1_KBPS: [usize; 16] = [
        0, 32, 48, 56, 64, 80, 96, 112, 128, 144, 160, 176, 192, 224, 256, 0,
    ];
    const MPEG2_L23_KBPS: [usize; 16] = [
        0, 8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160, 0,
    ];

    let is_mpeg1 = version_bits == 0x03;
    let kbps = match (is_mpeg1, layer_bits) {
        (true, 0x03) => MPEG1_L1_KBPS[bitrate_idx],
        (true, 0x02) => MPEG1_L2_KBPS[bitrate_idx],
        (true, 0x01) => MPEG1_L3_KBPS[bitrate_idx],
        (false, 0x03) => MPEG2_L1_KBPS[bitrate_idx],
        (false, 0x02 | 0x01) => MPEG2_L23_KBPS[bitrate_idx],
        _ => 0,
    };
    if kbps == 0 {
        return None;
    }

    let sample_rate = match version_bits {
        0x03 => [44_100usize, 48_000, 32_000][sample_rate_idx],
        0x02 => [22_050usize, 24_000, 16_000][sample_rate_idx],
        0x00 => [11_025usize, 12_000, 8_000][sample_rate_idx],
        _ => return None,
    };

    let bitrate_bps = kbps * 1000;
    let frame_len = match layer_bits {
        0x03 => ((12 * bitrate_bps) / sample_rate + padding) * 4,
        0x02 => (144 * bitrate_bps) / sample_rate + padding,
        0x01 => {
            let coeff = if is_mpeg1 { 144 } else { 72 };
            (coeff * bitrate_bps) / sample_rate + padding
        }
        _ => return None,
    };
    if frame_len < 4 {
        return None;
    }

    Some(MpegHeaderInfo {
        version_bits,
        layer_bits,
        frame_len,
    })
}

fn is_valid_raw_mpeg_audio(bytes: &[u8], ext_hint: Option<&str>) -> bool {
    let Some(first) = parse_mpeg_audio_header(bytes) else {
        return false;
    };
    if first.frame_len + 3 <= bytes.len() {
        if let Some(second) = parse_mpeg_audio_header(&bytes[first.frame_len..]) {
            return second.version_bits == first.version_bits
                && second.layer_bits == first.layer_bits;
        }
        return false;
    }
    // Short single-frame/truncated probe: require explicit MPEG audio extension hint
    matches!(ext_hint, Some("mp3" | "mp2" | "mp1"))
}

fn parse_adts_frame_len(bytes: &[u8]) -> Option<usize> {
    if bytes.len() < 7 || bytes[0] != 0xFF || (bytes[1] & 0xF6) != 0xF0 {
        return None;
    }
    let sample_freq_idx = (bytes[2] >> 2) & 0x0F;
    if sample_freq_idx >= 13 {
        return None;
    }
    let frame_len = (((bytes[3] & 0x03) as usize) << 11)
        | ((bytes[4] as usize) << 3)
        | (((bytes[5] >> 5) & 0x07) as usize);
    (frame_len >= 7).then_some(frame_len)
}

fn is_valid_raw_adts_aac(bytes: &[u8], ext_hint: Option<&str>) -> bool {
    let Some(frame_len) = parse_adts_frame_len(bytes) else {
        return false;
    };
    if frame_len + 7 <= bytes.len() {
        return parse_adts_frame_len(&bytes[frame_len..]).is_some();
    }
    ext_hint == Some("aac")
}

fn is_iso_audio_brand(brand: &[u8]) -> bool {
    matches!(
        brand,
        b"M4A " | b"M4B " | b"M4P " | b"mp4a" | b"aac " | b"alac" | b"F4A "
    )
}

fn is_iso_video_brand(brand: &[u8]) -> bool {
    matches!(
        brand,
        b"mp41"
            | b"mp42"
            | b"avc1"
            | b"hvc1"
            | b"hev1"
            | b"M4V "
            | b"m4v "
            | b"mp71"
            | b"dash"
            | b"msnv"
            | b"3gp4"
            | b"3gp5"
            | b"3g2a"
            | b"F4V "
    )
}

fn is_iso_generic_brand(brand: &[u8]) -> bool {
    matches!(
        brand,
        b"isom" | b"iso2" | b"iso3" | b"iso4" | b"iso5" | b"iso6"
    )
}

fn sniff_iso_bmff(bytes: &[u8], ext_hint: Option<&str>) -> Option<SniffedMedia> {
    if bytes.len() < 12 || &bytes[4..8] != b"ftyp" {
        return None;
    }
    let major = &bytes[8..12];
    if is_iso_audio_brand(major) {
        return Some(SniffedMedia {
            modality: AttachmentModality::Audio,
            mime_type: "audio/mp4",
        });
    }
    if major == b"qt  " {
        return Some(SniffedMedia {
            modality: AttachmentModality::Video,
            mime_type: "video/quicktime",
        });
    }
    if is_iso_video_brand(major) {
        return Some(SniffedMedia {
            modality: AttachmentModality::Video,
            mime_type: "video/mp4",
        });
    }

    let declared_size = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
    let box_end = if declared_size >= 16 {
        declared_size.min(bytes.len()).min(128)
    } else {
        bytes.len().min(64)
    };
    let mut has_audio_compat = false;
    let mut has_video_compat = false;
    let mut has_qt_compat = false;
    if box_end >= 20 {
        for chunk in bytes[16..box_end].as_chunks::<4>().0 {
            if is_iso_audio_brand(chunk) {
                has_audio_compat = true;
            } else if chunk == b"qt  " {
                has_qt_compat = true;
            } else if is_iso_video_brand(chunk) {
                has_video_compat = true;
            }
        }
    }

    if !is_iso_generic_brand(major) && !has_audio_compat && !has_video_compat && !has_qt_compat {
        return None;
    }

    if has_audio_compat && !has_video_compat {
        return Some(SniffedMedia {
            modality: AttachmentModality::Audio,
            mime_type: "audio/mp4",
        });
    }
    if has_qt_compat && !has_video_compat {
        return Some(SniffedMedia {
            modality: AttachmentModality::Video,
            mime_type: "video/quicktime",
        });
    }
    if has_video_compat {
        return Some(SniffedMedia {
            modality: AttachmentModality::Video,
            mime_type: "video/mp4",
        });
    }

    match ext_hint {
        Some("m4a" | "aac") => Some(SniffedMedia {
            modality: AttachmentModality::Audio,
            mime_type: "audio/mp4",
        }),
        Some("mov") => Some(SniffedMedia {
            modality: AttachmentModality::Video,
            mime_type: "video/quicktime",
        }),
        _ => Some(SniffedMedia {
            modality: AttachmentModality::Video,
            mime_type: "video/mp4",
        }),
    }
}

fn sniff_ebml(bytes: &[u8], ext_hint: Option<&str>) -> Option<SniffedMedia> {
    if !bytes.starts_with(&[0x1A, 0x45, 0xDF, 0xA3]) {
        return None;
    }
    let probe = &bytes[..bytes.len().min(SNIFF_HEADER_BYTES)];
    let has_video_codec = contains_subslice(probe, b"V_VP8")
        || contains_subslice(probe, b"V_VP9")
        || contains_subslice(probe, b"V_AV1")
        || contains_subslice(probe, b"V_MPEG")
        || contains_subslice(probe, b"V_MS");
    let has_audio_codec = contains_subslice(probe, b"A_OPUS")
        || contains_subslice(probe, b"A_VORBIS")
        || contains_subslice(probe, b"OpusHead")
        || contains_subslice(probe, b"A_AAC")
        || contains_subslice(probe, b"A_FLAC")
        || contains_subslice(probe, b"A_MPEG");
    let has_matroska = contains_subslice(probe, b"matroska");

    if matches!(ext_hint, Some("opus" | "weba" | "mka")) || (has_audio_codec && !has_video_codec) {
        return Some(SniffedMedia {
            modality: AttachmentModality::Audio,
            mime_type: "audio/webm",
        });
    }
    if ext_hint == Some("mkv") || (has_matroska && ext_hint != Some("webm")) {
        return Some(SniffedMedia {
            modality: AttachmentModality::Video,
            mime_type: "video/x-matroska",
        });
    }
    Some(SniffedMedia {
        modality: AttachmentModality::Video,
        mime_type: "video/webm",
    })
}

fn sniff_media_inner(
    bytes: &[u8],
    filename_hint: Option<&str>,
    include_viewer_images: bool,
) -> Option<SniffedMedia> {
    // 1. Raster images
    let img_mime = if include_viewer_images {
        sniff_viewer_image_mime(bytes)
    } else {
        sniff_supported_image_mime(bytes)
    };
    if let Some(mime_type) = img_mime {
        return Some(SniffedMedia {
            modality: AttachmentModality::Image,
            mime_type,
        });
    }

    // 2. PDF (`%PDF-`, tolerating leading UTF-8 BOM)
    let pdf_slice = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    if pdf_slice.starts_with(b"%PDF-") {
        return Some(SniffedMedia {
            modality: AttachmentModality::Pdf,
            mime_type: "application/pdf",
        });
    }

    // 3. Unambiguous audio magic headers
    if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WAVE" {
        return Some(SniffedMedia {
            modality: AttachmentModality::Audio,
            mime_type: "audio/wav",
        });
    }
    if bytes.starts_with(b"OggS") {
        return Some(SniffedMedia {
            modality: AttachmentModality::Audio,
            mime_type: "audio/ogg",
        });
    }
    if bytes.starts_with(b"fLaC") {
        return Some(SniffedMedia {
            modality: AttachmentModality::Audio,
            mime_type: "audio/flac",
        });
    }
    if bytes.starts_with(b"ID3") {
        return Some(SniffedMedia {
            modality: AttachmentModality::Audio,
            mime_type: "audio/mpeg",
        });
    }

    let ext_hint = extension_lower_from_hint(filename_hint);

    // 4. Headerless raw MPEG audio & ADTS AAC (strictly rejects UTF-16LE BOM `0xFF 0xFE` and
    // validates second-frame sync or explicit extension hint)
    if is_valid_raw_mpeg_audio(bytes, ext_hint.as_deref()) {
        return Some(SniffedMedia {
            modality: AttachmentModality::Audio,
            mime_type: "audio/mpeg",
        });
    }
    if is_valid_raw_adts_aac(bytes, ext_hint.as_deref()) {
        return Some(SniffedMedia {
            modality: AttachmentModality::Audio,
            mime_type: "audio/aac",
        });
    }

    // 5. ISO BMFF containers (`MP4`, `MOV`, `M4A`)
    if let Some(iso) = sniff_iso_bmff(bytes, ext_hint.as_deref()) {
        return Some(iso);
    }

    // 6. EBML containers (`WebM`, `MKV`, `WebM/Opus`)
    if let Some(ebml) = sniff_ebml(bytes, ext_hint.as_deref()) {
        return Some(ebml);
    }

    None
}

/// Sniff provider-supported multimodal media strictly from magic header bytes
/// (PNG/JPEG/GIF/WebP, PDF, Audio, Video).
pub fn sniff_media_mime(bytes: &[u8], filename_hint: Option<&str>) -> Option<SniffedMedia> {
    sniff_media_inner(bytes, filename_hint, false)
}

/// Sniff viewer-supported media strictly from magic header bytes (also recognizes BMP & ICO).
pub fn sniff_media_or_viewer_image_mime(
    bytes: &[u8],
    filename_hint: Option<&str>,
) -> Option<SniffedMedia> {
    sniff_media_inner(bytes, filename_hint, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_utf16le_bom_and_validates_mpeg_and_adts_second_frame_sync() {
        // UTF-16LE text ("ABC") MUST NEVER be misclassified as audio/mpeg (even with .mp3 ext!)
        let utf16_text = [0xFF, 0xFE, 0x41, 0x00, 0x42, 0x00, 0x43, 0x00];
        assert_eq!(sniff_media_mime(&utf16_text, Some("notes.txt")), None);
        assert_eq!(sniff_media_mime(&utf16_text, Some("notes.mp3")), None);
        assert_eq!(
            sniff_media_or_viewer_image_mime(&utf16_text, Some("readme.txt")),
            None
        );

        // Two consecutive MPEG-1 Layer III 128 kbps 44.1 kHz frames (frame_len = 417 bytes)
        let mut two_frames = vec![0u8; 417 * 2];
        two_frames[0..4].copy_from_slice(&[0xFF, 0xFB, 0x90, 0x64]);
        two_frames[417..421].copy_from_slice(&[0xFF, 0xFB, 0x90, 0x64]);
        assert_eq!(
            sniff_media_mime(&two_frames, Some("stream.bin")),
            Some(SniffedMedia {
                modality: AttachmentModality::Audio,
                mime_type: "audio/mpeg",
            })
        );

        // Single MPEG header followed by garbage at computed frame boundary is rejected even with .mp3
        let mut broken_frames = vec![0u8; 500];
        broken_frames[0..4].copy_from_slice(&[0xFF, 0xFB, 0x90, 0x64]);
        assert_eq!(sniff_media_mime(&broken_frames, Some("corrupt.mp3")), None);

        // Short header-only probe requires explicit .mp3/.mp2/.mp1 hint
        assert_eq!(
            sniff_media_mime(&[0xFF, 0xFB, 0x90, 0x64], Some("blob.bin")),
            None
        );
        assert_eq!(
            sniff_media_mime(&[0xFF, 0xFB, 0x90, 0x64], Some("clip.mp3")),
            Some(SniffedMedia {
                modality: AttachmentModality::Audio,
                mime_type: "audio/mpeg",
            })
        );

        // Valid 2-frame ADTS AAC (frame_len = 16 bytes)
        let mut adts = vec![0u8; 32];
        let hdr = [0xFF, 0xF1, 0x50, 0x80, 0x02, 0x00, 0xFC]; // frame_len = (0x02 << 3) = 16
        adts[0..7].copy_from_slice(&hdr);
        adts[16..23].copy_from_slice(&hdr);
        assert_eq!(
            sniff_media_mime(&adts, Some("audio.bin")),
            Some(SniffedMedia {
                modality: AttachmentModality::Audio,
                mime_type: "audio/aac",
            })
        );
    }

    #[test]
    fn viewer_image_sniffer_validates_bmp_and_ico_magic_bytes() {
        let mut bmp = vec![0u8; 20];
        bmp[0..2].copy_from_slice(b"BM");
        bmp[2..6].copy_from_slice(&20u32.to_le_bytes());
        bmp[10..14].copy_from_slice(&14u32.to_le_bytes());
        assert_eq!(
            sniff_media_or_viewer_image_mime(&bmp, Some("img.bmp")),
            Some(SniffedMedia {
                modality: AttachmentModality::Image,
                mime_type: "image/bmp",
            })
        );
        // Provider sniffer still excludes BMP/ICO
        assert_eq!(sniff_media_mime(&bmp, Some("img.bmp")), None);

        let ico = [0x00, 0x00, 0x01, 0x00, 0x01, 0x00, 0x10, 0x10];
        assert_eq!(
            sniff_media_or_viewer_image_mime(&ico, Some("favicon.ico")),
            Some(SniffedMedia {
                modality: AttachmentModality::Image,
                mime_type: "image/x-icon",
            })
        );
        // Spoofed PNG text rejected
        assert_eq!(
            sniff_media_or_viewer_image_mime(b"plain text", Some("fake.png")),
            None
        );
    }
}
