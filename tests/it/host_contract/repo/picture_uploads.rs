use guided_review::viewer::{CodeHost, PictureUploadPort};

use crate::host_contract::{HostBuilder, PullOnHost, Reach, pr};
use crate::support::png_of;

fn uploads_of(host: &dyn CodeHost) -> &dyn PictureUploadPort {
    host.picture_uploads()
        .expect("the host offers picture uploads")
}

pub fn an_uploaded_picture_gets_an_address_on_the_host(host: HostBuilder) {
    let folder = tempfile::tempdir().unwrap();
    let picture = folder.path().join("before.png");
    std::fs::write(&picture, png_of(2, 2, [255, 0, 0, 255])).unwrap();
    let host = host(PullOnHost::default());

    let address = uploads_of(host.as_ref())
        .upload_picture(&pr(), &picture)
        .unwrap();

    assert!(address.starts_with("https://"), "{address}");
    assert!(
        !address.contains(&*folder.path().to_string_lossy()),
        "{address}"
    );
}

pub fn a_file_that_is_no_picture_or_cannot_be_read_is_not_uploaded(host: HostBuilder) {
    let folder = tempfile::tempdir().unwrap();
    let notes = folder.path().join("notes.txt");
    std::fs::write(&notes, "not a picture").unwrap();
    let missing = folder.path().join("missing.png");
    let host = host(PullOnHost::default());
    let uploads = uploads_of(host.as_ref());

    let of_notes = uploads.upload_picture(&pr(), &notes);
    let of_missing = uploads.upload_picture(&pr(), &missing);

    assert!(of_notes.is_err(), "{of_notes:?}");
    assert!(of_missing.is_err(), "{of_missing:?}");
}

pub fn an_upload_without_a_token_or_to_a_host_not_answering_fails(host: HostBuilder) {
    let folder = tempfile::tempdir().unwrap();
    let picture = folder.path().join("before.png");
    std::fs::write(&picture, png_of(2, 2, [255, 0, 0, 255])).unwrap();
    for reach in [Reach::NoToken, Reach::ServerError] {
        let host = host(PullOnHost {
            reach,
            ..PullOnHost::default()
        });

        let uploaded = uploads_of(host.as_ref()).upload_picture(&pr(), &picture);

        assert!(uploaded.is_err(), "{uploaded:?}");
    }
}
