//! The title bar: the PR's title, its merge route, status, checks and related issues.

use std::rc::Rc;
use std::sync::Arc;

use gpui::prelude::FluentBuilder;
use gpui::{
    AnyElement, App, Context, Div, FontWeight, InteractiveElement, IntoElement, MouseButton,
    ParentElement, Pixels, SharedString, Stateful, StatefulInteractiveElement, Styled, Window, div,
    px,
};

use super::OneStepLayout;
use super::issue_card::{CardSide, issue_card};
use crate::app::components::button::{
    ButtonKind, ButtonSize, button, button_icon, button_keys, icon_label_button,
};
use crate::app::components::chip::chip;
use crate::app::components::icon::{Icon, icon, sized_icon};
use crate::app::components::key_cap::{KeyLabel, key_cap};
use crate::app::components::markdown_view::{InlineMarkdown, PreparedMarkdown};
use crate::app::components::motion::{glide_to, glide_to_share};
use crate::app::components::overlay::popover;
use crate::app::components::palette::{
    CARD_RADIUS, CODE_FONT, CONTROL_RADIUS, Colors, INNER_RADIUS, Tone, TrailStage,
};
use crate::app::components::scrollbar::{ScrollbarDrag, scrollbar};
use crate::app::components::thread_view::avatar_circle;
use crate::app::components::title_bar::{TitleBarArea, TitleBarEdge};
use crate::app::components::tooltip::tooltip;
use crate::viewer::RelatedIssue;
use crate::viewer::{
    AutoMerge, CheckList, CheckState, Checks, Command, Intent, MergeChange, MergeOffer, Merging,
    OpenPrStatus, PrStatus, Readiness,
};

impl OneStepLayout {
    /// The PR this review is of, and the window-wide actions.
    pub(super) fn title_bar(
        &self,
        is_writing_tour: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let colors = Colors::active(cx);
        let review = self.session.read(cx).review();
        let subject_name = review.subject().short();
        let title = review.title().to_owned();
        let local_heading = review.local_heading().map(SharedString::from);
        let is_local = local_heading.is_some();
        let host = review
            .subject()
            .pull_request()
            .map(|pr| pr.host)
            .filter(|_| review.can_open_on_host());
        let status = review.pr_status();
        let merge_in_flight = review.merge_in_flight();
        let merging = review.merging();
        let jump = button(
            &colors,
            "title-bar-jump",
            ButtonKind::Ghost,
            ButtonSize::Small,
        )
        .child(button_icon(
            &colors,
            ButtonKind::Ghost,
            ButtonSize::Small,
            Icon::Search,
        ))
        .child("Jump")
        .child(button_keys(&colors, ButtonKind::Ghost, Command::Jump, cx))
        .on_click(cx.listener(|this, _, window, cx| this.act(Intent::ToggleJump, window, cx)));
        let open_on_host = host.map(|host| {
            button(
                &colors,
                "title-bar-open-on-host",
                ButtonKind::Ghost,
                ButtonSize::Small,
            )
            .child(button_icon(
                &colors,
                ButtonKind::Ghost,
                ButtonSize::Small,
                Icon::ExternalLink,
            ))
            .child(host.name())
            .child(button_keys(
                &colors,
                ButtonKind::Ghost,
                Command::OpenOnHost,
                cx,
            ))
            .on_click(cx.listener(|this, _, window, cx| this.act(Intent::OpenOnHost, window, cx)))
        });
        let open_pr = review.can_open_pr().then(|| {
            button(
                &colors,
                "title-bar-open-pr",
                ButtonKind::Ghost,
                ButtonSize::Small,
            )
            .child(button_icon(
                &colors,
                ButtonKind::Ghost,
                ButtonSize::Small,
                Icon::PullRequest,
            ))
            .child("Open PR")
            .child(button_keys(&colors, ButtonKind::Ghost, Command::OpenPr, cx))
            .tooltip(tooltip(|_| {
                "Ask your agent to open the PR: it types `pr it`".into()
            }))
            .on_click(cx.listener(|this, _, window, cx| this.act(Intent::OpenPr, window, cx)))
        });
        let mut switch_pr = div().id("title-bar-switch-pr");
        switch_pr.style().flex_shrink = Some(TITLE_SHRINKS_FIRST);
        let switch_pr = switch_pr
            .min_w(px(PR_TITLE_MIN_WIDTH))
            .overflow_hidden()
            .flex()
            .items_center()
            .gap_2()
            .px_2()
            .py_1()
            .rounded(px(CONTROL_RADIUS))
            .cursor_pointer()
            .hover(|button| button.bg(colors.hover))
            .active(|button| button.bg(colors.pressed))
            .tooltip(tooltip(move |_| {
                if is_local {
                    "Switch review"
                } else {
                    "Switch pull request"
                }
                .into()
            }))
            .on_click(
                cx.listener(|this, _, window, cx| this.act(Intent::TogglePrSwitcher, window, cx)),
            )
            .child(icon(
                if is_local {
                    Icon::FolderGit
                } else {
                    Icon::PullRequest
                },
                match &status {
                    Some(PrStatus::Open(OpenPrStatus {
                        readiness: Readiness::Ready,
                        ..
                    })) => colors.success,
                    Some(PrStatus::Merged) => colors.accent,
                    Some(PrStatus::Closed) => colors.danger,
                    Some(PrStatus::Open(_)) | None => colors.muted_text,
                },
            ))
            .children(match local_heading {
                Some(heading) => vec![
                    div()
                        .min_w_0()
                        .truncate()
                        .font_weight(FontWeight::MEDIUM)
                        .child(heading)
                        .into_any_element(),
                ],
                None => vec![
                    div()
                        .flex_none()
                        .text_color(colors.muted_text)
                        .child(subject_name)
                        .into_any_element(),
                    div()
                        .min_w_0()
                        .truncate()
                        .font_weight(FontWeight::MEDIUM)
                        .child(InlineMarkdown::new("title-bar-title", &title))
                        .into_any_element(),
                ],
            })
            .child(sized_icon(Icon::ChevronDown, colors.muted_text, px(12.)))
            .child(button_keys(
                &colors,
                ButtonKind::Ghost,
                Command::SwitchPr,
                cx,
            ));
        let page_share = scrolled_share(
            -self.screen_list.scroll_px_offset_for_scrollbar().y,
            self.screen_list.max_offset_for_scrollbar().y,
        );
        TitleBarArea::new("title-bar", gpui::transparent_black())
            .edge(TitleBarEdge::Open)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .px_4()
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_sm()
                    .child(switch_pr)
                    .children(self.merge_route_parts(&colors, cx))
                    .children(status.as_ref().map(|status| {
                        match status {
                            PrStatus::Open(open) => {
                                self.status_chip(&colors, open, cx).into_any_element()
                            }
                            PrStatus::Merged => closed_pr_chip(
                                &colors,
                                Tone::Accent,
                                Icon::GitMerge,
                                status.label(),
                            )
                            .into_any_element(),
                            PrStatus::Closed => {
                                closed_pr_chip(&colors, Tone::Danger, Icon::Close, status.label())
                                    .into_any_element()
                            }
                        }
                    }))
                    .children(
                        merging.and_then(|merging| {
                            merge_button(&colors, merging, merge_in_flight, cx)
                        }),
                    )
                    .child(div().flex_1())
                    .child(jump)
                    .children(self.issue_button(&colors, cx))
                    .children(open_pr)
                    .children(open_on_host),
            )
            .when(!is_writing_tour, |bar| {
                bar.child(page_hairline(&colors, page_share))
            })
    }

    /// Beside the title: the PR author's picture, the branch it merges into, and the branch it comes
    /// from. Each is a child of the bar so that the head branch alone is cut, before the title.
    fn merge_route_parts(&self, colors: &Colors, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let review = self.session.read(cx).review();
        let (Some(route), Some(sentence)) = (review.merge_route(), review.merge_route_sentence())
        else {
            return Vec::new();
        };
        let sentence = SharedString::from(sentence);
        let explained = |id: &'static str| {
            let sentence = sentence.clone();
            div().id(id).tooltip(tooltip(move |_| sentence.clone()))
        };
        let picture = review.pr_author().cloned().map(|author| {
            explained("title-bar-pr-author")
                .flex_none()
                .child(avatar_circle(
                    colors,
                    &author.login,
                    author.avatar_url.as_deref(),
                    ROUTE_AVATAR_SIZE,
                    cx,
                ))
                .into_any_element()
        });
        let mut base = explained("title-bar-base-branch");
        base.style().flex_shrink = Some(BASE_BRANCH_SHRINKS_BEFORE_TITLE);
        let mut base_chip = chip(colors, Tone::Neutral);
        base_chip.style().flex_shrink = Some(1.);
        let base = base
            .min_w(base_branch_min_width(&route.base.to_string(), cx))
            .flex()
            .child(
                base_chip
                    .min_w_0()
                    .font_family(*CODE_FONT)
                    .child(div().min_w_0().truncate().child(route.base.to_string())),
            )
            .into_any_element();
        let mut head = explained("title-bar-head-branch");
        head.style().flex_shrink = Some(HEAD_BRANCH_SHRINKS_BEFORE_TITLE);
        let head = head
            .min_w_0()
            .truncate()
            .font_family(*CODE_FONT)
            .text_xs()
            .text_color(colors.muted_text)
            .child(format!("← {}", route.head))
            .into_any_element();
        picture.into_iter().chain([base, head]).collect()
    }

    /// The review's issue, or how many there are; a click is the key `I`.
    fn issue_button(
        &self,
        colors: &Colors,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement + use<>> {
        let issues = self.session.read(cx).review().related_issues();
        let (label, tip): (SharedString, SharedString) = match &*issues {
            [] => return None,
            [only] => (
                only.link.label().into(),
                only.title
                    .as_deref()
                    .map_or_else(|| format!("Open {}", only.link), str::to_owned)
                    .into(),
            ),
            several => (
                format!("{} issues", several.len()).into(),
                "List the related issues".into(),
            ),
        };
        let several = issues.len() > 1;
        let list = (self.issue_list_open && several).then(|| self.issue_list(colors, &issues, cx));
        let button = button(
            colors,
            "title-bar-issue",
            ButtonKind::Ghost,
            ButtonSize::Small,
        )
        .child(button_icon(
            colors,
            ButtonKind::Ghost,
            ButtonSize::Small,
            Icon::CircleDot,
        ))
        .child(label)
        .child(button_keys(
            colors,
            ButtonKind::Ghost,
            Command::OpenIssue,
            cx,
        ))
        .when(several, |button| {
            button.tooltip(tooltip(move |_| tip.clone()))
        })
        .on_click(cx.listener(|this, _, window, cx| this.act(Intent::OpenIssue, window, cx)));
        let card = match (&*issues, self.issue_hover.shown()) {
            ([only], Some(0)) => Some(popover(self.issue_card_element(
                colors,
                only,
                0,
                CardSide::Below,
                cx,
            ))),
            _ => None,
        };
        Some(
            div()
                .id("title-bar-issue-hover")
                .flex_none()
                .when(!several, |wrapper| {
                    wrapper.on_hover(cx.listener(|this, is_hovered: &bool, _, cx| {
                        this.issue_hover.target = is_hovered.then_some(0);
                        cx.notify();
                    }))
                })
                .child(button)
                .children(card)
                .children(list),
        )
    }

    /// The card of one issue, with a gap on the side it is opened from that the mouse may cross.
    fn issue_card_element(
        &self,
        colors: &Colors,
        issue: &RelatedIssue,
        place: usize,
        side: CardSide,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let details = self.session.read(cx).review().issue_details(&issue.link);
        let body = details.as_ref().and_then(|details| {
            let mut prepared = self.issue_card_body.borrow_mut();
            let is_current = prepared
                .as_ref()
                .is_some_and(|(shown, _)| Arc::ptr_eq(shown, details));
            if !is_current {
                let markdown = details.body.clone()?;
                *prepared = Some((Arc::clone(details), Rc::new(PreparedMarkdown::of(markdown))));
            }
            prepared.as_ref().map(|(_, body)| Rc::clone(body))
        });
        div()
            .id(("title-bar-issue-card", place))
            .on_hover(cx.listener(move |this, is_hovered: &bool, _, cx| {
                if !*is_hovered && this.issue_card_drag.is_dragging() {
                    return;
                }
                this.issue_hover.card = is_hovered.then_some(place);
                cx.notify();
            }))
            .map(|element| match side {
                CardSide::Below => element.pt_1(),
                CardSide::Right => element.pl_2(),
            })
            // The thumb is drawn from the scroll offset, so a wheel turn redraws the card.
            .on_scroll(cx.listener(|_, _, _, cx| cx.notify()))
            .child(issue_card(
                colors,
                &issue.link,
                details.as_deref(),
                body,
                &self.issue_card_scroll,
                self.issue_card_scrollbar(colors, cx),
            ))
    }

    /// The card description's scrollbar, shown only when some of it is out of sight.
    fn issue_card_scrollbar(&self, colors: &Colors, cx: &mut Context<Self>) -> Option<AnyElement> {
        let thumb = ScrollbarDrag::thumb(&self.issue_card_scroll)?;
        Some(
            scrollbar(
                colors,
                "issue-card-scrollbar",
                thumb,
                self.issue_card_drag.is_dragging(),
            )
            .top(px(8.))
            .h(self.issue_card_scroll.bounds().size.height)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &gpui::MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    this.issue_card_drag
                        .press(&this.issue_card_scroll, event.position.y);
                    cx.notify();
                }),
            )
            .into_any_element(),
        )
    }

    /// Under the issue button: each issue with its number key; a click or the key opens it.
    fn issue_list(
        &self,
        colors: &Colors,
        issues: &[RelatedIssue],
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let rows = issues.iter().enumerate().map(|(place, issue)| {
            let title = issue
                .title
                .as_deref()
                .map_or_else(|| issue.link.to_string(), str::to_owned);
            div()
                .id(("title-bar-issue-row", place))
                .flex()
                .items_center()
                .gap_2()
                .px_2()
                .h(px(28.))
                .rounded(px(INNER_RADIUS))
                .cursor_pointer()
                .hover(|row| row.bg(colors.hover))
                .on_hover(cx.listener(move |this, is_hovered: &bool, _, cx| {
                    let leaves_row = !is_hovered && this.issue_hover.target == Some(place);
                    if *is_hovered {
                        this.issue_hover.target = Some(place);
                    } else if leaves_row {
                        this.issue_hover.target = None;
                    }
                    cx.notify();
                }))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.act(Intent::OpenIssueAt(place), window, cx);
                }))
                .children(
                    ISSUE_KEYS
                        .get(place)
                        .map(|key| key_cap(colors, KeyLabel::Text(key))),
                )
                .child(
                    div()
                        .flex_none()
                        .text_color(colors.muted_text)
                        .child(issue.link.label()),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .child(InlineMarkdown::new(
                            format!("title-bar-issue-title-{place}"),
                            &title,
                        )),
                )
        });
        let rows: Vec<_> = rows.collect();
        let card = self.issue_hover.shown().and_then(|place| {
            let issue = issues.get(place)?;
            Some(
                self.issue_card_element(colors, issue, place, CardSide::Right, cx)
                    .absolute()
                    .top_0()
                    .left(gpui::relative(1.)),
            )
        });
        popover(
            div()
                .id("title-bar-issue-list")
                .relative()
                .mt_1()
                .w(px(CHECKS_LIST_WIDTH))
                .p_1()
                .flex()
                .flex_col()
                .text_sm()
                .bg(colors.overlay)
                .border_1()
                .border_color(colors.border)
                .rounded(px(CARD_RADIUS))
                .shadow(colors.overlay_shadow())
                .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                    this.issue_list_open = false;
                    cx.notify();
                }))
                .children(rows)
                .children(card),
        )
    }

    /// While the issue list is open, a number opens its issue and Escape closes it.
    pub(super) fn pick_issue_by_key(
        &mut self,
        event: &gpui::KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let key = event.keystroke.key.as_str();
        if key == "escape" {
            self.issue_list_open = false;
            cx.stop_propagation();
            cx.notify();
        } else if let Some(place) = ISSUE_KEYS.iter().position(|face| *face == key) {
            cx.stop_propagation();
            self.act(Intent::OpenIssueAt(place), window, cx);
        }
    }

    /// Draft or ready, in the colour and with the icon of the checks, which a hover lists. Pressing
    /// it is the key `D`.
    fn status_chip(
        &self,
        colors: &Colors,
        status: &OpenPrStatus,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let tone = match status.checks {
            Checks::Failing => Tone::Danger,
            Checks::Running => Tone::Warning,
            Checks::Passing => Tone::Success,
            Checks::NoneRun => Tone::Neutral,
        };
        let mark = match status.checks {
            Checks::Failing => Some(Icon::Close),
            Checks::Running => Some(Icon::Clock),
            Checks::Passing => Some(Icon::Check),
            Checks::NoneRun => None,
        };
        let text_color = colors.tone(tone).text;
        let list = status.check_list();
        let is_listed = self.checks_hover.is_on() && !list.shown.is_empty();
        let tinted = colors.tone(tone);
        let chip = button(
            colors,
            "title-bar-pr-status",
            ButtonKind::Ghost,
            ButtonSize::Small,
        )
        .bg(tinted.background)
        .border_color(tinted.border)
        .text_color(tinted.text)
        .children(mark.map(|mark| sized_icon(mark, text_color, ButtonSize::Small.icon_size())))
        .child(status.label())
        .when(status.viewer_may_change_readiness, |chip| {
            chip.child(button_keys(
                colors,
                ButtonKind::Ghost,
                Command::ToggleDraft,
                cx,
            ))
            .on_click(cx.listener(|this, _, window, cx| {
                this.act(Intent::ToggleDraft, window, cx);
            }))
        })
        .when(!status.viewer_may_change_readiness, |chip| {
            chip.cursor_default()
        });
        div()
            .id("title-bar-checks")
            .flex_none()
            .on_hover(cx.listener(|this, is_hovered: &bool, _, cx| {
                this.checks_hover.chip = *is_hovered;
                cx.notify();
            }))
            .child(chip)
            .when(is_listed, |wrapper| {
                wrapper.child(self.checks_list(colors, &list, cx))
            })
    }

    /// Under the chip: each check with where it stands, failing first; a click opens its run.
    fn checks_list(
        &self,
        colors: &Colors,
        list: &CheckList,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let rows = list.shown.iter().enumerate().map(|(place, check)| {
            let (mark, color, state) = match check.state {
                CheckState::Failing => (Icon::Close, colors.danger, "failing"),
                CheckState::Running => (Icon::Clock, colors.warning, "running"),
                CheckState::Passing => (Icon::Check, colors.success, "passing"),
                CheckState::Neutral => (Icon::Square, colors.muted_text, "neutral"),
                CheckState::Skipped => (Icon::Minus, colors.muted_text, "skipped"),
            };
            div()
                .id(("title-bar-check", place))
                .flex()
                .items_center()
                .gap_2()
                .px_2()
                .h(px(28.))
                .rounded(px(INNER_RADIUS))
                .cursor_pointer()
                .hover(|row| row.bg(colors.hover))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.act(Intent::OpenCheck(place), window, cx);
                }))
                .child(sized_icon(mark, color, px(12.)))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .child(SharedString::from(check.name.to_string())),
                )
                .child(div().flex_none().text_xs().text_color(color).child(state))
        });
        let more = (list.hidden > 0).then(|| {
            div()
                .id("title-bar-checks-more")
                .flex()
                .items_center()
                .px_2()
                .h(px(28.))
                .rounded(px(INNER_RADIUS))
                .cursor_pointer()
                .text_color(colors.muted_text)
                .hover(|row| row.bg(colors.hover).text_color(colors.text))
                .on_click({
                    let past_the_list = list.shown.len();
                    cx.listener(move |this, _, window, cx| {
                        this.act(Intent::OpenCheck(past_the_list), window, cx);
                    })
                })
                .child(format!("and {} more on the checks page", list.hidden))
        });
        popover(
            div()
                .id("title-bar-checks-hover")
                .on_hover(cx.listener(|this, is_hovered: &bool, _, cx| {
                    this.checks_hover.list = *is_hovered;
                    cx.notify();
                }))
                .child(div().h(px(4.)))
                .child(
                    div()
                        .id("title-bar-checks-list")
                        .w(px(CHECKS_LIST_WIDTH))
                        .p_1()
                        .flex()
                        .flex_col()
                        .text_sm()
                        .bg(colors.overlay)
                        .border_1()
                        .border_color(colors.border)
                        .rounded(px(CARD_RADIUS))
                        .shadow(colors.overlay_shadow())
                        .children(rows)
                        .children(more),
                ),
        )
    }
}

const CHECKS_LIST_WIDTH: f32 = 340.;
/// The keys that open the issue list's rows, in order.
const ISSUE_KEYS: [&str; 9] = ["1", "2", "3", "4", "5", "6", "7", "8", "9"];
/// What the PR's name and title keep of the bar: its icon, the name (which never truncates), the
/// chevron and the switch keys, with a few characters of title. Past it the title is cut with an ellipsis, down to
/// nothing, and the controls to its right keep their width.
const PR_TITLE_MIN_WIDTH: f32 = 290.;
/// The head branch gives up its room first, then the base branch down to a few characters, then the
/// PR's title; the author's picture never shrinks.
const HEAD_BRANCH_SHRINKS_BEFORE_TITLE: f32 = TITLE_SHRINKS_FIRST * 100.;
const BASE_BRANCH_SHRINKS_BEFORE_TITLE: f32 = TITLE_SHRINKS_FIRST * 10.;
const BASE_BRANCH_MIN_WIDTH: f32 = 120.;

/// The base branch keeps `BASE_BRANCH_MIN_WIDTH` when the title bar is short of room, but never
/// more than its chip: a fixed minimum left a short name like `main` with a gap beside its chip.
fn base_branch_min_width(name: &str, cx: &App) -> Pixels {
    // The chip's `text_xs` code, its padding and its border, as `chip` draws them.
    const CHIP_TEXT_SIZE: f32 = 12.;
    const CHIP_EDGES: f32 = 14.;
    let text = cx.text_system();
    let font_id = text.resolve_font(&gpui::font(*CODE_FONT));
    let name_width = name
        .chars()
        .filter_map(|ch| text.advance(font_id, px(CHIP_TEXT_SIZE), ch).ok())
        .fold(px(CHIP_EDGES), |width, glyph| width + glyph.width);
    name_width.min(px(BASE_BRANCH_MIN_WIDTH))
}
const ROUTE_AVATAR_SIZE: f32 = 18.;
/// The PR's name and title shrink this many times faster than the status chip, so they give up
/// room down to their minimum before the chip loses any.
const TITLE_SHRINKS_FIRST: f32 = 10_000.;

/// In place of the status chip and the merge button once the PR is merged or closed, in GitHub's
/// colour for it. Nothing is left to change, so it is no button.
fn closed_pr_chip(colors: &Colors, tone: Tone, mark: Icon, label: &'static str) -> Div {
    let strong = colors.strong_tone(tone);
    chip(colors, tone)
        .bg(strong.background)
        .border_color(strong.border)
        .text_color(strong.text)
        .h(ButtonSize::Small.height())
        .px_2()
        .gap_1p5()
        .rounded(px(CONTROL_RADIUS))
        .font_weight(FontWeight::MEDIUM)
        .child(sized_icon(mark, strong.text, ButtonSize::Small.icon_size()))
        .child(label)
}

/// Beside the PR status: Auto-merge, Auto-merge on or Merge now, as `Shift+M` would act; hidden
/// where GitHub would refuse. A click is the key, so it too needs a second click within 10 s.
/// From the confirming click until GitHub answers it is a chip saying what is under way, so there
/// is nothing to click twice.
fn merge_button(
    colors: &Colors,
    merging: Merging,
    in_flight: Option<MergeChange>,
    cx: &mut Context<OneStepLayout>,
) -> Option<AnyElement> {
    let label = merging.button_label()?;
    if let Some(change) = in_flight {
        let doing = match change {
            MergeChange::MergeNow(_) => "Merging",
            MergeChange::EnableAutoMerge(_) => "Turning on auto-merge",
            MergeChange::DisableAutoMerge => "Turning off auto-merge",
        };
        return Some(
            closed_pr_chip(colors, Tone::Accent, Icon::GitMerge, doing).into_any_element(),
        );
    }
    let icon = match merging.offer().ok()? {
        MergeOffer::DisableAutoMerge => Icon::Clock,
        MergeOffer::EnableAutoMerge | MergeOffer::MergeNow => Icon::GitMerge,
    };
    let kind = match merging.auto_merge {
        AutoMerge::On(_) => ButtonKind::Pressed,
        AutoMerge::Off => ButtonKind::Ghost,
    };
    let method = match merging.auto_merge {
        AutoMerge::On(method) => Some(method.label()),
        AutoMerge::Off => None,
    };
    Some(
        icon_label_button(
            colors,
            "title-bar-merge",
            kind,
            ButtonSize::Small,
            icon,
            label,
        )
        .children(method.map(|method| div().text_color(colors.muted_text).child(method)))
        .child(button_keys(colors, kind, Command::ToggleAutoMerge, cx))
        .tooltip(tooltip(|_| "Click twice to confirm".into()))
        .on_click(cx.listener(|this, _, window, cx| {
            this.act(Intent::ToggleAutoMerge, window, cx);
        }))
        .into_any_element(),
    )
}

/// How far down the screen the reader is, gliding after the scroll; at the end the green fades in
/// over the trail. The steps read are the sidebar's meter.
fn page_hairline(colors: &Colors, share: f32) -> impl IntoElement {
    let is_at_end = share >= 1.;
    let green = glide_to(
        div()
            .absolute()
            .inset_0()
            .bg(colors.trail_fill(TrailStage::Complete)),
        "page-hairline-green",
        if is_at_end { 1. } else { 0. },
        |green, shown| green.opacity(shown.clamp(0., 1.)),
    );
    div()
        .absolute()
        .left_0()
        .right_0()
        .bottom_0()
        .h(px(2.))
        .bg(colors.subtle_border)
        .child(glide_to_share(
            div()
                .relative()
                .h_full()
                .bg(colors.trail_fill(TrailStage::UnderWay))
                .child(green),
            "page-hairline-fill",
            share,
        ))
}

/// How far down its page the reader is, 0 at the top and 1 at the end; a page that fits is read whole.
fn scrolled_share(scrolled: Pixels, scrollable: Pixels) -> f32 {
    if scrollable <= Pixels::ZERO {
        return 1.;
    }
    (scrolled / scrollable).clamp(0., 1.)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_page_line_follows_the_scroll_from_top_to_end() {
        assert_eq!(scrolled_share(px(0.), px(400.)), 0.);
        assert_eq!(scrolled_share(px(100.), px(400.)), 0.25);
        assert_eq!(scrolled_share(px(400.), px(400.)), 1.);
    }

    #[test]
    fn a_page_that_fits_the_window_is_seen_whole() {
        assert_eq!(scrolled_share(px(0.), px(0.)), 1.);
    }
}
