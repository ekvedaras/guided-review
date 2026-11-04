use std::collections::HashMap;
use std::path::Path;

use guided_review::file_path::FilePath;

#[test]
fn an_empty_absolute_or_backslashed_path_is_refused() {
    for refused in ["", "/etc/passwd", "app\\Order.php"] {
        assert!(FilePath::new(refused).is_err(), "{refused:?} was accepted");
    }
}

#[test]
fn a_repo_path_is_kept_as_written() {
    let path = FilePath::new("app/../Order.php").unwrap();

    assert_eq!(path.as_str(), "app/../Order.php");
    assert_eq!(path.to_string(), "app/../Order.php");
}

#[test]
fn a_clone_shares_the_text_and_a_lookup_by_str_finds_it() {
    let path = FilePath::new("app/Order.php").unwrap();
    let copy = path.clone();
    assert!(std::ptr::eq(path.as_str(), copy.as_str()));

    let by_path = HashMap::from([(path, 1)]);

    assert_eq!(by_path.get("app/Order.php"), Some(&1));
}

#[test]
fn it_is_written_and_read_as_a_plain_string_and_the_reader_refuses_a_bad_one() {
    let path = FilePath::new("app/Order.php").unwrap();

    assert_eq!(serde_json::to_string(&path).unwrap(), "\"app/Order.php\"");
    assert_eq!(
        serde_json::from_str::<FilePath>("\"app/Order.php\"").unwrap(),
        path
    );
    assert!(serde_json::from_str::<FilePath>("\"/abs\"").is_err());
}

#[test]
fn it_is_joined_onto_a_clone_root_by_name() {
    let path = FilePath::new("app/Order.php").unwrap();

    assert_eq!(
        path.under(Path::new("/code/shop")),
        Path::new("/code/shop/app/Order.php")
    );
}
