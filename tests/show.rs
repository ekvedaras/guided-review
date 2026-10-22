#[allow(dead_code)]
mod support;

use support::{FakeGitHub, FixtureRepo, GuidedReview};

const PR: &str = "https://github.com/acme/shop/pull/7";

fn order_pr() -> (FixtureRepo, FakeGitHub) {
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
        $base = 1;

        return $base;
    }
}
"#,
    );
    repo.write("notes.txt", "one\n");
    let base = repo.commit("base");
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
        $base = 2;

        return $base;
    }
}
"#,
    );
    repo.write("notes.txt", "one\ntwo\n");
    let head = repo.commit("head");
    let github = FakeGitHub::serving_pull("acme/shop", 7, &base, &head);
    (repo, github)
}

#[test]
fn show_prints_the_diff_of_the_requested_pieces_only() {
    let (repo, github) = order_pr();

    let run = GuidedReview::new(&repo, &github).run(&[
        "show",
        PR,
        "app/Order.php#Order::total",
        "notes.txt",
    ]);

    assert_eq!(
        run.stdout_with(&[]),
        "\
app/Order.php#Order::total  method  modified  +1 −1
-12: \x20       $base = 1;
+12: \x20       $base = 2;

notes.txt  file  modified  +1 −0
+2: two
"
    );
}

#[test]
fn show_around_adds_the_piece_as_it_is_at_head() {
    let (repo, github) = order_pr();

    let run = GuidedReview::new(&repo, &github).run(&[
        "show",
        "--around",
        PR,
        "app/Order.php#Order::total",
    ]);

    assert_eq!(
        run.stdout_with(&[]),
        "\
app/Order.php#Order::total  method  modified  +1 −1
-12: \x20       $base = 1;
+12: \x20       $base = 2;

around, at head (lines 10-15):
10:     public function total(): int
11:     {
12:         $base = 2;
13: 
14:         return $base;
15:     }
"
    );
}

#[test]
fn show_fails_on_an_unknown_piece_id_before_printing_anything() {
    let (repo, github) = order_pr();

    let run = GuidedReview::new(&repo, &github).run(&[
        "show",
        PR,
        "notes.txt",
        "app/Order.php#Order::nope",
    ]);

    assert!(!run.success);
    assert_eq!(run.stdout, "");
    assert!(
        run.stderr
            .contains("unknown piece id `app/Order.php#Order::nope`"),
        "{}",
        run.stderr
    );
}
