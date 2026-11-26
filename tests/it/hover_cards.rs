use std::sync::Arc;

use guided_review::code_source::{BlobInfo, CodeSource, FileDiff};
use guided_review::codebase::Codebase;
use guided_review::file_path::FilePath;
use guided_review::local_git::LocalGit;
use guided_review::navigation::{
    AnsweredBy, Column, FallbackReason, ServerAnswer, ServerName, Waiting, Word,
};
use guided_review::pull_request::Sha;
use guided_review::review_subject::WorkTreeRoot;
use guided_review::tour::GuidedTour;
use guided_review::viewer::{
    Capture, CardNote, CodeRun, DefinitionCard, FoundNames, HoveredName, JumpPlace, LineLocation,
    NameTarget, Review, ReviewStateFile, Side, Snippet,
};
use serde_json::{Value, json};

use crate::support::fake_ports::FakeNavigator;
use crate::support::{FixtureRepo, file_path, line, on_github};

struct Shop {
    repo: FixtureRepo,
    base: String,
    head: String,
}

const CHECKOUT_AT_BASE: &str = "import { Cart } from \"./cart\";\n\nexport function pay(cart: Cart) {\n  return cart.total();\n}\n";

impl Shop {
    fn new() -> Self {
        Self::with_files(&[])
    }

    fn with_files(extra: &[(String, String)]) -> Self {
        Self::with_changes(extra, |_| {})
    }

    /// `extra` is written at base, `change` runs before head is committed.
    fn with_changes(extra: &[(String, String)], change: impl FnOnce(&FixtureRepo)) -> Self {
        let repo = FixtureRepo::cloned_from("acme/shop");
        for (path, text) in extra {
            repo.write(path, text);
        }
        repo.write(
            "src/cart.ts",
            "export class Cart {\n  total() { return 1; }\n}\n",
        );
        repo.write("src/checkout.ts", CHECKOUT_AT_BASE);
        repo.write("src/pricing.ts", "export function price() { return 1; }\n");
        repo.write("legacy/old.ts", "export class Legacy {}\n");
        repo.write(
            "src/report.ts",
            "import { Legacy } from \"../legacy/old\";\n\nnew Legacy();\n",
        );
        let base = repo.commit("base");
        repo.write(
            "src/checkout.ts",
            "import { Cart } from \"./cart\";\nimport { price } from \"./pricing\";\n\nexport function pay(cart: Cart) {\n  return cart.total() + price();\n}\n",
        );
        repo.write("src/pricing.ts", "export function price() { return 2; }\n");
        repo.write("src/report.ts", "export const report = 1;\n");
        repo.git(&["rm", "-q", "legacy/old.ts"]);
        change(&repo);
        let head = repo.commit("head");
        Self { repo, base, head }
    }

    fn source(&self) -> LocalGit {
        LocalGit::find(&crate::support::shop_on_github(), self.repo.path())
            .unwrap()
            .unwrap()
    }

    fn open(&self, source: &dyn CodeSource) -> Review {
        let tour: GuidedTour = serde_json::from_value(json!({
            "pr": "https://github.com/acme/shop/pull/7",
            "base": self.base,
            "head": self.head,
            "title": "Checkout adds the price",
            "summary": [],
            "steps": [
                step("checkout", "Checkout adds the price", json!([{"file": "src/checkout.ts"}])),
                step("pricing", "Price is two", json!([{"file": "src/pricing.ts"}])),
            ],
        }))
        .unwrap();
        let state_file = ReviewStateFile::for_subject(
            &self.repo.folders(),
            &"https://github.com/acme/shop/pull/7".parse().unwrap(),
        );
        Review::open(
            tour,
            source,
            state_file,
            crate::support::fake_ports::every_capability(),
        )
        .unwrap()
    }
}

fn step(id: &str, title: &str, pieces: Value) -> Value {
    json!({"id": id, "title": title, "why": "because", "pieces": pieces})
}

/// A name at the start of its line, where these tests' names are not: they are looked up by name.
fn hover(path: &str, side: Side, at: usize, word: &str) -> HoveredName {
    hover_at(path, side, at, 0, word)
}

fn hover_at(path: &str, side: Side, at: usize, column: usize, word: &str) -> HoveredName {
    HoveredName {
        at: LineLocation {
            path: file_path(path),
            side,
            line: line(at),
        },
        column: Column::try_from(column).unwrap(),
        word: Word::new(word).unwrap(),
    }
}

/// What the app does: asks, runs the work off the UI thread, lands it, then draws the card.
fn card_of(review: &mut Review, source: &dyn CodeSource, hovered: &HoveredName) -> DefinitionCard {
    if let Some(work) = review.look_up_name(hovered) {
        review.name_looked_up(work.run(source));
    }
    (*review.definition_card(hovered).unwrap()).clone()
}

fn found(card: DefinitionCard) -> FoundNames {
    match card {
        DefinitionCard::Found(found) => found,
        other => panic!("expected a found card, got {other:?}"),
    }
}

#[test]
fn a_context_line_is_looked_up_at_head_and_its_definition_outside_the_pr_opens_as_a_code_link() {
    let shop = Shop::new();
    let source = shop.source();
    let mut review = shop.open(&source);

    let card = found(card_of(
        &mut review,
        &source,
        &hover("src/checkout.ts", Side::Head, 4, "Cart"),
    ));

    let [definition] = card.definitions.as_slice() else {
        panic!("one definition expected: {:?}", card.definitions);
    };
    assert_eq!(&*definition.place, "src/cart.ts:1");
    assert!(!definition.in_pr);
    assert!(definition.signature.contains("class Cart"));
    let NameTarget::Code(link) = &definition.target else {
        panic!("not in the tour, so a code link: {:?}", definition.target);
    };
    assert_eq!(
        on_github(&review, link),
        format!(
            "https://github.com/acme/shop/blob/{}/src/cart.ts#L1",
            shop.head
        )
    );
}

#[test]
fn a_parameter_is_looked_up_in_its_function_and_defined_where_the_function_takes_it() {
    let shop = Shop::new();
    let source = shop.source();
    let mut review = shop.open(&source);

    let card = found(card_of(
        &mut review,
        &source,
        &hover_at("src/checkout.ts", Side::Head, 5, 9, "cart"),
    ));

    let [definition] = card.definitions.as_slice() else {
        panic!("one definition expected: {:?}", card.definitions);
    };
    assert_eq!(&*definition.place, "src/checkout.ts:4");
    assert_eq!(&*definition.signature, "export function pay(cart: Cart) {");
    assert!(card.usages.is_empty(), "{:?}", card.usages);
}

#[test]
fn the_hovered_line_is_left_out_of_the_usages() {
    let shop = Shop::new();
    let source = shop.source();
    let mut review = shop.open(&source);

    let card = found(card_of(
        &mut review,
        &source,
        &hover("src/checkout.ts", Side::Head, 4, "Cart"),
    ));

    let places: Vec<(&str, &str)> = card
        .usages
        .iter()
        .flat_map(|file| file.lines.iter().map(|line| (&*file.path, &*line.number)))
        .collect();
    assert_eq!(places, [("src/checkout.ts", "1")]);
    assert_eq!(&*card.counts, "1 definition · 1 usage in 1 file");
}

#[test]
fn a_definition_in_a_changed_file_the_tour_shows_jumps_in_the_tour() {
    let shop = Shop::new();
    let source = shop.source();
    let mut review = shop.open(&source);

    let card = found(card_of(
        &mut review,
        &source,
        &hover("src/checkout.ts", Side::Head, 5, "price"),
    ));

    let [definition] = card.definitions.as_slice() else {
        panic!("one definition expected: {:?}", card.definitions);
    };
    assert!(definition.in_pr);
    let NameTarget::Tour(target) = &definition.target else {
        panic!("shown in the tour, so a jump: {:?}", definition.target);
    };
    let line = LineLocation {
        path: file_path("src/pricing.ts"),
        side: Side::Head,
        line: line(1),
    };
    assert_eq!(target.place, JumpPlace::Line(line.clone()));
    review.jump_to(target);
    assert_eq!(
        Some(review.page().row_of_jump(target)),
        review.page().row_of_line(&line),
        "the page scrolls to the line itself"
    );
}

#[test]
fn a_removed_line_is_looked_up_at_base() {
    let shop = Shop::new();
    let source = shop.source();
    let mut review = shop.open(&source);

    let card = found(card_of(
        &mut review,
        &source,
        &hover("src/report.ts", Side::Base, 3, "Legacy"),
    ));

    let [definition] = card.definitions.as_slice() else {
        panic!(
            "the definition is in a file head deleted: {:?}",
            card.definitions
        );
    };
    assert_eq!(&*definition.place, "legacy/old.ts:1");
    assert!(definition.in_pr);
    let NameTarget::Tour(target) = &definition.target else {
        panic!(
            "the safety net shows the deleted file: {:?}",
            definition.target
        );
    };
    assert_eq!(
        target.place,
        JumpPlace::Line(LineLocation {
            path: file_path("legacy/old.ts"),
            side: Side::Base,
            line: line(1),
        })
    );
    let cart = found(card_of(
        &mut review,
        &source,
        &hover("src/checkout.ts", Side::Base, 1, "Cart"),
    ));
    let NameTarget::Code(link) = &cart.definitions[0].target else {
        panic!(
            "an unchanged file is not in the tour: {:?}",
            cart.definitions[0].target
        );
    };
    assert_eq!(
        on_github(&review, link),
        format!(
            "https://github.com/acme/shop/blob/{}/src/cart.ts#L1",
            shop.base
        )
    );
    let at_head = found(card_of(
        &mut review,
        &source,
        &hover("src/checkout.ts", Side::Head, 1, "Legacy"),
    ));
    assert_eq!(at_head.definitions.len(), 0);
}

#[test]
fn a_place_in_a_changed_file_outside_every_shown_hunk_opens_as_a_code_link() {
    let far_at_base = format!("export function far() {{}}\n{}far();\n", "//\n".repeat(20));
    let shop = Shop::with_changes(&[("src/far.ts".to_owned(), far_at_base)], |repo| {
        repo.write(
            "src/far.ts",
            &format!(
                "export function far() {{}}\n{}far(); far();\n",
                "//\n".repeat(20)
            ),
        );
    });
    let source = shop.source();
    let mut review = shop.open(&source);

    let card = found(card_of(
        &mut review,
        &source,
        &hover("src/far.ts", Side::Head, 22, "far"),
    ));

    let [definition] = card.definitions.as_slice() else {
        panic!("one definition expected: {:?}", card.definitions);
    };
    assert!(definition.in_pr);
    let NameTarget::Code(link) = &definition.target else {
        panic!(
            "line 1 is far from the change, so no page shows it: {:?}",
            definition.target
        );
    };
    assert_eq!(
        on_github(&review, link),
        format!(
            "https://github.com/acme/shop/blob/{}/src/far.ts#L1",
            shop.head
        )
    );
}

#[test]
fn an_unchanged_line_found_at_base_jumps_to_the_row_that_shows_it() {
    let shop = Shop::with_changes(
        &[(
            "src/use.ts".to_owned(),
            "price();\nprice();\nprice();\n".to_owned(),
        )],
        |repo| repo.write("src/use.ts", "price();\nprice();\nprice(); price();\n"),
    );
    let source = shop.source();
    let mut review = shop.open(&source);
    let files_at_head = review.files_at_head_request().read(&source);
    review.set_files_at_head(files_at_head);

    let card = found(card_of(
        &mut review,
        &source,
        &hover("src/use.ts", Side::Base, 3, "price"),
    ));

    let usage = card
        .usages
        .iter()
        .filter(|file| &*file.path == "src/use.ts")
        .flat_map(|file| &file.lines)
        .find(|line| &*line.number == "1")
        .expect("line 1 uses price at base");
    let NameTarget::Tour(target) = &usage.target else {
        panic!("the safety net shows line 1: {:?}", usage.target);
    };
    assert_eq!(
        target.place,
        JumpPlace::Line(LineLocation {
            path: file_path("src/use.ts"),
            side: Side::Head,
            line: line(1),
        }),
        "an unchanged row is shown at its head line"
    );
}

#[test]
fn without_a_clone_a_removed_line_reads_the_old_path_of_a_renamed_file() {
    let basket = "export class Basket {}\n// a\n// b\n// c\n// d\n";
    let shop = Shop::with_changes(
        &[
            ("src/old_name.ts".to_owned(), basket.to_owned()),
            ("src/buy.ts".to_owned(), "new Basket();\n".to_owned()),
        ],
        |repo| {
            repo.git(&["mv", "src/old_name.ts", "src/new_name.ts"]);
            repo.write("src/new_name.ts", &format!("{basket}// e\n"));
            repo.write("src/buy.ts", "new Thing();\n");
        },
    );
    let source = NoClone(shop.source());
    let mut review = shop.open(&source);

    let card = found(card_of(
        &mut review,
        &source,
        &hover("src/buy.ts", Side::Base, 1, "Basket"),
    ));

    let places: Vec<&str> = card
        .definitions
        .iter()
        .map(|definition| &*definition.place)
        .collect();
    assert_eq!(places, ["src/old_name.ts:1"]);
}

#[test]
fn a_name_is_asked_once_and_shows_loading_until_it_lands() {
    let shop = Shop::new();
    let source = shop.source();
    let mut review = shop.open(&source);
    let hovered = hover("src/checkout.ts", Side::Head, 4, "Cart");

    assert!(review.definition_card(&hovered).is_none());
    let work = review.look_up_name(&hovered).expect("the first hover asks");
    assert!(matches!(
        review.definition_card(&hovered).as_deref(),
        Some(DefinitionCard::Loading { .. })
    ));
    assert!(
        review.look_up_name(&hovered).is_none(),
        "already on its way"
    );
    review.name_looked_up(work.run(&source));
    assert!(review.look_up_name(&hovered).is_none(), "already known");
    assert!(matches!(
        review.definition_card(&hovered).as_deref(),
        Some(DefinitionCard::Found(_))
    ));
}

#[test]
fn a_failed_lookup_shows_why_and_is_asked_again_on_the_next_hover() {
    let shop = Shop::new();
    let source = shop.source();
    let mut review = shop.open(&source);
    let hovered = hover("src/checkout.ts", Side::Head, 4, "Cart");
    let git = shop.repo.path().join(".git");
    let git_away = shop.repo.path().join(".git-away");

    std::fs::rename(&git, &git_away).unwrap();
    let work = review.look_up_name(&hovered).unwrap();
    review.name_looked_up(work.run(&source));
    std::fs::rename(&git_away, &git).unwrap();

    let card = review.definition_card(&hovered);
    let Some(DefinitionCard::Failed { says, .. }) = card.as_deref() else {
        panic!("git grep cannot run, so the lookup fails: {card:?}");
    };
    assert!(says.starts_with("Could not look up Cart: "), "{says}");
    let again = review
        .look_up_name(&hovered)
        .expect("a failure is tried again");
    review.name_looked_up(again.run(&source));
    assert!(matches!(
        review.definition_card(&hovered).as_deref(),
        Some(DefinitionCard::Found(_))
    ));
}

#[test]
fn more_than_twenty_usages_show_twenty_and_count_the_rest() {
    let uses: String = "price();\n".repeat(25);
    let shop = Shop::with_files(&[("src/many.ts".to_owned(), uses)]);
    let source = shop.source();
    let mut review = shop.open(&source);

    let card = found(card_of(
        &mut review,
        &source,
        &hover("src/checkout.ts", Side::Head, 5, "price"),
    ));

    let shown: usize = card.usages.iter().map(|file| file.lines.len()).sum();
    assert_eq!(shown, 20);
    assert_eq!(card.usages_not_shown, 6);
    assert_eq!(&*card.counts, "1 definition · 26 usages in 2 files");
    assert_eq!(card.more_usages.as_deref(), Some("and 6 more in 1 file"));
    let files: Vec<(&str, &str, &str, usize)> = card
        .usages
        .iter()
        .map(|file| (&*file.folder, &*file.name, &*file.count, file.lines.len()))
        .collect();
    assert_eq!(
        files,
        [("src", "checkout.ts", "1", 1), ("src", "many.ts", "25", 19)],
        "usages in the PR's files come first; a file's count is all its usages, its rows the ones shown"
    );
}

#[test]
fn usages_are_grouped_by_file_and_a_file_in_the_root_has_no_folder() {
    let shop = Shop::with_files(&[
        ("zeta.ts".to_owned(), "price();\n".to_owned()),
        (
            "src/deep/er/alpha.ts".to_owned(),
            "price();\nprice();\n".to_owned(),
        ),
    ]);
    let source = shop.source();
    let mut review = shop.open(&source);

    let card = found(card_of(
        &mut review,
        &source,
        &hover("src/checkout.ts", Side::Head, 5, "price"),
    ));

    let files: Vec<(&str, &str, &str)> = card
        .usages
        .iter()
        .map(|file| (&*file.folder, &*file.name, &*file.path))
        .collect();
    assert_eq!(
        files,
        [
            ("src", "checkout.ts", "src/checkout.ts"),
            ("src/deep/er", "alpha.ts", "src/deep/er/alpha.ts"),
            ("", "zeta.ts", "zeta.ts"),
        ]
    );
    let alpha = &card.usages[1];
    let numbers: Vec<&str> = alpha.lines.iter().map(|line| &*line.number).collect();
    assert_eq!(numbers, ["1", "2"]);
    assert_eq!(
        alpha.target, alpha.lines[0].target,
        "the file row opens its first usage"
    );
}

#[test]
fn a_snippet_marks_the_name_and_starts_before_it_when_the_name_is_far_right() {
    let near = Snippet::of("  price();", "price");
    assert_eq!(&*near.text, "price();");
    assert_eq!(near.name_text(), Some("price"));

    let far_line = "const aVeryLongVariableNameThatPushesTheCallFarRight = total + price();";
    let far = Snippet::of(far_line, "price");
    assert!(far.text.starts_with('…'), "{}", far.text);
    assert!(far.text.ends_with("price();"), "{}", far.text);
    assert!(far.text.chars().count() < far_line.chars().count());
    assert_eq!(far.name_text(), Some("price"));
}

#[test]
fn a_snippet_marks_the_whole_word_and_not_a_longer_name_that_contains_it() {
    let snippet = Snippet::of("priceList(); price();", "price");
    assert_eq!(snippet.name_text(), Some("price"));
    assert_eq!(snippet.name, Some(13..18));
    assert_eq!(Snippet::of("other();", "price").name_text(), None);
}

#[test]
fn without_a_clone_the_card_says_usages_are_from_the_prs_files_only() {
    let shop = Shop::new();
    let source = NoClone(shop.source());
    let mut review = shop.open(&source);

    let card = found(card_of(
        &mut review,
        &source,
        &hover("src/checkout.ts", Side::Head, 5, "price"),
    ));

    assert_eq!(card.notes, [CardNote::UsagesFromChangedFilesOnly]);
    assert_eq!(
        CardNote::UsagesFromChangedFilesOnly.text(),
        "Usages from the PR's files only"
    );
    assert_eq!(
        card.definitions.len(),
        1,
        "a definition in a changed file is still found"
    );
}

#[test]
fn a_word_in_too_many_files_says_the_search_was_cut_short() {
    let generated: Vec<(String, String)> = (0..=guided_review::navigation::FILES_PARSED_AT_MOST)
        .map(|n| (format!("gen/f{n:04}.ts"), "price();\n".to_owned()))
        .collect();
    let shop = Shop::with_files(&generated);
    let source = shop.source();
    let mut review = shop.open(&source);

    let card = found(card_of(
        &mut review,
        &source,
        &hover("src/checkout.ts", Side::Head, 5, "price"),
    ));

    assert_eq!(card.notes, [CardNote::SearchCutShort]);
}

struct NoClone(LocalGit);

impl CodeSource for NoClone {
    fn file_at(&self, sha: &Sha, path: &FilePath) -> anyhow::Result<String> {
        self.0.file_at(sha, path)
    }
    fn blob_at(&self, sha: &Sha, path: &FilePath) -> anyhow::Result<Arc<[u8]>> {
        self.0.blob_at(sha, path)
    }
    fn blob_info_at(&self, sha: &Sha, path: &FilePath) -> anyhow::Result<BlobInfo> {
        self.0.blob_info_at(sha, path)
    }
    fn diff(&self, base: &Sha, head: &Sha) -> anyhow::Result<Vec<FileDiff>> {
        self.0.diff(base, head)
    }
    fn merge_base(&self, a: &Sha, b: &Sha) -> anyhow::Result<Sha> {
        self.0.merge_base(a, b)
    }
    fn working_tree_is_at(&self, head: &Sha) -> bool {
        self.0.working_tree_is_at(head)
    }
}

/// The text of each run with its capture, so a test reads the colours as a reviewer sees them.
fn painted<'a>(text: &'a str, runs: &[CodeRun]) -> Vec<(&'a str, Option<Capture>, bool)> {
    runs.iter()
        .map(|run| {
            (
                text.get(run.range.clone()).unwrap(),
                run.capture,
                run.is_name,
            )
        })
        .collect()
}

#[test]
fn a_php_usage_is_coloured_by_its_file_and_not_as_a_lone_line() {
    let shop = Shop::with_files(&[(
        "app/Orders.php".to_owned(),
        "<?php\n\nclass Orders\n{\n    public function make()\n    {\n        return new Cart('x');\n    }\n}\n"
            .to_owned(),
    )]);
    let source = shop.source();
    let mut review = shop.open(&source);

    let card = found(card_of(
        &mut review,
        &source,
        &hover("src/checkout.ts", Side::Head, 4, "Cart"),
    ));

    let php = card
        .usages
        .iter()
        .find(|file| &*file.path == "app/Orders.php")
        .unwrap();
    let snippet = &php.lines[0].snippet;
    assert_eq!(&*snippet.text, "return new Cart('x');");
    let painted = painted(&snippet.text, &snippet.runs);
    assert!(
        painted.contains(&("return", Some(Capture::Keyword), false)),
        "{painted:?}"
    );
    assert!(
        painted.contains(&("new", Some(Capture::Keyword), false)),
        "{painted:?}"
    );
    assert!(
        painted.contains(&("Cart", Some(Capture::Constructor), true)),
        "{painted:?}"
    );
    assert!(
        painted.contains(&("x", Some(Capture::String), false)),
        "{painted:?}"
    );
}

#[test]
fn a_rust_signature_is_coloured_where_its_lines_were_joined() {
    let shop = Shop::with_files(&[(
        "src/cart.rs".to_owned(),
        "pub fn total(\n    cart: &Cart, // what is bought\n    tax: u32,\n) -> u32 {\n    1\n}\n"
            .to_owned(),
    )]);
    let source = shop.source();
    let mut review = shop.open(&source);

    let card = found(card_of(
        &mut review,
        &source,
        &hover("src/checkout.ts", Side::Head, 5, "total"),
    ));

    let rust = card
        .definitions
        .iter()
        .find(|row| row.place.starts_with("src/cart.rs"))
        .unwrap();
    assert_eq!(
        &*rust.signature,
        "pub fn total( cart: &Cart, // what is bought tax: u32, ) -> u32"
    );
    let painted = painted(&rust.signature, &rust.signature_runs);
    assert!(
        painted.contains(&("fn", Some(Capture::Keyword), false)),
        "{painted:?}"
    );
    assert!(
        painted.contains(&("total", Some(Capture::Function), false)),
        "{painted:?}"
    );
    assert!(
        painted.contains(&("Cart", Some(Capture::Type), false)),
        "{painted:?}"
    );
    assert!(
        painted.contains(&("u32", Some(Capture::TypeBuiltin), false)),
        "{painted:?}"
    );
    assert!(
        painted.contains(&("bought", Some(Capture::Comment), false)),
        "{painted:?}"
    );
}

#[test]
fn a_snippet_cut_at_the_start_keeps_its_colours_on_the_same_text() {
    let line = "const aVeryLongVariableNameThatPushesTheCallFarRight = total + price();";
    let captures = [
        (0..5, Capture::Keyword),
        (6..58, Capture::Variable),
        (63..68, Capture::Function),
    ];

    let snippet = Snippet::highlighted(line, "price", &captures);

    assert!(snippet.text.starts_with('…'), "{}", snippet.text);
    let painted = painted(&snippet.text, &snippet.runs);
    assert_eq!(
        painted.last(),
        Some(&("price", Some(Capture::Function), true))
    );
    assert_eq!(snippet.name_text(), Some("price"));
    assert!(
        !painted.iter().any(|(text, ..)| *text == "const"),
        "cut away: {painted:?}"
    );
    assert!(
        painted.contains(&(line.get(55..58).unwrap(), Some(Capture::Variable), false)),
        "{painted:?}"
    );
}

#[test]
fn the_name_stays_marked_without_any_colours() {
    let snippet = Snippet::of("  total + price();", "price");

    assert_eq!(
        painted(&snippet.text, &snippet.runs),
        [("price", None, true)]
    );
}

fn rust_analyzer() -> ServerName {
    ServerName::new("rust-analyzer")
}

#[test]
fn the_card_asks_the_navigator_for_the_name_at_its_place_and_column() {
    let shop = Shop::new();
    let source = shop.source();
    let mut review = shop.open(&source);
    let navigator = FakeNavigator::answering_by(AnsweredBy::LanguageServer(rust_analyzer()));
    review.use_navigator(Arc::new(navigator.clone()));

    let hovered = hover_at("src/checkout.ts", Side::Head, 5, 9, "cart");
    found(card_of(&mut review, &source, &hovered));

    let [asked] = navigator.asked().try_into().expect("one request");
    assert_eq!(asked.sha, Sha::new(&shop.head));
    assert_eq!(asked.at.path, file_path("src/checkout.ts"));
    assert_eq!(asked.at.line, line(5));
    assert_eq!(asked.column, Column::try_from(9).unwrap());
    assert_eq!(asked.word, Word::new("cart").unwrap());
}

#[test]
fn the_card_shows_each_state_the_navigator_waits_in_then_the_answer() {
    let shop = Shop::new();
    let source = shop.source();
    let mut review = shop.open(&source);
    review.use_navigator(Arc::new(
        FakeNavigator::answering_by(AnsweredBy::LanguageServer(rust_analyzer())).waiting(&[
            Waiting::Downloading(rust_analyzer()),
            Waiting::Starting(rust_analyzer()),
            Waiting::Indexing,
        ]),
    ));
    let hovered = hover("src/checkout.ts", Side::Head, 4, "Cart");
    let work = review.look_up_name(&hovered).unwrap();

    let review = std::cell::RefCell::new(review);
    let seen = std::cell::RefCell::new(Vec::new());
    let outcome = work.run_reporting(&source, &|waiting| {
        let mut review = review.borrow_mut();
        review.name_waiting(waiting);
        let card = review.definition_card(&hovered).unwrap();
        let DefinitionCard::Waiting { says, .. } = &*card else {
            panic!("a waiting card expected, got {card:?}");
        };
        seen.borrow_mut().push(says.to_string());
    });
    let mut review = review.into_inner();
    review.name_looked_up(outcome);

    assert_eq!(
        seen.into_inner(),
        [
            "Downloading rust-analyzer…",
            "Starting rust-analyzer…",
            "Indexing…"
        ]
    );
    assert!(matches!(
        review.definition_card(&hovered).as_deref(),
        Some(DefinitionCard::Found(_))
    ));
}

#[test]
fn an_answer_by_name_with_a_reason_carries_a_note_naming_it() {
    let shop = Shop::new();
    let source = shop.source();
    let mut review = shop.open(&source);
    review.use_navigator(Arc::new(FakeNavigator::answering_by(AnsweredBy::ByName {
        because: Some(FallbackReason::new("rust-analyzer did not answer in time")),
    })));

    let card = found(card_of(
        &mut review,
        &source,
        &hover("src/checkout.ts", Side::Head, 4, "Cart"),
    ));

    assert_eq!(
        card.notes,
        [CardNote::AnsweredByName(FallbackReason::new(
            "rust-analyzer did not answer in time"
        ))]
    );
    assert_eq!(
        card.notes[0].text(),
        "Answered by name: rust-analyzer did not answer in time"
    );
    assert_eq!(
        *card.note_lines,
        [Arc::from(
            "Answered by name: rust-analyzer did not answer in time"
        )]
    );
}

#[test]
fn an_answer_by_name_with_no_reason_and_a_language_server_answer_carry_no_note() {
    let shop = Shop::new();
    let source = shop.source();
    for answered_by in [
        AnsweredBy::ByName { because: None },
        AnsweredBy::LanguageServer(rust_analyzer()),
    ] {
        let mut review = shop.open(&source);
        review.use_navigator(Arc::new(FakeNavigator::answering_by(answered_by)));
        let card = found(card_of(
            &mut review,
            &source,
            &hover("src/checkout.ts", Side::Head, 4, "Cart"),
        ));
        assert_eq!(card.notes, []);
    }
}

const SERVER_QUESTION: &str =
    "Use language servers for acme/shop? typescript-language-server may build it";

fn shop_repo() -> Codebase {
    Codebase::Hosted(crate::support::shop_on_github())
}

/// A review whose language server adapter is `adapter`, with the repo not asked yet.
fn review_with_servers(shop: &Shop, source: &LocalGit, adapter: &FakeNavigator) -> Review {
    let mut review = shop.open(source);
    review.use_language_servers(Arc::new(adapter.clone()), shop_repo(), None);
    review
}

fn language_server_adapter() -> FakeNavigator {
    FakeNavigator::answering_by(AnsweredBy::LanguageServer(rust_analyzer()))
}

fn question_of(card: &FoundNames) -> Option<String> {
    card.question
        .as_ref()
        .map(|question| question.says.to_string())
}

#[test]
fn the_first_hover_in_a_repo_asks_to_use_language_servers_and_the_by_name_lookup_answers() {
    let shop = Shop::new();
    let source = shop.source();
    let adapter = language_server_adapter();
    let mut review = review_with_servers(&shop, &source, &adapter);

    let card = found(card_of(
        &mut review,
        &source,
        &hover("src/checkout.ts", Side::Head, 4, "Cart"),
    ));

    assert_eq!(question_of(&card), Some(SERVER_QUESTION.to_owned()));
    assert_eq!(card.question.unwrap().codebase, shop_repo());
    assert_eq!(card.definitions.len(), 1, "the by-name lookup answered");
    assert_eq!(adapter.asked(), []);
}

#[test]
fn the_question_for_a_folder_with_no_github_origin_names_its_path() {
    let shop = Shop::new();
    let source = shop.source();
    let mut review = shop.open(&source);
    let folder = Codebase::Folder(WorkTreeRoot::new("/work/shop".into()).unwrap());
    review.use_language_servers(Arc::new(language_server_adapter()), folder, None);

    let card = found(card_of(
        &mut review,
        &source,
        &hover("src/checkout.ts", Side::Head, 4, "Cart"),
    ));

    assert_eq!(
        question_of(&card),
        Some(
            "Use language servers for /work/shop? typescript-language-server may build it"
                .to_owned()
        )
    );
}

#[test]
fn the_question_names_the_server_of_the_hovered_files_language() {
    let shop = Shop::with_changes(&[], |repo| {
        repo.write("src/Order.php", "<?php\nclass Order {}\n");
        repo.write("src/lib.rs", "pub struct Order;\n");
    });
    let source = shop.source();
    let mut review = review_with_servers(&shop, &source, &language_server_adapter());

    let php = found(card_of(
        &mut review,
        &source,
        &hover("src/Order.php", Side::Head, 2, "Order"),
    ));
    let rust = found(card_of(
        &mut review,
        &source,
        &hover("src/lib.rs", Side::Head, 1, "Order"),
    ));

    assert_eq!(
        question_of(&php),
        Some("Use language servers for acme/shop? Intelephense may build it".to_owned())
    );
    assert_eq!(
        question_of(&rust),
        Some("Use language servers for acme/shop? rust-analyzer may build it".to_owned())
    );
}

#[test]
fn a_python_name_asks_whether_pyright_may_answer() {
    let shop = Shop::with_changes(&[], |repo| {
        repo.write("src/order.py", "class Order:\n    pass\n");
    });
    let source = shop.source();
    let mut review = review_with_servers(&shop, &source, &language_server_adapter());

    let python = found(card_of(
        &mut review,
        &source,
        &hover("src/order.py", Side::Head, 1, "Order"),
    ));

    assert_eq!(
        question_of(&python),
        Some("Use language servers for acme/shop? Pyright may build it".to_owned())
    );
}

#[test]
fn a_go_name_asks_no_question_since_no_server_answers_it() {
    let shop = Shop::with_changes(&[], |repo| {
        repo.write("src/basket.go", "package shop\n\ntype Basket struct{}\n");
    });
    let source = shop.source();
    let mut review = review_with_servers(&shop, &source, &language_server_adapter());

    let go = found(card_of(
        &mut review,
        &source,
        &hover("src/basket.go", Side::Head, 3, "Basket"),
    ));

    assert_eq!(question_of(&go), None);
    assert_eq!(go.definitions.len(), 1, "the by-name lookup answered");
}

/// Answers with a definition in a file git does not hold, as rust-analyzer does for the standard library.
struct StandardLibraryNavigator;

impl guided_review::navigation::CodeNavigator for StandardLibraryNavigator {
    fn navigate(
        &self,
        _code: &dyn CodeSource,
        request: &guided_review::navigation::NavigationRequest,
        _waiting: &dyn Fn(Waiting),
    ) -> anyhow::Result<guided_review::navigation::Answer> {
        let place = guided_review::navigation::Place {
            path: file_path("opt/rust/library/core/src/option.rs"),
            line: line(10),
        };
        Ok(guided_review::navigation::Answer {
            lookup: guided_review::navigation::Lookup {
                word: request.word.clone(),
                definitions: vec![guided_review::navigation::Definition {
                    name: request.word.clone(),
                    kind: guided_review::navigation::DefinitionKind::Enum,
                    place,
                    signature: "pub enum Option<T>".into(),
                    signature_origin: Arc::new([]),
                    doc: None,
                }],
                usages: Arc::new([]),
                reach: guided_review::navigation::Reach::WholeRepo,
                cut_short: false,
                on_disk_files: Arc::new([(
                    file_path("opt/rust/library/core/src/option.rs"),
                    "/opt/rust/library/core/src/option.rs".into(),
                )]),
            },
            answered_by: AnsweredBy::LanguageServer(rust_analyzer()),
        })
    }
}

#[test]
fn a_row_in_a_file_outside_git_opens_that_file_on_disk() {
    let shop = Shop::new();
    let source = shop.source();
    let mut review = shop.open(&source);
    review.use_navigator(Arc::new(StandardLibraryNavigator));

    let card = found(card_of(
        &mut review,
        &source,
        &hover("src/checkout.ts", Side::Head, 4, "Cart"),
    ));

    assert_eq!(
        card.definitions[0].target,
        NameTarget::OnDisk {
            path: "/opt/rust/library/core/src/option.rs".into(),
            line: line(10),
        }
    );
    assert!(!card.definitions[0].in_pr);
}

#[test]
fn a_review_with_no_language_server_adapter_asks_nothing() {
    let shop = Shop::new();
    let source = shop.source();
    let mut review = shop.open(&source);

    let card = found(card_of(
        &mut review,
        &source,
        &hover("src/checkout.ts", Side::Head, 4, "Cart"),
    ));

    assert_eq!(card.question, None);
}

#[test]
fn a_name_on_the_base_side_asks_nothing_since_no_server_answers_there() {
    let shop = Shop::new();
    let source = shop.source();
    let adapter = language_server_adapter();
    let mut review = review_with_servers(&shop, &source, &adapter);

    let card = found(card_of(
        &mut review,
        &source,
        &hover("src/report.ts", Side::Base, 3, "Legacy"),
    ));

    assert_eq!(card.question, None);
}

#[test]
fn saying_no_takes_the_question_off_the_cards_and_no_later_hover_asks_the_adapter() {
    let shop = Shop::new();
    let source = shop.source();
    let adapter = language_server_adapter();
    let mut review = review_with_servers(&shop, &source, &adapter);
    let first = hover("src/checkout.ts", Side::Head, 4, "Cart");
    found(card_of(&mut review, &source, &first));

    let repo = review.answer_language_servers(ServerAnswer::No);

    assert_eq!(repo, Some(shop_repo()));
    let after = found(card_of(&mut review, &source, &first));
    assert_eq!(after.question, None);
    assert_eq!(after.definitions.len(), 1);
    let later = found(card_of(
        &mut review,
        &source,
        &hover("src/checkout.ts", Side::Head, 5, "price"),
    ));
    assert_eq!(later.question, None);
    assert_eq!(adapter.asked(), []);
}

#[test]
fn saying_yes_asks_the_name_again_of_the_adapter_and_the_card_has_no_question() {
    let shop = Shop::new();
    let source = shop.source();
    let adapter = language_server_adapter();
    let mut review = review_with_servers(&shop, &source, &adapter);
    let first = hover("src/checkout.ts", Side::Head, 4, "Cart");
    found(card_of(&mut review, &source, &first));
    assert_eq!(adapter.asked(), []);

    let repo = review.answer_language_servers(ServerAnswer::Yes);

    assert_eq!(repo, Some(shop_repo()));
    assert!(review.definition_card(&first).is_none(), "asked again");
    let after = found(card_of(&mut review, &source, &first));
    assert_eq!(after.question, None);
    assert_eq!(adapter.asked().len(), 1);
    assert_eq!(adapter.asked()[0].word, Word::new("Cart").unwrap());
}

#[test]
fn a_repo_that_said_yes_before_asks_no_question_and_the_adapter_answers() {
    let shop = Shop::new();
    let source = shop.source();
    let adapter = language_server_adapter();
    let mut review = shop.open(&source);
    review.use_language_servers(
        Arc::new(adapter.clone()),
        shop_repo(),
        Some(ServerAnswer::Yes),
    );

    let card = found(card_of(
        &mut review,
        &source,
        &hover("src/checkout.ts", Side::Head, 4, "Cart"),
    ));

    assert_eq!(card.question, None);
    assert_eq!(adapter.asked().len(), 1);
}

#[test]
fn a_repo_that_said_no_before_asks_no_question_and_the_adapter_is_never_asked() {
    let shop = Shop::new();
    let source = shop.source();
    let adapter = language_server_adapter();
    let mut review = shop.open(&source);
    review.use_language_servers(
        Arc::new(adapter.clone()),
        shop_repo(),
        Some(ServerAnswer::No),
    );

    let card = found(card_of(
        &mut review,
        &source,
        &hover("src/checkout.ts", Side::Head, 4, "Cart"),
    ));

    assert_eq!(card.question, None);
    assert_eq!(card.definitions.len(), 1, "the by-name lookup answered");
    assert_eq!(adapter.asked(), []);
}

const DEPENDENCY_NOTE: &str = "The PR changes dependency files, so answers may be off";

fn notes_of_a_hover_in_a_pr_that_changes(file: &str, answered_by: AnsweredBy) -> Vec<String> {
    let shop = Shop::with_changes(&[], |repo| repo.write(file, "changed\n"));
    let source = shop.source();
    let mut review = shop.open(&source);
    review.use_navigator(Arc::new(FakeNavigator::answering_by(answered_by)));
    let card = found(card_of(
        &mut review,
        &source,
        &hover("src/checkout.ts", Side::Head, 4, "Cart"),
    ));
    card.notes
        .iter()
        .map(|note| note.text().into_owned())
        .collect()
}

#[test]
fn a_language_server_answer_says_it_may_be_off_when_the_pr_changes_a_dependency_file() {
    for file in [
        "composer.json",
        "composer.lock",
        "package.json",
        "package-lock.json",
        "npm-shrinkwrap.json",
        "yarn.lock",
        "pnpm-lock.yaml",
        "bun.lock",
        "bun.lockb",
        "Cargo.toml",
        "Cargo.lock",
        "packages/app/package.json",
    ] {
        assert_eq!(
            notes_of_a_hover_in_a_pr_that_changes(
                file,
                AnsweredBy::LanguageServer(rust_analyzer())
            ),
            [DEPENDENCY_NOTE],
            "{file}"
        );
    }
}

#[test]
fn a_pr_that_changes_no_dependency_file_and_an_answer_by_name_carry_no_dependency_note() {
    assert_eq!(
        notes_of_a_hover_in_a_pr_that_changes(
            "src/composer.php",
            AnsweredBy::LanguageServer(rust_analyzer())
        ),
        Vec::<String>::new()
    );
    assert_eq!(
        notes_of_a_hover_in_a_pr_that_changes(
            "composer.lock",
            AnsweredBy::ByName { because: None }
        ),
        Vec::<String>::new()
    );
}
