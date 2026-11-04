#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "a panic in a test helper is the test failing"
)]

#[allow(dead_code)]
mod support;

use support::{FakeGitHub, FixtureRepo, GuidedReview};

const PR: &str = "https://github.com/acme/shop/pull/7";

/// The outline below its header: the file and piece rows only.
fn rows_of_change(base_files: &[(&str, &str)], head_files: &[(&str, &str)]) -> String {
    let repo = FixtureRepo::cloned_from("acme/shop");
    for (path, content) in base_files {
        repo.write(path, content);
    }
    let base = repo.commit("base");
    for (path, content) in head_files {
        repo.write(path, content);
    }
    let head = repo.commit("head");
    let github = FakeGitHub::serving_pull("acme/shop", 7, &base, &head);

    let run = GuidedReview::new(&repo, &github).run(&["outline", PR]);

    let stdout = run.stdout_with(&[]);
    let (_header, rows) = stdout.split_once("\n\n").expect("outline has a header");
    rows.to_string()
}

fn order(imports: &str, before_class: &str, class_docblock: &str, body: &str) -> String {
    format!(
        "<?php\n\nnamespace App;\n\n{imports}\n{before_class}\n{class_docblock}class Order\n{{\n{body}}}\n"
    )
}

const TOTAL: &str = "    public function total(): int\n    {\n        return 1;\n    }\n";

#[test]
fn changed_imports_become_an_imports_piece_and_only_when_they_changed() {
    let rows = rows_of_change(
        &[("app/Order.php", &order("use App\\A;", "", "", TOTAL))],
        &[(
            "app/Order.php",
            &order(
                "use App\\B;",
                "",
                "",
                &TOTAL.replace("return 1", "return 2"),
            ),
        )],
    );

    assert_eq!(
        rows,
        "\
app/Order.php  modified  +2 −2
  #imports  imports  modified  +1 −1
  #Order::total  method  modified  +1 −1  public function total(): int
"
    );
}

#[test]
fn a_method_change_alone_makes_no_imports_or_top_piece() {
    let rows = rows_of_change(
        &[(
            "app/Order.php",
            &order("use App\\A;", "define('X', 1);", "", TOTAL),
        )],
        &[(
            "app/Order.php",
            &order(
                "use App\\A;",
                "define('X', 1);",
                "",
                &TOTAL.replace("return 1", "return 2"),
            ),
        )],
    );

    assert_eq!(
        rows,
        "\
app/Order.php  modified  +1 −1
  #Order::total  method  modified  +1 −1  public function total(): int
"
    );
}

#[test]
fn changes_outside_symbols_and_imports_become_a_top_piece() {
    let rows = rows_of_change(
        &[(
            "app/Order.php",
            &order("use App\\A;", "define('X', 1);", "", TOTAL),
        )],
        &[(
            "app/Order.php",
            &order("use App\\A;", "define('X', 2);", "", TOTAL),
        )],
    );

    assert_eq!(
        rows,
        "\
app/Order.php  modified  +1 −1
  #(top)  top  modified  +1 −1
"
    );
}

#[test]
fn a_docblock_belongs_to_the_symbol_below_it() {
    let method_with = |docblock: &str| format!("    {docblock}\n{TOTAL}");
    let rows = rows_of_change(
        &[(
            "app/Order.php",
            &order("", "", "/** Old class. */\n", &method_with("/** Old. */")),
        )],
        &[(
            "app/Order.php",
            &order("", "", "/** New class. */\n", &method_with("/** New. */")),
        )],
    );

    assert_eq!(
        rows,
        "\
app/Order.php  modified  +2 −2
  #Order  class  modified  +1 −1  class Order
  #Order::total  method  modified  +1 −1  public function total(): int
"
    );
}

#[test]
fn a_renamed_method_is_one_modified_piece_under_its_new_name() {
    let rows = rows_of_change(
        &[("app/Order.php", &order("", "", "", TOTAL))],
        &[(
            "app/Order.php",
            &order("", "", "", &TOTAL.replace("total", "sum")),
        )],
    );

    assert_eq!(
        rows,
        "\
app/Order.php  modified  +1 −1
  #Order::sum  method  modified  +1 −1  public function sum(): int
"
    );
}

#[test]
fn a_renamed_class_keeps_its_methods_paired() {
    let rows = rows_of_change(
        &[("app/Order.php", &order("", "", "", TOTAL))],
        &[(
            "app/Order.php",
            &order("", "", "", &TOTAL.replace("return 1", "return 2"))
                .replace("class Order", "class Purchase"),
        )],
    );

    assert_eq!(
        rows,
        "\
app/Order.php  modified  +2 −2
  #Purchase  class  modified  +1 −1  class Purchase
  #Purchase::total  method  modified  +1 −1  public function total(): int
"
    );
}

#[test]
fn renames_are_paired_top_down() {
    let two = "    public function a(): int\n    {\n        return 1;\n    }\n\n    public function b(): int\n    {\n        return 2;\n    }\n";
    let renamed = "    public function x(): int\n    {\n        return 1;\n        return 5;\n    }\n\n    public function y(): int\n    {\n        return 2;\n    }\n";
    let rows = rows_of_change(
        &[("app/Order.php", &order("", "", "", two))],
        &[("app/Order.php", &order("", "", "", renamed))],
    );

    assert_eq!(
        rows,
        "\
app/Order.php  modified  +3 −2
  #Order::x  method  modified  +2 −1  public function x(): int
  #Order::y  method  modified  +1 −1  public function y(): int
"
    );
}

#[test]
fn a_layout_only_change_is_a_piece_marked_format() {
    let rows = rows_of_change(
        &[(
            "app/Order.php",
            &order("", "", "", &TOTAL.replace("return 1;", "return $a+$b;")),
        )],
        &[(
            "app/Order.php",
            &order("", "", "", &TOTAL.replace("return 1;", "return $a + $b;")),
        )],
    );

    assert_eq!(
        rows,
        "\
app/Order.php  modified  +1 −1
  #Order::total  method  modified  +1 −1  format  public function total(): int
"
    );
}

#[test]
fn a_symbol_moved_inside_its_file_with_an_unchanged_body_is_marked_moved() {
    let method = |name: &str, result: u8| {
        format!("    public function {name}(): int\n    {{\n        return {result};\n    }}\n")
    };
    let (a, b, c) = (method("a", 1), method("b", 2), method("c", 3));
    let rows = rows_of_change(
        &[(
            "app/Order.php",
            &order("", "", "", &format!("{a}\n{b}\n{c}")),
        )],
        &[(
            "app/Order.php",
            &order("", "", "", &format!("{c}\n{a}\n{b}")),
        )],
    );

    assert_eq!(
        rows,
        "\
app/Order.php  modified  +5 −5
  #Order::c  method  modified  +5 −5  moved  public function c(): int
"
    );
}

#[test]
fn a_symbol_moved_to_another_file_with_an_unchanged_body_is_marked_moved_on_both_sides() {
    let class =
        "class Invoice\n{\n    public function total(): int\n    {\n        return 1;\n    }\n}\n";
    let rows = rows_of_change(
        &[
            (
                "app/Order.php",
                &format!("<?php\n\nclass Order\n{{\n}}\n\n{class}"),
            ),
            ("app/Invoice.php", "<?php\n"),
        ],
        &[
            ("app/Order.php", "<?php\n\nclass Order\n{\n}\n"),
            ("app/Invoice.php", &format!("<?php\n\n{class}")),
        ],
    );

    assert_eq!(
        rows,
        "\
app/Invoice.php  modified  +8 −0
  #Invoice  class  added  +4 −0  moved  class Invoice
  #Invoice::total  method  added  +4 −0  moved  public function total(): int

app/Order.php  modified  +0 −8
  #Invoice  class  deleted  +0 −4  moved  class Invoice
  #Invoice::total  method  deleted  +0 −4  moved  public function total(): int
"
    );
}

#[test]
fn a_blank_line_only_change_is_its_own_format_piece_when_nothing_else_changed() {
    let rows = rows_of_change(
        &[("app/Order.php", &order("", "", "", TOTAL))],
        &[("app/Order.php", &order("", "", "", &format!("\n{TOTAL}")))],
    );

    assert_eq!(
        rows,
        "\
app/Order.php  modified  +1 −0
  #Order  class  modified  +1 −0  format  class Order
"
    );
}

#[test]
fn typescript_imports_and_docblocks_of_exported_symbols_follow_the_same_rules() {
    let cart = |import: &str, docblock: &str, total: &str| {
        format!(
            "{import}\n\n{docblock}\nexport function total(items: number[]): number {{\n  return {total};\n}}\n"
        )
    };
    let rows = rows_of_change(
        &[(
            "src/cart.ts",
            &cart("import { a } from './a';", "/** Old. */", "1"),
        )],
        &[(
            "src/cart.ts",
            &cart("import { b } from './b';", "/** New. */", "1"),
        )],
    );

    assert_eq!(
        rows,
        "\
src/cart.ts  modified  +2 −2
  #imports  imports  modified  +1 −1
  #total  function  modified  +1 −1  function total(items: number[]): number
"
    );
}

#[test]
fn typescript_renames_and_layout_only_changes_are_found() {
    let rows = rows_of_change(
        &[(
            "src/cart.tsx",
            "export function total(a: number) {\n  return a+1;\n}\n\nexport function tax(a: number) {\n  return a;\n}\n",
        )],
        &[(
            "src/cart.tsx",
            "export function total(a: number) {\n  return a + 1;\n}\n\nexport function vat(a: number) {\n  return a;\n}\n",
        )],
    );

    assert_eq!(
        rows,
        "\
src/cart.tsx  modified  +2 −2
  #total  function  modified  +1 −1  format  function total(a: number)
  #vat  function  modified  +1 −1  function vat(a: number)
"
    );
}
