use std::sync::Arc;

use guided_review::code_source::CodeSource;
use guided_review::language::Language;
use guided_review::local_git::LocalGit;
use guided_review::navigation::{
    DefinitionKind, FileSearch, FilesWithWord, GitGrep, Lookups, Reach, Word, WordInFile,
};
use guided_review::pull_request::{RepoName, Sha};

use crate::support::fake_ports::FakeFilesWithWord;
use crate::support::{FixtureRepo, file_path, line};

fn word(text: &str) -> Word {
    Word::new(text).unwrap()
}

/// What a test reads of a definition: kind, `file:line`, signature and doc comment.
fn definitions(found: &WordInFile) -> Vec<(DefinitionKind, String, &str, Option<&str>)> {
    found
        .definitions()
        .iter()
        .map(|definition| {
            (
                definition.kind,
                format!("{}:{}", definition.place.path, definition.place.line.get()),
                &*definition.signature,
                definition.doc.as_deref(),
            )
        })
        .collect()
}

fn usages(found: &WordInFile) -> Vec<(u32, &str)> {
    found
        .usages()
        .iter()
        .map(|usage| (usage.place.line.get(), &*usage.text))
        .collect()
}

const RUST_CART: &str = r"use crate::item::Item;

/// A cart of items.
/// Never empty.
#[derive(Debug)]
pub struct Cart {
    items: Vec<Item>,
}

impl Cart {
    /// What the items cost.
    pub fn total(&self) -> u32 {
        self.items.iter().map(Item::price).sum()
    }
}

pub fn total(cart: &Cart) -> u32 {
    // not a doc comment
    cart.total()
}
";

#[test]
fn a_rust_file_gives_its_definitions_with_signature_and_doc_comment() {
    let found = WordInFile::read(
        &file_path("src/cart.rs"),
        RUST_CART,
        Language::Rust,
        &word("Cart"),
    );

    assert_eq!(
        definitions(&found),
        [(
            DefinitionKind::Struct,
            "src/cart.rs:6".to_owned(),
            "pub struct Cart",
            Some("A cart of items.\nNever empty."),
        )]
    );
    assert_eq!(
        usages(&found),
        [
            (10, "impl Cart {"),
            (17, "pub fn total(cart: &Cart) -> u32 {")
        ]
    );
}

#[test]
fn two_rust_definitions_of_one_name_are_both_found() {
    let found = WordInFile::read(
        &file_path("src/cart.rs"),
        RUST_CART,
        Language::Rust,
        &word("total"),
    );

    assert_eq!(
        definitions(&found),
        [
            (
                DefinitionKind::Method,
                "src/cart.rs:12".to_owned(),
                "pub fn total(&self) -> u32",
                Some("What the items cost."),
            ),
            (
                DefinitionKind::Function,
                "src/cart.rs:17".to_owned(),
                "pub fn total(cart: &Cart) -> u32",
                None,
            ),
        ]
    );
    assert_eq!(usages(&found), [(19, "cart.total()")]);
}

const PHP_ORDER: &str = r"<?php

namespace App\Models;

use App\Support\Money;

/**
 * An order a customer placed.
 */
final class Order
{
    /**
     * The sum of its lines.
     */
    public function total(): Money
    {
        return Money::sum($this->lines);
    }
}

function total(Order $order): Money
{
    return $order->total();
}
";

#[test]
fn a_php_file_gives_its_definitions_and_usages() {
    let order = WordInFile::read(
        &file_path("app/Models/Order.php"),
        PHP_ORDER,
        Language::Php,
        &word("Order"),
    );
    let total = WordInFile::read(
        &file_path("app/Models/Order.php"),
        PHP_ORDER,
        Language::Php,
        &word("total"),
    );

    assert_eq!(
        definitions(&order),
        [(
            DefinitionKind::Class,
            "app/Models/Order.php:10".to_owned(),
            "final class Order",
            Some("An order a customer placed."),
        )]
    );
    assert_eq!(
        usages(&order),
        [(21, "function total(Order $order): Money")]
    );
    assert_eq!(
        definitions(&total),
        [
            (
                DefinitionKind::Method,
                "app/Models/Order.php:15".to_owned(),
                "public function total(): Money",
                Some("The sum of its lines."),
            ),
            (
                DefinitionKind::Function,
                "app/Models/Order.php:21".to_owned(),
                "function total(Order $order): Money",
                None,
            ),
        ]
    );
    assert_eq!(usages(&total), [(23, "return $order->total();")]);
}

const TS_CART: &str = r#"import { Item } from "./item";

/** A cart of items. */
export class Cart {
  /**
   * What the items cost.
   */
  total(): number {
    return this.items.length;
  }
}

/** The total of any cart. */
export const total = (cart: Cart): number => cart.total();
"#;

#[test]
fn a_typescript_file_gives_its_definitions_and_usages() {
    let cart = WordInFile::read(
        &file_path("src/cart.ts"),
        TS_CART,
        Language::TypeScript,
        &word("Cart"),
    );
    let total = WordInFile::read(
        &file_path("src/cart.ts"),
        TS_CART,
        Language::TypeScript,
        &word("total"),
    );

    assert_eq!(
        definitions(&cart),
        [(
            DefinitionKind::Class,
            "src/cart.ts:4".to_owned(),
            "export class Cart",
            Some("A cart of items."),
        )]
    );
    assert_eq!(
        usages(&cart),
        [(
            14,
            "export const total = (cart: Cart): number => cart.total();"
        )]
    );
    assert_eq!(
        definitions(&total),
        [
            (
                DefinitionKind::Method,
                "src/cart.ts:8".to_owned(),
                "total(): number",
                Some("What the items cost."),
            ),
            (
                DefinitionKind::Function,
                "src/cart.ts:14".to_owned(),
                "export const total = (cart: Cart): number =>",
                Some("The total of any cart."),
            ),
        ]
    );
    assert_eq!(
        usages(&total),
        [(
            14,
            "export const total = (cart: Cart): number => cart.total();"
        )]
    );
}

#[test]
fn tsx_files_are_read_with_the_tsx_grammar() {
    let source = "export function Badge() {\n  return <Label text=\"x\" />;\n}\n";

    let found = WordInFile::read(
        &file_path("src/badge.tsx"),
        source,
        Language::Tsx,
        &word("Label"),
    );

    assert_eq!(usages(&found), [(2, "return <Label text=\"x\" />;")]);
}

#[test]
fn rust_files_are_parsed_but_not_cut_into_pieces() {
    assert_eq!(Language::for_path("src/cart.rs"), Some(Language::Rust));
    assert_eq!(Language::for_pieces("src/cart.rs"), None);
    assert_eq!(
        Language::for_pieces("src/cart.ts"),
        Some(Language::TypeScript)
    );
}

#[test]
fn a_word_is_one_identifier() {
    assert!(Word::new("total").is_some());
    assert!(Word::new("$order").is_none());
    assert!(Word::new("a b").is_none());
    assert!(Word::new("").is_none());
    assert!(Word::new("->").is_none());
}

/// A clone with a PR's head: two `Cart`s, one of them imported by the hovered file.
struct Shop {
    repo: FixtureRepo,
    head: Sha,
}

impl Shop {
    fn new() -> Self {
        let repo = FixtureRepo::cloned_from("acme/shop");
        repo.write(
            "src/checkout.ts",
            "import { Cart } from \"./cart\";\n\nexport function pay(cart: Cart) {\n  return cart;\n}\n",
        );
        repo.write("src/cart.ts", TS_CART);
        repo.write(
            "legacy/basket.ts",
            "/** The old cart. */\nexport class Cart {}\n",
        );
        repo.write("README.md", "Cart is not code here.\n");
        let head = Sha::new(&repo.commit("head"));
        Self { repo, head }
    }

    fn code(&self) -> Arc<dyn CodeSource> {
        Arc::new(
            LocalGit::configured_clone(&RepoName::new("acme", "shop"), self.repo.path()).unwrap(),
        )
    }
}

#[test]
fn git_grep_finds_the_parsed_files_at_a_sha_that_hold_the_whole_word() {
    let shop = Shop::new();
    shop.repo.write("src/cart.ts", "export class Basket {}\n");
    shop.repo.commit("later");

    let files = GitGrep::in_clone(shop.repo.path())
        .files_with(&shop.head, &word("Cart"))
        .unwrap();

    assert_eq!(
        files,
        [
            file_path("legacy/basket.ts"),
            file_path("src/cart.ts"),
            file_path("src/checkout.ts")
        ]
    );
    let none: Vec<_> = GitGrep::in_clone(shop.repo.path())
        .files_with(&shop.head, &word("Car"))
        .unwrap();
    assert_eq!(none, Vec::<guided_review::file_path::FilePath>::new());
}

#[test]
fn definitions_rank_the_hovered_file_then_the_files_it_imports_then_the_rest() {
    let shop = Shop::new();
    let search = FakeFilesWithWord::default().with(
        "Cart",
        &["legacy/basket.ts", "src/cart.ts", "src/checkout.ts"],
    );
    let lookups = Lookups::new(FileSearch::whole_repo(Arc::new(search)));

    let lookup = lookups
        .look_up(
            &*shop.code(),
            &shop.head,
            &word("Cart"),
            &file_path("src/checkout.ts"),
        )
        .unwrap();

    let places: Vec<String> = lookup
        .definitions
        .iter()
        .map(|definition| format!("{}:{}", definition.place.path, definition.place.line.get()))
        .collect();
    assert_eq!(places, ["src/cart.ts:4", "legacy/basket.ts:2"]);
    assert_eq!(lookup.reach, Reach::WholeRepo);
    let usage_places: Vec<String> = lookup
        .usages
        .iter()
        .map(|usage| format!("{}:{}", usage.place.path, usage.place.line.get()))
        .collect();
    assert_eq!(
        usage_places,
        ["src/cart.ts:14", "src/checkout.ts:1", "src/checkout.ts:3"]
    );
}

#[test]
fn a_definition_in_the_hovered_file_comes_first() {
    let shop = Shop::new();
    let search = FakeFilesWithWord::default().with("Cart", &["legacy/basket.ts", "src/cart.ts"]);
    let lookups = Lookups::new(FileSearch::whole_repo(Arc::new(search)));

    let lookup = lookups
        .look_up(
            &*shop.code(),
            &shop.head,
            &word("Cart"),
            &file_path("legacy/basket.ts"),
        )
        .unwrap();

    assert_eq!(
        lookup.definitions.first().unwrap().place.path,
        file_path("legacy/basket.ts")
    );
    assert_eq!(lookup.definitions.first().unwrap().place.line, line(2));
}

#[test]
fn a_word_is_searched_once_per_sha() {
    let shop = Shop::new();
    let search = FakeFilesWithWord::default().with("Cart", &["src/cart.ts"]);
    let lookups = Lookups::new(FileSearch::whole_repo(Arc::new(search.clone())));

    let first = lookups
        .look_up(
            &*shop.code(),
            &shop.head,
            &word("Cart"),
            &file_path("src/cart.ts"),
        )
        .unwrap();
    let again = lookups
        .look_up(
            &*shop.code(),
            &shop.head,
            &word("Cart"),
            &file_path("src/checkout.ts"),
        )
        .unwrap();

    assert_eq!(search.searches(), ["Cart"]);
    assert_eq!(first.definitions.len(), again.definitions.len());
}

#[test]
fn without_a_clone_only_the_changed_files_are_read() {
    let shop = Shop::new();
    let lookups = Lookups::new(FileSearch::changed_files(
        [file_path("src/checkout.ts"), file_path("src/gone.ts")].into(),
    ));

    let lookup = lookups
        .look_up(
            &*shop.code(),
            &shop.head,
            &word("Cart"),
            &file_path("src/checkout.ts"),
        )
        .unwrap();

    assert_eq!(lookup.reach, Reach::ChangedFiles);
    assert!(lookup.definitions.is_empty());
    assert_eq!(lookup.usages.len(), 2);
}

#[test]
fn a_source_with_a_clone_searches_it_with_git_grep() {
    let shop = Shop::new();
    let lookups = Lookups::new(FileSearch::for_source(
        &*shop.code(),
        [file_path("src/checkout.ts")].into(),
    ));

    let lookup = lookups
        .look_up(
            &*shop.code(),
            &shop.head,
            &word("Cart"),
            &file_path("src/checkout.ts"),
        )
        .unwrap();

    assert_eq!(lookup.reach, Reach::WholeRepo);
    assert_eq!(lookup.definitions.len(), 2);
}

#[test]
fn a_word_in_more_files_than_the_cap_parses_the_first_ones_and_says_so() {
    let shop = Shop::new();
    let search = FakeFilesWithWord::default().with(
        "Cart",
        &["legacy/basket.ts", "src/cart.ts", "src/checkout.ts"],
    );
    let lookups = Lookups::new(FileSearch::whole_repo(Arc::new(search))).parsing_at_most(2);

    let lookup = lookups
        .look_up(
            &*shop.code(),
            &shop.head,
            &word("Cart"),
            &file_path("src/checkout.ts"),
        )
        .unwrap();

    assert!(lookup.cut_short);
    assert!(
        lookup
            .usages
            .iter()
            .all(|usage| usage.place.path != file_path("src/checkout.ts"))
    );
    let whole = Lookups::new(FileSearch::whole_repo(Arc::new(
        FakeFilesWithWord::default().with("Cart", &["src/cart.ts"]),
    )))
    .parsing_at_most(2)
    .look_up(
        &*shop.code(),
        &shop.head,
        &word("Cart"),
        &file_path("src/cart.ts"),
    )
    .unwrap();
    assert!(!whole.cut_short);
}
