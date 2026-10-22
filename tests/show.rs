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

#[test]
fn show_around_a_hunk_prints_the_lines_around_it_at_head() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    let config = |tenth: &str| -> String {
        (1..=20)
            .map(|number| match number {
                10 => format!("{tenth}\n"),
                _ => format!("key{number}: {number}\n"),
            })
            .collect()
    };
    repo.write("config.yml", &config("key10: 10"));
    let base = repo.commit("base");
    repo.write("config.yml", &config("key10: ten"));
    let head = repo.commit("head");
    let github = FakeGitHub::serving_pull("acme/shop", 7, &base, &head);

    let run = GuidedReview::new(&repo, &github).run(&["show", "--around", PR, "config.yml#hunk-1"]);

    assert_eq!(
        run.stdout_with(&[]),
        "\
config.yml#hunk-1  hunk  modified  +1 −1
-10: key10: 10
+10: key10: ten

around, at head (lines 7-13):
7: key7: 7
8: key8: 8
9: key9: 9
10: key10: ten
11: key11: 11
12: key12: 12
13: key13: 13
"
    );
}

#[test]
fn show_of_a_file_path_prints_every_piece_of_that_file() {
    let (repo, github) = order_pr();

    let run = GuidedReview::new(&repo, &github).run(&["show", PR, "app/Order.php"]);

    assert_eq!(
        run.stdout_with(&[]),
        "\
app/Order.php#Order::total  method  modified  +1 −1
-12: \x20       $base = 1;
+12: \x20       $base = 2;
"
    );
}

#[test]
fn show_prints_hunks_in_line_order_when_a_blank_line_change_joins_the_piece() {
    let repo = FixtureRepo::cloned_from("acme/shop");
    repo.write(
        "app/Order.php",
        "<?php\n\nclass Order\n{\n    public function total(): int\n    {\n        return 1;\n    }\n}\n",
    );
    let base = repo.commit("base");
    repo.write(
        "app/Order.php",
        "<?php\n\nclass Order\n{\n\n    public function total(): int\n    {\n        return 2;\n    }\n}\n",
    );
    let head = repo.commit("head");
    let github = FakeGitHub::serving_pull("acme/shop", 7, &base, &head);

    let run = GuidedReview::new(&repo, &github).run(&["show", PR, "app/Order.php#Order::total"]);

    assert_eq!(
        run.stdout_with(&[]),
        "\
app/Order.php#Order::total  method  modified  +2 −1
+5: 
-7: \x20       return 1;
+8: \x20       return 2;
"
    );
}

fn service_pr(base_source: &str, head_source: &str) -> (FixtureRepo, FakeGitHub) {
    let repo = FixtureRepo::cloned_from("acme/shop");
    repo.write("app/Service.php", base_source);
    let base = repo.commit("base");
    repo.write("app/Service.php", head_source);
    let head = repo.commit("head");
    let github = FakeGitHub::serving_pull("acme/shop", 7, &base, &head);
    (repo, github)
}

#[test]
fn show_prints_each_hunk_of_a_piece_with_its_removed_and_added_lines_together() {
    let method = |first: &str, second: &str| {
        format!(
            "<?php\n\nclass Service\n{{\n    public function run(): int\n    {{\n        $a = {first};\n        $keep = 0;\n        $b = {second};\n\n        return $a + $b;\n    }}\n}}\n"
        )
    };
    let (repo, github) = service_pr(&method("1", "1"), &method("2", "2"));

    let run = GuidedReview::new(&repo, &github).run(&["show", PR, "app/Service.php#Service::run"]);

    assert_eq!(
        run.stdout_with(&[]),
        "\
app/Service.php#Service::run  method  modified  +2 −2
-7: \x20       $a = 1;
+7: \x20       $a = 2;
-9: \x20       $b = 1;
+9: \x20       $b = 2;
"
    );
}

#[test]
fn show_around_imports_prints_the_lines_around_them_at_head() {
    let source = |import: &str| {
        format!(
            "<?php\n\nnamespace App;\n\nuse App\\A;\n{import}\nuse App\\C;\n\nclass Service\n{{\n}}\n"
        )
    };
    let (repo, github) = service_pr(&source("use App\\B;"), &source("use App\\Bee;"));

    let run =
        GuidedReview::new(&repo, &github).run(&["show", "--around", PR, "app/Service.php#imports"]);

    assert_eq!(
        run.stdout_with(&[]),
        "\
app/Service.php#imports  imports  modified  +1 −1
-6: use App\\B;
+6: use App\\Bee;

around, at head (lines 3-9):
3: namespace App;
4: 
5: use App\\A;
6: use App\\Bee;
7: use App\\C;
8: 
9: class Service
"
    );
}

#[test]
fn show_around_top_prints_the_lines_around_it_at_head() {
    let source = |strict: &str| {
        format!(
            "<?php\n\ndeclare(strict_types={strict});\n\nnamespace App;\n\nclass Service\n{{\n}}\n"
        )
    };
    let (repo, github) = service_pr(&source("0"), &source("1"));

    let run =
        GuidedReview::new(&repo, &github).run(&["show", "--around", PR, "app/Service.php#(top)"]);

    assert!(run.success, "{}", run.stderr);
    assert!(
        run.stdout
            .contains("around, at head (lines 1-6):\n1: <?php\n2: \n3: declare(strict_types=1);\n"),
        "{}",
        run.stdout
    );
}
