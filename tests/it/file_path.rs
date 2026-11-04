use std::collections::HashMap;
use std::path::Path;

use guided_review::code_source::parse_unified_diff;
use guided_review::file_path::FilePath;

#[test]
fn an_empty_or_absolute_path_is_refused() {
    for refused in ["", "/etc/passwd"] {
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

#[test]
fn a_backslash_is_a_legal_character_in_a_file_name() {
    assert_eq!(
        FilePath::new("app\\Order.php").unwrap().as_str(),
        "app\\Order.php"
    );
}

#[test]
fn a_changed_file_with_a_backslash_in_its_name_is_parsed() {
    let patch = "diff --git \"a/app\\\\Order.php\" \"b/app\\\\Order.php\"\n--- \"a/app\\\\Order.php\"\n+++ \"b/app\\\\Order.php\"\n@@ -1 +1 @@\n-a\n+b\n";

    let files = parse_unified_diff(patch).unwrap();

    assert_eq!(files.len(), 1);
    assert_eq!(files[0].path(), "app\\Order.php");
}

#[test]
fn a_diff_path_that_cannot_be_a_repo_path_is_reported_not_dropped() {
    let patch = "diff --git a/x b/x\nrename from x\nrename to /abs\n";

    let error = parse_unified_diff(patch).unwrap_err();

    assert!(error.to_string().contains("/abs"), "{error}");
}
