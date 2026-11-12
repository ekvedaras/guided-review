use std::time::Instant;

use gpui::{
    App, AppContext, Context, Entity, Global, InteractiveElement, IntoElement, ParentElement,
    Render, SharedString, StatefulInteractiveElement, Styled, Task, Window, div, px,
};

use crate::viewer::{Toast, ToastTone, Toasts};

use super::button::{ButtonKind, ButtonSize, icon_button};
use super::icon::{Icon, sized_icon};
use super::motion::rise_in;
use super::palette::{CARD_RADIUS, Colors, TEXT_FONT};

/// The window's toasts. It lives in a global, so a toast posted just before the PR reopens (after
/// Save in Settings) is still there in the new window.
#[derive(Clone)]
pub struct Toaster(pub Entity<ToastShelf>);

impl Global for Toaster {}

impl Toaster {
    pub fn install(cx: &mut App) {
        let shelf = cx.new(|_| ToastShelf::default());
        cx.set_global(Self(shelf));
    }
}

/// Posts a toast in whatever window shows the shelf; does nothing before the shelf is installed.
pub fn post_toast(cx: &mut App, tone: ToastTone, text: impl Into<String>) {
    let Some(Toaster(shelf)) = cx.try_global::<Toaster>().cloned() else {
        return;
    };
    let text = text.into();
    shelf.update(cx, |shelf, cx| shelf.post(tone, text, cx));
}

/// Draws the `Toasts` model and wakes when the next one is due to leave.
#[derive(Default)]
pub struct ToastShelf {
    toasts: Toasts,
    /// The toasts' texts, made once per post rather than every frame.
    texts: Vec<(u64, SharedString)>,
    wake: Option<Task<()>>,
}

impl ToastShelf {
    fn post(&mut self, tone: ToastTone, text: String, cx: &mut Context<Self>) {
        let id = self.toasts.post(tone, text.clone(), Instant::now());
        self.texts.push((id.get(), text.into()));
        self.settle(cx);
    }

    /// Drops what has expired and waits for the next to expire.
    fn settle(&mut self, cx: &mut Context<Self>) {
        let now = Instant::now();
        self.toasts.expire(now);
        let shown: Vec<u64> = self.toasts.shown().map(|toast| toast.id.get()).collect();
        self.texts.retain(|(id, _)| shown.contains(id));
        self.wake = self.toasts.next_expiry(now).map(|due| {
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(due).await;
                this.update(cx, Self::settle).ok();
            })
        });
        cx.notify();
    }

    fn hover(&mut self, is_over: bool, cx: &mut Context<Self>) {
        if is_over {
            self.toasts.pause(Instant::now());
        } else {
            self.toasts.resume(Instant::now());
        }
        self.settle(cx);
    }

    fn text_of(&self, toast: &Toast) -> SharedString {
        self.texts
            .iter()
            .find(|(id, _)| *id == toast.id.get())
            .map_or_default(|(_, text)| text.clone())
    }

    #[cfg(feature = "snapshots")]
    pub fn stage_samples(&mut self, cx: &mut Context<Self>) {
        self.post(
            ToastTone::Success,
            "Comment added to your review".to_owned(),
            cx,
        );
        self.post(ToastTone::Success, "Thread resolved".to_owned(), cx);
        self.post(
            ToastTone::Danger,
            "Could not open the IDE: no app answers idea://".to_owned(),
            cx,
        );
        self.toasts.pause(Instant::now());
        self.wake = None;
    }

    #[cfg(feature = "snapshots")]
    pub fn clear(&mut self, cx: &mut Context<Self>) {
        self.toasts = Toasts::default();
        self.texts.clear();
        self.wake = None;
        cx.notify();
    }
}

fn tone_icon(colors: &Colors, tone: ToastTone) -> impl IntoElement {
    let (icon, color) = match tone {
        ToastTone::Success => (Icon::CircleCheck, colors.success),
        ToastTone::Neutral => (Icon::CircleDot, colors.accent),
        ToastTone::Danger => (Icon::CircleAlert, colors.danger),
    };
    sized_icon(icon, color, px(16.))
}

impl Render for ToastShelf {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = Colors::active(cx);
        if self.toasts.shown().next().is_none() {
            return div().into_any_element();
        }
        let toasts: Vec<_> = self
            .toasts
            .shown()
            .map(|toast| {
                let id = toast.id;
                let close = icon_button(
                    &colors,
                    SharedString::from(format!("toast-close-{}", id.get())),
                    ButtonKind::Ghost,
                    ButtonSize::Small,
                    Icon::Close,
                    "Dismiss",
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.toasts.dismiss(id);
                    this.settle(cx);
                }));
                rise_in(
                    div()
                        .w(px(380.))
                        .pl_3()
                        .pr_1()
                        .py_1()
                        .min_h(px(44.))
                        .flex()
                        .items_center()
                        .gap_3()
                        .bg(colors.overlay)
                        .border_1()
                        .border_color(if toast.tone == ToastTone::Danger {
                            colors.danger_border
                        } else {
                            colors.border
                        })
                        .rounded(px(CARD_RADIUS))
                        .shadow(colors.overlay_shadow())
                        .child(tone_icon(&colors, toast.tone))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_sm()
                                .text_color(colors.text)
                                .child(self.text_of(toast)),
                        )
                        .child(close),
                    SharedString::from(format!("toast-{}", id.get())),
                )
                .into_any_element()
            })
            .collect();
        div()
            .id("toast-stack")
            .absolute()
            .bottom_5()
            .right_5()
            .flex()
            .flex_col()
            .items_end()
            .gap_2()
            .font_family(TEXT_FONT)
            .occlude()
            .on_hover(cx.listener(|this, is_over: &bool, _, cx| this.hover(*is_over, cx)))
            .children(toasts)
            .into_any_element()
    }
}
