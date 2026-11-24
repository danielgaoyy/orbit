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
#[cfg(target_arch = "wasm32")]
use wasm_mermaid as mermaid_rs_renderer;

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

const LIGHT_EDITORIAL_PIE_COLORS: [&str; 12] = [
    "#6366F1", "#38BDF8", "#34D399", "#FBBF24", "#FB7185", "#A78BFA", "#2DD4BF", "#FB923C",
    "#60A5FA", "#A3E635", "#F472B6", "#94A3B8",
];

const DARK_EDITORIAL_PIE_COLORS: [&str; 12] = [
    "#818CF8", "#38BDF8", "#34D399", "#FBBF24", "#FB7185", "#C084FC", "#2DD4BF", "#FB923C",
    "#60A5FA", "#A3E635", "#F472B6", "#94A3B8",
];

const LIGHT_EDITORIAL_GIT_COLORS: [&str; 8] = [
    "#4F46E5", "#0284C7", "#059669", "#D97706", "#E11D48", "#7C3AED", "#0D9488", "#EA580C",
];

const DARK_EDITORIAL_GIT_COLORS: [&str; 8] = [
    "#818CF8", "#38BDF8", "#34D399", "#FBBF24", "#FB7185", "#C084FC", "#2DD4BF", "#FB923C",
];

fn orbit_mermaid_theme(theme: DiagramTheme) -> MmdTheme {
    match theme {
        DiagramTheme::Light => {
            let mut t = MmdTheme::modern();
            t.font_family = ORBIT_DIAGRAM_FONT_FAMILY.to_string();
            t.font_size = 13.5;
            t.background = "#FFFFFF".to_string();
            // Gentle cool-slate tinted nodes with crisp hairline borders
            t.primary_color = "#F8FAFC".to_string();
            t.primary_border_color = "#CBD5E1".to_string();
            t.primary_text_color = "#18181B".to_string();
            t.text_color = "#334155".to_string();
            t.line_color = "#64748B".to_string();
            t.secondary_color = "#EFF6FF".to_string();
            t.tertiary_color = "#F4F6FA".to_string();
            // Subtle recessed subgraph containers with clean muted headers
            t.cluster_background = "#F1F5F9".to_string();
            t.cluster_border = "#DCE3EC".to_string();
            t.edge_label_background = "#FFFFFF".to_string();
            t.sequence_actor_fill = "#F8FAFC".to_string();
            t.sequence_actor_border = "#CBD5E1".to_string();
            t.sequence_actor_line = "#CBD5E1".to_string();
            t.sequence_note_fill = "#FFFBEB".to_string();
            t.sequence_note_border = "#FCD34D".to_string();
            t.sequence_activation_fill = "#EFF6FF".to_string();
            t.sequence_activation_border = "#93C5FD".to_string();
            t.git_colors = LIGHT_EDITORIAL_GIT_COLORS.map(str::to_string);
            t.git_branch_label_colors = ["#FFFFFF"; 8].map(str::to_string);
            t.git_commit_label_color = "#334155".to_string();
            t.git_commit_label_background = "#F8FAFC".to_string();
            t.git_tag_label_color = "#1E293B".to_string();
            t.git_tag_label_background = "#EFF6FF".to_string();
            t.git_tag_label_border = "#BFDBFE".to_string();
            t.pie_colors = LIGHT_EDITORIAL_PIE_COLORS.map(str::to_string);
            t.pie_title_text_size = 16.0;
            t.pie_title_text_color = "#18181B".to_string();
            t.pie_section_text_size = 12.5;
            t.pie_section_text_color = "#18181B".to_string();
            t.pie_legend_text_size = 12.5;
            t.pie_legend_text_color = "#334155".to_string();
            t.pie_stroke_color = "#FFFFFF".to_string();
            t.pie_stroke_width = 1.5;
            t.pie_outer_stroke_width = 1.0;
            t.pie_outer_stroke_color = "#E2E8F0".to_string();
            t.pie_opacity = 0.92;
            t
        }
        DiagramTheme::Dark => {
            let mut t = MmdTheme::dark();
            t.font_family = ORBIT_DIAGRAM_FONT_FAMILY.to_string();
            t.font_size = 13.5;
            t.background = "#141519".to_string();
            // Refined dark slate elevated nodes with muted cool hairline borders
            t.primary_color = "#1F222C".to_string();
            t.primary_border_color = "#3A4054".to_string();
            t.primary_text_color = "#F4F4F5".to_string();
            t.text_color = "#D4D4D8".to_string();
            t.line_color = "#8B95AB".to_string();
            t.secondary_color = "#242B3E".to_string();
            t.tertiary_color = "#1A1D26".to_string();
            t.cluster_background = "#181A22".to_string();
            t.cluster_border = "#2D3242".to_string();
            t.edge_label_background = "#181A22".to_string();
            t.sequence_actor_fill = "#1F222C".to_string();
            t.sequence_actor_border = "#3A4054".to_string();
            t.sequence_actor_line = "#4A5268".to_string();
            t.sequence_note_fill = "#2B251A".to_string();
            t.sequence_note_border = "#8C682B".to_string();
            t.sequence_activation_fill = "#242B3E".to_string();
            t.sequence_activation_border = "#4E5F8A".to_string();
            t.git_colors = DARK_EDITORIAL_GIT_COLORS.map(str::to_string);
            t.git_branch_label_colors = ["#141519"; 8].map(str::to_string);
            t.git_commit_label_color = "#D4D4D8".to_string();
            t.git_commit_label_background = "#1F222C".to_string();
            t.git_tag_label_color = "#F4F4F5".to_string();
            t.git_tag_label_background = "#242B3E".to_string();
            t.git_tag_label_border = "#4E5F8A".to_string();
            t.pie_colors = DARK_EDITORIAL_PIE_COLORS.map(str::to_string);
            t.pie_title_text_size = 16.0;
            t.pie_title_text_color = "#F4F4F5".to_string();
            t.pie_section_text_size = 12.5;
            t.pie_section_text_color = "#F4F4F5".to_string();
            t.pie_legend_text_size = 12.5;
            t.pie_legend_text_color = "#D4D4D8".to_string();
            t.pie_stroke_color = "#141519".to_string();
            t.pie_stroke_width = 1.5;
            t.pie_outer_stroke_width = 1.0;
            t.pie_outer_stroke_color = "#2D3242".to_string();
            t.pie_opacity = 0.90;
            t
        }
    }
}

fn orbit_mermaid_layout_config(theme: DiagramTheme) -> LayoutConfig {
    let mut cfg = LayoutConfig {
        node_spacing: 46.0,
        rank_spacing: 50.0,
        node_padding_x: 32.0,
        node_padding_y: 16.0,
        label_line_height: 1.48,
        ..LayoutConfig::default()
    };
    cfg.mindmap.default_corner_radius = 8.0;

    match theme {
        DiagramTheme::Light => {
            cfg.mindmap.section_colors = vec![
                "#EEF2FF", "#E0F2FE", "#ECFDF5", "#FEF3C7", "#FFE4E6", "#F3E8FF", "#CCFBF1",
                "#FFEDD5", "#DBEAFE", "#ECFCCB", "#FCE7F3", "#F1F5F9",
            ]
            .into_iter()
            .map(str::to_string)
            .collect();
            cfg.mindmap.section_label_colors = vec!["#18181B".to_string(); 12];
            cfg.mindmap.section_line_colors = LIGHT_EDITORIAL_PIE_COLORS
                .iter()
                .map(|c| (*c).to_string())
                .collect();
            cfg.mindmap.root_fill = Some("#18181B".to_string());
            cfg.mindmap.root_text = Some("#FFFFFF".to_string());
            cfg.requirement.fill = "#F8FAFC".to_string();
            cfg.requirement.box_stroke = "#CBD5E1".to_string();
            cfg.requirement.stroke = "#64748B".to_string();
            cfg.requirement.divider_color = "#CBD5E1".to_string();
            cfg.requirement.label_color = "#18181B".to_string();
            cfg.requirement.edge_stroke = "#64748B".to_string();
            cfg.requirement.edge_label_color = "#334155".to_string();
            cfg.requirement.edge_label_background = "#FFFFFF".to_string();
        }
        DiagramTheme::Dark => {
            cfg.mindmap.section_colors = vec![
                "#232742", "#1A2E3F", "#17332C", "#362B1A", "#3A2029", "#2D2240", "#163335",
                "#38251A", "#1E2B42", "#26331A", "#382032", "#232630",
            ]
            .into_iter()
            .map(str::to_string)
            .collect();
            cfg.mindmap.section_label_colors = vec!["#F4F4F5".to_string(); 12];
            cfg.mindmap.section_line_colors = DARK_EDITORIAL_PIE_COLORS
                .iter()
                .map(|c| (*c).to_string())
                .collect();
            cfg.mindmap.root_fill = Some("#2E3446".to_string());
            cfg.mindmap.root_text = Some("#F4F4F5".to_string());
            cfg.requirement.fill = "#1F222C".to_string();
            cfg.requirement.box_stroke = "#3A4054".to_string();
            cfg.requirement.stroke = "#8B95AB".to_string();
            cfg.requirement.divider_color = "#3A4054".to_string();
            cfg.requirement.label_color = "#F4F4F5".to_string();
            cfg.requirement.edge_stroke = "#8B95AB".to_string();
            cfg.requirement.edge_label_color = "#D4D4D8".to_string();
            cfg.requirement.edge_label_background = "#181A22".to_string();
        }
    }

    cfg
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
    let config = orbit_mermaid_layout_config(theme_mode);

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

    let hash = deterministic_hash(DiagramKind::Mermaid, theme_mode, source);
    let root_id = format!("orb_root_{:012x}", hash & 0xFFFF_FFFF_FFFF);

    // Strip the outer canvas background fill so the diagram inherits the Orbit card surface
    // while keeping internal shape/label fills opaque, then apply scoped visual polish.
    let transparent_canvas_svg = make_mermaid_canvas_transparent(&raw_svg, &mmd_theme.background);
    let polished_svg = polish_mermaid_svg(&transparent_canvas_svg, &root_id);
    let namespaced = namespace_svg_ids(&polished_svg, DiagramKind::Mermaid, theme_mode, source);
    let scoped = scope_svg_root_and_styles(&namespaced, &root_id);

    RenderDiagramView {
        kind: DiagramKind::Mermaid,
        theme: theme_mode,
        svg: Some(scoped),
        width: dims.viewbox_width.max(1.0),
        height: dims.viewbox_height.max(1.0),
        error: None,
        diagram_type_label: Some(label),
    }
}

/// Apply coordinate-preserving, tag-scoped visual refinement to Mermaid SVG output:
/// - Smooth modern corner rounding (`rx="8" ry="8"` on node rects, `rx="5" ry="5"` on edge pills),
///   strictly inside `<rect>` tags and skipping tiny swatches/bars (`<= 16px`).
/// - Crisp hairline border weight (`1.15px`) on shape tags and root-scoped typography rules.
fn polish_mermaid_svg(svg: &str, root_id: &str) -> String {
    let mut out = rewrite_svg_open_tags(svg, |tag| {
        if starts_with_ascii_ci(tag, "<rect")
            && tag
                .as_bytes()
                .get(5)
                .copied()
                .is_some_and(|b| b.is_ascii_whitespace())
        {
            let small_rect = extract_attr_value_ci(tag, "width")
                .and_then(parse_svg_length)
                .is_some_and(|w| w <= 16.0)
                || extract_attr_value_ci(tag, "height")
                    .and_then(parse_svg_length)
                    .is_some_and(|h| h <= 16.0);
            let mut next = tag.to_string();
            if !small_rect {
                next = next
                    .replace("rx=\"3\" ry=\"3\"", "rx=\"8\" ry=\"8\"")
                    .replace("rx=\"2\" ry=\"2\"", "rx=\"5\" ry=\"5\"");
            }
            next.replace(
                "stroke-width=\"1\" stroke-linejoin=\"round\" stroke-linecap=\"round\"/>",
                "stroke-width=\"1.15\" stroke-linejoin=\"round\" stroke-linecap=\"round\"/>",
            )
        } else if starts_with_ascii_ci(tag, "<polygon")
            || starts_with_ascii_ci(tag, "<circle")
            || starts_with_ascii_ci(tag, "<ellipse")
        {
            tag.replace(
                "stroke-width=\"1\" stroke-linejoin=\"round\" stroke-linecap=\"round\"/>",
                "stroke-width=\"1.15\" stroke-linejoin=\"round\" stroke-linecap=\"round\"/>",
            )
        } else {
            tag.to_string()
        }
    });

    let polish_style = format!(
        "<style>#{root_id} text, #{root_id} tspan {{ text-rendering: optimizeLegibility; font-weight: 500; }}</style>"
    );
    if let Some(defs_end) = out.find("</defs>") {
        let insert_at = defs_end + "</defs>".len();
        out.insert_str(insert_at, &polish_style);
    } else if let Some(gt) = out.find('>') {
        out.insert_str(gt + 1, &polish_style);
    }
    out
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

    let hash = deterministic_hash(DiagramKind::Graphviz, theme_mode, source);
    let root_id = format!("orb_root_{:012x}", hash & 0xFFFF_FFFF_FFFF);

    let cleaned = strip_xml_and_doctype(&raw);
    let themed = polish_graphviz_svg(&cleaned, theme_mode);

    let (width, height) = extract_svg_dimensions(&themed).unwrap_or((360.0, 240.0));
    let namespaced = namespace_svg_ids(&themed, DiagramKind::Graphviz, theme_mode, source);
    let scoped = scope_svg_root_and_styles(&namespaced, &root_id);

    RenderDiagramView {
        kind: DiagramKind::Graphviz,
        theme: theme_mode,
        svg: Some(scoped),
        width: width.max(1.0),
        height: height.max(1.0),
        error: None,
        diagram_type_label: Some("Graphviz".into()),
    }
}

/// Apply Orbit's editorial Light/Dark palette, legible typography, explicit arrowhead
/// coloring (avoiding WebKit `context-stroke` bugs), tag-scoped rounded node corners,
/// and balanced stroke weights to `layout-rs` Graphviz SVG output without distorting
/// coordinates or mutating `<text>` payloads.
fn polish_graphviz_svg(svg: &str, theme_mode: DiagramTheme) -> String {
    let (ink, node_fill, node_border, edge_stroke) = match theme_mode {
        DiagramTheme::Light => ("#18181B", "#F8FAFC", "#CBD5E1", "#64748B"),
        DiagramTheme::Dark => ("#F4F4F5", "#1F222C", "#3A4054", "#8B95AB"),
    };

    let style_extra = format!(
        "text, tspan {{ fill: {ink}; font-weight: 500; text-rendering: optimizeLegibility; }}\n\
         path, line, rect, ellipse, polygon {{ stroke-linecap: round; stroke-linejoin: round; }}\n\
         </style>"
    );

    // First rewrite `<style>` font family and append rules (scoped later by `scope_svg_root_and_styles`)
    let with_style = svg
        .replace(
            "font-family: Times, serif;",
            &format!("font-family: {ORBIT_DIAGRAM_FONT_FAMILY};"),
        )
        .replace("</style>", &style_extra);

    // Tag-scoped attribute polish (never mutates text node bodies)
    let work = rewrite_svg_open_tags(&with_style, |tag| {
        let is_rect = starts_with_ascii_ci(tag, "<rect")
            && tag
                .as_bytes()
                .get(5)
                .copied()
                .is_some_and(|b| b.is_ascii_whitespace());
        let is_shape = is_rect
            || starts_with_ascii_ci(tag, "<ellipse")
            || starts_with_ascii_ci(tag, "<polygon");
        let target_stroke = if is_shape { node_border } else { edge_stroke };

        let mut next = tag
            .replace(
                "fill=\"context-stroke\"",
                &format!("fill=\"{edge_stroke}\""),
            )
            .replace("stroke-width=\"2\"", "stroke-width=\"1.25\"")
            .replace("stroke-width=\"1\"", "stroke-width=\"1.35\"")
            .replace("fill=\"#ffffffff\"", &format!("fill=\"{node_fill}\""))
            .replace("fill=\"#ffffff\"", &format!("fill=\"{node_fill}\""))
            .replace("fill=\"white\"", &format!("fill=\"{node_fill}\""))
            .replace("fill=\"#000000ff\"", &format!("fill=\"{ink}\""))
            .replace("fill=\"#000000\"", &format!("fill=\"{ink}\""))
            .replace("fill=\"black\"", &format!("fill=\"{ink}\""))
            .replace(
                "stroke=\"#000000ff\"",
                &format!("stroke=\"{target_stroke}\""),
            )
            .replace("stroke=\"#000000\"", &format!("stroke=\"{target_stroke}\""))
            .replace("stroke=\"black\"", &format!("stroke=\"{target_stroke}\""));

        if is_rect {
            // Handle paired `rx="0" ry="0"` before bare `rx="0"` so duplicate `ry=` is never emitted
            if next.contains("rx=\"0\" ry=\"0\"") {
                next = next.replace("rx=\"0\" ry=\"0\"", "rx=\"7\" ry=\"7\"");
            } else if next.contains("ry=\"0\" rx=\"0\"") {
                next = next.replace("ry=\"0\" rx=\"0\"", "ry=\"7\" rx=\"7\"");
            } else if next.contains("rx=\"0\"") {
                if find_ascii_ci(&next, "ry=").is_some() {
                    next = next.replace("rx=\"0\"", "rx=\"7\"");
                } else {
                    next = next.replace("rx=\"0\"", "rx=\"7\" ry=\"7\"");
                }
            }
        }
        next
    });

    pad_graphviz_viewbox(&work, 8.0)
}

/// Apply `rewrite` strictly to opening XML tags (`<tag ...>` / `<tag .../>`), leaving closing tags,
/// comments, style bodies, and text content completely untouched.
fn rewrite_svg_open_tags(svg: &str, mut rewrite: impl FnMut(&str) -> String) -> String {
    let bytes = svg.as_bytes();
    let mut out = String::with_capacity(svg.len() + 64);
    let mut i = 0usize;

    while i < bytes.len() {
        let Some(rel_lt) = svg[i..].find('<') else {
            out.push_str(&svg[i..]);
            break;
        };
        let lt = i + rel_lt;
        out.push_str(&svg[i..lt]);

        if bytes
            .get(lt + 1)
            .copied()
            .is_some_and(|b| matches!(b, b'/' | b'!' | b'?'))
        {
            // Closing tag, comment, or PI — copy verbatim to its closing `>`
            let end = if svg[lt..].starts_with("<!--") {
                svg[lt + 4..].find("-->").map(|p| lt + 4 + p + 3)
            } else {
                svg[lt..].find('>').map(|p| lt + p + 1)
            };
            match end {
                Some(gt) => {
                    out.push_str(&svg[lt..gt]);
                    i = gt;
                }
                None => {
                    out.push_str(&svg[lt..]);
                    break;
                }
            }
            continue;
        }

        // Find end of opening tag respecting quoted attribute strings
        let mut j = lt + 1;
        let mut quote: Option<u8> = None;
        while j < bytes.len() {
            let b = bytes[j];
            if let Some(q) = quote {
                if b == q {
                    quote = None;
                }
            } else if b == b'"' || b == b'\'' {
                quote = Some(b);
            } else if b == b'>' {
                break;
            }
            j += 1;
        }
        if j >= bytes.len() {
            out.push_str(&svg[lt..]);
            break;
        }
        let gt = j + 1;
        let tag = &svg[lt..gt];
        out.push_str(&rewrite(tag));
        i = gt;
    }

    out
}

/// Assign a deterministic `id="{root_id}"` to the `<svg>` root element and scope every
/// CSS rule inside `<style>...</style>` blocks under `#{root_id}` so embedded SVGs never
/// leak global stylesheet rules to sibling SVGs on the page.
fn scope_svg_root_and_styles(svg: &str, root_id: &str) -> String {
    let with_root_id = assign_svg_root_id(svg, root_id);
    scope_style_blocks(&with_root_id, root_id)
}

fn assign_svg_root_id(svg: &str, root_id: &str) -> String {
    let Some(svg_open) = find_ascii_ci(svg, "<svg") else {
        return svg.to_string();
    };
    let Some(rel_gt) = svg[svg_open..].find('>') else {
        return svg.to_string();
    };
    let gt = svg_open + rel_gt;
    let header = &svg[svg_open..gt];
    if find_ascii_ci(header, &format!("id=\"{root_id}\"")).is_some() {
        return svg.to_string();
    }

    let insert_pos = svg_open + 4;
    let mut out = String::with_capacity(svg.len() + root_id.len() + 8);
    out.push_str(&svg[..insert_pos]);
    out.push_str(&format!(" id=\"{root_id}\""));
    // Strip any pre-existing `id="..."` attribute on `<svg>` to avoid duplicate `id` attrs
    let rest_header = strip_id_attr_from_header(&svg[insert_pos..gt]);
    out.push_str(&rest_header);
    out.push_str(&svg[gt..]);
    out
}

fn strip_id_attr_from_header(header_rest: &str) -> String {
    let bytes = header_rest.as_bytes();
    let mut out = String::with_capacity(header_rest.len());
    let mut i = 0usize;
    while i < bytes.len() {
        if i + 3 < bytes.len()
            && bytes[i].is_ascii_whitespace()
            && bytes[i + 1..i + 3].eq_ignore_ascii_case(b"id")
        {
            let mut eq = i + 3;
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
                    if let Some(rel_end) = header_rest[vs + 1..].find(q as char) {
                        i = vs + 1 + rel_end + 1;
                        continue;
                    }
                }
            }
        }
        let ch_len = header_rest[i..].chars().next().map_or(1, char::len_utf8);
        out.push_str(&header_rest[i..i + ch_len]);
        i += ch_len;
    }
    out
}

fn scope_style_blocks(svg: &str, root_id: &str) -> String {
    let mut out = String::with_capacity(svg.len() + 64);
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
        out.push_str(&scope_css_rules(&svg[body_start..body_end], root_id));
        out.push_str(&svg[body_end..body_end + "</style>".len()]);
        cursor = body_end + "</style>".len();
    }
    if cursor < svg.len() {
        out.push_str(&svg[cursor..]);
    }
    out
}

fn scope_css_rules(css: &str, root_id: &str) -> String {
    let prefix = format!("#{root_id}");
    let mut out = String::with_capacity(css.len() + 64);
    let mut rest = css;
    while let Some(open_brace) = rest.find('{') {
        let selector_part = &rest[..open_brace];
        let after_open = &rest[open_brace + 1..];
        let Some(close_brace) = after_open.find('}') else {
            out.push_str(rest);
            return out;
        };
        let decl_block = &after_open[..close_brace];
        let trimmed_sel = selector_part.trim();
        if trimmed_sel.is_empty() || trimmed_sel.starts_with('@') {
            out.push_str(selector_part);
        } else {
            let scoped_selectors: Vec<String> = trimmed_sel
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(|sel| {
                    if sel.starts_with(&prefix) {
                        sel.to_string()
                    } else if sel.eq_ignore_ascii_case("svg") {
                        prefix.clone()
                    } else {
                        format!("{prefix} {sel}")
                    }
                })
                .collect();
            out.push_str(&scoped_selectors.join(", "));
        }
        out.push_str(" {");
        out.push_str(decl_block);
        out.push_str("}\n");
        rest = &after_open[close_brace + 1..];
    }
    if !rest.trim().is_empty() {
        out.push_str(rest);
    }
    out
}

fn pad_graphviz_viewbox(svg: &str, pad: f32) -> String {
    let Some(open_end) = svg.find('>') else {
        return svg.to_string();
    };
    let header = &svg[..open_end];
    let Some(vb_raw) = extract_attr_value_ci(header, "viewbox") else {
        return svg.to_string();
    };
    let nums: Vec<f32> = vb_raw
        .split(|c: char| c.is_ascii_whitespace() || c == ',')
        .filter(|p| !p.is_empty())
        .filter_map(|p| p.parse::<f32>().ok())
        .collect();
    if nums.len() != 4 || nums[2] <= 0.0 || nums[3] <= 0.0 {
        return svg.to_string();
    }
    let padded_vb = format!(
        "{:.2} {:.2} {:.2} {:.2}",
        nums[0] - pad,
        nums[1] - pad,
        nums[2] + pad * 2.0,
        nums[3] + pad * 2.0
    );
    let new_header = header.replacen(vb_raw, &padded_vb, 1);
    let mut out = String::with_capacity(svg.len() + 32);
    out.push_str(&new_header);
    out.push_str(&svg[open_end..]);
    out
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

// ---------------------------------------------------------------------------
// Strict Pure-Rust XML Allowlist SVG Sanitizer (Zero Blocklist Regex/String Bypasses)
// ---------------------------------------------------------------------------

const MAX_SVG_XML_DEPTH: usize = 64;
const MAX_SVG_XML_NODES: usize = 16_384;

#[derive(Debug)]
struct SanitizedSvgNode {
    tag: &'static str,
    attrs: Vec<(&'static str, String)>,
    children: Vec<SanitizedSvgChild>,
}

#[derive(Debug)]
enum SanitizedSvgChild {
    Elem(SanitizedSvgNode),
    Text(String),
}

fn sanitize_svg(raw: &str) -> Result<(String, f32, f32), String> {
    let mut parser = StrictSvgXmlParser::new(raw);
    let mut root = parser.parse_single_svg_root()?;

    let (width, height) = extract_sanitized_svg_dimensions(&root).unwrap_or((480.0, 320.0));
    if !root.attrs.iter().any(|(k, _)| *k == "viewBox") {
        root.attrs
            .push(("viewBox", format!("0 0 {:.2} {:.2}", width, height)));
    }
    if !root.attrs.iter().any(|(k, _)| *k == "xmlns") {
        root.attrs
            .insert(0, ("xmlns", "http://www.w3.org/2000/svg".to_string()));
    }
    let uses_xlink = svg_node_uses_xlink(&root);
    if uses_xlink && !root.attrs.iter().any(|(k, _)| *k == "xmlns:xlink") {
        root.attrs
            .push(("xmlns:xlink", "http://www.w3.org/1999/xlink".to_string()));
    }

    let mut out = String::with_capacity(raw.len().min(MAX_DIAGRAM_SOURCE_BYTES) + 64);
    serialize_sanitized_svg(&root, &mut out);
    Ok((out, width, height))
}

fn svg_node_uses_xlink(node: &SanitizedSvgNode) -> bool {
    if node.attrs.iter().any(|(k, _)| *k == "xlink:href") {
        return true;
    }
    node.children.iter().any(|child| match child {
        SanitizedSvgChild::Elem(el) => svg_node_uses_xlink(el),
        SanitizedSvgChild::Text(_) => false,
    })
}

fn extract_sanitized_svg_dimensions(root: &SanitizedSvgNode) -> Option<(f32, f32)> {
    if let Some((_, vb)) = root.attrs.iter().find(|(k, _)| *k == "viewBox") {
        let nums: Vec<f32> = vb
            .split(|c: char| c.is_ascii_whitespace() || c == ',')
            .filter(|p| !p.is_empty())
            .filter_map(|p| p.parse::<f32>().ok())
            .collect();
        if nums.len() >= 4 && nums[2] > 0.0 && nums[3] > 0.0 {
            return Some((nums[2], nums[3]));
        }
    }
    let w = root
        .attrs
        .iter()
        .find(|(k, _)| *k == "width")
        .and_then(|(_, v)| parse_svg_length(v));
    let h = root
        .attrs
        .iter()
        .find(|(k, _)| *k == "height")
        .and_then(|(_, v)| parse_svg_length(v));
    if let (Some(wv), Some(hv)) = (w, h)
        && wv > 0.0
        && hv > 0.0
    {
        return Some((wv, hv));
    }
    None
}

struct StrictSvgXmlParser<'a> {
    input: &'a str,
    bytes: &'a [u8],
    pos: usize,
    node_count: usize,
}

impl<'a> StrictSvgXmlParser<'a> {
    fn new(raw: &'a str) -> Self {
        let trimmed = raw.trim_start_matches('\u{feff}');
        Self {
            input: trimmed,
            bytes: trimmed.as_bytes(),
            pos: 0,
            node_count: 0,
        }
    }

    fn skip_ws(&mut self) {
        while self.pos < self.bytes.len() && self.bytes[self.pos].is_ascii_whitespace() {
            self.pos += 1;
        }
    }

    fn rest(&self) -> &'a str {
        &self.input[self.pos..]
    }

    fn skip_non_element_markup(&mut self) -> Result<bool, String> {
        let r = self.rest();
        if let Some(after) = r.strip_prefix("<!--") {
            let end = after
                .find("-->")
                .ok_or_else(|| "Unclosed XML comment in SVG".to_string())?;
            self.pos += 4 + end + 3;
            return Ok(true);
        }
        if let Some(after) = r.strip_prefix("<?") {
            let end = after
                .find("?>")
                .ok_or_else(|| "Unclosed XML processing instruction in SVG".to_string())?;
            self.pos += 2 + end + 2;
            return Ok(true);
        }
        if starts_with_ascii_ci(r, "<![cdata[") {
            let end = r[9..]
                .find("]]>")
                .ok_or_else(|| "Unclosed CDATA section in SVG".to_string())?;
            self.pos += 9 + end + 3;
            return Ok(true);
        }
        if r.starts_with("<!") {
            let end =
                find_doctype_end(r).ok_or_else(|| "Unclosed XML declaration in SVG".to_string())?;
            self.pos += end;
            return Ok(true);
        }
        Ok(false)
    }

    fn parse_single_svg_root(&mut self) -> Result<SanitizedSvgNode, String> {
        loop {
            self.skip_ws();
            if self.pos >= self.bytes.len() {
                return Err("Missing <svg> root element".into());
            }
            if self.skip_non_element_markup()? {
                continue;
            }
            if self.bytes[self.pos] != b'<' {
                return Err("Missing <svg> root element".into());
            }
            break;
        }

        let Some(root) = self.parse_element(0, true)? else {
            return Err("Missing <svg> root element".into());
        };
        if root.tag != "svg" {
            return Err("Root element must be <svg>".into());
        }
        // Intentionally stop after the single root `<svg>...</svg>` element so trailing markup
        // such as `<svg></svg><img src=x/onerror=alert(1)>` can never enter the sanitized output.
        Ok(root)
    }

    fn parse_xml_name(&mut self) -> &'a str {
        let start = self.pos;
        while self.pos < self.bytes.len() {
            let b = self.bytes[self.pos];
            if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b':' | b'.') {
                self.pos += 1;
            } else {
                break;
            }
        }
        &self.input[start..self.pos]
    }

    fn parse_element(
        &mut self,
        depth: usize,
        require_svg_root: bool,
    ) -> Result<Option<SanitizedSvgNode>, String> {
        if depth > MAX_SVG_XML_DEPTH {
            return Err("SVG element nesting exceeds safety limit".into());
        }
        if self.pos >= self.bytes.len() || self.bytes[self.pos] != b'<' {
            return Ok(None);
        }
        self.pos += 1; // consume `<`

        let raw_tag_name = self.parse_xml_name();
        if raw_tag_name.is_empty() {
            // Malformed `<` inside markup (e.g. `<scr<script>`): skip to next `>` or `<`
            self.skip_malformed_tag_fragment();
            return Ok(None);
        }
        let local_name = raw_tag_name
            .rsplit_once(':')
            .map_or(raw_tag_name, |(_, local)| local);
        let canonical_tag = canonical_allowed_svg_tag(local_name);
        if require_svg_root && canonical_tag != Some("svg") {
            return Err("Missing <svg> root element".into());
        }

        let (raw_attrs, self_closing) = self.parse_tag_attributes()?;

        let Some(allowed_tag) = canonical_tag else {
            if !self_closing {
                self.discard_subtree(raw_tag_name, depth + 1)?;
            }
            return Ok(None);
        };

        self.node_count += 1;
        if self.node_count > MAX_SVG_XML_NODES {
            return Err("SVG element count exceeds safety limit".into());
        }

        let mut safe_attrs: Vec<(&'static str, String)> = Vec::new();
        for (raw_name, raw_val) in raw_attrs {
            if let Some((canon_attr, clean_val)) =
                sanitize_svg_attr(allowed_tag, &raw_name, &raw_val)
            {
                if let Some(existing) = safe_attrs.iter_mut().find(|(k, _)| *k == canon_attr) {
                    existing.1 = clean_val;
                } else {
                    safe_attrs.push((canon_attr, clean_val));
                }
            }
        }

        let mut children = Vec::new();
        if !self_closing {
            let mut closed = false;
            while self.pos < self.bytes.len() {
                if self.bytes[self.pos] == b'<' {
                    if self.rest().starts_with("</") {
                        self.pos += 2;
                        let close_name = self.parse_xml_name();
                        self.skip_to_gt();
                        if close_name.eq_ignore_ascii_case(raw_tag_name)
                            || close_name
                                .rsplit_once(':')
                                .map_or(close_name, |(_, l)| l)
                                .eq_ignore_ascii_case(local_name)
                        {
                            closed = true;
                            break;
                        }
                        // Mismatched close tag for an inner unclosed/malformed tag; keep scanning
                        continue;
                    }
                    if self.skip_non_element_markup()? {
                        continue;
                    }
                    let prev_pos = self.pos;
                    if let Some(child_elem) = self.parse_element(depth + 1, false)? {
                        children.push(SanitizedSvgChild::Elem(child_elem));
                    } else if self.pos == prev_pos {
                        self.pos += 1;
                    }
                } else {
                    let text_start = self.pos;
                    while self.pos < self.bytes.len() && self.bytes[self.pos] != b'<' {
                        self.pos += 1;
                    }
                    let decoded = decode_xml_entities(&self.input[text_start..self.pos]);
                    if !decoded.is_empty() {
                        children.push(SanitizedSvgChild::Text(decoded));
                    }
                }
            }
            if require_svg_root && !closed {
                return Err("Unclosed <svg> root element".into());
            }
        }

        Ok(Some(SanitizedSvgNode {
            tag: allowed_tag,
            attrs: safe_attrs,
            children,
        }))
    }

    fn parse_tag_attributes(&mut self) -> Result<(Vec<(String, String)>, bool), String> {
        let mut attrs = Vec::new();
        let mut self_closing = false;

        while self.pos < self.bytes.len() {
            self.skip_ws();
            if self.pos >= self.bytes.len() {
                return Err("Unclosed SVG tag".into());
            }
            let b = self.bytes[self.pos];
            if b == b'>' {
                self.pos += 1;
                break;
            }
            if b == b'/' {
                self.pos += 1;
                if self.pos < self.bytes.len() && self.bytes[self.pos] == b'>' {
                    self.pos += 1;
                    self_closing = true;
                    break;
                }
                // Bare `/` acting as HTML5-style attribute separator (e.g. `<svg/onload=...>`)
                continue;
            }
            if b == b'<' {
                // Malformed nested `<` inside tag (e.g. `<scr<script>`) — stop this broken tag
                break;
            }

            let name = self.parse_xml_name();
            if name.is_empty() {
                // Advance safely by one full UTF-8 character so parser never stalls
                let ch_len = self.rest().chars().next().map_or(1, char::len_utf8);
                self.pos += ch_len;
                continue;
            }

            self.skip_ws();
            if self.pos < self.bytes.len() && self.bytes[self.pos] == b'=' {
                self.pos += 1;
                self.skip_ws();
                let value = if self.pos < self.bytes.len()
                    && (self.bytes[self.pos] == b'"' || self.bytes[self.pos] == b'\'')
                {
                    let q = self.bytes[self.pos];
                    self.pos += 1;
                    let val_start = self.pos;
                    while self.pos < self.bytes.len() && self.bytes[self.pos] != q {
                        self.pos += 1;
                    }
                    let slice = &self.input[val_start..self.pos];
                    if self.pos < self.bytes.len() {
                        self.pos += 1; // consume closing quote
                    }
                    decode_xml_entities(slice)
                } else {
                    let val_start = self.pos;
                    while self.pos < self.bytes.len()
                        && !self.bytes[self.pos].is_ascii_whitespace()
                        && !matches!(self.bytes[self.pos], b'>' | b'/')
                    {
                        self.pos += 1;
                    }
                    decode_xml_entities(&self.input[val_start..self.pos])
                };
                attrs.push((name.to_string(), value));
            } else {
                // Boolean attribute with no value
                attrs.push((name.to_string(), String::new()));
            }
        }

        Ok((attrs, self_closing))
    }

    fn discard_subtree(&mut self, tag_name: &str, depth: usize) -> Result<(), String> {
        if depth > MAX_SVG_XML_DEPTH + 16 {
            self.pos = self.bytes.len();
            return Ok(());
        }
        let mut nesting = 1usize;
        while self.pos < self.bytes.len() {
            let Some(rel_lt) = self.rest().find('<') else {
                self.pos = self.bytes.len();
                break;
            };
            self.pos += rel_lt;
            if self.skip_non_element_markup()? {
                continue;
            }
            if self.rest().starts_with("</") {
                self.pos += 2;
                let close_name = self.parse_xml_name();
                self.skip_to_gt();
                if close_name.eq_ignore_ascii_case(tag_name) {
                    nesting = nesting.saturating_sub(1);
                    if nesting == 0 {
                        break;
                    }
                }
                continue;
            }
            self.pos += 1;
            let open_name = self.parse_xml_name();
            if open_name.is_empty() {
                self.skip_malformed_tag_fragment();
                continue;
            }
            let (_, self_closing) = self.parse_tag_attributes()?;
            if !self_closing && open_name.eq_ignore_ascii_case(tag_name) {
                nesting += 1;
            }
        }
        Ok(())
    }

    fn skip_malformed_tag_fragment(&mut self) {
        while self.pos < self.bytes.len() {
            let b = self.bytes[self.pos];
            if b == b'>' {
                self.pos += 1;
                break;
            }
            if b == b'<' {
                break;
            }
            self.pos += 1;
        }
    }

    fn skip_to_gt(&mut self) {
        while self.pos < self.bytes.len() {
            let b = self.bytes[self.pos];
            self.pos += 1;
            if b == b'>' {
                break;
            }
        }
    }
}

fn canonical_allowed_svg_tag(local_name: &str) -> Option<&'static str> {
    match local_name.to_ascii_lowercase().as_str() {
        "svg" => Some("svg"),
        "g" => Some("g"),
        "defs" => Some("defs"),
        "symbol" => Some("symbol"),
        "use" => Some("use"),
        "title" => Some("title"),
        "desc" => Some("desc"),
        "path" => Some("path"),
        "rect" => Some("rect"),
        "circle" => Some("circle"),
        "ellipse" => Some("ellipse"),
        "line" => Some("line"),
        "polyline" => Some("polyline"),
        "polygon" => Some("polygon"),
        "text" => Some("text"),
        "tspan" => Some("tspan"),
        "lineargradient" => Some("linearGradient"),
        "radialgradient" => Some("radialGradient"),
        "stop" => Some("stop"),
        "clippath" => Some("clipPath"),
        "mask" => Some("mask"),
        "pattern" => Some("pattern"),
        "marker" => Some("marker"),
        "filter" => Some("filter"),
        "feblend" => Some("feBlend"),
        "fecolormatrix" => Some("feColorMatrix"),
        "fecomponenttransfer" => Some("feComponentTransfer"),
        "fecomposite" => Some("feComposite"),
        "feconvolvematrix" => Some("feConvolveMatrix"),
        "fediffuselighting" => Some("feDiffuseLighting"),
        "fedisplacementmap" => Some("feDisplacementMap"),
        "fedropshadow" => Some("feDropShadow"),
        "feflood" => Some("feFlood"),
        "fefunca" => Some("feFuncA"),
        "fefuncb" => Some("feFuncB"),
        "fefuncg" => Some("feFuncG"),
        "fefuncr" => Some("feFuncR"),
        "fegaussianblur" => Some("feGaussianBlur"),
        "femerge" => Some("feMerge"),
        "femergenode" => Some("feMergeNode"),
        "femorphology" => Some("feMorphology"),
        "feoffset" => Some("feOffset"),
        "fespecularlighting" => Some("feSpecularLighting"),
        "fetile" => Some("feTile"),
        "feturbulence" => Some("feTurbulence"),
        _ => None,
    }
}

fn canonical_allowed_svg_attr(raw_name: &str) -> Option<&'static str> {
    let lower = raw_name.to_ascii_lowercase();
    if lower.starts_with("on") {
        return None;
    }
    match lower.as_str() {
        "xmlns" => Some("xmlns"),
        "xmlns:xlink" => Some("xmlns:xlink"),
        "id" => Some("id"),
        "class" => Some("class"),
        "viewbox" => Some("viewBox"),
        "width" => Some("width"),
        "height" => Some("height"),
        "x" => Some("x"),
        "y" => Some("y"),
        "x1" => Some("x1"),
        "y1" => Some("y1"),
        "x2" => Some("x2"),
        "y2" => Some("y2"),
        "cx" => Some("cx"),
        "cy" => Some("cy"),
        "r" => Some("r"),
        "rx" => Some("rx"),
        "ry" => Some("ry"),
        "d" => Some("d"),
        "points" => Some("points"),
        "dx" => Some("dx"),
        "dy" => Some("dy"),
        "rotate" => Some("rotate"),
        "textlength" => Some("textLength"),
        "lengthadjust" => Some("lengthAdjust"),
        "preserveaspectratio" => Some("preserveAspectRatio"),
        "transform" => Some("transform"),
        "gradienttransform" => Some("gradientTransform"),
        "patterntransform" => Some("patternTransform"),
        "gradientunits" => Some("gradientUnits"),
        "patternunits" => Some("patternUnits"),
        "patterncontentunits" => Some("patternContentUnits"),
        "clippathunits" => Some("clipPathUnits"),
        "maskunits" => Some("maskUnits"),
        "maskcontentunits" => Some("maskContentUnits"),
        "filterunits" => Some("filterUnits"),
        "primitiveunits" => Some("primitiveUnits"),
        "markerwidth" => Some("markerWidth"),
        "markerheight" => Some("markerHeight"),
        "markerunits" => Some("markerUnits"),
        "refx" => Some("refX"),
        "refy" => Some("refY"),
        "orient" => Some("orient"),
        "offset" => Some("offset"),
        "fx" => Some("fx"),
        "fy" => Some("fy"),
        "fr" => Some("fr"),
        "spreadmethod" => Some("spreadMethod"),
        "fill" => Some("fill"),
        "fill-opacity" => Some("fill-opacity"),
        "fill-rule" => Some("fill-rule"),
        "stroke" => Some("stroke"),
        "stroke-width" => Some("stroke-width"),
        "stroke-opacity" => Some("stroke-opacity"),
        "stroke-linecap" => Some("stroke-linecap"),
        "stroke-linejoin" => Some("stroke-linejoin"),
        "stroke-miterlimit" => Some("stroke-miterlimit"),
        "stroke-dasharray" => Some("stroke-dasharray"),
        "stroke-dashoffset" => Some("stroke-dashoffset"),
        "opacity" => Some("opacity"),
        "stop-color" => Some("stop-color"),
        "stop-opacity" => Some("stop-opacity"),
        "color" => Some("color"),
        "visibility" => Some("visibility"),
        "display" => Some("display"),
        "overflow" => Some("overflow"),
        "vector-effect" => Some("vector-effect"),
        "shape-rendering" => Some("shape-rendering"),
        "text-rendering" => Some("text-rendering"),
        "image-rendering" => Some("image-rendering"),
        "color-interpolation" => Some("color-interpolation"),
        "color-interpolation-filters" => Some("color-interpolation-filters"),
        "mix-blend-mode" => Some("mix-blend-mode"),
        "isolation" => Some("isolation"),
        "paint-order" => Some("paint-order"),
        "font-family" => Some("font-family"),
        "font-size" => Some("font-size"),
        "font-weight" => Some("font-weight"),
        "font-style" => Some("font-style"),
        "font-variant" => Some("font-variant"),
        "letter-spacing" => Some("letter-spacing"),
        "word-spacing" => Some("word-spacing"),
        "text-anchor" => Some("text-anchor"),
        "dominant-baseline" => Some("dominant-baseline"),
        "alignment-baseline" => Some("alignment-baseline"),
        "baseline-shift" => Some("baseline-shift"),
        "text-decoration" => Some("text-decoration"),
        "writing-mode" => Some("writing-mode"),
        "direction" => Some("direction"),
        "unicode-bidi" => Some("unicode-bidi"),
        "xml:space" => Some("xml:space"),
        "clip-path" => Some("clip-path"),
        "mask" => Some("mask"),
        "filter" => Some("filter"),
        "marker-start" => Some("marker-start"),
        "marker-mid" => Some("marker-mid"),
        "marker-end" => Some("marker-end"),
        "href" => Some("href"),
        "xlink:href" => Some("xlink:href"),
        "in" => Some("in"),
        "in2" => Some("in2"),
        "result" => Some("result"),
        "stddeviation" => Some("stdDeviation"),
        "mode" => Some("mode"),
        "type" => Some("type"),
        "values" => Some("values"),
        "slope" => Some("slope"),
        "intercept" => Some("intercept"),
        "amplitude" => Some("amplitude"),
        "exponent" => Some("exponent"),
        "tablevalues" => Some("tableValues"),
        "operator" => Some("operator"),
        "k1" => Some("k1"),
        "k2" => Some("k2"),
        "k3" => Some("k3"),
        "k4" => Some("k4"),
        "order" => Some("order"),
        "kernelmatrix" => Some("kernelMatrix"),
        "divisor" => Some("divisor"),
        "bias" => Some("bias"),
        "targetx" => Some("targetX"),
        "targety" => Some("targetY"),
        "edgemode" => Some("edgeMode"),
        "surfacescale" => Some("surfaceScale"),
        "diffuseconstant" => Some("diffuseConstant"),
        "specularconstant" => Some("specularConstant"),
        "specularexponent" => Some("specularExponent"),
        "scale" => Some("scale"),
        "xchannelselector" => Some("xChannelSelector"),
        "ychannelselector" => Some("yChannelSelector"),
        "flood-color" => Some("flood-color"),
        "flood-opacity" => Some("flood-opacity"),
        "radius" => Some("radius"),
        "basefrequency" => Some("baseFrequency"),
        "numoctaves" => Some("numOctaves"),
        "seed" => Some("seed"),
        "stitchtiles" => Some("stitchTiles"),
        "role" => Some("role"),
        "aria-label" => Some("aria-label"),
        "aria-labelledby" => Some("aria-labelledby"),
        "aria-describedby" => Some("aria-describedby"),
        "aria-hidden" => Some("aria-hidden"),
        "focusable" => Some("focusable"),
        _ => None,
    }
}

fn is_safe_local_fragment_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
}

fn sanitize_svg_attr(
    tag: &'static str,
    raw_name: &str,
    decoded_val: &str,
) -> Option<(&'static str, String)> {
    let canon = canonical_allowed_svg_attr(raw_name)?;
    let trimmed = decoded_val.trim();

    match canon {
        "xmlns" => {
            if tag == "svg" && trimmed == "http://www.w3.org/2000/svg" {
                return Some((canon, trimmed.to_string()));
            }
            return None;
        }
        "xmlns:xlink" => {
            if tag == "svg" && trimmed == "http://www.w3.org/1999/xlink" {
                return Some((canon, trimmed.to_string()));
            }
            return None;
        }
        "id" => {
            if is_safe_local_fragment_id(trimmed) {
                return Some((canon, trimmed.to_string()));
            }
            return None;
        }
        "href" | "xlink:href" => {
            // Only allow local fragment references (`#id`) on safe reuse/paint-server tags
            if !matches!(tag, "use" | "linearGradient" | "radialGradient" | "pattern") {
                return None;
            }
            let frag = trimmed.strip_prefix('#')?;
            if is_safe_local_fragment_id(frag) {
                return Some((canon, format!("#{frag}")));
            }
            return None;
        }
        "fill" | "stroke" | "clip-path" | "mask" | "filter" | "marker-start" | "marker-mid"
        | "marker-end" => {
            let compact_lower: String = trimmed
                .chars()
                .filter(|c| !c.is_ascii_whitespace() && !c.is_control())
                .map(|c| c.to_ascii_lowercase())
                .collect();
            if compact_lower.contains('\\')
                || compact_lower.contains("javascript:")
                || compact_lower.contains("vbscript:")
                || compact_lower.contains("expression(")
                || compact_lower.contains("data:")
            {
                return None;
            }
            if compact_lower.contains("url(") {
                let inner = compact_lower.strip_prefix("url(")?.strip_suffix(')')?;
                let unquoted = inner.trim_matches(|c| c == '"' || c == '\'');
                // Recover original case fragment from `trimmed`
                let raw_open = find_ascii_ci(trimmed, "url(")?;
                let raw_close = trimmed.rfind(')')?;
                if raw_close <= raw_open + 4 {
                    return None;
                }
                let raw_inner = trimmed[raw_open + 4..raw_close]
                    .trim()
                    .trim_matches(|c| c == '"' || c == '\'')
                    .trim();
                let frag = raw_inner.strip_prefix('#')?;
                if unquoted.starts_with('#') && is_safe_local_fragment_id(frag) {
                    return Some((canon, format!("url(#{frag})")));
                }
                return None;
            }
        }
        _ => {
            let compact_lower: String = trimmed
                .chars()
                .filter(|c| !c.is_ascii_whitespace() && !c.is_control())
                .map(|c| c.to_ascii_lowercase())
                .collect();
            if compact_lower.contains("javascript:")
                || compact_lower.contains("vbscript:")
                || compact_lower.contains("expression(")
                || compact_lower.contains("-moz-binding")
                || compact_lower.contains("url(")
            {
                return None;
            }
        }
    }

    Some((canon, trimmed.to_string()))
}

fn decode_xml_entities(raw: &str) -> String {
    if !raw.contains('&') {
        return raw
            .chars()
            .filter(|c| !is_illegal_xml_control(*c))
            .collect();
    }
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(amp) = rest.find('&') {
        for ch in rest[..amp].chars() {
            if !is_illegal_xml_control(ch) {
                out.push(ch);
            }
        }
        let after_amp = &rest[amp + 1..];
        if let Some(semi) = after_amp.find(';')
            && semi <= 12
        {
            let entity = &after_amp[..semi];
            let decoded_char = match entity {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                _ if entity.starts_with("#x") || entity.starts_with("#X") => {
                    u32::from_str_radix(&entity[2..], 16)
                        .ok()
                        .and_then(char::from_u32)
                }
                _ if entity.starts_with('#') => {
                    entity[1..].parse::<u32>().ok().and_then(char::from_u32)
                }
                _ => None,
            };
            if let Some(ch) = decoded_char {
                if !is_illegal_xml_control(ch) {
                    out.push(ch);
                }
                rest = &after_amp[semi + 1..];
                continue;
            }
        }
        out.push('&');
        rest = after_amp;
    }
    for ch in rest.chars() {
        if !is_illegal_xml_control(ch) {
            out.push(ch);
        }
    }
    out
}

fn is_illegal_xml_control(ch: char) -> bool {
    let cp = ch as u32;
    cp <= 0x08 || cp == 0x0B || cp == 0x0C || (0x0E..=0x1F).contains(&cp)
}

fn serialize_sanitized_svg(node: &SanitizedSvgNode, out: &mut String) {
    out.push('<');
    out.push_str(node.tag);
    for (name, val) in &node.attrs {
        out.push(' ');
        out.push_str(name);
        out.push_str("=\"");
        push_xml_escaped(out, val);
        out.push('"');
    }
    if node.children.is_empty() && node.tag != "svg" {
        out.push_str("/>");
        return;
    }
    out.push('>');
    for child in &node.children {
        match child {
            SanitizedSvgChild::Elem(el) => serialize_sanitized_svg(el, out),
            SanitizedSvgChild::Text(t) => push_xml_escaped(out, t),
        }
    }
    out.push_str("</");
    out.push_str(node.tag);
    out.push('>');
}

fn push_xml_escaped(out: &mut String, raw: &str) {
    for ch in raw.chars() {
        if is_illegal_xml_control(ch) {
            continue;
        }
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
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

    #[test]
    fn polished_mermaid_graphviz_and_svg_preserve_refined_attributes() {
        let mmd = render_diagram(&RenderDiagramRequest {
            kind: DiagramKind::Mermaid,
            source: "flowchart LR\n  A[Client] -->|RPC| B[Daemon]".into(),
            theme: DiagramTheme::Light,
        });
        let mmd_svg = mmd.svg.expect("mermaid svg");
        assert!(
            mmd_svg.contains("rx=\"8\" ry=\"8\""),
            "expected refined node corner rounding"
        );
        assert!(
            mmd_svg.contains("#F8FAFC"),
            "expected gentle light tinted node fill"
        );
        assert!(
            mmd_svg.contains("#CBD5E1"),
            "expected crisp light hairline border"
        );
        assert!(mmd_svg.contains("text-rendering: optimizeLegibility"));

        let dot_dark = render_diagram(&RenderDiagramRequest {
            kind: DiagramKind::Graphviz,
            source: "digraph G { node [shape=box]; A -> B; }".into(),
            theme: DiagramTheme::Dark,
        });
        let dot_svg = dot_dark.svg.expect("graphviz dark svg");
        assert!(
            dot_svg.contains("fill: #F4F4F5"),
            "dark graphviz labels must be legible"
        );
        assert!(
            !dot_svg.contains("context-stroke"),
            "arrowheads must use explicit theme stroke"
        );
        assert!(
            dot_svg.contains("rx=\"7\" ry=\"7\""),
            "default boxes should have soft corners"
        );
        assert!(
            dot_svg.contains("#3A4054"),
            "dark node border should use refined slate"
        );
        assert!(
            dot_svg.contains("#8B95AB"),
            "dark edge stroke should use refined slate"
        );

        let rich_svg = r##"<svg viewBox="0 0 120 60" xmlns="http://www.w3.org/2000/svg">
  <rect x="4" y="4" width="112" height="52" rx="10" ry="10" fill="#6366F1" fill-opacity="0.15" stroke="#6366F1" stroke-opacity="0.8" stroke-linecap="round" stroke-linejoin="round" opacity="0.95"/>
  <a xlink:href="data:image/svg+xml,<svg onload=alert(1)/>"><text x="20" y="34">Safe</text></a>
</svg>"##;
        let clean = render_diagram(&RenderDiagramRequest {
            kind: DiagramKind::Svg,
            source: rich_svg.into(),
            theme: DiagramTheme::Light,
        });
        let clean_svg = clean.svg.expect("sanitized rich svg");
        for attr in [
            "rx=\"10\"",
            "ry=\"10\"",
            "fill-opacity=\"0.15\"",
            "stroke-opacity=\"0.8\"",
            "stroke-linecap=\"round\"",
            "stroke-linejoin=\"round\"",
            "opacity=\"0.95\"",
        ] {
            assert!(
                clean_svg.contains(attr),
                "missing preserved attr {attr} in {clean_svg}"
            );
        }
        assert!(
            !clean_svg
                .to_ascii_lowercase()
                .contains("data:image/svg+xml")
        );
        // Verify scoped `#orb_root_` style selectors on both Mermaid and Graphviz
        assert!(
            mmd_svg.contains("id=\"orb_root_"),
            "mermaid svg must have root id"
        );
        assert!(
            mmd_svg.contains("#orb_root_"),
            "mermaid <style> selectors must be scoped under root id"
        );
        assert!(
            dot_svg.contains("id=\"orb_root_"),
            "graphviz svg must have root id"
        );
        assert!(
            dot_svg.contains("#orb_root_"),
            "graphviz <style> selectors must be scoped under root id"
        );
        assert!(
            !dot_svg.contains("\ntext, tspan"),
            "graphviz <style> must not leak unscoped global text selector"
        );
    }

    #[test]
    fn allowlist_svg_sanitizer_blocks_all_xss_bypasses_and_preserves_cjk_attrs() {
        // 1. Slash-separated event handler (`<svg/onload=...>`)
        let p1 = render_diagram(&RenderDiagramRequest {
            kind: DiagramKind::Svg,
            source:
                r#"<svg/onload=alert(1) width="100" height="50"><rect width="40" height="20"/></svg>"#
                    .into(),
            theme: DiagramTheme::Light,
        });
        let s1 = p1.svg.expect("p1 svg");
        assert!(!s1.to_ascii_lowercase().contains("onload"));
        assert!(!s1.contains("alert(1)"));
        assert!(s1.contains("<rect"));

        // 2. Comment quote spoofing (`<!-- " --><svg onload=...>`)
        let p2 = render_diagram(&RenderDiagramRequest {
            kind: DiagramKind::Svg,
            source: r#"<!-- " --><svg width="100" height="50" onload="alert(2)"><circle cx="10" cy="10" r="5"/></svg>"#.into(),
            theme: DiagramTheme::Light,
        });
        let s2 = p2.svg.expect("p2 svg");
        assert!(!s2.to_ascii_lowercase().contains("onload"));
        assert!(!s2.contains("alert(2)"));
        assert!(s2.contains("<circle"));

        // 3. Numeric entity obfuscated javascript URI (`&#106;avascript:...`)
        let p3 = render_diagram(&RenderDiagramRequest {
            kind: DiagramKind::Svg,
            source: r##"<svg viewBox="0 0 100 50"><a href="&#106;avascript:alert(3)"><text x="5" y="20">click</text></a><use href="&#106;avascript:alert(3)"/></svg>"##.into(),
            theme: DiagramTheme::Light,
        });
        let s3 = p3.svg.expect("p3 svg");
        assert!(!s3.to_ascii_lowercase().contains("javascript:"));
        assert!(!s3.to_ascii_lowercase().contains("<a"));
        assert!(!s3.contains("alert(3)"));

        // 4. Nested tag reassembly (`<scr<script></script>ipt>`)
        let p4 = render_diagram(&RenderDiagramRequest {
            kind: DiagramKind::Svg,
            source: r#"<svg viewBox="0 0 100 50"><scr<script></script>ipt>alert(4)</scr</script>ipt><rect width="10" height="10"/></svg>"#.into(),
            theme: DiagramTheme::Light,
        });
        let s4 = p4.svg.expect("p4 svg");
        assert!(!s4.to_ascii_lowercase().contains("<script"));
        assert!(!s4.contains("alert(4)"));
        assert!(s4.contains("<rect"));

        // 5. Trailing markup after `</svg>` (`<svg></svg><img src=x/onerror=...>`)
        let p5 = render_diagram(&RenderDiagramRequest {
            kind: DiagramKind::Svg,
            source: r#"<svg viewBox="0 0 50 50"><rect width="10" height="10"/></svg><img src=x/onerror=alert(5)>"#.into(),
            theme: DiagramTheme::Light,
        });
        let s5 = p5.svg.expect("p5 svg");
        assert!(!s5.to_ascii_lowercase().contains("<img"));
        assert!(!s5.to_ascii_lowercase().contains("onerror"));
        assert!(s5.ends_with("</svg>"));

        // 6. CSS `url()` escapes & external `url()` inside presentation attributes + CJK attribute preservation
        let p6 = render_diagram(&RenderDiagramRequest {
            kind: DiagramKind::Svg,
            source: r##"<svg viewBox="0 0 120 60" aria-label="架构图"><defs><linearGradient id="g1"><stop offset="0%" stop-color="#38BDF8"/></linearGradient></defs><rect x="2" y="2" width="100" height="40" fill="url(#g1)" stroke="\75rl(https://evil.test/s)" filter="url(https://evil.test/f.svg#x)"/><text x="10" y="25">系统架构</text></svg>"##.into(),
            theme: DiagramTheme::Light,
        });
        let s6 = p6.svg.expect("p6 svg");
        assert!(
            s6.contains("aria-label=\"架构图\""),
            "CJK attribute must survive intact: {s6}"
        );
        assert!(
            s6.contains("系统架构"),
            "CJK text must survive intact: {s6}"
        );
        assert!(
            s6.contains("fill=\"url(#orb_"),
            "local fragment url(#id) must be preserved"
        );
        assert!(
            !s6.contains("evil.test"),
            "external & escaped url() must be stripped: {s6}"
        );
    }

    #[test]
    fn graphviz_polish_preserves_text_payload_and_avoids_duplicate_ry() {
        let dot = r#"digraph G { node [shape=box]; A [label="rx=\"0\" text"]; A -> B; }"#;
        let out = render_diagram(&RenderDiagramRequest {
            kind: DiagramKind::Graphviz,
            source: dot.into(),
            theme: DiagramTheme::Light,
        });
        let svg = out.svg.expect("graphviz svg");
        assert!(!svg.contains("ry=\"7\" ry=\""));
        assert!(!svg.contains("rx=&quot;7&quot;"));
    }
}
