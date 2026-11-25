use std::sync::{Arc, LazyLock};

use gpui::Rgba;
use merman::svg::export::NativeFontOptions;
use merman::svg::{
    CssOverridePolicy, HostTheme, HostThemeAppearance, Presentation, SvgOutputPolicy,
    SvgPipelinePreset, ThemeRole,
};
use merman::{
    Engine, OperationControl, RenderOutput, RenderRequest, Renderer, SvgEnvironment, SvgRequest,
};

use crate::app::components::palette::{Appearance, Colors, TEXT_FONT, font_files};
use crate::app::components::svg_picture;

/// A diagram drawn as SVG for one appearance. Only this text is kept between steps;
/// bitmaps are made from it again when the diagram is on screen.
#[derive(Debug, Clone)]
pub struct Svg {
    text: Arc<str>,
    width: f32,
    height: f32,
}

impl Svg {
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Logical pixels, the unit of the app layout.
    pub fn width(&self) -> f32 {
        self.width
    }

    pub fn height(&self) -> f32 {
        self.height
    }

    pub(super) fn tree(&self) -> Result<usvg::Tree, String> {
        svg_picture::parse(self.text.as_bytes()).map_err(|e| e.to_string())
    }
}

/// Turns Mermaid text into SVG in our colours. Loading the font is the slow part,
/// so there is one drawer per appearance for the life of the process.
pub struct DiagramDrawer {
    setup: Result<Setup, String>,
}

struct Setup {
    renderer: Renderer,
    request: SvgRequest,
}

static LIGHT: LazyLock<DiagramDrawer> = LazyLock::new(|| DiagramDrawer::new(Appearance::Light));
static DARK: LazyLock<DiagramDrawer> = LazyLock::new(|| DiagramDrawer::new(Appearance::Dark));

impl DiagramDrawer {
    pub fn for_appearance(appearance: Appearance) -> &'static Self {
        match appearance {
            Appearance::Light => &LIGHT,
            Appearance::Dark => &DARK,
        }
    }

    fn new(appearance: Appearance) -> Self {
        Self {
            setup: Setup::new(appearance),
        }
    }

    pub fn svg(&self, source: &str) -> Result<Svg, String> {
        let setup = self
            .setup
            .as_ref()
            .map_err(|reason| format!("the diagram drawer could not start: {reason}"))?;
        setup.svg(source)
    }
}

impl Setup {
    fn new(appearance: Appearance) -> Result<Self, String> {
        let colors = Colors::of(appearance);
        let output = SvgOutputPolicy {
            preset: SvgPipelinePreset::ResvgSafe,
            css_override_policy: CssOverridePolicy::StripExistingImportant,
            root_background_color: None,
            ..SvgOutputPolicy::default()
        };
        let presentation = Presentation::new()
            .with_theme(host_theme(appearance, &colors)?)
            .resolve();
        Ok(Self {
            renderer: Renderer::new().with_engine(presentation.materialize_engine(Engine::new())),
            request: SvgRequest {
                pipeline: Some(output.pipeline()),
                presentation: presentation.render_policy(),
                environment: measured_with_app_font()?,
                ..Default::default()
            },
        })
    }

    fn svg(&self, source: &str) -> Result<Svg, String> {
        let output = self
            .renderer
            .render(RenderRequest::svg(
                source,
                OperationControl::new(),
                self.request.clone(),
            ))
            .map_err(|error| error.to_string())?;
        let RenderOutput::Svg(Some(svg)) = output else {
            return Err("no Mermaid diagram found".to_owned());
        };
        let text: Arc<str> = svg.svg().into();
        let tree = svg_picture::parse(text.as_bytes()).map_err(|e| e.to_string())?;
        let (width, height) = drawn_size(&tree);
        Ok(Svg {
            text,
            width,
            height,
        })
    }
}

/// Merman can leave an invisible dot far below or right of the shapes, and sizes the SVG to reach
/// it: the page then shows an empty band under the diagram. The size ends at the shapes instead,
/// with merman's own margin; drawing at this size cuts the band off.
fn drawn_size(tree: &usvg::Tree) -> (f32, f32) {
    let size = tree.size();
    let everything = tree.root().abs_stroke_bounding_box();
    let Some((right, bottom)) = far_corner_of_shapes(tree.root()) else {
        return (size.width(), size.height());
    };
    let margin_right = (size.width() - everything.right()).max(0.);
    let margin_bottom = (size.height() - everything.bottom()).max(0.);
    (
        (right + margin_right).min(size.width()),
        (bottom + margin_bottom).min(size.height()),
    )
}

const SMALLEST_SHAPE: f32 = 1.;

fn far_corner_of_shapes(group: &usvg::Group) -> Option<(f32, f32)> {
    group
        .children()
        .iter()
        .filter_map(|node| {
            if let usvg::Node::Group(group) = node {
                return far_corner_of_shapes(group);
            }
            let bounds = node.abs_stroke_bounding_box();
            (bounds.width().max(bounds.height()) >= SMALLEST_SHAPE)
                .then(|| (bounds.right(), bounds.bottom()))
        })
        .reduce(|(right, bottom), (other_right, other_bottom)| {
            (right.max(other_right), bottom.max(other_bottom))
        })
}

/// Merman places boxes around the text it measures; with any other font the labels overflow them.
fn measured_with_app_font() -> Result<SvgEnvironment, String> {
    let files = font_files(*TEXT_FONT);
    if files.is_empty() {
        return Err(format!("no file holds the app font {}", *TEXT_FONT));
    }
    let fonts = files
        .into_iter()
        .fold(
            NativeFontOptions::default(),
            NativeFontOptions::with_font_file,
        )
        .load(&OperationControl::new())
        .map_err(|error| format!("the app font did not load: {error}"))?;
    SvgEnvironment::deterministic()
        .with_native_font_context(fonts)
        .map_err(|error| format!("the app font did not load into merman: {error}"))
}

fn host_theme(appearance: Appearance, colors: &Colors) -> Result<HostTheme, String> {
    let roles = [
        (ThemeRole::Canvas, colors.background),
        (ThemeRole::Surface, colors.surface),
        (ThemeRole::SurfaceAlt, colors.current),
        (ThemeRole::SurfaceMuted, colors.background),
        (ThemeRole::Text, colors.text),
        (ThemeRole::SubtleText, colors.muted_text),
        (ThemeRole::Border, colors.muted_text),
        (ThemeRole::Line, colors.muted_text),
        (ThemeRole::EdgeLabelBackground, colors.background),
        (ThemeRole::ClusterBackground, colors.background),
        (ThemeRole::ClusterBorder, colors.border),
        (ThemeRole::NoteBackground, colors.surface),
        (ThemeRole::NoteBorder, colors.warning),
        (ThemeRole::NoteText, colors.text),
        (ThemeRole::ActorBackground, colors.surface),
        (ThemeRole::ActorBorder, colors.accent),
        (ThemeRole::ActorText, colors.text),
        (ThemeRole::ActivationBackground, colors.border),
        (ThemeRole::ActivationBorder, colors.accent),
        (ThemeRole::Error, colors.danger),
        (ThemeRole::Warning, colors.warning),
        (ThemeRole::Success, colors.success),
    ];
    let theme = HostTheme::new()
        .with_appearance(match appearance {
            Appearance::Light => HostThemeAppearance::Light,
            Appearance::Dark => HostThemeAppearance::Dark,
        })
        .try_with_font_family(*TEXT_FONT)
        .map_err(|error| format!("the app font name is not a font family: {error}"))?;
    let theme = roles.into_iter().try_fold(theme, |theme, (role, color)| {
        theme
            .try_with_role(role, hex(color))
            .map_err(|error| format!("{role:?} has no valid colour: {error}"))
    })?;
    theme
        .try_with_series_palette([colors.accent, colors.success, colors.warning].map(hex))
        .map_err(|error| format!("the series palette is not valid: {error}"))
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the channel is clamped to 0..=255 first"
)]
fn channel_byte(channel: f32) -> u8 {
    (channel * 255.0).round().clamp(0.0, 255.0) as u8
}

fn hex(color: Rgba) -> String {
    let [r, g, b] = [color.r, color.g, color.b].map(channel_byte);
    format!("#{r:02x}{g:02x}{b:02x}")
}
