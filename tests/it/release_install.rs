use std::ffi::OsString;
use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};

use flate2::Compression;
use flate2::write::GzEncoder;
use guided_review::update::{
    Build, BundlePlaced, InTheWay, ReleaseAsset, ReleaseInstall, SkillLink, Target, Update,
    Version, info_plist,
};
use sha2::{Digest, Sha256};
use tempfile::TempDir;

use crate::support::fake_ports::FakeReleases;

const TARGET: Target = Target::AppleSilicon;

fn update_to(version: &str) -> Update {
    let archive = format!("guided-review-{version}-{}.tar.gz", TARGET.triple());
    Update {
        version: version.parse().unwrap(),
        target: TARGET,
        checksum: ReleaseAsset {
            name: format!("{archive}.sha256"),
            url: String::new(),
        },
        archive: ReleaseAsset {
            name: archive,
            url: String::new(),
        },
    }
}

/// A release archive as the release workflow packs it.
fn archive_of(version: &str, binary: &str) -> Vec<u8> {
    let folder = format!("guided-review-{version}-{}", TARGET.triple());
    packed(&[
        (format!("{folder}/guided-review"), binary.to_owned(), 0o755),
        (
            format!("{folder}/skills/tour/SKILL.md"),
            "# tour".to_owned(),
            0o644,
        ),
    ])
}

/// A release archive from 0.7.0 on, which carries `Guided Review.app` beside the binary.
fn archive_with_bundle_of(version: &str, binary: &str) -> Vec<u8> {
    let folder = format!("guided-review-{version}-{}", TARGET.triple());
    let app = format!("{folder}/Guided Review.app/Contents");
    packed(&[
        (format!("{folder}/guided-review"), binary.to_owned(), 0o755),
        (
            format!("{folder}/skills/tour/SKILL.md"),
            "# tour".to_owned(),
            0o644,
        ),
        (
            format!("{app}/Info.plist"),
            info_plist(version.parse().unwrap()),
            0o644,
        ),
        (
            format!("{app}/Resources/AppIcon.icns"),
            format!("icon {version}"),
            0o644,
        ),
    ])
}

fn packed(files: &[(String, String, u32)]) -> Vec<u8> {
    let mut tar = tar::Builder::new(GzEncoder::new(Vec::new(), Compression::fast()));
    for (path, content, mode) in files {
        let mode = *mode;
        let mut header = tar::Header::new_gnu();
        header.set_size(content.len() as u64);
        header.set_mode(mode);
        header.set_cksum();
        tar.append_data(&mut header, path, content.as_bytes())
            .unwrap();
    }
    tar.into_inner().unwrap().finish().unwrap()
}

fn checksum_of(update: &Update, archive: &[u8]) -> String {
    let digest = Sha256::digest(archive);
    format!(
        "{}  {}\n",
        guided_review::hex::lower(&digest),
        update.archive.name
    )
}

/// A home with release 0.4.0 installed the way the README's install prompt does it.
struct Home {
    dir: TempDir,
}

impl Home {
    fn with_release_installed() -> Self {
        let home = Self {
            dir: TempDir::new().unwrap(),
        };
        let folder = home.release_folder("0.4.0");
        fs::create_dir_all(folder.join("skills/tour")).unwrap();
        fs::write(folder.join("guided-review"), "binary 0.4.0").unwrap();
        fs::write(folder.join("skills/tour/SKILL.md"), "# tour 0.4.0").unwrap();
        fs::create_dir_all(home.path().join(".local/bin")).unwrap();
        symlink(folder.join("guided-review"), home.bin_link()).unwrap();
        fs::create_dir_all(home.path().join(".claude/skills")).unwrap();
        symlink(folder.join("skills/tour"), home.skill_link()).unwrap();
        home
    }

    fn path(&self) -> &Path {
        self.dir.path()
    }

    fn release_folder(&self, version: &str) -> PathBuf {
        self.path().join(format!(
            ".local/share/guided-review/{version}/guided-review-{version}-{}",
            TARGET.triple()
        ))
    }

    fn bin_link(&self) -> PathBuf {
        self.path().join(".local/bin/guided-review")
    }

    fn skill_link(&self) -> PathBuf {
        self.path().join(".claude/skills/tour")
    }

    fn bundle(&self) -> PathBuf {
        self.path().join("Applications/Guided Review.app")
    }

    /// As the 0.6.0 updater leaves a home after updating to `version`: the whole archive unpacked in its
    /// version folder, the links moved to it, and no bundle in `~/Applications`, which it knew nothing of.
    fn as_the_0_6_0_updater_leaves_it(version: &str) -> Self {
        let home = Self::with_release_installed();
        let update = update_to(version);
        let archive = archive_with_bundle_of(version, &format!("binary {version}"));
        let folder = home.path().join(".local/share/guided-review").join(version);
        fs::create_dir_all(&folder).unwrap();
        tar::Archive::new(flate2::read::GzDecoder::new(archive.as_slice()))
            .unpack(&folder)
            .unwrap();
        let release = folder.join(update.unpacked_folder());
        for (link, target) in [
            (home.bin_link(), release.join("guided-review")),
            (home.skill_link(), release.join("skills/tour")),
        ] {
            fs::remove_file(&link).unwrap();
            symlink(target, link).unwrap();
        }
        home
    }

    fn install(&self) -> ReleaseInstall {
        ReleaseInstall::in_home(self.path())
    }
}

#[test]
fn the_binary_the_bin_link_points_into_the_release_folders_at_is_a_release_install() {
    let home = Home::with_release_installed();

    assert!(home.install().runs(&home.bin_link()));
    assert!(
        home.install()
            .runs(&home.release_folder("0.4.0").join("guided-review"))
    );
}

#[test]
fn a_binary_built_from_source_is_not_a_release_install() {
    let home = Home::with_release_installed();
    let cargo_bin = home.source_build();

    assert!(!home.install().runs(&cargo_bin));
}

#[test]
fn a_binary_moved_into_the_bin_folder_by_hand_is_not_a_release_install() {
    let home = Home::with_release_installed();
    fs::remove_file(home.bin_link()).unwrap();
    fs::write(home.bin_link(), "binary 0.4.0").unwrap();

    assert!(!home.install().runs(&home.bin_link()));
}

#[test]
fn a_bin_link_pointing_outside_the_release_folders_is_not_a_release_install() {
    let home = Home::with_release_installed();
    let elsewhere = home.path().join("elsewhere/guided-review");
    fs::create_dir_all(elsewhere.parent().unwrap()).unwrap();
    fs::write(&elsewhere, "binary").unwrap();
    fs::remove_file(home.bin_link()).unwrap();
    symlink(&elsewhere, home.bin_link()).unwrap();

    assert!(!home.install().runs(&elsewhere));
}

impl Home {
    fn source_build(&self) -> PathBuf {
        let cargo_bin = self.path().join(".cargo/bin/guided-review");
        fs::create_dir_all(cargo_bin.parent().unwrap()).unwrap();
        fs::write(&cargo_bin, "binary from source").unwrap();
        cargo_bin
    }

    fn bin_folder(&self) -> PathBuf {
        self.path().join(".local/bin")
    }

    fn cargo_bin_folder(&self) -> PathBuf {
        self.path().join(".cargo/bin")
    }
}

fn path_of(folders: &[PathBuf]) -> OsString {
    std::env::join_paths(folders).unwrap()
}

#[test]
fn the_release_install_running_and_first_on_path_has_nothing_in_its_way() {
    let home = Home::with_release_installed();
    let path = path_of(&[home.path().join("empty"), home.bin_folder()]);

    assert_eq!(home.install().in_the_way(&home.bin_link(), &path), None);
}

#[test]
fn a_source_build_running_is_a_local_build_with_nothing_in_its_way() {
    let home = Home::with_release_installed();
    let cargo_bin = home.source_build();
    let path = path_of(&[home.cargo_bin_folder(), home.bin_folder()]);

    assert_eq!(home.install().in_the_way(&cargo_bin, &path), None);
    assert_eq!(home.install().build_of(&cargo_bin), Build::Local);
}

#[test]
fn the_binary_the_bin_link_points_at_is_the_release_build() {
    let home = Home::with_release_installed();

    assert_eq!(home.install().build_of(&home.bin_link()), Build::Release);
}

#[test]
fn a_guided_review_before_the_bin_link_on_path_is_in_the_way_of_the_running_release_install() {
    let home = Home::with_release_installed();
    let cargo_bin = home.source_build();
    let path = path_of(&[home.cargo_bin_folder(), home.bin_folder()]);

    assert_eq!(
        home.install().in_the_way(&home.bin_link(), &path),
        Some(InTheWay::FirstOnPath {
            binary: cargo_bin,
            release_install: home.bin_link(),
        })
    );
}

#[test]
fn another_link_to_the_release_install_first_on_path_is_not_in_its_way() {
    let home = Home::with_release_installed();
    let other_link = home.path().join("other/bin/guided-review");
    fs::create_dir_all(other_link.parent().unwrap()).unwrap();
    symlink(home.bin_link(), &other_link).unwrap();
    let path = path_of(&[home.path().join("other/bin"), home.bin_folder()]);

    assert_eq!(home.install().in_the_way(&home.bin_link(), &path), None);
}

#[test]
fn a_source_build_with_no_release_installed_has_nothing_to_be_in_the_way_of() {
    let home = Home {
        dir: TempDir::new().unwrap(),
    };
    let cargo_bin = home.source_build();
    let path = path_of(&[home.cargo_bin_folder()]);

    assert_eq!(home.install().in_the_way(&cargo_bin, &path), None);
}

#[test]
fn installing_unpacks_the_release_beside_the_old_one_and_moves_both_links_to_it() {
    let home = Home::with_release_installed();
    let update = update_to("0.5.0");
    let archive = archive_of("0.5.0", "binary 0.5.0");

    let installed = home
        .install()
        .install(&update, &archive, &checksum_of(&update, &archive))
        .unwrap();

    let binary = home.release_folder("0.5.0").join("guided-review");
    assert_eq!(fs::read_link(home.bin_link()).unwrap(), binary);
    assert_eq!(fs::read_to_string(home.bin_link()).unwrap(), "binary 0.5.0");
    assert_eq!(
        fs::metadata(&binary).unwrap().permissions().mode() & 0o111,
        0o111
    );
    assert_eq!(
        fs::read_link(home.skill_link()).unwrap(),
        home.release_folder("0.5.0").join("skills/tour")
    );
    assert_eq!(installed.version, Version::from_tag("v0.5.0").unwrap());
    assert_eq!(installed.program, binary);
    assert_eq!(installed.skill, SkillLink::Moved);
    assert_eq!(
        fs::read_to_string(home.release_folder("0.4.0").join("guided-review")).unwrap(),
        "binary 0.4.0"
    );
}

#[test]
fn an_archive_that_does_not_match_its_checksum_installs_nothing() {
    let home = Home::with_release_installed();
    let update = update_to("0.5.0");
    let archive = archive_of("0.5.0", "binary 0.5.0");
    let checksum = checksum_of(&update, &archive_of("0.5.0", "something else"));

    let error = home
        .install()
        .install(&update, &archive, &checksum)
        .unwrap_err();

    assert!(format!("{error:#}").contains("checksum"), "{error:#}");
    assert_eq!(
        fs::read_link(home.bin_link()).unwrap(),
        home.release_folder("0.4.0").join("guided-review")
    );
    assert!(
        !home
            .path()
            .join(".local/share/guided-review/0.5.0")
            .exists()
    );
}

#[test]
fn an_archive_without_the_binary_installs_nothing() {
    let home = Home::with_release_installed();
    let update = update_to("0.5.0");
    let archive = archive_of("0.6.0", "binary 0.6.0");

    let error = home
        .install()
        .install(&update, &archive, &checksum_of(&update, &archive))
        .unwrap_err();

    assert!(
        format!("{error:#}").contains("guided-review-0.5.0-aarch64-apple-darwin/guided-review"),
        "{error:#}"
    );
    assert_eq!(
        fs::read_link(home.bin_link()).unwrap(),
        home.release_folder("0.4.0").join("guided-review")
    );
    assert!(
        !home
            .path()
            .join(".local/share/guided-review/0.5.0")
            .exists()
    );
}

#[test]
fn a_skill_folder_that_is_not_a_link_into_the_release_folders_is_left_alone() {
    let home = Home::with_release_installed();
    fs::remove_file(home.skill_link()).unwrap();
    fs::create_dir_all(home.skill_link()).unwrap();
    fs::write(home.skill_link().join("SKILL.md"), "# my own tour").unwrap();
    let update = update_to("0.5.0");
    let archive = archive_of("0.5.0", "binary 0.5.0");

    let installed = home
        .install()
        .install(&update, &archive, &checksum_of(&update, &archive))
        .unwrap();

    assert_eq!(installed.skill, SkillLink::NotOurs);
    assert_eq!(
        fs::read_to_string(home.skill_link().join("SKILL.md")).unwrap(),
        "# my own tour"
    );
}

#[test]
fn installing_again_over_a_half_unpacked_folder_of_the_same_version_replaces_it() {
    let home = Home::with_release_installed();
    let leftover = home.path().join(".local/share/guided-review/0.5.0");
    fs::create_dir_all(&leftover).unwrap();
    fs::write(leftover.join("half"), "").unwrap();
    let update = update_to("0.5.0");
    let archive = archive_of("0.5.0", "binary 0.5.0");

    home.install()
        .install(&update, &archive, &checksum_of(&update, &archive))
        .unwrap();

    assert!(!leftover.join("half").exists());
    assert_eq!(fs::read_to_string(home.bin_link()).unwrap(), "binary 0.5.0");
}

#[test]
fn an_update_is_downloaded_from_its_release_files_and_installed() {
    let home = Home::with_release_installed();
    let mut update = update_to("0.5.0");
    update.archive.url = "https://example.test/archive".to_owned();
    update.checksum.url = "https://example.test/checksum".to_owned();
    let archive = archive_of("0.5.0", "binary 0.5.0");
    let checksum = checksum_of(&update, &archive);
    let releases = FakeReleases {
        files: [
            (update.archive.url.clone(), archive),
            (update.checksum.url.clone(), checksum.into_bytes()),
        ]
        .into(),
        ..FakeReleases::default()
    };

    let installed = home
        .install()
        .download_and_install(&releases, &update)
        .unwrap();

    assert_eq!(
        installed.program,
        home.release_folder("0.5.0").join("guided-review")
    );
    assert_eq!(fs::read_to_string(home.bin_link()).unwrap(), "binary 0.5.0");
}

#[test]
fn the_app_restarts_into_the_binary_it_installed_whatever_the_bin_link_or_path_say_later() {
    let home = Home::with_release_installed();
    let update = update_to("0.5.0");
    let archive = archive_of("0.5.0", "binary 0.5.0");
    let installed = home
        .install()
        .install(&update, &archive, &checksum_of(&update, &archive))
        .unwrap();

    fs::remove_file(home.bin_link()).unwrap();
    fs::write(home.bin_link(), "something else").unwrap();

    assert!(installed.program.is_absolute());
    assert_eq!(
        fs::read_to_string(&installed.program).unwrap(),
        "binary 0.5.0"
    );
}

#[test]
fn an_update_puts_the_bundle_of_its_release_in_applications() {
    let home = Home::with_release_installed();
    let update = update_to("0.7.0");
    let archive = archive_with_bundle_of("0.7.0", "binary 0.7.0");

    home.install()
        .install(&update, &archive, &checksum_of(&update, &archive))
        .unwrap();

    assert_eq!(
        fs::read_to_string(home.bundle().join("Contents/Info.plist")).unwrap(),
        info_plist(Version::from_tag("v0.7.0").unwrap())
    );
    assert_eq!(
        fs::read_to_string(home.bundle().join("Contents/MacOS/guided-review")).unwrap(),
        "binary 0.7.0"
    );
}

#[test]
fn a_release_install_updated_by_0_6_0_puts_its_own_bundle_in_place_when_it_starts() {
    let home = Home::as_the_0_6_0_updater_leaves_it("0.7.0");
    assert!(!home.bundle().exists());

    let placed = home
        .install()
        .bring_bundle_up_to_date(&home.bin_link())
        .unwrap();

    assert_eq!(placed, BundlePlaced::Refreshed);
    assert_eq!(
        fs::read_to_string(home.bundle().join("Contents/Info.plist")).unwrap(),
        info_plist(Version::from_tag("v0.7.0").unwrap())
    );
    assert_eq!(
        fs::read_to_string(home.bundle().join("Contents/MacOS/guided-review")).unwrap(),
        "binary 0.7.0"
    );
    assert_eq!(
        home.install()
            .bring_bundle_up_to_date(&home.bin_link())
            .unwrap(),
        BundlePlaced::Current
    );
}

#[test]
fn a_source_build_starting_leaves_the_bundle_alone() {
    let home = Home::as_the_0_6_0_updater_leaves_it("0.7.0");
    let cargo_bin = home.source_build();

    let placed = home.install().bring_bundle_up_to_date(&cargo_bin).unwrap();

    assert_eq!(placed, BundlePlaced::NotInRelease);
    assert!(!home.bundle().exists());
}

#[test]
fn a_failed_download_installs_nothing() {
    let home = Home::with_release_installed();
    let update = update_to("0.5.0");

    let error = home
        .install()
        .download_and_install(&FakeReleases::default(), &update)
        .unwrap_err();

    assert!(format!("{error:#}").contains("404"), "{error:#}");
    assert_eq!(fs::read_to_string(home.bin_link()).unwrap(), "binary 0.4.0");
}
