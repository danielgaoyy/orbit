//! Pure-Rust monochrome SVG QR code renderer for mobile pairing URIs.
//!
//! Emits minimal, deterministic `<svg>` geometry with a 4-module quiet zone and zero
//! scripts, external references, or unsafe attributes.

use std::fmt::Write as _;

use qrcode::{Color, EcLevel, QrCode};

pub const MAX_QR_CONTENT_BYTES: usize = 2048;
const QUIET_ZONE_MODULES: usize = 4;

/// Render `content` as a crisp monochrome SVG QR code with a 4-module quiet zone.
pub fn render_qr_svg_string(content: &str) -> Result<String, String> {
    if content.trim().is_empty() {
        return Err("QR content must not be empty".to_string());
    }
    if content.len() > MAX_QR_CONTENT_BYTES {
        return Err(format!(
            "QR content exceeds maximum length of {MAX_QR_CONTENT_BYTES} bytes"
        ));
    }

    let code = QrCode::with_error_correction_level(content.as_bytes(), EcLevel::M)
        .or_else(|_| QrCode::with_error_correction_level(content.as_bytes(), EcLevel::L))
        .map_err(|e| format!("failed to encode QR code: {e}"))?;

    let width = code.width();
    let total = width + QUIET_ZONE_MODULES * 2;

    let mut path_data = String::with_capacity(width * width * 2);
    for y in 0..width {
        let mut x = 0;
        while x < width {
            if code[(x, y)] == Color::Dark {
                let run_start = x;
                while x < width && code[(x, y)] == Color::Dark {
                    x += 1;
                }
                let run_len = x - run_start;
                let px = run_start + QUIET_ZONE_MODULES;
                let py = y + QUIET_ZONE_MODULES;
                let _ = write!(path_data, "M{px} {py}h{run_len}v1h-{run_len}z");
            } else {
                x += 1;
            }
        }
    }

    let mut svg = String::with_capacity(path_data.len() + 256);
    let _ = write!(
        svg,
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {total} {total}" shape-rendering="crispEdges" role="img" aria-label="QR code"><rect width="{total}" height="{total}" fill="#FFFFFF"/><path fill="#000000" d="{path_data}"/></svg>"##
    );
    Ok(svg)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_clean_monochrome_svg_for_pairing_uri() {
        let uri = "orbit://pair?v=1&host=devbox&pk=AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=&relay=wss%3A%2F%2Frelay.example.com";
        let svg = render_qr_svg_string(uri).expect("QR SVG should render");

        assert!(svg.starts_with("<svg "));
        assert!(svg.ends_with("</svg>"));
        assert!(svg.contains("viewBox=\"0 0 "));
        assert!(svg.contains("shape-rendering=\"crispEdges\""));
        assert!(svg.contains("<rect width=\""));
        assert!(svg.contains("fill=\"#FFFFFF\""));
        assert!(svg.contains("<path fill=\"#000000\" d=\"M"));

        let lower = svg.to_ascii_lowercase();
        assert!(!lower.contains("<script"));
        assert!(!lower.contains("onload"));
        assert!(!lower.contains("onerror"));
        assert!(!lower.contains("javascript:"));
    }

    #[test]
    fn rejects_empty_and_oversized_inputs() {
        assert!(render_qr_svg_string("").is_err());
        assert!(render_qr_svg_string("   \n\t  ").is_err());

        let too_large = "a".repeat(MAX_QR_CONTENT_BYTES + 1);
        assert!(render_qr_svg_string(&too_large).is_err());

        let at_limit = "a".repeat(MAX_QR_CONTENT_BYTES);
        let ok_svg = render_qr_svg_string(&at_limit).expect("2048 bytes should encode");
        assert!(ok_svg.starts_with("<svg "));
        assert!(ok_svg.ends_with("</svg>"));
    }
}
