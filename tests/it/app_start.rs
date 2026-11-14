use gpui::{Menu, MenuItem};
use guided_review::platform::Os;

gpui::actions!(app_start_test, [Quit]);

fn menu_names(menus: &[Menu]) -> Vec<(String, Vec<String>)> {
    menus
        .iter()
        .map(|menu| {
            let items = menu
                .items
                .iter()
                .map(|item| match item {
                    MenuItem::Action { name, .. } => name.to_string(),
                    _ => "not an action".to_owned(),
                })
                .collect();
            (menu.name.to_string(), items)
        })
        .collect()
}

#[test]
fn macos_has_an_app_menu_that_quits() {
    let menus = Os::MacOs.app_menus(Quit);

    assert_eq!(
        menu_names(&menus),
        vec![(
            "guided-review".to_owned(),
            vec!["Quit guided-review".to_owned()]
        )]
    );
}

#[test]
fn linux_has_no_app_menu() {
    assert!(Os::Linux.app_menus(Quit).is_empty());
}
