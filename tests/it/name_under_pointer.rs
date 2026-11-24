use guided_review::viewer::{HoveredName, LineLocation, Side};

use crate::support::{file_path, line};

fn at(path: &str) -> LineLocation {
    LineLocation {
        path: file_path(path),
        side: Side::Head,
        line: line(3),
    }
}

fn name_at(path: &str, code: &str, byte: usize) -> Option<(String, std::ops::Range<usize>)> {
    HoveredName::under_pointer(at(path), code, byte)
        .map(|found| (found.name.word.to_string(), found.range))
}

#[test]
fn a_php_variable_is_the_name_without_its_dollar() {
    assert_eq!(
        name_at("src/Cart.php", "    $order = new Order();", 6),
        Some(("order".to_owned(), 5..10))
    );
}

#[test]
fn a_path_gives_the_part_under_the_pointer() {
    let code = "let t = Cart::total();";
    assert_eq!(
        name_at("src/cart.rs", code, 9),
        Some(("Cart".to_owned(), 8..12))
    );
    assert_eq!(
        name_at("src/cart.rs", code, 15),
        Some(("total".to_owned(), 14..19))
    );
}

#[test]
fn the_end_of_a_name_still_counts_but_a_gap_between_names_does_not() {
    let code = "pay(cart)";
    assert_eq!(name_at("src/a.ts", code, 3), Some(("pay".to_owned(), 0..3)));
    assert_eq!(
        name_at("src/a.ts", code, 4),
        Some(("cart".to_owned(), 4..8))
    );
    assert_eq!(name_at("src/a.ts", "a  b", 2), None);
    assert_eq!(name_at("src/a.ts", "a  b", 9), None);
}

#[test]
fn a_number_or_an_operator_is_no_name() {
    assert_eq!(name_at("src/a.ts", "x = 42 + 1", 5), None);
    assert_eq!(name_at("src/a.ts", "a->b", 1), Some(("a".to_owned(), 0..1)));
    assert_eq!(name_at("src/a.ts", "a->b", 2), None);
}

#[test]
fn only_files_of_a_known_language_have_names() {
    assert_eq!(name_at("README.md", "cart total", 1), None);
    assert_eq!(
        name_at("src/a.tsx", "<Cart />", 2).map(|found| found.0),
        Some("Cart".to_owned())
    );
}

#[test]
fn a_byte_inside_a_multibyte_char_is_cut_on_char_boundaries() {
    let code = "é = cart";
    assert_eq!(name_at("src/a.ts", code, 1), Some(("é".to_owned(), 0..2)));
    assert_eq!(
        name_at("src/a.ts", code, 6),
        Some(("cart".to_owned(), 5..9))
    );
}
