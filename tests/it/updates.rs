use std::path::PathBuf;

use assert_cmd::cargo::cargo_bin_cmd;
use guided_review::platform::Os;
use guided_review::update::{
    AppVersion, Cpu, Installed, PublishedRelease, ReleaseAsset, SkillLink, Target, Update,
    UpdateOffer, find_update,
};
use guided_review::version::Version;

use crate::support::fake_ports::FakeReleases;

const APPLE_SILICON: Target = Target {
    os: Os::MacOs,
    cpu: Cpu::Arm64,
};
const INTEL_MAC: Target = Target {
    os: Os::MacOs,
    cpu: Cpu::X86_64,
};
const LINUX_X86_64: Target = Target {
    os: Os::Linux,
    cpu: Cpu::X86_64,
};

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
        assets: [APPLE_SILICON, INTEL_MAC, LINUX_X86_64]
            .iter()
            .flat_map(|target| {
                let archive = format!("guided-review-{version}-{}.tar.gz", target.triple());
                [asset(&format!("{archive}.sha256")), asset(&archive)]
            })
            .collect(),
    }
}

#[test]
fn the_app_shows_the_version_its_cli_prints() {
    let printed = cargo_bin_cmd!("guided-review")
        .arg("--version")
        .output()
        .unwrap();

    assert_eq!(
        String::from_utf8(printed.stdout).unwrap(),
        format!("guided-review {}\n", AppVersion::RUNNING)
    );
}

#[test]
fn the_running_version_names_the_release_and_the_commit_it_was_built_from() {
    let shown = AppVersion::RUNNING.to_string();

    assert!(
        shown.starts_with(&format!("{} (", Version::running())),
        "{shown}"
    );
    assert!(shown.ends_with(')'), "{shown}");
}

#[test]
fn a_release_build_links_to_its_release_page() {
    let release = AppVersion::new("0.6.0 (f7bb7c4)", Some("v0.6.0"));

    assert_eq!(
        release.page(),
        "https://github.com/ekvedaras/guided-review/releases/tag/v0.6.0"
    );
}

#[test]
fn a_source_build_that_is_no_release_links_to_the_list_of_releases() {
    let source_build = AppVersion::new("0.6.0 (f7bb7c4-dirty)", None);

    assert_eq!(
        source_build.page(),
        "https://github.com/ekvedaras/guided-review/releases"
    );
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
        APPLE_SILICON,
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
fn a_release_target_is_named_by_its_cpu_and_os() {
    let named = |os, cpu| Target { os, cpu }.triple();

    assert_eq!(named(Os::MacOs, Cpu::Arm64), "aarch64-apple-darwin");
    assert_eq!(named(Os::MacOs, Cpu::X86_64), "x86_64-apple-darwin");
    assert_eq!(named(Os::Linux, Cpu::Arm64), "aarch64-unknown-linux-gnu");
    assert_eq!(named(Os::Linux, Cpu::X86_64), "x86_64-unknown-linux-gnu");
}

#[test]
fn a_linux_build_is_offered_the_linux_archive_never_a_mac_one() {
    let update = Update::offered(
        version("0.4.0"),
        &published("v0.5.0", "0.5.0"),
        LINUX_X86_64,
    )
    .unwrap();

    assert_eq!(
        update.archive.name,
        "guided-review-0.5.0-x86_64-unknown-linux-gnu.tar.gz"
    );
}

#[test]
fn a_release_with_only_mac_archives_is_not_offered_on_linux() {
    let mut release = published("v0.5.0", "0.5.0");
    release
        .assets
        .retain(|asset| asset.name.contains("apple-darwin"));

    assert!(Update::offered(version("0.4.0"), &release, LINUX_X86_64).is_none());
}

#[test]
fn the_running_target_is_this_builds_os() {
    assert_eq!(Target::running().os, Os::RUNNING);
}

#[test]
fn the_running_version_or_an_older_one_is_not_offered() {
    let running = version("0.5.0");

    assert!(Update::offered(running, &published("v0.5.0", "0.5.0"), INTEL_MAC).is_none());
    assert!(Update::offered(running, &published("v0.4.0", "0.4.0"), INTEL_MAC).is_none());
}

#[test]
fn a_release_without_this_macs_archive_and_its_checksum_is_not_offered() {
    let mut release = published("v0.5.0", "0.5.0");
    release
        .assets
        .retain(|asset| !asset.name.ends_with("x86_64-apple-darwin.tar.gz.sha256"));

    assert!(Update::offered(version("0.4.0"), &release, INTEL_MAC).is_none());
    assert!(Update::offered(version("0.4.0"), &release, APPLE_SILICON).is_some());
}

#[test]
fn a_release_whose_tag_is_not_a_version_is_not_offered() {
    let release = published("nightly", "0.5.0");

    assert!(Update::offered(version("0.4.0"), &release, APPLE_SILICON).is_none());
}

#[test]
fn finding_an_update_asks_github_for_its_latest_release() {
    let releases = FakeReleases {
        latest: Some(published("v0.5.0", "0.5.0")),
        ..FakeReleases::default()
    };

    let update = find_update(&releases, version("0.4.0"), INTEL_MAC).unwrap();

    assert_eq!(update.map(|update| update.version), Some(version("0.5.0")));
}

#[test]
fn finding_an_update_fails_when_github_does() {
    let releases = FakeReleases::default();

    let error = find_update(&releases, version("0.4.0"), INTEL_MAC).unwrap_err();

    assert!(format!("{error:#}").contains("GitHub is down"), "{error:#}");
}

fn update_to(text: &str) -> Update {
    Update::offered(
        version("0.0.1"),
        &published(&format!("v{text}"), text),
        APPLE_SILICON,
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
