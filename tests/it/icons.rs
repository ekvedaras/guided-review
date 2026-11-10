use gpui::AssetSource;
use guided_review::app::assets::Assets;
use guided_review::app::components::icon::Icon;

#[test]
fn every_icon_is_built_into_the_binary_as_an_svg() {
    for icon in Icon::ALL {
        let bytes = Assets
            .load(icon.path())
            .unwrap()
            .unwrap_or_else(|| panic!("{icon:?} is not registered at {}", icon.path()));
        assert!(
            bytes.starts_with(b"<svg"),
            "{icon:?} at {} is not an svg",
            icon.path()
        );
    }
}
