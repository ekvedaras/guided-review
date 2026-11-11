use guided_review::github::GitHub;
use guided_review::update::{ReleaseAsset, ReleasesPort};
use sha2::{Digest, Sha256};

use crate::support::serve_files;

#[test]
fn the_latest_release_is_read_from_the_apps_own_repository() {
    let url = serve_files(vec![(
        "/repos/ekvedaras/guided-review/releases/latest",
        br#"{"tag_name":"v0.5.0","draft":false,"assets":[
            {"name":"guided-review-0.5.0-aarch64-apple-darwin.tar.gz","browser_download_url":"https://github.com/a.tar.gz","size":3}
        ]}"#
        .to_vec(),
    )]);

    let latest = GitHub::without_token_at(&url).latest_release().unwrap();

    assert_eq!(latest.tag, "v0.5.0");
    assert_eq!(
        latest.assets,
        vec![ReleaseAsset {
            name: "guided-review-0.5.0-aarch64-apple-darwin.tar.gz".to_owned(),
            url: "https://github.com/a.tar.gz".to_owned(),
        }]
    );
}

#[test]
fn a_release_file_is_downloaded_whole_however_large() {
    let archive: Vec<u8> = (0..20_000_000_u32).map(|n| (n % 251) as u8).collect();
    let url = serve_files(vec![("/archive.tar.gz", archive.clone())]);

    let downloaded = GitHub::without_token_at(&url)
        .download(&format!("{url}/archive.tar.gz"))
        .unwrap();

    assert_eq!(downloaded.len(), archive.len());
    assert_eq!(Sha256::digest(&downloaded), Sha256::digest(&archive));
}

#[test]
fn a_missing_release_file_fails_the_download() {
    let url = serve_files(vec![]);

    let error = GitHub::without_token_at(&url)
        .download(&format!("{url}/archive.tar.gz"))
        .unwrap_err();

    assert!(format!("{error:#}").contains("404"), "{error:#}");
}
