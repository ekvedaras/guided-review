#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "a panic in a test helper is the test failing"
)]
#![allow(
    clippy::needless_pass_by_value,
    reason = "test helpers take owned arguments so a case reads as a plain call"
)]

mod support;

use support::{FakeGitHub, FixtureRepo, GuidedReview};

const PR: &str = "https://github.com/acme/shop/pull/7";

#[test]
fn outline_lists_a_changed_method_as_one_piece() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    repo.write(
        "app/Order.php",
        r"<?php

class Order
{
    public function total(): int
    {
        return 1;
    }
}
",
    );
    let base = repo.commit("base");
    repo.write(
        "app/Order.php",
        r"<?php

class Order
{
    public function total(): int
    {
        return 2;
    }
}
",
    );
    let head = repo.commit("head");
    let github = FakeGitHub::serving_pull("acme/shop", 7, &base, &head);

    let run = GuidedReview::new(&repo, &github).run(&["outline", PR]);

    assert_eq!(
        run.stdout_with(&[(&base, "<base>"), (&head, "<head>")]),
        "\
pr: https://github.com/acme/shop/pull/7
base: <base>
head: <head>
worktree at head: yes
piece id: <file>#<symbol>, or <file> for a whole-file piece
1 file, 1 piece, +1 −1
small PR: 2 changed lines, under 300. Read the full diff with `guided-review show <pr-url> <every piece id>` instead of planning from this outline.

app/Order.php  modified  +1 −1
  #Order::total  method  modified  +1 −1  public function total(): int
"
    );
}

#[test]
fn piece_ids_do_not_change_when_lines_move() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    repo.write(
        "app/Order.php",
        r"<?php

class Order
{
    public function discount(): int
    {
        return 0;
    }

    public function total(): int
    {
        return 1;
    }
}
",
    );
    let base = repo.commit("base");
    repo.write(
        "app/Order.php",
        r"<?php

class Order
{
    public function discount(): int
    {
        $percent = 10;

        return $percent;
    }

    public function total(): int
    {
        return 2;
    }
}
",
    );
    let head = repo.commit("head");
    let github = FakeGitHub::serving_pull("acme/shop", 7, &base, &head);

    let run = GuidedReview::new(&repo, &github).run(&["outline", PR]);

    assert_eq!(
        run.stdout_with(&[(&base, "<base>"), (&head, "<head>")]),
        "\
pr: https://github.com/acme/shop/pull/7
base: <base>
head: <head>
worktree at head: yes
piece id: <file>#<symbol>, or <file> for a whole-file piece
1 file, 2 pieces, +4 −2
small PR: 6 changed lines, under 300. Read the full diff with `guided-review show <pr-url> <every piece id>` instead of planning from this outline.

app/Order.php  modified  +4 −2
  #Order::discount  method  modified  +3 −1  public function discount(): int
  #Order::total  method  modified  +1 −1  public function total(): int
"
    );
}

#[test]
fn outline_shows_deleted_methods_changed_functions_and_changed_classes() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    repo.write(
        "app/Order.php",
        r"<?php

final class Order
{
    private int $cents = 0;

    #[Deprecated]
    public function legacyTotal(): int
    {
        return 1;
    }
}
",
    );
    repo.write(
        "app/helpers.php",
        r#"<?php

function money(int $cents): string
{
    return "$cents";
}
"#,
    );
    let base = repo.commit("base");
    repo.write(
        "app/Order.php",
        r"<?php

final class Order
{
    private int $cents = 100;

}
",
    );
    repo.write(
        "app/helpers.php",
        r"<?php

function money(int $cents): string
{
    return number_format($cents / 100, 2);
}
",
    );
    let head = repo.commit("head");
    let github = FakeGitHub::serving_pull("acme/shop", 7, &base, &head);

    let run = GuidedReview::new(&repo, &github).run(&["outline", PR]);

    assert_eq!(
        run.stdout_with(&[(&base, "<base>"), (&head, "<head>")]),
        "\
pr: https://github.com/acme/shop/pull/7
base: <base>
head: <head>
worktree at head: yes
piece id: <file>#<symbol>, or <file> for a whole-file piece
2 files, 3 pieces, +2 −7
small PR: 9 changed lines, under 300. Read the full diff with `guided-review show <pr-url> <every piece id>` instead of planning from this outline.

app/Order.php  modified  +1 −6
  #Order  class  modified  +1 −1  final class Order
  #Order::legacyTotal  method  deleted  +0 −5  public function legacyTotal(): int

app/helpers.php  modified  +1 −1
  #money  function  modified  +1 −1  function money(int $cents): string
"
    );
}

const ORDER_V1: &str = r"<?php

class Order
{
    public function total(): int
    {
        return 1;
    }
}
";

const ORDER_V2: &str = r"<?php

class Order
{
    public function total(): int
    {
        return 2;
    }
}
";

const ORDER_OUTLINE: &str = "\
pr: https://github.com/acme/shop/pull/7
base: <base>
head: <head>
worktree at head: yes
piece id: <file>#<symbol>, or <file> for a whole-file piece
1 file, 1 piece, +1 −1
small PR: 2 changed lines, under 300. Read the full diff with `guided-review show <pr-url> <every piece id>` instead of planning from this outline.

app/Order.php  modified  +1 −1
  #Order::total  method  modified  +1 −1  public function total(): int
";

#[test]
fn outline_uses_the_clone_from_config_when_the_current_folder_is_not_a_clone() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    repo.write("app/Order.php", ORDER_V1);
    let base = repo.commit("base");
    repo.write("app/Order.php", ORDER_V2);
    let head = repo.commit("head");
    let github = FakeGitHub::serving_pull("acme/shop", 7, &base, &head);
    let elsewhere = tempfile::TempDir::new().unwrap();
    std::fs::create_dir_all(repo.home().join(".guided-review")).unwrap();
    std::fs::write(
        repo.home().join(".guided-review/config.toml"),
        format!("[clones]\n\"Acme/Shop\" = \"{}\"\n", repo.path().display()),
    )
    .unwrap();

    let run = GuidedReview::new(&repo, &github)
        .in_dir(elsewhere.path())
        .run(&["outline", PR]);

    assert_eq!(
        run.stdout_with(&[(&base, "<base>"), (&head, "<head>")]),
        ORDER_OUTLINE
    );
}

#[test]
fn outline_ignores_uncommitted_work_in_the_working_tree() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    repo.write("app/Order.php", ORDER_V1);
    let base = repo.commit("base");
    repo.write("app/Order.php", ORDER_V2);
    let head = repo.commit("head");
    repo.write("app/Order.php", "<?php\n\nfunction uncommitted() {}\n");
    repo.write("app/Draft.php", "<?php\n\nclass Draft {}\n");
    let github = FakeGitHub::serving_pull("acme/shop", 7, &base, &head);

    let run = GuidedReview::new(&repo, &github).run(&["outline", PR]);

    assert_eq!(
        run.stdout_with(&[(&base, "<base>"), (&head, "<head>")]),
        ORDER_OUTLINE.replace("worktree at head: yes", "worktree at head: no")
    );
}

#[test]
fn outline_compares_head_with_where_it_branched_off_the_base_branch() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    repo.write("app/Order.php", ORDER_V1);
    repo.write("app/Cart.php", "<?php\n\nclass Cart\n{\n}\n");
    let branched_from = repo.commit("base");
    repo.write("app/Order.php", ORDER_V2);
    let head = repo.commit("head");
    repo.branch_from("main", &branched_from);
    repo.write(
        "app/Cart.php",
        "<?php\n\nclass Cart\n{\n    public int $items = 0;\n}\n",
    );
    let base_branch = repo.commit("base branch moves on");
    let github = FakeGitHub::serving_pull("acme/shop", 7, &base_branch, &head);

    let run = GuidedReview::new(&repo, &github).run(&["outline", PR]);

    assert_eq!(
        run.stdout_with(&[(&branched_from, "<base>"), (&head, "<head>")]),
        ORDER_OUTLINE.replace("worktree at head: yes", "worktree at head: no")
    );
}

#[test]
fn outline_lists_a_non_php_file_as_one_whole_file_piece() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    repo.write("docs/setup.txt", "# Setup\n\nRun it.\n");
    let base = repo.commit("base");
    repo.write("docs/setup.txt", "# Setup\n\nRun it twice.\nThen stop.\n");
    let head = repo.commit("head");
    let github = FakeGitHub::serving_pull("acme/shop", 7, &base, &head);

    let run = GuidedReview::new(&repo, &github).run(&["outline", PR]);

    assert_eq!(
        run.stdout_with(&[(&base, "<base>"), (&head, "<head>")]),
        "\
pr: https://github.com/acme/shop/pull/7
base: <base>
head: <head>
worktree at head: yes
piece id: <file>#<symbol>, or <file> for a whole-file piece
1 file, 1 piece, +2 −1
small PR: 3 changed lines, under 300. Read the full diff with `guided-review show <pr-url> <every piece id>` instead of planning from this outline.

docs/setup.txt  modified  +2 −1
  docs/setup.txt  file  modified  +2 −1
"
    );
}

#[test]
fn signatures_are_one_line_without_comments() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    repo.write(
        "app/Order.php",
        r"<?php

class Order
{
    public function place(
        Cart $cart, // TODO: remove
        /* later */ string $url = 'https://shop.test',
    ): void {
        $cart->empty();
    }
}
",
    );
    let base = repo.commit("base");
    repo.write(
        "app/Order.php",
        r"<?php

class Order
{
    public function place(
        Cart $cart, // TODO: remove
        /* later */ string $url = 'https://shop.test',
    ): void {
        $cart->clear();
    }
}
",
    );
    let head = repo.commit("head");
    let github = FakeGitHub::serving_pull("acme/shop", 7, &base, &head);

    let run = GuidedReview::new(&repo, &github).run(&["outline", PR]);

    assert!(
        run.stdout_with(&[]).ends_with(
            "  #Order::place  method  modified  +1 −1  public function place(Cart $cart, string $url = 'https://shop.test'): void\n"
        ),
        "{}",
        run.stdout
    );
}

#[test]
fn outline_cuts_php_files_with_spaces_in_their_path() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    repo.write("app/Gift Card.php", ORDER_V1);
    let base = repo.commit("base");
    repo.write("app/Gift Card.php", ORDER_V2);
    let head = repo.commit("head");
    let github = FakeGitHub::serving_pull("acme/shop", 7, &base, &head);

    let run = GuidedReview::new(&repo, &github).run(&["outline", PR]);

    assert_eq!(
        run.stdout_with(&[(&base, "<base>"), (&head, "<head>")]),
        ORDER_OUTLINE.replace("app/Order.php", "app/Gift Card.php")
    );
}

#[test]
fn outline_uses_the_current_folder_even_when_config_is_broken() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    repo.write("app/Order.php", ORDER_V1);
    let base = repo.commit("base");
    repo.write("app/Order.php", ORDER_V2);
    let head = repo.commit("head");
    let github = FakeGitHub::serving_pull("acme/shop", 7, &base, &head);
    std::fs::create_dir_all(repo.home().join(".guided-review")).unwrap();
    std::fs::write(repo.home().join(".guided-review/config.toml"), "[clones\n").unwrap();

    let run = GuidedReview::new(&repo, &github).run(&["outline", PR]);

    assert_eq!(
        run.stdout_with(&[(&base, "<base>"), (&head, "<head>")]),
        ORDER_OUTLINE
    );
}

#[test]
fn methods_of_anonymous_classes_belong_to_the_named_symbol_around_them() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    let order = |total: &str| {
        format!(
            r"<?php

class Order
{{
    public function pricing(): Pricing
    {{
        return new class implements Pricing {{
            public function total(): int
            {{
                return {total};
            }}
        }};
    }}
}}
"
        )
    };
    repo.write("app/Order.php", &order("1"));
    let base = repo.commit("base");
    repo.write("app/Order.php", &order("2"));
    let head = repo.commit("head");
    let github = FakeGitHub::serving_pull("acme/shop", 7, &base, &head);

    let run = GuidedReview::new(&repo, &github).run(&["outline", PR]);

    assert!(
        run.stdout_with(&[]).ends_with(
            "  #Order::pricing  method  modified  +1 −1  public function pricing(): Pricing\n"
        ),
        "{}",
        run.stdout
    );
}

#[test]
fn a_renamed_php_file_without_line_changes_is_one_whole_file_piece() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    repo.write("app/Order.php", ORDER_V1);
    let base = repo.commit("base");
    repo.git(&["mv", "app/Order.php", "app/Purchase.php"]);
    let head = repo.commit("head");
    let github = FakeGitHub::serving_pull("acme/shop", 7, &base, &head);

    let run = GuidedReview::new(&repo, &github).run(&["outline", PR]);

    assert_eq!(
        run.stdout_with(&[(&base, "<base>"), (&head, "<head>")]),
        "\
pr: https://github.com/acme/shop/pull/7
base: <base>
head: <head>
worktree at head: yes
piece id: <file>#<symbol>, or <file> for a whole-file piece
1 file, 1 piece, +0 −0
small PR: 0 changed lines, under 300. Read the full diff with `guided-review show <pr-url> <every piece id>` instead of planning from this outline.

app/Purchase.php  renamed  +0 −0
  app/Purchase.php  file  modified  +0 −0
"
    );
}

const OUTLINE_HEADER: &str = "\
pr: https://github.com/acme/shop/pull/7
base: <base>
head: <head>
worktree at head: yes
piece id: <file>#<symbol>, or <file> for a whole-file piece
";

fn outline_of(repo: &FixtureRepo, base: &str, head: &str) -> String {
    let github = FakeGitHub::serving_pull("acme/shop", 7, base, head);
    let run = GuidedReview::new(repo, &github).run(&["outline", PR]);
    run.stdout_with(&[(base, "<base>"), (head, "<head>")])
        .strip_prefix(OUTLINE_HEADER)
        .expect("outline starts with the header")
        .to_owned()
}

#[test]
fn a_deleted_php_file_is_one_piece() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    repo.write(
        "app/Order.php",
        "<?php\n\nclass Order\n{\n    public function a(): int\n    {\n        return 1;\n    }\n\n    public function b(): int\n    {\n        return 2;\n    }\n}\n",
    );
    let base = repo.commit("base");
    repo.git(&["rm", "-q", "app/Order.php"]);
    let head = repo.commit("head");

    assert_eq!(
        outline_of(&repo, &base, &head),
        "\
1 file, 1 piece, +0 −14
small PR: 14 changed lines, under 300. Read the full diff with `guided-review show <pr-url> <every piece id>` instead of planning from this outline.

app/Order.php  deleted  +0 −14
  app/Order.php  file  deleted  +0 −14
"
    );
}

#[test]
fn a_new_php_file_is_one_piece_per_symbol() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    repo.write("README.md", "x\n");
    let base = repo.commit("base");
    repo.write(
        "app/Order.php",
        "<?php\n\nclass Order\n{\n    public function a(): int\n    {\n        return 1;\n    }\n\n    public function b(): int\n    {\n        return 2;\n    }\n}\n",
    );
    let head = repo.commit("head");

    let outline = outline_of(&repo, &base, &head);

    assert!(outline.contains("  #Order  class  added"), "{outline}");
    assert!(
        outline.contains("  #Order::a  method  added  +4 −0"),
        "{outline}"
    );
    assert!(
        outline.contains("  #Order::b  method  added  +4 −0"),
        "{outline}"
    );
}

#[test]
fn a_known_generated_file_is_one_piece_marked_generated() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    repo.write(
        "composer.lock",
        "{\n  \"a\": 1,\n  \"b\": 2,\n  \"c\": 3\n}\n",
    );
    repo.write("tests/__snapshots__/order.snap", "one\n");
    let base = repo.commit("base");
    repo.write(
        "composer.lock",
        "{\n  \"a\": 9,\n  \"b\": 2,\n  \"c\": 9\n}\n",
    );
    repo.write("tests/__snapshots__/order.snap", "two\n");
    let head = repo.commit("head");

    assert_eq!(
        outline_of(&repo, &base, &head),
        "\
2 files, 2 pieces, +3 −3
small PR: 6 changed lines, under 300. Read the full diff with `guided-review show <pr-url> <every piece id>` instead of planning from this outline.

composer.lock  modified  +2 −2
  composer.lock  file  modified  +2 −2  generated

tests/__snapshots__/order.snap  modified  +1 −1
  tests/__snapshots__/order.snap  file  modified  +1 −1  generated
"
    );
}

#[test]
fn a_file_marked_linguist_generated_is_one_piece_marked_generated() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    repo.write(
        ".gitattributes",
        "*.gen.php linguist-generated\nkeep.gen.php -linguist-generated\n",
    );
    repo.write(
        "app/Api.gen.php",
        "<?php\n\nfunction a()\n{\n    return 1;\n}\n",
    );
    repo.write(
        "keep.gen.php",
        "<?php\n\nfunction k()\n{\n    return 1;\n}\n",
    );
    let base = repo.commit("base");
    repo.write(
        "app/Api.gen.php",
        "<?php\n\nfunction a()\n{\n    return 2;\n}\n",
    );
    repo.write(
        "keep.gen.php",
        "<?php\n\nfunction k()\n{\n    return 2;\n}\n",
    );
    let head = repo.commit("head");

    assert_eq!(
        outline_of(&repo, &base, &head),
        "\
2 files, 2 pieces, +2 −2
small PR: 4 changed lines, under 300. Read the full diff with `guided-review show <pr-url> <every piece id>` instead of planning from this outline.

app/Api.gen.php  modified  +1 −1
  app/Api.gen.php  file  modified  +1 −1  generated

keep.gen.php  modified  +1 −1
  #k  function  modified  +1 −1  function k()
"
    );
}

#[test]
fn yaml_json_xml_and_markdown_are_cut_one_piece_per_hunk() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    let lines = |word: &str| {
        (1..=9)
            .map(|n| {
                if n == 2 || n == 8 {
                    format!("{word} {n}\n")
                } else {
                    format!("line {n}\n")
                }
            })
            .collect::<String>()
    };
    for path in ["ci.yml", "ci.yaml", "a.json", "a.xml", "README.md"] {
        repo.write(path, &lines("old"));
    }
    let base = repo.commit("base");
    for path in ["ci.yml", "ci.yaml", "a.json", "a.xml", "README.md"] {
        repo.write(path, &lines("new"));
    }
    let head = repo.commit("head");

    let outline = outline_of(&repo, &base, &head);

    assert!(
        outline.starts_with("5 files, 10 pieces, +10 −10\n"),
        "{outline}"
    );
    assert!(
        outline.contains(
            "a.json  modified  +2 −2
  #hunk-1  hunk  modified  +1 −1  @@ -2 +2 @@
  #hunk-2  hunk  modified  +1 −1  @@ -8 +8 @@
"
        ),
        "{outline}"
    );
}

#[test]
fn a_binary_file_is_one_piece() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    repo.write("assets/data.bin", "\0\u{1}\u{2}one");
    let base = repo.commit("base");
    repo.write("assets/data.bin", "\0\u{1}\u{2}two");
    repo.write("assets/new.bin", "\0\u{3}");
    let head = repo.commit("head");

    assert_eq!(
        outline_of(&repo, &base, &head),
        "\
2 files, 2 pieces, +0 −0
small PR: 0 changed lines, under 300. Read the full diff with `guided-review show <pr-url> <every piece id>` instead of planning from this outline.

assets/data.bin  modified  +0 −0
  assets/data.bin  file  modified  +0 −0

assets/new.bin  added  +0 −0
  assets/new.bin  file  added  +0 −0
"
    );
}
