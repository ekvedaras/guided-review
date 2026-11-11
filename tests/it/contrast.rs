use gpui::Rgba;
use guided_review::app::components::palette::{Appearance, Colors};
use guided_review::viewer::Capture;

const BODY_TEXT: f32 = 4.5;

fn channel(value: f32) -> f32 {
    if value <= 0.03928 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

fn luminance(color: Rgba) -> f32 {
    0.2126 * channel(color.r) + 0.7152 * channel(color.g) + 0.0722 * channel(color.b)
}

fn contrast(text: Rgba, ground: Rgba) -> f32 {
    let (a, b) = (luminance(text) + 0.05, luminance(ground) + 0.05);
    a.max(b) / a.min(b)
}

fn hex(color: Rgba) -> String {
    format!("#{:06x}", u32::from(color) >> 8)
}

/// Every pair that falls short, named, so one run lists all of them.
fn shortfalls(appearance: Appearance) -> Vec<String> {
    let colors = Colors::of(appearance);
    let mut pairs: Vec<(String, Rgba, Rgba)> = Vec::new();
    let mut on = |text_name: &str, text: Rgba, grounds: &[(&str, Rgba)]| {
        for (ground_name, ground) in grounds {
            pairs.push((format!("{text_name} on {ground_name}"), text, *ground));
        }
    };
    let page_grounds = [
        ("background", colors.background),
        ("sidebar top", colors.sidebar.start),
        ("sidebar bottom", colors.sidebar.end),
        ("surface", colors.surface),
        ("hover", colors.hover),
        ("current", colors.current),
        ("overlay", colors.overlay),
        ("page_wash top", colors.page_wash.start),
    ];
    on("text", colors.text, &page_grounds);
    on("muted_text", colors.muted_text, &page_grounds);
    on(
        "accent",
        colors.accent,
        &[
            ("background", colors.background),
            ("surface", colors.surface),
            ("current", colors.current),
            ("page_wash top", colors.page_wash.start),
            ("sidebar top", colors.sidebar.start),
            ("sidebar bottom", colors.sidebar.end),
        ],
    );
    for (name, tone, own_ground) in [
        ("success", colors.success, colors.success_background),
        ("warning", colors.warning, colors.warning_background),
        ("danger", colors.danger, colors.danger_background),
    ] {
        on(
            name,
            tone,
            &[
                ("background", colors.background),
                ("its own background", own_ground),
                ("sidebar top", colors.sidebar.start),
                ("sidebar bottom", colors.sidebar.end),
            ],
        );
    }
    on(
        "on_accent_fill",
        colors.on_accent_fill,
        &[
            ("accent_fill top", colors.accent_fill.start),
            ("accent_fill bottom", colors.accent_fill.end),
            ("accent_fill_hover top", colors.accent_fill_hover.start),
            ("accent_fill_hover bottom", colors.accent_fill_hover.end),
            ("accent_fill_pressed", colors.accent_fill_pressed),
        ],
    );
    for capture in Capture::ALL {
        on(
            &format!("code {capture:?}"),
            colors.code(capture),
            &[
                ("background", colors.background),
                ("surface", colors.surface),
                ("added_line", colors.added_line),
                ("removed_line", colors.removed_line),
            ],
        );
    }
    on(
        "muted_text",
        colors.muted_text,
        &[
            ("added_gutter", colors.added_gutter),
            ("removed_gutter", colors.removed_gutter),
        ],
    );
    on(
        "text",
        colors.text,
        &[
            ("text_selection", colors.text_selection),
            (
                "added_word, a changed word in plain text",
                colors.added_word,
            ),
            (
                "removed_word, a changed word in plain text",
                colors.removed_word,
            ),
            (
                "current_border, the gutter of selected lines",
                colors.current_border,
            ),
        ],
    );
    let control_grounds = [
        ("raised top", colors.raised.start),
        ("raised bottom", colors.raised.end),
        ("inset", colors.inset),
    ];
    on("text", colors.text, &control_grounds);
    on("muted_text", colors.muted_text, &control_grounds);
    on("accent", colors.accent, &control_grounds);
    on(
        "danger",
        colors.danger,
        &[
            ("surface", colors.surface),
            ("raised top", colors.raised.start),
            ("overlay", colors.overlay),
        ],
    );
    on(
        "success",
        colors.success,
        &[
            ("success_fill top", colors.success_fill.start),
            ("success_fill bottom", colors.success_fill.end),
        ],
    );
    on(
        "accent",
        colors.accent,
        &[
            ("icon_tile top", colors.icon_tile.start),
            ("icon_tile bottom", colors.icon_tile.end),
        ],
    );
    on(
        "sidebar_text",
        colors.sidebar_text,
        &[
            ("sidebar top", colors.sidebar.start),
            ("sidebar bottom", colors.sidebar.end),
            ("hover", colors.hover),
            ("pressed", colors.pressed),
        ],
    );
    pairs
        .into_iter()
        .filter_map(|(name, text, ground)| {
            let ratio = contrast(text, ground);
            (ratio < BODY_TEXT)
                .then(|| format!("{name}: {} on {} is {ratio:.2}", hex(text), hex(ground)))
        })
        .collect()
}

#[test]
fn every_text_colour_of_the_light_appearance_reads_at_body_contrast() {
    assert_eq!(shortfalls(Appearance::Light), Vec::<String>::new());
}

#[test]
fn every_text_colour_of_the_dark_appearance_reads_at_body_contrast() {
    assert_eq!(shortfalls(Appearance::Dark), Vec::<String>::new());
}
