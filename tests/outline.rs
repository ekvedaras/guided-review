mod support;

use support::{FakeGitHub, FixtureRepo, GuidedReview};

const PR: &str = "https://github.com/acme/shop/pull/7";

#[test]
fn outline_lists_a_changed_method_as_one_piece() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    repo.write(
        "app/Order.php",
        r#"<?php

class Order
{
    public function total(): int
    {
        return 1;
    }
}
"#,
    );
    let base = repo.commit("base");
    repo.write(
        "app/Order.php",
        r#"<?php

class Order
{
    public function total(): int
    {
        return 2;
    }
}
"#,
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
1 file, 1 piece, +1 −1

app/Order.php  modified  +1 −1
  app/Order.php#Order::total  method  modified  +1 −1  public function total(): int
"
    );
}

#[test]
fn piece_ids_do_not_change_when_lines_move() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    repo.write(
        "app/Order.php",
        r#"<?php

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
"#,
    );
    let base = repo.commit("base");
    repo.write(
        "app/Order.php",
        r#"<?php

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
"#,
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
1 file, 2 pieces, +4 −2

app/Order.php  modified  +4 −2
  app/Order.php#Order::discount  method  modified  +3 −1  public function discount(): int
  app/Order.php#Order::total  method  modified  +1 −1  public function total(): int
"
    );
}

#[test]
fn outline_shows_deleted_methods_changed_functions_and_changed_classes() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    repo.write(
        "app/Order.php",
        r#"<?php

final class Order
{
    private int $cents = 0;

    #[Deprecated]
    public function legacyTotal(): int
    {
        return 1;
    }
}
"#,
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
        r#"<?php

final class Order
{
    private int $cents = 100;

}
"#,
    );
    repo.write(
        "app/helpers.php",
        r#"<?php

function money(int $cents): string
{
    return number_format($cents / 100, 2);
}
"#,
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
2 files, 3 pieces, +2 −7

app/Order.php  modified  +1 −6
  app/Order.php#Order  class  modified  +1 −1  final class Order
  app/Order.php#Order::legacyTotal  method  deleted  +0 −5  public function legacyTotal(): int

app/helpers.php  modified  +1 −1
  app/helpers.php#money  function  modified  +1 −1  function money(int $cents): string
"
    );
}

const ORDER_V1: &str = r#"<?php

class Order
{
    public function total(): int
    {
        return 1;
    }
}
"#;

const ORDER_V2: &str = r#"<?php

class Order
{
    public function total(): int
    {
        return 2;
    }
}
"#;

const ORDER_OUTLINE: &str = "\
pr: https://github.com/acme/shop/pull/7
base: <base>
head: <head>
1 file, 1 piece, +1 −1

app/Order.php  modified  +1 −1
  app/Order.php#Order::total  method  modified  +1 −1  public function total(): int
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
fn outline_fails_without_a_local_clone() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    repo.write("app/Order.php", ORDER_V1);
    let base = repo.commit("base");
    repo.write("app/Order.php", ORDER_V2);
    let head = repo.commit("head");
    let github = FakeGitHub::serving_pull("acme/shop", 7, &base, &head);
    let other_repo = FixtureRepo::cloned_from("acme/blog");

    let run = GuidedReview::new(&repo, &github)
        .in_dir(other_repo.path())
        .run(&["outline", PR]);

    assert!(!run.success);
    assert!(
        run.stderr.contains("no local clone of acme/shop found"),
        "{}",
        run.stderr
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
        ORDER_OUTLINE
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
        ORDER_OUTLINE
    );
}

#[test]
fn outline_lists_a_non_php_file_as_one_whole_file_piece() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    repo.write("docs/setup.md", "# Setup\n\nRun it.\n");
    let base = repo.commit("base");
    repo.write("docs/setup.md", "# Setup\n\nRun it twice.\nThen stop.\n");
    let head = repo.commit("head");
    let github = FakeGitHub::serving_pull("acme/shop", 7, &base, &head);

    let run = GuidedReview::new(&repo, &github).run(&["outline", PR]);

    assert_eq!(
        run.stdout_with(&[(&base, "<base>"), (&head, "<head>")]),
        "\
pr: https://github.com/acme/shop/pull/7
base: <base>
head: <head>
1 file, 1 piece, +2 −1

docs/setup.md  modified  +2 −1
  docs/setup.md  file  modified  +2 −1
"
    );
}

#[test]
fn signatures_are_one_line_without_comments() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    repo.write(
        "app/Order.php",
        r#"<?php

class Order
{
    public function place(
        Cart $cart, // TODO: remove
        /* later */ string $url = 'https://shop.test',
    ): void {
        $cart->empty();
    }
}
"#,
    );
    let base = repo.commit("base");
    repo.write(
        "app/Order.php",
        r#"<?php

class Order
{
    public function place(
        Cart $cart, // TODO: remove
        /* later */ string $url = 'https://shop.test',
    ): void {
        $cart->clear();
    }
}
"#,
    );
    let head = repo.commit("head");
    let github = FakeGitHub::serving_pull("acme/shop", 7, &base, &head);

    let run = GuidedReview::new(&repo, &github).run(&["outline", PR]);

    assert!(
        run.stdout_with(&[]).ends_with(
            "  app/Order.php#Order::place  method  modified  +1 −1  public function place( Cart $cart, string $url = 'https://shop.test', ): void\n"
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
            r#"<?php

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
"#
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
            "  app/Order.php#Order::pricing  method  modified  +1 −1  public function pricing(): Pricing\n"
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
1 file, 1 piece, +0 −0

app/Purchase.php  renamed  +0 −0
  app/Purchase.php  file  modified  +0 −0
"
    );
}
