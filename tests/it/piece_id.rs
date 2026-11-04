use guided_review::file_path::FilePath;
use guided_review::pieces::PieceId;

fn path(text: &str) -> FilePath {
    FilePath::new(text).unwrap()
}

#[test]
fn ids_survive_display_then_from_str() {
    let ids = [
        PieceId::whole_file(path("app/Order.php")),
        PieceId::symbol(path("app/Order.php"), "Order::total").unwrap(),
        PieceId::symbol(path("app/@scope/Order.php"), "imports").unwrap(),
        PieceId::symbol(path("README.md"), "hunk-2").unwrap(),
        PieceId::whole_file(path("docs/a#b/notes.md")),
    ];

    for id in ids {
        assert_eq!(id.to_string().parse::<PieceId>().unwrap(), id);
    }
}

#[test]
fn a_symbol_that_is_empty_or_holds_a_hash_at_or_slash_is_refused() {
    for symbol in ["", "a#b", "a@since-read", "a/b"] {
        assert!(
            PieceId::symbol(path("a.php"), symbol).is_err(),
            "{symbol:?} was accepted"
        );
    }
    for text in ["a.php#", "a.php#x@y", "/abs.php#x"] {
        assert!(text.parse::<PieceId>().is_err(), "{text:?} was accepted");
    }
}

#[test]
fn an_id_splits_where_the_eye_looks() {
    let id: PieceId = "app/Models/Order.php#Order::total".parse().unwrap();

    assert_eq!(id.path().as_str(), "app/Models/Order.php");
    assert_eq!(id.symbol_path(), Some("Order::total"));
    assert_eq!(id.folder(), "app/Models/");
    assert_eq!(id.file_name(), "Order.php");
    assert_eq!(id.symbol_name(), Some("total"));
    assert_eq!(id.within_file(), "#Order::total");
    assert!(!id.is_hunk());

    let whole: PieceId = "Makefile".parse().unwrap();
    assert_eq!(whole.folder(), "");
    assert_eq!(whole.file_name(), "Makefile");
    assert_eq!(whole.symbol_name(), None);
    assert_eq!(whole.within_file(), "Makefile");

    assert!("ci.yml#hunk-1".parse::<PieceId>().unwrap().is_hunk());
}

#[test]
fn an_id_is_written_and_read_as_its_text() {
    let id: PieceId = "app/Order.php#Order::total".parse().unwrap();

    assert_eq!(
        serde_json::to_string(&id).unwrap(),
        "\"app/Order.php#Order::total\""
    );
    assert_eq!(
        serde_json::from_str::<PieceId>("\"app/Order.php#Order::total\"").unwrap(),
        id
    );
}
