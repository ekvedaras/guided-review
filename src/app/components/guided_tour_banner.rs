use gpui::{
    App, ClickEvent, IntoElement, ParentElement, RenderOnce, StatefulInteractiveElement, Styled,
    Window, div,
};

use crate::viewer::GuidedTourBanner;

use super::button::{ButtonKind, ButtonSize, icon_label_button};
use super::chip::{dropping_in, notice_band};
use super::icon::Icon;
use super::palette::{Colors, Tone};

type OnSwitch = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

/// Says in the page header that the guided tour is ready, until the reviewer switches to it. The
/// window does not move when it appears.
#[derive(IntoElement)]
pub struct GuidedTourReadyBanner {
    banner: GuidedTourBanner,
    on_switch: OnSwitch,
}

impl GuidedTourReadyBanner {
    pub fn new(
        banner: GuidedTourBanner,
        on_switch: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        Self {
            banner,
            on_switch: Box::new(on_switch),
        }
    }
}

/// The switch button, drawn off with the reason beside it while the switch is refused.
pub fn switch_button(
    colors: &Colors,
    id: &'static str,
    banner: &GuidedTourBanner,
    on_switch: OnSwitch,
) -> impl IntoElement {
    let button = icon_label_button(
        colors,
        id,
        ButtonKind::Secondary,
        ButtonSize::Small,
        Icon::ArrowRight,
        banner.switch_label,
    );
    match banner.switch_refusal {
        None => div().child(button.on_click(on_switch)),
        Some(refusal) => div()
            .flex()
            .items_center()
            .gap_2()
            .child(button.opacity(0.5).cursor_not_allowed())
            .child(
                div()
                    .text_xs()
                    .text_color(colors.muted_text)
                    .child(refusal.reason()),
            ),
    }
}

impl RenderOnce for GuidedTourReadyBanner {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let colors = Colors::active(cx);
        let band = notice_band(&colors, Tone::Success)
            .py_2()
            .child(self.banner.text)
            .child(switch_button(
                &colors,
                "banner-switch-to-guided-tour",
                &self.banner,
                self.on_switch,
            ));
        dropping_in(band, "guided-tour-ready-banner")
    }
}
