use std::path::PathBuf;

use guided_review::update::{
    Installed, PublishedRelease, ReleaseAsset, SkillLink, Target, Update, UpdateOffer, Version,
    find_update,
};

use crate::support::fake_ports::FakeReleases;

fn version(text: &str) -> Version {
    text.parse().unwrap()
}

fn asset(name: &str) -> ReleaseAsset {
    ReleaseAsset {
        name: name.to_owned(),
        url: format!("https://example.test/{name}"),
    }
}

fn published(tag: &str, version: &str) -> PublishedRelease {
    PublishedRelease {
        tag: tag.to_owned(),
        assets: [Target::AppleSilicon, Target::Intel]
            .iter()
            .flat_map(|target| {
                let archive = format!("guided-review-{version}-{}.tar.gz", target.triple());
                [asset(&format!("{archive}.sha256")), asset(&archive)]
            })
            .collect(),
    }
}

#[test]
fn versions_compare_by_number_not_by_text() {
    assert!(version("0.10.0") > version("0.9.3"));
    assert!(version("1.0.0") > version("0.99.99"));
    assert_eq!(version("0.4.0"), version("0.4.0"));
}

#[test]
fn a_release_tag_names_its_version_after_a_v() {
    assert_eq!(Version::from_tag("v0.5.1"), Some(version("0.5.1")));
    assert_eq!(Version::from_tag("0.5.1"), None);
    assert_eq!(Version::from_tag("v0.5"), None);
    assert_eq!(Version::from_tag("v0.5.1-rc.1"), None);
}

#[test]
fn a_newer_release_is_offered_with_the_archive_for_this_mac() {
    let update = Update::offered(
        version("0.4.0"),
        &published("v0.5.0", "0.5.0"),
        Target::AppleSilicon,
    )
    .unwrap();

    assert_eq!(update.version, version("0.5.0"));
    assert_eq!(
        update.archive.name,
        "guided-review-0.5.0-aarch64-apple-darwin.tar.gz"
    );
    assert_eq!(
        update.checksum.name,
        "guided-review-0.5.0-aarch64-apple-darwin.tar.gz.sha256"
    );
}

#[test]
fn the_running_version_or_an_older_one_is_not_offered() {
    let running = version("0.5.0");

    assert!(Update::offered(running, &published("v0.5.0", "0.5.0"), Target::Intel).is_none());
    assert!(Update::offered(running, &published("v0.4.0", "0.4.0"), Target::Intel).is_none());
}

#[test]
fn a_release_without_this_macs_archive_and_its_checksum_is_not_offered() {
    let mut release = published("v0.5.0", "0.5.0");
    release
        .assets
        .retain(|asset| !asset.name.ends_with("x86_64-apple-darwin.tar.gz.sha256"));

    assert!(Update::offered(version("0.4.0"), &release, Target::Intel).is_none());
    assert!(Update::offered(version("0.4.0"), &release, Target::AppleSilicon).is_some());
}

#[test]
fn a_release_whose_tag_is_not_a_version_is_not_offered() {
    let release = published("nightly", "0.5.0");

    assert!(Update::offered(version("0.4.0"), &release, Target::AppleSilicon).is_none());
}

#[test]
fn finding_an_update_asks_github_for_its_latest_release() {
    let releases = FakeReleases {
        latest: Some(published("v0.5.0", "0.5.0")),
        ..FakeReleases::default()
    };

    let update = find_update(&releases, version("0.4.0"), Target::Intel).unwrap();

    assert_eq!(update.map(|update| update.version), Some(version("0.5.0")));
}

#[test]
fn finding_an_update_fails_when_github_does() {
    let releases = FakeReleases::default();

    let error = find_update(&releases, version("0.4.0"), Target::Intel).unwrap_err();

    assert!(format!("{error:#}").contains("GitHub is down"), "{error:#}");
}

fn update_to(text: &str) -> Update {
    Update::offered(
        version("0.0.1"),
        &published(&format!("v{text}"), text),
        Target::AppleSilicon,
    )
    .unwrap()
}

fn installed(text: &str) -> Installed {
    Installed {
        version: version(text),
        program: PathBuf::from("/home/.local/bin/guided-review"),
        skill: SkillLink::Moved,
    }
}

#[test]
fn an_update_found_is_offered_and_one_found_later_takes_its_place() {
    let mut offer = UpdateOffer::default();

    offer.found(Some(update_to("0.5.0")));
    offer.found(Some(update_to("0.6.0")));

    assert_eq!(offer, UpdateOffer::Available(update_to("0.6.0")));
}

#[test]
fn starting_the_install_hands_out_the_update_once() {
    let mut offer = UpdateOffer::default();
    offer.found(Some(update_to("0.5.0")));

    assert_eq!(offer.start_install(), Some(update_to("0.5.0")));
    assert_eq!(offer.start_install(), None);
    assert_eq!(offer, UpdateOffer::Installing(update_to("0.5.0")));
}

#[test]
fn nothing_to_install_hands_out_nothing() {
    let mut offer = UpdateOffer::default();

    assert_eq!(offer.start_install(), None);
    assert_eq!(offer, UpdateOffer::Nothing);
}

#[test]
fn a_check_while_installing_or_once_installed_changes_nothing() {
    let mut offer = UpdateOffer::default();
    offer.found(Some(update_to("0.5.0")));
    offer.start_install();

    offer.found(None);
    assert_eq!(offer, UpdateOffer::Installing(update_to("0.5.0")));

    offer.install_ended(Ok(installed("0.5.0"))).unwrap();
    offer.found(Some(update_to("0.6.0")));
    assert_eq!(offer, UpdateOffer::Installed(installed("0.5.0")));
}

#[test]
fn a_failed_install_offers_the_update_again() {
    let mut offer = UpdateOffer::default();
    offer.found(Some(update_to("0.5.0")));
    offer.start_install();

    let error = offer
        .install_ended(Err(anyhow::anyhow!("disk full")))
        .unwrap_err();

    assert!(format!("{error:#}").contains("disk full"));
    assert_eq!(offer, UpdateOffer::Available(update_to("0.5.0")));
}
