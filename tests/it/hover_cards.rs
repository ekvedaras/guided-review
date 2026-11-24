use std::sync::Arc;

use guided_review::code_source::{BlobInfo, CodeSource, FileDiff};
use guided_review::file_path::FilePath;
use guided_review::local_git::LocalGit;
use guided_review::navigation::Word;
use guided_review::pull_request::{RepoName, Sha};
use guided_review::tour::GuidedTour;
use guided_review::viewer::{
    CardNote, DefinitionCard, HoveredName, JumpPlace, LineLocation, NameTarget, Review,
    ReviewStateFile, Side, Snippet,
};
use serde_json::{Value, json};

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
        LocalGit::find(&RepoName::new("acme", "shop"), self.repo.path())
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
        Review::open(tour, source, state_file).unwrap()
    }
}

fn step(id: &str, title: &str, pieces: Value) -> Value {
    json!({"id": id, "title": title, "why": "because", "pieces": pieces})
}

fn hover(path: &str, side: Side, at: usize, word: &str) -> HoveredName {
    HoveredName {
        at: LineLocation {
            path: file_path(path),
            side,
            line: line(at),
        },
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

fn found(card: DefinitionCard) -> guided_review::viewer::FoundNames {
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
