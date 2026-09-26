//! Provider logos for model rows, the status line and the header.
//!
//! Logos are bundled monochrome SVGs (Simple Icons), recolored to the brand
//! color and rasterized at the terminal's cell size with `resvg`. They are then
//! handed to the same inline-image pipeline mermaid diagrams use, so terminals
//! with a graphics protocol (kitty, iTerm2, Ghostty, WezTerm) show the real
//! logo. Everywhere else [`badge`] gives a colored text mark instead.
//!
//! Trademarks belong to their owners; the logos identify the provider.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

/// Logos are drawn in a 2-column, 1-row box so a list row keeps its height.
pub const LOGO_COLS: u16 = 2;

/// SVG plus brand color and the single-letter fallback mark.
struct Logo {
    svg: &'static str,
    color: Color,
    mark: &'static str,
}

const fn logo(svg: &'static str, rgb: (u8, u8, u8), mark: &'static str) -> Logo {
    Logo {
        svg,
        color: Color::Rgb(rgb.0, rgb.1, rgb.2),
        mark,
    }
}

/// Resolve a model-route provider label to a logo. Labels vary by route
/// ("claude", "anthropic", "awsbedrock"), so match on substrings of the
/// lowercased label, most specific first.
fn lookup(provider: &str) -> Option<&'static Logo> {
    let key = provider.to_ascii_lowercase();
    let has = |needle: &str| key.contains(needle);

    // Order matters: "githubcopilot" contains neither "openai" nor "github"
    // once normalized, but "copilot" must beat a bare "github" match, and
    // "googlegemini" must beat "google".
    static ANTHROPIC: LazyLock<Logo> = LazyLock::new(|| {
        logo(
            include_str!("../../../../assets/provider-logos/anthropic.svg"),
            (0xD9, 0x78, 0x57),
            "A",
        )
    });
    static CLAUDE: LazyLock<Logo> = LazyLock::new(|| {
        logo(
            include_str!("../../../../assets/provider-logos/claude.svg"),
            (0xD9, 0x78, 0x57),
            "C",
        )
    });
    static OPENAI: LazyLock<Logo> = LazyLock::new(|| {
        logo(
            include_str!("../../../../assets/provider-logos/openai.svg"),
            (0x41, 0x29, 0x91),
            "O",
        )
    });
    static GEMINI: LazyLock<Logo> = LazyLock::new(|| {
        logo(
            include_str!("../../../../assets/provider-logos/googlegemini.svg"),
            (0x88, 0x6F, 0xBF),
            "G",
        )
    });
    static GOOGLE: LazyLock<Logo> = LazyLock::new(|| {
        logo(
            include_str!("../../../../assets/provider-logos/google.svg"),
            (0x42, 0x85, 0xF4),
            "G",
        )
    });
    static COPILOT: LazyLock<Logo> = LazyLock::new(|| {
        logo(
            include_str!("../../../../assets/provider-logos/githubcopilot.svg"),
            (0xFF, 0xFF, 0xFF),
            "C",
        )
    });
    static OPENROUTER: LazyLock<Logo> = LazyLock::new(|| {
        logo(
            include_str!("../../../../assets/provider-logos/openrouter.svg"),
            (0x6E, 0x6E, 0x80),
            "R",
        )
    });
    static MISTRAL: LazyLock<Logo> = LazyLock::new(|| {
        logo(
            include_str!("../../../../assets/provider-logos/mistralai.svg"),
            (0xFA, 0x52, 0x0F),
            "M",
        )
    });
    static META: LazyLock<Logo> = LazyLock::new(|| {
        logo(
            include_str!("../../../../assets/provider-logos/meta.svg"),
            (0x00, 0x64, 0xE0),
            "M",
        )
    });
    static OLLAMA: LazyLock<Logo> = LazyLock::new(|| {
        logo(
            include_str!("../../../../assets/provider-logos/ollama.svg"),
            (0xFF, 0xFF, 0xFF),
            "L",
        )
    });
    static XAI: LazyLock<Logo> = LazyLock::new(|| {
        logo(
            include_str!("../../../../assets/provider-logos/x.svg"),
            (0xFF, 0xFF, 0xFF),
            "X",
        )
    });
    static PERPLEXITY: LazyLock<Logo> = LazyLock::new(|| {
        logo(
            include_str!("../../../../assets/provider-logos/perplexity.svg"),
            (0x1F, 0xB8, 0xCD),
            "P",
        )
    });
    static HUGGINGFACE: LazyLock<Logo> = LazyLock::new(|| {
        logo(
            include_str!("../../../../assets/provider-logos/huggingface.svg"),
            (0xFF, 0xD2, 0x1E),
            "H",
        )
    });

    if has("copilot") {
        return Some(&COPILOT);
    }
    if has("openrouter") {
        return Some(&OPENROUTER);
    }
    if has("claude") {
        return Some(&CLAUDE);
    }
    if has("anthropic") || has("bedrock") {
        return Some(&ANTHROPIC);
    }
    if has("openai") || has("azure") || has("codex") {
        return Some(&OPENAI);
    }
    if has("gemini") || has("antigravity") {
        return Some(&GEMINI);
    }
    if has("google") || has("vertex") {
        return Some(&GOOGLE);
    }
    if has("mistral") {
        return Some(&MISTRAL);
    }
    if has("llama") || has("meta") {
        return Some(&META);
    }
    if has("ollama") {
        return Some(&OLLAMA);
    }
    if has("grok") || has("xai") {
        return Some(&XAI);
    }
    if has("perplexity") {
        return Some(&PERPLEXITY);
    }
    if has("hugging") {
        return Some(&HUGGINGFACE);
    }
    None
}

/// Colored one-letter mark, for terminals with no graphics protocol and for
/// providers with no bundled logo (the first letter of the label).
pub fn badge(provider: &str) -> (String, Color) {
    match lookup(provider) {
        Some(logo) => (logo.mark.to_string(), logo.color),
        None => (
            provider
                .chars()
                .find(|c| c.is_alphanumeric())
                .map(|c| c.to_ascii_uppercase().to_string())
                .unwrap_or_else(|| "·".to_string()),
            Color::Gray,
        ),
    }
}

/// Cache of rasterized logos, keyed by provider label and pixel size, holding
/// the inline-image id the render pipeline draws by.
static RENDERED: LazyLock<Mutex<HashMap<(String, u16, u16), Option<u64>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Recolor the monochrome SVG and rasterize it to a PNG of `px_w` × `px_h`.
fn rasterize(logo: &Logo, px_w: u32, px_h: u32) -> Option<Vec<u8>> {
    let Color::Rgb(r, g, b) = logo.color else {
        return None;
    };
    // Simple Icons paths carry no fill, so they inherit this one.
    let tinted = logo.svg.replacen(
        "<svg ",
        &format!("<svg fill=\"#{r:02x}{g:02x}{b:02x}\" "),
        1,
    );
    let tree = usvg::Tree::from_str(&tinted, &usvg::Options::default()).ok()?;
    let size = tree.size();
    // Fit the square icon inside the box without distorting it.
    let scale = (px_w as f32 / size.width()).min(px_h as f32 / size.height());
    let (draw_w, draw_h) = (size.width() * scale, size.height() * scale);
    let mut pixmap = resvg::tiny_skia::Pixmap::new(px_w, px_h)?;
    let transform = resvg::tiny_skia::Transform::from_translate(
        (px_w as f32 - draw_w) / 2.0,
        (px_h as f32 - draw_h) / 2.0,
    )
    .pre_scale(scale, scale);
    resvg::render(&tree, transform, &mut pixmap.as_mut());
    pixmap.encode_png().ok()
}

/// Inline-image id for this provider at this cell size, rasterizing on first
/// use. `None` when there is no logo or no graphics protocol.
fn logo_id(provider: &str, cols: u16, rows: u16) -> Option<u64> {
    if !crate::tui::mermaid::image_protocol_available() {
        return None;
    }
    let key = (provider.to_ascii_lowercase(), cols, rows);
    if let Ok(cache) = RENDERED.lock()
        && let Some(cached) = cache.get(&key)
    {
        return *cached;
    }

    let id = (|| {
        let logo = lookup(provider)?;
        let (cell_w, cell_h) = crate::tui::mermaid::get_font_size()?;
        let png = rasterize(
            logo,
            u32::from(cols) * u32::from(cell_w),
            u32::from(rows) * u32::from(cell_h),
        )?;
        let data_b64 = {
            use base64::Engine as _;
            base64::engine::general_purpose::STANDARD.encode(&png)
        };
        let id = crate::tui::mermaid::inline_image_id("image/png", &data_b64);
        crate::tui::mermaid::materialize_inline_image_by_id(id, "image/png", &data_b64)?;
        Some(id)
    })();

    if let Ok(mut cache) = RENDERED.lock() {
        cache.insert(key, id);
    }
    id
}

/// Whether logos can be drawn as real images in this terminal.
pub fn graphics_available() -> bool {
    crate::tui::mermaid::image_protocol_available()
}

/// Draw the provider logo into `area`. Returns false when the caller should
/// fall back to [`badge`].
pub fn render(provider: &str, area: Rect, buf: &mut Buffer) -> bool {
    if area.width == 0 || area.height == 0 {
        return false;
    }
    let Some(id) = logo_id(provider, area.width, area.height) else {
        return false;
    };
    crate::tui::mermaid::render_image_widget(id, area, buf, true, false) > 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_labels_map_to_logos() {
        assert!(lookup("anthropic").is_some());
        assert!(lookup("Claude Pro").is_some());
        assert!(lookup("githubcopilot").is_some());
        // Specific beats general: a copilot route is not an OpenAI logo.
        assert_eq!(
            lookup("copilot").map(|l| l.mark),
            lookup("githubcopilot").map(|l| l.mark)
        );
        assert_eq!(lookup("googlegemini").map(|l| l.mark), Some("G"));
        assert!(lookup("some-local-thing").is_none());
    }

    #[test]
    fn badge_falls_back_to_first_letter() {
        assert_eq!(badge("anthropic").0, "A");
        assert_eq!(badge("deepseek").0, "D");
        assert_eq!(badge("").0, "·");
    }

    #[test]
    fn rasterize_produces_a_png_of_the_requested_size() {
        let logo = lookup("anthropic").expect("bundled logo");
        let png = rasterize(logo, 20, 10).expect("rasterized");
        assert_eq!(&png[0..8], b"\x89PNG\r\n\x1a\n");
        let width = u32::from_be_bytes([png[16], png[17], png[18], png[19]]);
        let height = u32::from_be_bytes([png[20], png[21], png[22], png[23]]);
        assert_eq!((width, height), (20, 10));
    }
}
