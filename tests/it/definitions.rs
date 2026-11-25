use std::sync::Arc;

use guided_review::code_source::CodeSource;
use guided_review::language::Language;
use guided_review::local_git::LocalGit;
use guided_review::navigation::{
    DefinitionKind, FileSearch, FilesWithWord, GitGrep, Import, Lookups, Reach, Word, WordInFile,
};
use guided_review::pull_request::Sha;

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
            LocalGit::configured_clone(&crate::support::shop_on_github(), self.repo.path())
                .unwrap(),
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

/// A clone at one commit holding `files`, and the lookup's ranking of `word` hovered in `from`.
fn ranked(files: &[(&str, &str)], word_text: &str, from: &str) -> Vec<String> {
    let repo = FixtureRepo::cloned_from("acme/shop");
    for (path, text) in files {
        repo.write(path, text);
    }
    let head = Sha::new(&repo.commit("head"));
    let code = LocalGit::configured_clone(&crate::support::shop_on_github(), repo.path()).unwrap();
    let paths: Vec<&str> = files.iter().map(|(path, _)| *path).collect();
    let search = FakeFilesWithWord::default().with(word_text, &paths);
    let lookup = Lookups::new(FileSearch::whole_repo(Arc::new(search)))
        .look_up(&code, &head, &word(word_text), &file_path(from))
        .unwrap();
    lookup
        .definitions
        .iter()
        .map(|definition| definition.place.path.to_string())
        .collect()
}

#[test]
fn a_rust_definition_ranks_as_imported_when_its_module_path_is_the_imports() {
    let ranked = ranked(
        &[
            ("src/aaa/word.rs", "pub struct Word;\n"),
            ("src/navigation/word.rs", "pub struct Word;\n"),
            (
                "src/app/hover.rs",
                "pub(crate) use crate::navigation::Word;\n\nfn f(_: Word) {}\n",
            ),
        ],
        "Word",
        "src/app/hover.rs",
    );

    assert_eq!(ranked, ["src/navigation/word.rs", "src/aaa/word.rs"]);
}

#[test]
fn a_rust_import_of_a_module_in_a_mod_rs_file_or_by_super_ranks_it_first() {
    let cart = "pub struct Cart;\n";
    let files = [
        ("src/a/cart.rs", cart),
        ("src/shop/cart/mod.rs", cart),
        (
            "src/shop/checkout.rs",
            "use super::cart::Cart;\n\nfn f(_: Cart) {}\n",
        ),
        (
            "src/web/pay.rs",
            "use crate::shop::cart::Cart;\n\nfn f(_: Cart) {}\n",
        ),
    ];

    assert_eq!(
        ranked(&files, "Cart", "src/shop/checkout.rs"),
        ["src/shop/cart/mod.rs", "src/a/cart.rs"]
    );
    assert_eq!(
        ranked(&files, "Cart", "src/web/pay.rs"),
        ["src/shop/cart/mod.rs", "src/a/cart.rs"]
    );
}

#[test]
fn a_php_definition_ranks_as_imported_when_its_folder_ends_with_the_used_namespace() {
    let order = "<?php\nclass Order {}\n";
    let ranked = ranked(
        &[
            ("app/Legacy/Order.php", order),
            ("app/Models/Order.php", order),
            (
                "app/Http/Checkout.php",
                "<?php\nnamespace App\\Http;\n\nuse App\\Models\\Order;\n\nnew Order();\n",
            ),
        ],
        "Order",
        "app/Http/Checkout.php",
    );

    assert_eq!(ranked, ["app/Models/Order.php", "app/Legacy/Order.php"]);
}

#[test]
fn a_typescript_definition_ranks_as_imported_when_the_import_resolves_to_its_file() {
    let cart = "export class Cart {}\n";
    let ranked = ranked(
        &[
            ("lib/cart.ts", cart),
            ("src/shop/cart.ts", cart),
            (
                "src/web/checkout.ts",
                "import { Cart } from \"../shop/cart\";\n\nnew Cart();\n",
            ),
        ],
        "Cart",
        "src/web/checkout.ts",
    );

    assert_eq!(ranked, ["src/shop/cart.ts", "lib/cart.ts"]);
}

#[test]
fn a_file_imported_for_another_name_does_not_rank_the_word() {
    let cart = "export class Cart {}\nexport class Item {}\n";
    let ranked = ranked(
        &[
            ("lib/cart.ts", cart),
            ("src/cart.ts", cart),
            (
                "src/checkout.ts",
                "import { Item } from \"./cart\";\n\nnew Cart();\n",
            ),
        ],
        "Cart",
        "src/checkout.ts",
    );

    assert_eq!(ranked, ["lib/cart.ts", "src/cart.ts"]);
}

#[test]
fn rust_imports_behind_any_visibility_are_read() {
    let names: Vec<String> = Import::all_in(
        Language::Rust,
        "pub(crate) use crate::cart::Cart;\npub(super) use super::Item;\npub(in crate::shop) use crate::Price;\npub use crate::Tax;\n",
    )
    .into_iter()
    .map(|import| import.name)
    .collect();

    for name in ["Cart", "Item", "Price", "Tax"] {
        assert!(
            names.iter().any(|imported| imported == name),
            "{name} in {names:?}"
        );
    }
}

#[test]
fn a_file_that_cannot_be_read_is_skipped_and_the_rest_still_answer() {
    let shop = Shop::new();
    let search = FakeFilesWithWord::default().with(
        "Cart",
        &["legacy/basket.ts", "src/cart.ts", "src/checkout.ts"],
    );
    let lookups = Lookups::new(FileSearch::whole_repo(Arc::new(search)));
    let code = UnreadableAt {
        code: shop.code(),
        paths: vec![file_path("legacy/basket.ts"), file_path("src/checkout.ts")],
    };

    let lookup = lookups
        .look_up(
            &code,
            &shop.head,
            &word("Cart"),
            &file_path("src/checkout.ts"),
        )
        .unwrap();

    let places: Vec<String> = lookup
        .definitions
        .iter()
        .map(|definition| definition.place.path.to_string())
        .collect();
    assert_eq!(places, ["src/cart.ts"]);
}

/// Reads like `code` but fails for `paths`.
struct UnreadableAt {
    code: Arc<dyn CodeSource>,
    paths: Vec<guided_review::file_path::FilePath>,
}

impl CodeSource for UnreadableAt {
    fn file_at(
        &self,
        sha: &Sha,
        path: &guided_review::file_path::FilePath,
    ) -> anyhow::Result<String> {
        if self.paths.contains(path) {
            anyhow::bail!("cannot read {path}");
        }
        self.code.file_at(sha, path)
    }
    fn blob_at(
        &self,
        sha: &Sha,
        path: &guided_review::file_path::FilePath,
    ) -> anyhow::Result<Arc<[u8]>> {
        self.code.blob_at(sha, path)
    }
    fn blob_info_at(
        &self,
        sha: &Sha,
        path: &guided_review::file_path::FilePath,
    ) -> anyhow::Result<guided_review::code_source::BlobInfo> {
        self.code.blob_info_at(sha, path)
    }
    fn diff(
        &self,
        base: &Sha,
        head: &Sha,
    ) -> anyhow::Result<Vec<guided_review::code_source::FileDiff>> {
        self.code.diff(base, head)
    }
    fn merge_base(&self, a: &Sha, b: &Sha) -> anyhow::Result<Sha> {
        self.code.merge_base(a, b)
    }
    fn working_tree_is_at(&self, head: &Sha) -> bool {
        self.code.working_tree_is_at(head)
    }
}
