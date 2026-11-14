use gpui::{point, px};
use guided_review::platform::Os;

#[test]
fn macos_draws_the_title_bar_itself_with_the_traffic_lights_in_its_corner() {
    let titlebar = Os::MacOs.titlebar("Pick a pull request".into());

    assert_eq!(titlebar.title.as_deref(), Some("Pick a pull request"));
    assert!(titlebar.appears_transparent);
    assert_eq!(
        titlebar.traffic_light_position,
        Some(point(px(14.), px(14.)))
    );
    assert_eq!(Os::MacOs.window_buttons_inset(), 80.);
}

#[test]
fn linux_leaves_decorations_to_the_window_manager_and_no_gap_for_buttons() {
    let titlebar = Os::Linux.titlebar("Pick a pull request".into());

    assert_eq!(titlebar.title.as_deref(), Some("Pick a pull request"));
    assert!(!titlebar.appears_transparent);
    assert_eq!(titlebar.traffic_light_position, None);
    assert_eq!(Os::Linux.window_buttons_inset(), 0.);
}
