#[allow(dead_code)]
mod support;

use support::{FakeGitHub, FixtureRepo, GuidedReview};

const PR: &str = "https://github.com/acme/shop/pull/7";

/// Commits `base_files`, then `head_files` on top, and returns the outline lines
/// from the piece rows down, with the header left out.
fn pieces_section(base_files: &[(&str, &str)], head_files: &[(&str, &str)]) -> Vec<String> {
    let repo = FixtureRepo::cloned_from("acme/shop");
    repo.write("README.md", "# Shop\n");
    for (path, content) in base_files {
        repo.write(path, content);
    }
    let base = repo.commit("base");
    for (path, content) in head_files {
        repo.write(path, content);
    }
    let head = repo.commit("head");
    let github = FakeGitHub::serving_pull("acme/shop", 7, &base, &head);

    GuidedReview::new(&repo, &github)
        .run(&["outline", PR])
        .stdout_with(&[(&base, "<base>"), (&head, "<head>")])
        .lines()
        .skip_while(|line| !line.is_empty())
        .skip(1)
        .map(str::to_string)
        .collect()
}

fn subject_lines(section: &[String]) -> Vec<&str> {
    section
        .iter()
        .map(String::as_str)
        .filter(|line| line.starts_with("    subjects:"))
        .collect()
}

fn lines_of_code(count: usize) -> String {
    (0..count).map(|n| format!("line {n}\n")).collect()
}

#[test]
fn outline_says_whether_the_worktree_is_at_head() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    repo.write("a.md", "one\n");
    let base = repo.commit("base");
    repo.write("a.md", "two\n");
    let head = repo.commit("head");
    let github = FakeGitHub::serving_pull("acme/shop", 7, &base, &head);
    let review = GuidedReview::new(&repo, &github);

    let at_head = review.run(&["outline", PR]).stdout_with(&[]);
    repo.git(&["checkout", "-q", &base]);
    let elsewhere = review.run(&["outline", PR]).stdout_with(&[]);

    assert!(at_head.contains("\nworktree at head: yes\n"), "{at_head}");
    assert!(
        elsewhere.contains("\nworktree at head: no\n"),
        "{elsewhere}"
    );
}

#[test]
fn small_prs_are_told_to_be_read_in_full() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    repo.write("a.md", "one\n");
    let base = repo.commit("base");
    repo.write("a.md", "two\n");
    let head = repo.commit("head");
    let github = FakeGitHub::serving_pull("acme/shop", 7, &base, &head);

    let outline = GuidedReview::new(&repo, &github)
        .run(&["outline", PR])
        .stdout_with(&[]);

    assert!(
        outline.contains(
            "\nsmall PR: 2 changed lines, under 300. Read the full diff with `guided-review show <pr-url> <every piece id>` instead of planning from this outline.\n"
        ),
        "{outline}"
    );
}

#[test]
fn big_prs_get_no_read_in_full_hint() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    repo.write("big.md", "x\n");
    let base = repo.commit("base");
    repo.write("big.md", &lines_of_code(300));
    let head = repo.commit("head");
    let github = FakeGitHub::serving_pull("acme/shop", 7, &base, &head);

    let outline = GuidedReview::new(&repo, &github)
        .run(&["outline", PR])
        .stdout_with(&[]);

    assert!(!outline.contains("small PR"), "{outline}");
}

const ORDER_PHP: &str = r#"<?php

class Order
{
    public function total(): int
    {
        return 1;
    }

    public function tax(): int
    {
        return 1;
    }
}
"#;

const ORDER_PHP_CHANGED: &str = r#"<?php

class Order
{
    public function total(): int
    {
        return 2;
    }

    public function tax(): int
    {
        return 2;
    }
}
"#;

const ORDER_TEST_PHP: &str = r#"<?php

class OrderTest
{
    public function testTotal(): void
    {
        assert(1 === 1);
    }
}
"#;

const ORDER_TEST_PHP_CHANGED: &str = r#"<?php

class OrderTest
{
    public function testTotal(): void
    {
        assert(2 === 2);
    }
}
"#;

#[test]
fn php_test_methods_are_test_pieces_with_subjects_found_by_name() {
    let section = pieces_section(
        &[
            ("app/Order.php", ORDER_PHP),
            ("tests/OrderTest.php", ORDER_TEST_PHP),
        ],
        &[
            ("app/Order.php", ORDER_PHP_CHANGED),
            ("tests/OrderTest.php", ORDER_TEST_PHP_CHANGED),
        ],
    );

    assert_eq!(
        section,
        [
            "app/Order.php  modified  +2 −2",
            "  #Order::total  method  modified  +1 −1  public function total(): int",
            "  #Order::tax  method  modified  +1 −1  public function tax(): int",
            "",
            "tests/OrderTest.php  modified  +1 −1",
            "  #OrderTest::testTotal  test  modified  +1 −1  public function testTotal(): void",
            "    subjects: app/Order.php#Order::total (name), app/Order.php#Order::tax (name)",
        ]
    );
}

#[test]
fn several_name_matches_are_listed_best_first_up_to_three() {
    let legacy = ORDER_PHP.replace("class Order", "namespace Legacy;\n\nclass Order");
    let legacy_changed =
        ORDER_PHP_CHANGED.replace("class Order", "namespace Legacy;\n\nclass Order");
    let section = pieces_section(
        &[
            ("app/Order.php", ORDER_PHP),
            ("app/Legacy/Order.php", &legacy),
            ("tests/OrderTest.php", ORDER_TEST_PHP),
        ],
        &[
            ("app/Order.php", ORDER_PHP_CHANGED),
            ("app/Legacy/Order.php", &legacy_changed),
            ("tests/OrderTest.php", ORDER_TEST_PHP_CHANGED),
        ],
    );

    assert_eq!(
        subject_lines(&section),
        [
            "    subjects: app/Legacy/Order.php#Order::total (name), app/Order.php#Order::total (name), app/Legacy/Order.php#Order::tax (name), +1 more"
        ]
    );
}

#[test]
fn renamed_tests_are_paired_by_their_head_names() {
    let section = pieces_section(
        &[
            ("app/Invoice.php", &ORDER_PHP.replace("Order", "Invoice")),
            ("tests/OrderTest.php", ORDER_TEST_PHP),
        ],
        &[
            (
                "app/Invoice.php",
                &ORDER_PHP_CHANGED.replace("Order", "Invoice"),
            ),
            (
                "tests/InvoiceTest.php",
                &ORDER_TEST_PHP_CHANGED.replace("Order", "Invoice"),
            ),
        ],
    );

    assert_eq!(
        subject_lines(&section),
        [
            "    subjects: app/Invoice.php#Invoice::total (name), app/Invoice.php#Invoice::tax (name)"
        ]
    );
}

#[test]
fn imports_are_the_fallback_after_names() {
    let cart_ts = "export function addItem(): number {\n  return 1;\n}\n";
    let cart_ts_changed = "export function addItem(): number {\n  return 2;\n}\n";
    let test_ts = |value: u8| {
        format!(
            "import {{ addItem }} from \"./cart\";\n\nit(\"adds\", () => {{\n  expect(addItem()).toBe({value});\n}});\n"
        )
    };
    let section = pieces_section(
        &[
            ("src/cart.ts", cart_ts),
            ("src/basket.test.ts", &test_ts(1)),
        ],
        &[
            ("src/cart.ts", cart_ts_changed),
            ("src/basket.test.ts", &test_ts(2)),
        ],
    );

    assert_eq!(
        subject_lines(&section),
        ["    subjects: src/cart.ts#addItem (import)"]
    );
}

#[test]
fn a_test_without_candidates_says_so_instead_of_guessing() {
    let section = pieces_section(
        &[(
            "tests/LonelyTest.php",
            ORDER_TEST_PHP.replace("Order", "Lonely").as_str(),
        )],
        &[(
            "tests/LonelyTest.php",
            ORDER_TEST_PHP_CHANGED.replace("Order", "Lonely").as_str(),
        )],
    );

    assert_eq!(subject_lines(&section), ["    subjects: none found"]);
}

#[test]
fn tests_that_share_their_subjects_say_same_instead_of_repeating_the_list() {
    let tests = |amount: u8| {
        ORDER_TEST_PHP.replace(
            "    public function testTotal(): void\n    {\n        assert(1 === 1);\n    }\n",
            &format!(
                "    public function testSums(): void\n    {{\n        assert(1 === {amount});\n    }}\n\n    public function testRounding(): void\n    {{\n        assert(1 === {amount});\n    }}\n"
            ),
        )
    };
    let section = pieces_section(
        &[
            ("app/Order.php", ORDER_PHP),
            ("tests/OrderTest.php", &tests(1)),
        ],
        &[
            ("app/Order.php", ORDER_PHP_CHANGED),
            ("tests/OrderTest.php", &tests(2)),
        ],
    );

    assert_eq!(
        subject_lines(&section),
        [
            "    subjects: app/Order.php#Order::total (name), app/Order.php#Order::tax (name)",
            "    subjects: same",
        ]
    );
}

#[test]
fn imports_and_top_pieces_are_never_offered_as_subjects() {
    let with_import = |body: &str| body.replace("class Order", "use App\\Money;\n\nclass Order");
    let section = pieces_section(
        &[
            ("app/Order.php", &with_import(ORDER_PHP)),
            ("tests/OrderTest.php", ORDER_TEST_PHP),
        ],
        &[
            (
                "app/Order.php",
                &with_import(ORDER_PHP_CHANGED).replace("App\\Money", "App\\Cash"),
            ),
            ("tests/OrderTest.php", ORDER_TEST_PHP_CHANGED),
        ],
    );

    assert!(
        section.iter().any(|line| line.starts_with("  #imports")),
        "{section:#?}"
    );
    assert_eq!(
        subject_lines(&section),
        ["    subjects: app/Order.php#Order::total (name), app/Order.php#Order::tax (name)"]
    );
}
