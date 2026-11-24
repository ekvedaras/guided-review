use std::path::Path;
use std::sync::Arc;

use guided_review::code_source::{BlobInfo, CodeSource, FileDiff};
use guided_review::file_path::FilePath;
use guided_review::local_git::LocalGit;
use guided_review::local_work_tree::OnDisk;
use guided_review::navigation::Word;
use guided_review::pull_request::{RepoName, Sha};
use guided_review::review_subject::LocalChangeSet;
use guided_review::tour::GuidedTour;
use guided_review::viewer::{
    CardNote, DefinitionCard, HoveredName, JumpPlace, LineLocation, NameTarget, Review,
    ReviewStateFile, Side,
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
    assert_eq!(definition.place, "src/cart.ts:1");
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

    let places: Vec<&str> = card
        .usages
        .iter()
        .map(|usage| usage.place.as_str())
        .collect();
    assert_eq!(places, ["src/checkout.ts:1"]);
    assert_eq!(card.usages_label().as_deref(), Some("1 usage"));
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
    assert_eq!(
        target.place,
        JumpPlace::Piece("src/pricing.ts#price".parse().unwrap())
    );
    review.jump_to(target);
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
    assert_eq!(definition.place, "legacy/old.ts:1");
    assert!(definition.in_pr);
    let NameTarget::Tour(target) = &definition.target else {
        panic!(
            "the safety net shows the deleted file: {:?}",
            definition.target
        );
    };
    assert_eq!(target.place, JumpPlace::File(file_path("legacy/old.ts")));
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

    let work = review.look_up_name(&hovered).unwrap();
    review.name_looked_up(work.run(&Unreadable(shop.source())));

    let card = review.definition_card(&hovered);
    let Some(DefinitionCard::Failed { because, .. }) = card.as_deref() else {
        panic!("expected a failed card");
    };
    assert!(because.contains("cannot read"), "{because}");
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

    assert_eq!(card.usages.len(), 20);
    assert_eq!(card.usages_not_shown, 6);
    assert_eq!(card.usages_label().as_deref(), Some("26 usages"));
    assert_eq!(card.more_usages_label().as_deref(), Some("and 6 more"));
    assert_eq!(
        card.usages.first().map(|usage| usage.place.as_str()),
        Some("src/checkout.ts:2"),
        "usages in the PR's files come first"
    );
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

struct Unreadable(LocalGit);

impl CodeSource for Unreadable {
    fn file_at(&self, _sha: &Sha, path: &FilePath) -> anyhow::Result<String> {
        anyhow::bail!("cannot read {path}")
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
    fn change_set_on_disk(
        &self,
        change_set: &LocalChangeSet,
        snapshot: &Sha,
    ) -> anyhow::Result<OnDisk> {
        self.0.change_set_on_disk(change_set, snapshot)
    }
    fn local_clone(&self) -> Option<&Path> {
        self.0.local_clone()
    }
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
