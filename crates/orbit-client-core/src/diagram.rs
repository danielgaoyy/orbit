//! Pure-Rust native diagram rendering pipeline:
//! - Mermaid (23 diagram types via `mermaid-rs-renderer`)
//! - Graphviz DOT (`layout-rs`)
//! - Sanitized inline SVG (`svg`)
//!
//! Includes Orbit Light/Dark themes with CJK system font stacks, deterministic SVG
//! ID namespacing (preventing `<defs>` collisions across inline SVGs), and a bounded
//! in-memory LRU cache shared across Desktop, iOS, Android, and HarmonyOS clients.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Mutex, OnceLock};

use layout::backends::svg::SVGWriter;
use layout::gv::{DotParser, GraphBuilder};
use mermaid_rs_renderer::{
    DiagramKind as MmdKind, LayoutConfig, Theme as MmdTheme, compute_layout,
    measure_svg_dimensions, parse_mermaid, parse_mermaid_strict, render_svg,
};
use serde::{Deserialize, Serialize};

const MAX_DIAGRAM_SOURCE_BYTES: usize = 512 * 1024;
const DIAGRAM_CACHE_CAPACITY: usize = 128;

const ORBIT_DIAGRAM_FONT_FAMILY: &str = "-apple-system, BlinkMacSystemFont, 'SF Pro Text', 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', ui-sans-serif, system-ui, sans-serif";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagramKind {
    Mermaid,
    Svg,
    Graphviz,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagramTheme {
    Light,
    Dark,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenderDiagramRequest {
    pub kind: DiagramKind,
    pub source: String,
    pub theme: DiagramTheme,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RenderDiagramView {
    pub kind: DiagramKind,
    pub theme: DiagramTheme,
    pub svg: Option<String>,
    pub width: f32,
    pub height: f32,
    pub error: Option<String>,
    pub diagram_type_label: Option<String>,
}

/// Detect whether a fenced code block language/source represents a native diagram.
pub fn detect_diagram_kind(language: &str, source: &str) -> Option<DiagramKind> {
    let lang = language.trim().to_ascii_lowercase();
    match lang.as_str() {
        "mermaid" | "mmd" => Some(DiagramKind::Mermaid),
        "svg" => Some(DiagramKind::Svg),
        "dot" | "graphviz" => Some(DiagramKind::Graphviz),
        "xml" | "html" if looks_like_standalone_svg(source) => Some(DiagramKind::Svg),
        _ => None,
    }
}

fn looks_like_standalone_svg(source: &str) -> bool {
    let mut rest = source.trim_start_matches('\u{feff}').trim_start();
    loop {
        if rest.starts_with("<?") {
            let Some(end) = rest.find("?>") else {
                return false;
            };
            rest = rest[end + 2..].trim_start();
        } else if rest.starts_with("<!--") {
            let Some(end) = rest.find("-->") else {
                return false;
            };
            rest = rest[end + 3..].trim_start();
        } else if starts_with_ascii_ci(rest, "<!doctype") {
            let Some(end) = rest.find('>') else {
                return false;
            };
            rest = rest[end + 1..].trim_start();
        } else {
            break;
        }
    }
    if !starts_with_ascii_ci(rest, "<svg") {
        return false;
    }
    matches!(
        rest.as_bytes().get(4).copied(),
        Some(b'>') | Some(b' ') | Some(b'\n') | Some(b'\r') | Some(b'\t')
    )
}

#[derive(Hash, PartialEq, Eq, Clone)]
struct DiagramCacheKey {
    kind: DiagramKind,
    theme: DiagramTheme,
    source: String,
}

struct DiagramLruCache {
    capacity: usize,
    entries: HashMap<DiagramCacheKey, RenderDiagramView>,
    order: VecDeque<DiagramCacheKey>,
}

impl DiagramLruCache {
    fn new(capacity: usize) -> Self {
        Self {
            capacity,
            entries: HashMap::with_capacity(capacity),
            order: VecDeque::with_capacity(capacity),
        }
    }

    fn get(&mut self, key: &DiagramCacheKey) -> Option<RenderDiagramView> {
        let value = self.entries.get(key)?.clone();
        if let Some(pos) = self.order.iter().position(|k| k == key)
            && pos + 1 < self.order.len()
        {
            self.order.remove(pos);
            self.order.push_back(key.clone());
        }
        Some(value)
    }

    fn insert(&mut self, key: DiagramCacheKey, value: RenderDiagramView) {
        if self.entries.contains_key(&key) {
            if let Some(pos) = self.order.iter().position(|k| k == &key) {
                self.order.remove(pos);
            }
        } else if self.entries.len() >= self.capacity
            && let Some(evicted) = self.order.pop_front()
        {
            self.entries.remove(&evicted);
        }
        self.order.push_back(key.clone());
        self.entries.insert(key, value);
    }
}

fn global_diagram_cache() -> &'static Mutex<DiagramLruCache> {
    static CACHE: OnceLock<Mutex<DiagramLruCache>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(DiagramLruCache::new(DIAGRAM_CACHE_CAPACITY)))
}

/// Render a diagram (`Mermaid`, `Graphviz`, or sanitized `Svg`) to theme-aware vector SVG.
pub fn render_diagram(req: &RenderDiagramRequest) -> RenderDiagramView {
    if req.source.len() > MAX_DIAGRAM_SOURCE_BYTES {
        return error_view(
            req.kind,
            req.theme,
            "Diagram source exceeds 512 KB size limit".into(),
        );
    }
    if req.source.trim().is_empty() {
        return error_view(req.kind, req.theme, "Empty diagram source".into());
    }

    let key = DiagramCacheKey {
        kind: req.kind,
        theme: req.theme,
        source: req.source.clone(),
    };
    if let Ok(mut guard) = global_diagram_cache().lock()
        && let Some(hit) = guard.get(&key)
    {
        return hit;
    }

    let rendered = match req.kind {
        DiagramKind::Mermaid => render_mermaid_svg(&req.source, req.theme),
        DiagramKind::Graphviz => render_graphviz_svg(&req.source, req.theme),
        DiagramKind::Svg => render_raw_svg(&req.source, req.theme),
    };

    if let Ok(mut guard) = global_diagram_cache().lock() {
        guard.insert(key, rendered.clone());
    }
    rendered
}

fn error_view(kind: DiagramKind, theme: DiagramTheme, message: String) -> RenderDiagramView {
    RenderDiagramView {
        kind,
        theme,
        svg: None,
        width: 0.0,
        height: 0.0,
        error: Some(message),
        diagram_type_label: None,
    }
}

fn orbit_mermaid_theme(theme: DiagramTheme) -> MmdTheme {
    match theme {
        DiagramTheme::Light => {
            let mut t = MmdTheme::modern();
            t.font_family = ORBIT_DIAGRAM_FONT_FAMILY.to_string();
            t.background = "#FFFFFF".to_string();
            t.primary_color = "#FFFFFF".to_string();
            t.primary_border_color = "#D4D4D8".to_string();
            t.primary_text_color = "#18181B".to_string();
            t.text_color = "#18181B".to_string();
            t.line_color = "#71717A".to_string();
            t.secondary_color = "#F4F4F5".to_string();
            t.tertiary_color = "#FAFAFA".to_string();
            t.cluster_background = "#F4F4F5".to_string();
            t.cluster_border = "#E4E4E7".to_string();
            t.edge_label_background = "#FFFFFF".to_string();
            t.sequence_actor_fill = "#FFFFFF".to_string();
            t.sequence_actor_border = "#D4D4D8".to_string();
            t.sequence_actor_line = "#A1A1AA".to_string();
            t.sequence_note_fill = "#FFF7ED".to_string();
            t.sequence_note_border = "#FDBA74".to_string();
            t.sequence_activation_fill = "#F4F4F5".to_string();
            t.sequence_activation_border = "#A1A1AA".to_string();
            t
        }
        DiagramTheme::Dark => {
            let mut t = MmdTheme::dark();
            t.font_family = ORBIT_DIAGRAM_FONT_FAMILY.to_string();
            t.font_size = 14.0;
            t.background = "#141519".to_string();
            t.primary_color = "#1E2028".to_string();
            t.primary_border_color = "#3F3F46".to_string();
            t.primary_text_color = "#F4F4F5".to_string();
            t.text_color = "#F4F4F5".to_string();
            t.line_color = "#A1A1AA".to_string();
            t.secondary_color = "#272A34".to_string();
            t.tertiary_color = "#181A20".to_string();
            t.cluster_background = "#181A20".to_string();
            t.cluster_border = "#2E313B".to_string();
            t.edge_label_background = "#141519".to_string();
            t.sequence_actor_fill = "#1E2028".to_string();
            t.sequence_actor_border = "#3F3F46".to_string();
            t.sequence_actor_line = "#71717A".to_string();
            t.sequence_note_fill = "#2B2219".to_string();
            t.sequence_note_border = "#9A5B25".to_string();
            t.sequence_activation_fill = "#272A34".to_string();
            t.sequence_activation_border = "#52525B".to_string();
            t.pie_title_text_color = "#F4F4F5".to_string();
            t.pie_section_text_color = "#F4F4F5".to_string();
            t.pie_legend_text_color = "#D4D4D8".to_string();
            t.pie_stroke_color = "#141519".to_string();
            t.pie_outer_stroke_color = "#3F3F46".to_string();
            t
        }
    }
}

fn mermaid_kind_label(kind: MmdKind) -> &'static str {
    match kind {
        MmdKind::Flowchart => "Flowchart",
        MmdKind::Class => "Class",
        MmdKind::State => "State",
        MmdKind::Sequence => "Sequence",
        MmdKind::Er => "ER Diagram",
        MmdKind::Pie => "Pie Chart",
        MmdKind::Mindmap => "Mindmap",
        MmdKind::Journey => "Journey",
        MmdKind::Timeline => "Timeline",
        MmdKind::Gantt => "Gantt",
        MmdKind::Requirement => "Requirement",
        MmdKind::GitGraph => "Git Graph",
        MmdKind::C4 => "C4 Diagram",
        MmdKind::Sankey => "Sankey",
        MmdKind::Quadrant => "Quadrant",
        MmdKind::ZenUML => "ZenUML",
        MmdKind::Block => "Block",
        MmdKind::Packet => "Packet",
        MmdKind::Kanban => "Kanban",
        MmdKind::Architecture => "Architecture",
        MmdKind::Radar => "Radar",
        MmdKind::Treemap => "Treemap",
        MmdKind::XYChart => "XY Chart",
    }
}

fn render_mermaid_svg(source: &str, theme_mode: DiagramTheme) -> RenderDiagramView {
    let mmd_theme = orbit_mermaid_theme(theme_mode);
    let config = LayoutConfig::default();

    let parsed = match parse_mermaid_strict(source) {
        Ok(p) => p,
        Err(strict_err) => match parse_mermaid(source) {
            Ok(p) => p,
            Err(_) => {
                return error_view(DiagramKind::Mermaid, theme_mode, strict_err.to_string());
            }
        },
    };

    let label = mermaid_kind_label(parsed.graph.kind).to_string();
    let layout = compute_layout(&parsed.graph, &mmd_theme, &config);
    let dims = measure_svg_dimensions(&layout, &config, None);
    let raw_svg = render_svg(&layout, &mmd_theme, &config);

    // Strip the outer canvas background fill so the diagram inherits the Orbit card surface
    // while keeping internal shape/label fills opaque.
    let transparent_canvas_svg = make_mermaid_canvas_transparent(&raw_svg, &mmd_theme.background);
    let namespaced = namespace_svg_ids(
        &transparent_canvas_svg,
        DiagramKind::Mermaid,
        theme_mode,
        source,
    );

    RenderDiagramView {
        kind: DiagramKind::Mermaid,
        theme: theme_mode,
        svg: Some(namespaced),
        width: dims.viewbox_width.max(1.0),
        height: dims.viewbox_height.max(1.0),
        error: None,
        diagram_type_label: Some(label),
    }
}

fn make_mermaid_canvas_transparent(svg: &str, bg_hex: &str) -> String {
    let needle = format!("fill=\"{bg_hex}\"/>");
    if let Some(defs_pos) = svg.find("<defs>")
        && let Some(rect_pos) = svg[..defs_pos].find("<rect ")
        && let Some(fill_rel) = svg[rect_pos..defs_pos].find(&needle)
    {
        let start = rect_pos + fill_rel;
        let end = start + needle.len();
        let mut out = String::with_capacity(svg.len());
        out.push_str(&svg[..start]);
        out.push_str("fill=\"none\"/>");
        out.push_str(&svg[end..]);
        return out;
    }
    svg.to_string()
}

fn render_graphviz_svg(source: &str, theme_mode: DiagramTheme) -> RenderDiagramView {
    let mut parser = DotParser::new(source);
    let graph = match parser.process() {
        Ok(g) => g,
        Err(err) => {
            return error_view(DiagramKind::Graphviz, theme_mode, err);
        }
    };

    let mut builder = GraphBuilder::new();
    builder.visit_graph(&graph);
    let mut visual = builder.get();
    if visual.num_nodes() == 0 {
        return error_view(
            DiagramKind::Graphviz,
            theme_mode,
            "Graphviz graph contains no nodes".into(),
        );
    }

    let mut writer = SVGWriter::new();
    visual.do_it(false, false, false, &mut writer);
    let raw = writer.finalize();

    let cleaned = strip_xml_and_doctype(&raw);
    let font_styled = cleaned.replace(
        "font-family: Times, serif;",
        &format!("font-family: {ORBIT_DIAGRAM_FONT_FAMILY};"),
    );

    let themed = match theme_mode {
        DiagramTheme::Light => font_styled
            .replace("fill=\"#000000ff\"", "fill=\"#18181B\"")
            .replace("fill=\"#000000\"", "fill=\"#18181B\"")
            .replace("fill=\"black\"", "fill=\"#18181B\"")
            .replace("stroke=\"#000000ff\"", "stroke=\"#52525B\"")
            .replace("stroke=\"#000000\"", "stroke=\"#52525B\"")
            .replace("stroke=\"black\"", "stroke=\"#52525B\"")
            .replace("fill=\"#ffffffff\"", "fill=\"#FFFFFF\"")
            .replace("fill=\"#ffffff\"", "fill=\"#FFFFFF\"")
            .replace("fill=\"white\"", "fill=\"#FFFFFF\""),
        DiagramTheme::Dark => font_styled
            .replace("fill=\"#000000ff\"", "fill=\"#F4F4F5\"")
            .replace("fill=\"#000000\"", "fill=\"#F4F4F5\"")
            .replace("fill=\"black\"", "fill=\"#F4F4F5\"")
            .replace("stroke=\"#000000ff\"", "stroke=\"#A1A1AA\"")
            .replace("stroke=\"#000000\"", "stroke=\"#A1A1AA\"")
            .replace("stroke=\"black\"", "stroke=\"#A1A1AA\"")
            .replace("fill=\"#ffffffff\"", "fill=\"#1E2028\"")
            .replace("fill=\"#ffffff\"", "fill=\"#1E2028\"")
            .replace("fill=\"white\"", "fill=\"#1E2028\""),
    };

    let (width, height) = extract_svg_dimensions(&themed).unwrap_or((360.0, 240.0));
    let namespaced = namespace_svg_ids(&themed, DiagramKind::Graphviz, theme_mode, source);

    RenderDiagramView {
        kind: DiagramKind::Graphviz,
        theme: theme_mode,
        svg: Some(namespaced),
        width: width.max(1.0),
        height: height.max(1.0),
        error: None,
        diagram_type_label: Some("Graphviz".into()),
    }
}

fn render_raw_svg(source: &str, theme_mode: DiagramTheme) -> RenderDiagramView {
    match sanitize_svg(source) {
        Ok((sanitized, width, height)) => {
            let namespaced = namespace_svg_ids(&sanitized, DiagramKind::Svg, theme_mode, source);
            RenderDiagramView {
                kind: DiagramKind::Svg,
                theme: theme_mode,
                svg: Some(namespaced),
                width: width.max(1.0),
                height: height.max(1.0),
                error: None,
                diagram_type_label: Some("SVG".into()),
            }
        }
        Err(err) => error_view(DiagramKind::Svg, theme_mode, err),
    }
}

fn sanitize_svg(raw: &str) -> Result<(String, f32, f32), String> {
    let pre = strip_xml_and_doctype(raw);
    let Some(svg_start) = find_ascii_ci(&pre, "<svg") else {
        return Err("Missing <svg> root element".into());
    };
    let slice = &pre[svg_start..];
    let svg_end = find_last_ascii_ci(slice, "</svg>")
        .map(|pos| pos + "</svg>".len())
        .ok_or_else(|| "Unclosed <svg> root element".to_string())?;
    let mut work = slice[..svg_end].to_string();

    for tag in [
        "script",
        "foreignobject",
        "iframe",
        "object",
        "embed",
        "applet",
        "noscript",
        "template",
        "form",
        "textarea",
        "select",
    ] {
        work = strip_paired_tag_ci(&work, tag);
    }
    for tag in ["meta", "link", "base", "input", "button", "script"] {
        work = strip_self_or_open_tag_ci(&work, tag);
    }

    work = strip_event_handler_attrs(&work);
    work = strip_unsafe_uri_attrs(&work);
    work = sanitize_style_constructs(&work);
    work = ensure_svg_xmlns(&work);

    let (width, height) = extract_svg_dimensions(&work).unwrap_or((480.0, 320.0));
    work = ensure_svg_viewbox(&work, width, height);

    Ok((work, width, height))
}

fn strip_xml_and_doctype(raw: &str) -> String {
    let mut s = raw.trim_start_matches('\u{feff}').to_string();
    loop {
        let trimmed = s.trim_start();
        if trimmed.starts_with("<?xml")
            && let Some(end) = trimmed.find("?>")
        {
            s = trimmed[end + 2..].to_string();
            continue;
        }
        if starts_with_ascii_ci(trimmed, "<!doctype")
            && let Some(end) = find_doctype_end(trimmed)
        {
            s = trimmed[end..].to_string();
            continue;
        }
        return trimmed.to_string();
    }
}

fn find_doctype_end(s: &str) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut bracket_depth = 0i32;
    let mut i = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b'[' => bracket_depth += 1,
            b']' => bracket_depth = (bracket_depth - 1).max(0),
            b'>' if bracket_depth == 0 => return Some(i + 1),
            _ => {}
        }
        i += 1;
    }
    None
}

fn strip_paired_tag_ci(input: &str, tag: &str) -> String {
    let open_pat = format!("<{tag}");
    let close_pat = format!("</{tag}");
    let mut out = String::with_capacity(input.len());
    let mut cursor = 0usize;

    while let Some(rel_start) = find_ascii_ci(&input[cursor..], &open_pat) {
        let start = cursor + rel_start;
        let after = input.as_bytes().get(start + open_pat.len()).copied();
        if !matches!(
            after,
            Some(b'>') | Some(b'/') | Some(b' ') | Some(b'\n') | Some(b'\r') | Some(b'\t')
        ) {
            out.push_str(&input[cursor..start + open_pat.len()]);
            cursor = start + open_pat.len();
            continue;
        }
        out.push_str(&input[cursor..start]);

        // If self-closing `<tag .../>`, stop at `>`.
        let Some(tag_close_rel) = input[start..].find('>') else {
            cursor = input.len();
            break;
        };
        let tag_end = start + tag_close_rel + 1;
        if input[start..tag_end - 1].trim_end().ends_with('/') {
            cursor = tag_end;
            continue;
        }

        if let Some(end_rel) = find_ascii_ci(&input[tag_end..], &close_pat) {
            let close_start = tag_end + end_rel;
            if let Some(gt_rel) = input[close_start..].find('>') {
                cursor = close_start + gt_rel + 1;
                continue;
            }
        }
        cursor = input.len();
        break;
    }

    if cursor < input.len() {
        out.push_str(&input[cursor..]);
    }
    out
}

fn strip_self_or_open_tag_ci(input: &str, tag: &str) -> String {
    let open_pat = format!("<{tag}");
    let mut out = String::with_capacity(input.len());
    let mut cursor = 0usize;

    while let Some(rel_start) = find_ascii_ci(&input[cursor..], &open_pat) {
        let start = cursor + rel_start;
        let after = input.as_bytes().get(start + open_pat.len()).copied();
        if !matches!(
            after,
            Some(b'>') | Some(b'/') | Some(b' ') | Some(b'\n') | Some(b'\r') | Some(b'\t')
        ) {
            out.push_str(&input[cursor..start + open_pat.len()]);
            cursor = start + open_pat.len();
            continue;
        }
        out.push_str(&input[cursor..start]);
        if let Some(gt_rel) = input[start..].find('>') {
            cursor = start + gt_rel + 1;
        } else {
            cursor = input.len();
            break;
        }
    }

    if cursor < input.len() {
        out.push_str(&input[cursor..]);
    }
    out
}

fn strip_event_handler_attrs(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = String::with_capacity(input.len());
    let mut i = 0usize;
    let mut in_tag = false;
    let mut quote: Option<u8> = None;

    while i < bytes.len() {
        if let Some(q) = quote {
            out.push(bytes[i] as char);
            if bytes[i] == q {
                quote = None;
            }
            i += 1;
            continue;
        }
        if !in_tag {
            if bytes[i] == b'<' {
                in_tag = true;
            }
            let ch_len = utf8_char_len(bytes, i);
            out.push_str(&input[i..i + ch_len]);
            i += ch_len;
            continue;
        }

        if bytes[i] == b'>' {
            in_tag = false;
            out.push('>');
            i += 1;
            continue;
        }
        if bytes[i] == b'"' || bytes[i] == b'\'' {
            quote = Some(bytes[i]);
            out.push(bytes[i] as char);
            i += 1;
            continue;
        }

        if bytes[i].is_ascii_whitespace() {
            // Check for ` on<alpha>... = ...`
            let mut ws_end = i;
            while ws_end < bytes.len() && bytes[ws_end].is_ascii_whitespace() {
                ws_end += 1;
            }
            if ws_end + 3 < bytes.len()
                && bytes[ws_end].eq_ignore_ascii_case(&b'o')
                && bytes[ws_end + 1].eq_ignore_ascii_case(&b'n')
                && bytes[ws_end + 2].is_ascii_alphabetic()
            {
                let mut name_end = ws_end + 2;
                while name_end < bytes.len() && bytes[name_end].is_ascii_alphanumeric() {
                    name_end += 1;
                }
                let mut eq_pos = name_end;
                while eq_pos < bytes.len() && bytes[eq_pos].is_ascii_whitespace() {
                    eq_pos += 1;
                }
                if eq_pos < bytes.len() && bytes[eq_pos] == b'=' {
                    let mut val_start = eq_pos + 1;
                    while val_start < bytes.len() && bytes[val_start].is_ascii_whitespace() {
                        val_start += 1;
                    }
                    if val_start < bytes.len() {
                        let vq = bytes[val_start];
                        let mut val_end = val_start;
                        if vq == b'"' || vq == b'\'' {
                            val_end += 1;
                            while val_end < bytes.len() && bytes[val_end] != vq {
                                val_end += 1;
                            }
                            if val_end < bytes.len() {
                                val_end += 1;
                            }
                        } else {
                            while val_end < bytes.len()
                                && !bytes[val_end].is_ascii_whitespace()
                                && bytes[val_end] != b'>'
                            {
                                val_end += 1;
                            }
                        }
                        i = val_end;
                        continue;
                    }
                }
            }
        }

        let ch_len = utf8_char_len(bytes, i);
        out.push_str(&input[i..i + ch_len]);
        i += ch_len;
    }

    out
}

fn strip_unsafe_uri_attrs(input: &str) -> String {
    let mut out = input.to_string();
    for attr in ["xlink:href", "href", "src"] {
        out = scrub_single_uri_attr(&out, attr);
    }
    out
}

fn scrub_single_uri_attr(input: &str, attr_name: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = String::with_capacity(input.len());
    let mut i = 0usize;
    let name_bytes = attr_name.as_bytes();

    while i < bytes.len() {
        if i > 0
            && bytes[i - 1].is_ascii_whitespace()
            && i + name_bytes.len() < bytes.len()
            && bytes[i..i + name_bytes.len()].eq_ignore_ascii_case(name_bytes)
        {
            let mut eq = i + name_bytes.len();
            while eq < bytes.len() && bytes[eq].is_ascii_whitespace() {
                eq += 1;
            }
            if eq < bytes.len() && bytes[eq] == b'=' {
                let mut vs = eq + 1;
                while vs < bytes.len() && bytes[vs].is_ascii_whitespace() {
                    vs += 1;
                }
                if vs < bytes.len() && (bytes[vs] == b'"' || bytes[vs] == b'\'') {
                    let q = bytes[vs];
                    if let Some(rel_end) = input[vs + 1..].find(q as char) {
                        let raw_val = &input[vs + 1..vs + 1 + rel_end];
                        let compact: String = raw_val
                            .chars()
                            .filter(|c| !c.is_ascii_whitespace() && !c.is_control())
                            .collect();
                        let lower = compact.to_ascii_lowercase();
                        if lower.starts_with("javascript:")
                            || lower.starts_with("vbscript:")
                            || lower.starts_with("data:text/html")
                        {
                            i = vs + 1 + rel_end + 1;
                            continue;
                        }
                    }
                }
            }
        }
        let ch_len = utf8_char_len(bytes, i);
        out.push_str(&input[i..i + ch_len]);
        i += ch_len;
    }

    out
}

fn sanitize_style_constructs(input: &str) -> String {
    let mut out = replace_ascii_ci(input, "expression(", "invalid(");
    out = replace_ascii_ci(&out, "@import", "/* @import */");
    out = replace_ascii_ci(&out, "-moz-binding", "none");
    out = replace_ascii_ci(&out, "javascript:", "blocked:");
    out = replace_ascii_ci(&out, "url(http://", "url(#blocked-");
    out = replace_ascii_ci(&out, "url(https://", "url(#blocked-");
    out = replace_ascii_ci(&out, "url(\"http://", "url(\"#blocked-");
    out = replace_ascii_ci(&out, "url(\"https://", "url(\"#blocked-");
    out = replace_ascii_ci(&out, "url('http://", "url('#blocked-");
    out = replace_ascii_ci(&out, "url('https://", "url('#blocked-");
    out
}

fn ensure_svg_xmlns(svg: &str) -> String {
    let Some(gt) = svg.find('>') else {
        return svg.to_string();
    };
    if find_ascii_ci(&svg[..gt], "xmlns=").is_some() {
        return svg.to_string();
    }
    let mut out = String::with_capacity(svg.len() + 36);
    out.push_str(&svg[..gt]);
    out.push_str(" xmlns=\"http://www.w3.org/2000/svg\"");
    out.push_str(&svg[gt..]);
    out
}

fn ensure_svg_viewbox(svg: &str, width: f32, height: f32) -> String {
    let Some(gt) = svg.find('>') else {
        return svg.to_string();
    };
    if find_ascii_ci(&svg[..gt], "viewbox=").is_some() {
        return svg.to_string();
    }
    let mut out = String::with_capacity(svg.len() + 40);
    out.push_str(&svg[..gt]);
    out.push_str(&format!(" viewBox=\"0 0 {:.2} {:.2}\"", width, height));
    out.push_str(&svg[gt..]);
    out
}

fn extract_svg_dimensions(svg: &str) -> Option<(f32, f32)> {
    let open_end = svg.find('>')?;
    let header = &svg[..open_end];
    if let Some(vb) = extract_attr_value_ci(header, "viewbox") {
        let nums: Vec<f32> = vb
            .split(|c: char| c.is_ascii_whitespace() || c == ',')
            .filter(|p| !p.is_empty())
            .filter_map(|p| p.parse::<f32>().ok())
            .collect();
        if nums.len() >= 4 && nums[2] > 0.0 && nums[3] > 0.0 {
            return Some((nums[2], nums[3]));
        }
    }
    let w = extract_attr_value_ci(header, "width").and_then(parse_svg_length);
    let h = extract_attr_value_ci(header, "height").and_then(parse_svg_length);
    if let (Some(wv), Some(hv)) = (w, h)
        && wv > 0.0
        && hv > 0.0
    {
        return Some((wv, hv));
    }
    None
}

fn parse_svg_length(raw: &str) -> Option<f32> {
    let clean = raw.trim();
    if clean.ends_with('%') {
        return None;
    }
    let num_part: String = clean
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.' || *c == '-')
        .collect();
    num_part.parse::<f32>().ok()
}

fn extract_attr_value_ci<'a>(tag_header: &'a str, attr_name: &str) -> Option<&'a str> {
    let bytes = tag_header.as_bytes();
    let name_bytes = attr_name.as_bytes();
    let mut i = 0usize;
    while i + name_bytes.len() < bytes.len() {
        if (i == 0 || bytes[i - 1].is_ascii_whitespace())
            && bytes[i..i + name_bytes.len()].eq_ignore_ascii_case(name_bytes)
        {
            let mut eq = i + name_bytes.len();
            while eq < bytes.len() && bytes[eq].is_ascii_whitespace() {
                eq += 1;
            }
            if eq < bytes.len() && bytes[eq] == b'=' {
                let mut vs = eq + 1;
                while vs < bytes.len() && bytes[vs].is_ascii_whitespace() {
                    vs += 1;
                }
                if vs < bytes.len() && (bytes[vs] == b'"' || bytes[vs] == b'\'') {
                    let q = bytes[vs];
                    if let Some(rel_end) = tag_header[vs + 1..].find(q as char) {
                        return Some(&tag_header[vs + 1..vs + 1 + rel_end]);
                    }
                }
            }
        }
        i += 1;
    }
    None
}

fn namespace_svg_ids(svg: &str, kind: DiagramKind, theme: DiagramTheme, source: &str) -> String {
    let ids = collect_svg_ids(svg);
    if ids.is_empty() {
        return svg.to_string();
    }
    let hash = deterministic_hash(kind, theme, source);
    let prefix = format!("orb_{:012x}_", hash & 0xFFFF_FFFF_FFFF);

    let mut out = svg.to_string();
    // Rewrite longest IDs first so prefix-sharing IDs never corrupt each other.
    let mut sorted_ids: Vec<String> = ids.into_iter().collect();
    sorted_ids.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));

    for id in &sorted_ids {
        let new_id = format!("{prefix}{id}");
        out = out
            .replace(&format!("id=\"{id}\""), &format!("id=\"{new_id}\""))
            .replace(&format!("id='{id}'"), &format!("id='{new_id}'"))
            .replace(&format!("url(#{id})"), &format!("url(#{new_id})"))
            .replace(&format!("url(\"#{id}\")"), &format!("url(\"#{new_id}\")"))
            .replace(&format!("url('#{id}')"), &format!("url('#{new_id}')"))
            .replace(&format!("href=\"#{id}\""), &format!("href=\"#{new_id}\""))
            .replace(&format!("href='#{id}'"), &format!("href='#{new_id}'"));
    }

    // Namespace `#id` selectors inside `<style>...</style>` blocks.
    rewrite_style_id_selectors(&out, &sorted_ids, &prefix)
}

fn collect_svg_ids(svg: &str) -> HashSet<String> {
    let mut ids = HashSet::new();
    let bytes = svg.as_bytes();
    let mut i = 0usize;
    while i + 4 < bytes.len() {
        if (i == 0 || bytes[i - 1].is_ascii_whitespace())
            && bytes[i..i + 2].eq_ignore_ascii_case(b"id")
        {
            let mut eq = i + 2;
            while eq < bytes.len() && bytes[eq].is_ascii_whitespace() {
                eq += 1;
            }
            if eq < bytes.len() && bytes[eq] == b'=' {
                let mut vs = eq + 1;
                while vs < bytes.len() && bytes[vs].is_ascii_whitespace() {
                    vs += 1;
                }
                if vs < bytes.len() && (bytes[vs] == b'"' || bytes[vs] == b'\'') {
                    let q = bytes[vs];
                    if let Some(rel_end) = svg[vs + 1..].find(q as char) {
                        let id_val = &svg[vs + 1..vs + 1 + rel_end];
                        if !id_val.is_empty()
                            && !id_val.starts_with("orb_")
                            && id_val
                                .chars()
                                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
                        {
                            ids.insert(id_val.to_string());
                        }
                    }
                }
            }
        }
        i += 1;
    }
    ids
}

fn rewrite_style_id_selectors(svg: &str, sorted_ids: &[String], prefix: &str) -> String {
    let mut out = String::with_capacity(svg.len());
    let mut cursor = 0usize;
    while let Some(rel_open) = find_ascii_ci(&svg[cursor..], "<style") {
        let open_pos = cursor + rel_open;
        let Some(rel_gt) = svg[open_pos..].find('>') else {
            break;
        };
        let body_start = open_pos + rel_gt + 1;
        let Some(rel_close) = find_ascii_ci(&svg[body_start..], "</style>") else {
            break;
        };
        let body_end = body_start + rel_close;
        out.push_str(&svg[cursor..body_start]);

        let mut style_body = svg[body_start..body_end].to_string();
        for id in sorted_ids {
            style_body = replace_css_id_token(&style_body, id, prefix);
        }
        out.push_str(&style_body);
        out.push_str(&svg[body_end..body_end + "</style>".len()]);
        cursor = body_end + "</style>".len();
    }
    if cursor < svg.len() {
        out.push_str(&svg[cursor..]);
    }
    out
}

fn replace_css_id_token(css: &str, id: &str, prefix: &str) -> String {
    let needle = format!("#{id}");
    let mut out = String::with_capacity(css.len());
    let mut cursor = 0usize;
    while let Some(rel) = css[cursor..].find(&needle) {
        let pos = cursor + rel;
        let after = pos + needle.len();
        let next_is_ident = css
            .as_bytes()
            .get(after)
            .copied()
            .is_some_and(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
        out.push_str(&css[cursor..pos]);
        if !next_is_ident {
            out.push('#');
            out.push_str(prefix);
            out.push_str(id);
        } else {
            out.push_str(&needle);
        }
        cursor = after;
    }
    if cursor < css.len() {
        out.push_str(&css[cursor..]);
    }
    out
}

fn deterministic_hash(kind: DiagramKind, theme: DiagramTheme, source: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let kind_b = match kind {
        DiagramKind::Mermaid => 1u8,
        DiagramKind::Svg => 2u8,
        DiagramKind::Graphviz => 3u8,
    };
    let theme_b = match theme {
        DiagramTheme::Light => 1u8,
        DiagramTheme::Dark => 2u8,
    };
    for &b in &[kind_b, theme_b] {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    for &b in source.as_bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

fn starts_with_ascii_ci(haystack: &str, needle: &str) -> bool {
    haystack.len() >= needle.len()
        && haystack.as_bytes()[..needle.len()].eq_ignore_ascii_case(needle.as_bytes())
}

fn find_ascii_ci(haystack: &str, needle: &str) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack
        .as_bytes()
        .windows(needle.len())
        .position(|w| w.eq_ignore_ascii_case(needle.as_bytes()))
}

fn find_last_ascii_ci(haystack: &str, needle: &str) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack
        .as_bytes()
        .windows(needle.len())
        .rposition(|w| w.eq_ignore_ascii_case(needle.as_bytes()))
}

fn replace_ascii_ci(haystack: &str, needle: &str, replacement: &str) -> String {
    let mut out = String::with_capacity(haystack.len());
    let mut cursor = 0usize;
    while let Some(rel) = find_ascii_ci(&haystack[cursor..], needle) {
        let pos = cursor + rel;
        out.push_str(&haystack[cursor..pos]);
        out.push_str(replacement);
        cursor = pos + needle.len();
    }
    if cursor < haystack.len() {
        out.push_str(&haystack[cursor..]);
    }
    out
}

fn utf8_char_len(bytes: &[u8], idx: usize) -> usize {
    let b = bytes[idx];
    if b < 0x80 {
        1
    } else if (b & 0xE0) == 0xC0 {
        2.min(bytes.len() - idx)
    } else if (b & 0xF0) == 0xE0 {
        3.min(bytes.len() - idx)
    } else {
        4.min(bytes.len() - idx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCREENSHOT_FLOWCHART: &str = r#"flowchart LR
    subgraph Host["目标主机 (Mac / Linux / Devbox)"]
        CLI["AI CLI / Shell<br/>(agy / codex / claude / PTY)"]
        SHIM["orbit-shim<br/>Unix Socket 独立进程隔离"]
        DAEMON["orbit-daemon<br/>会话编排 + Git/FS + seq 排序"]
        DB_D[("SQLite WAL<br/>主库")]
        CLI <--> SHIM <--> DAEMON
        DAEMON <--> DB_D
    end

    subgraph Transport["传输与零知识安全层"]
        DIRECT["LAN Direct WS / SSH Tunnel<br/>(Zstd Wire + 二进制 PTY)"]
        RELAY["orbit-relay<br/>(Noise NK E2EE 端到端加密)"]
    end

    subgraph Core["orbit-client-core (四端共享 Rust 内核)"]
        POOL["多主机连接池<br/>connect_smart Happy-Eyeballs"]
        DB_C[("SQLite WAL<br/>0ms 副本 + since_seq 游标")]
        MEM["HostMemoryState<br/>内存态 + Markdown缓存 + vt100"]
        RENDER["render.rs / stage.rs / catalog.rs"]
        BUS["CoreNotification 事件总线<br/>(直通 / Coalescing 24ms)"]
        POOL <--> DB_C
        POOL --> MEM --> RENDER
        MEM --> BUS
    end"#;

    #[test]
    fn renders_screenshot_cjk_multi_subgraph_flowchart_light_and_dark() {
        let light = render_diagram(&RenderDiagramRequest {
            kind: DiagramKind::Mermaid,
            source: SCREENSHOT_FLOWCHART.into(),
            theme: DiagramTheme::Light,
        });
        assert!(light.error.is_none(), "unexpected error: {:?}", light.error);
        let light_svg = light.svg.as_ref().expect("light svg");
        assert!(light_svg.contains("<svg"), "svg:\n{light_svg}");
        assert!(light_svg.contains("目标主机"), "svg:\n{light_svg}");
        assert!(
            light_svg.contains("传输与零知识安全层"),
            "svg:\n{light_svg}"
        );
        assert!(
            light_svg.contains("四端共享") && light_svg.contains("Rust 内核"),
            "svg:\n{light_svg}"
        );
        assert!(light_svg.contains("fill=\"none\""));
        assert!(light_svg.contains("id=\"orb_"));
        assert!(light.width > 100.0 && light.height > 100.0);
        assert_eq!(light.diagram_type_label.as_deref(), Some("Flowchart"));

        let dark = render_diagram(&RenderDiagramRequest {
            kind: DiagramKind::Mermaid,
            source: SCREENSHOT_FLOWCHART.into(),
            theme: DiagramTheme::Dark,
        });
        assert!(dark.error.is_none(), "unexpected error: {:?}", dark.error);
        let dark_svg = dark.svg.as_ref().expect("dark svg");
        assert!(!dark_svg.contains("fill=\"#FFFFFF\"/>"));
        assert!(!dark_svg.contains("fill=\"#141519\"/>"));
        assert!(dark_svg.contains("fill=\"none\"/>"));
        assert!(dark_svg.contains("#F4F4F5"));
    }

    #[test]
    fn renders_multiple_mermaid_families_and_hits_cache() {
        let seq = render_diagram(&RenderDiagramRequest {
            kind: DiagramKind::Mermaid,
            source: "sequenceDiagram\n  Client->>Daemon: Connect\n  Daemon-->>Client: Welcome"
                .into(),
            theme: DiagramTheme::Light,
        });
        assert!(seq.error.is_none());
        assert_eq!(seq.diagram_type_label.as_deref(), Some("Sequence"));

        let er = render_diagram(&RenderDiagramRequest {
            kind: DiagramKind::Mermaid,
            source: "erDiagram\n  HOST ||--o{ AGENT : hosts".into(),
            theme: DiagramTheme::Dark,
        });
        assert!(er.error.is_none());
        assert_eq!(er.diagram_type_label.as_deref(), Some("ER Diagram"));

        let pie_req = RenderDiagramRequest {
            kind: DiagramKind::Mermaid,
            source: "pie title Tasks\n  \"Done\" : 70\n  \"Pending\" : 30".into(),
            theme: DiagramTheme::Light,
        };
        let pie1 = render_diagram(&pie_req);
        let pie2 = render_diagram(&pie_req);
        assert_eq!(pie1, pie2);
        assert_eq!(pie1.diagram_type_label.as_deref(), Some("Pie Chart"));
    }

    #[test]
    fn renders_graphviz_dot_with_dark_theme_adaptation() {
        let res = render_diagram(&RenderDiagramRequest {
            kind: DiagramKind::Graphviz,
            source: "digraph Orbit { Client -> Relay -> Daemon; }".into(),
            theme: DiagramTheme::Dark,
        });
        assert!(res.error.is_none(), "dot error: {:?}", res.error);
        let svg = res.svg.expect("graphviz svg");
        assert!(svg.contains("<svg"));
        assert!(!svg.contains("Times, serif"));
        assert!(svg.contains("#F4F4F5") || svg.contains("#A1A1AA"));
        assert_eq!(res.diagram_type_label.as_deref(), Some("Graphviz"));
    }

    #[test]
    fn sanitizes_raw_svg_against_xss_and_namespaces_ids() {
        let dirty = r#"<?xml version="1.0"?>
<!DOCTYPE svg [<!ENTITY xxe "bad">]>
<svg width="200" height="100" onload="alert(1)">
  <script>alert('xss')</script>
  <foreignObject><div>bad</div></foreignObject>
  <style>@import url(https://evil.test/x.css); #box { fill: red; }</style>
  <defs><linearGradient id="box"><stop offset="0%"/></linearGradient></defs>
  <rect id="r1" onclick="steal()" width="100" height="50" fill="url(#box)"/>
  <a href="javascript:alert(2)"><text x="10" y="20">hi</text></a>
</svg>"#;
        let out = render_diagram(&RenderDiagramRequest {
            kind: DiagramKind::Svg,
            source: dirty.into(),
            theme: DiagramTheme::Light,
        });
        assert!(out.error.is_none(), "svg err: {:?}", out.error);
        let svg = out.svg.expect("sanitized svg");
        let lower = svg.to_ascii_lowercase();
        assert!(!lower.contains("<script"));
        assert!(!lower.contains("foreignobject"));
        assert!(!lower.contains("onload="));
        assert!(!lower.contains("onclick="));
        assert!(!lower.contains("javascript:"));
        assert!(!lower.contains("https://evil.test"));
        assert!(svg.contains("viewBox=\"0 0 200.00 100.00\""));
        assert!(svg.contains("url(#orb_"));
    }

    #[test]
    fn detects_diagram_languages_and_reports_invalid_syntax_gracefully() {
        assert_eq!(
            detect_diagram_kind("mermaid", "flowchart TD; A-->B"),
            Some(DiagramKind::Mermaid)
        );
        assert_eq!(detect_diagram_kind("mmd", ""), Some(DiagramKind::Mermaid));
        assert_eq!(
            detect_diagram_kind("dot", "digraph {}"),
            Some(DiagramKind::Graphviz)
        );
        assert_eq!(
            detect_diagram_kind("svg", "<svg></svg>"),
            Some(DiagramKind::Svg)
        );
        assert_eq!(
            detect_diagram_kind(
                "xml",
                "<?xml version=\"1.0\"?><svg viewBox=\"0 0 1 1\"></svg>"
            ),
            Some(DiagramKind::Svg)
        );
        assert_eq!(detect_diagram_kind("rust", "fn main() {}"), None);

        let bad = render_diagram(&RenderDiagramRequest {
            kind: DiagramKind::Mermaid,
            source: "completely_invalid_syntax @@@ ###".into(),
            theme: DiagramTheme::Light,
        });
        assert!(bad.svg.is_none());
        assert!(bad.error.is_some());
    }
}
