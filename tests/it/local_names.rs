use guided_review::local_git::LocalGit;
use guided_review::navigation::{
    Column, DefinitionKind, FileSearch, Lookup, Lookups, Place, Reach, Word,
};
use guided_review::pull_request::Sha;
use std::sync::Arc;

use crate::support::fake_ports::FakeFilesWithWord;
use crate::support::{FixtureRepo, file_path, line};

/// One file committed in a clone, and lookups whose file search records what it was asked.
struct OneFile {
    repo: FixtureRepo,
    head: Sha,
    path: &'static str,
    text: &'static str,
    search: FakeFilesWithWord,
}

impl OneFile {
    fn new(path: &'static str, text: &'static str) -> Self {
        let repo = FixtureRepo::cloned_from("acme/shop");
        repo.write(path, text);
        let head = Sha::new(&repo.commit("head"));
        Self {
            repo,
            head,
            path,
            text,
            search: FakeFilesWithWord::default(),
        }
    }

    /// The lookup of the name that starts at `at` on line `number`; `at` is the first text on
    /// that line that begins with the name.
    fn hover(&self, number: usize, at: &str) -> Lookup {
        let text = self.text.lines().nth(number - 1).unwrap();
        let column = text
            .find(at)
            .unwrap_or_else(|| panic!("{at:?} not on line {number}"));
        let name: String = at
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        let code = LocalGit::configured_clone(&crate::support::shop_on_github(), self.repo.path())
            .unwrap();
        Lookups::new(FileSearch::whole_repo(Arc::new(self.search.clone())))
            .look_up_at(
                &code,
                &self.head,
                &Word::new(&name).unwrap(),
                &Place {
                    path: file_path(self.path),
                    line: line(number),
                },
                Column::try_from(column).unwrap(),
            )
            .unwrap()
    }
}

fn definitions(lookup: &Lookup) -> Vec<(DefinitionKind, u32)> {
    lookup
        .definitions
        .iter()
        .map(|definition| (definition.kind, definition.place.line.get()))
        .collect()
}

fn usage_lines(lookup: &Lookup) -> Vec<u32> {
    lookup
        .usages
        .iter()
        .map(|usage| usage.place.line.get())
        .collect()
}

const PHP_DELIVERIES: &str = r"<?php
class Deliveries
{
    private $deliveryDate;

    public function plan(Order $order): void
    {
        $deliveryDate = $order->deliveryDate();
        $this->deliveryDate = $deliveryDate;
        $this->schedule($deliveryDate);
    }

    public function deliveryDate(): string
    {
        $deliveryDate = 'monday';
        return $deliveryDate;
    }
}
";

#[test]
fn a_php_variable_is_looked_up_in_its_method_only() {
    let file = OneFile::new("app/Deliveries.php", PHP_DELIVERIES);

    let lookup = file.hover(10, "deliveryDate);");

    assert_eq!(definitions(&lookup), [(DefinitionKind::Variable, 8)]);
    assert_eq!(
        &*lookup.definitions[0].signature,
        "$deliveryDate = $order->deliveryDate();"
    );
    assert_eq!(usage_lines(&lookup), [9, 10]);
    assert_eq!(lookup.reach, Reach::EnclosingFunction);
    assert_eq!(file.search.searches(), Vec::<String>::new());
}

#[test]
fn a_php_parameter_is_defined_where_its_function_takes_it() {
    let file = OneFile::new("app/Deliveries.php", PHP_DELIVERIES);

    let lookup = file.hover(8, "order->");

    assert_eq!(definitions(&lookup), [(DefinitionKind::Parameter, 6)]);
    assert_eq!(usage_lines(&lookup), [8]);
    assert_eq!(file.search.searches(), Vec::<String>::new());
}

const PHP_LATER: &str = r"<?php
function later(array $days): array
{
    $date = 'monday';
    $plan = function ($date) {
        return $date;
    };
    $next = fn () => $date;
    return [$plan, $next];
}
";

#[test]
fn a_php_closure_is_a_function_of_its_own_and_an_arrow_function_sees_its_parent() {
    let file = OneFile::new("app/later.php", PHP_LATER);

    let in_closure = file.hover(6, "date;");
    let in_arrow = file.hover(8, "date;");

    assert_eq!(definitions(&in_closure), [(DefinitionKind::Parameter, 5)]);
    assert_eq!(usage_lines(&in_closure), [6]);
    assert_eq!(definitions(&in_arrow), [(DefinitionKind::Variable, 4)]);
    assert_eq!(usage_lines(&in_arrow), [8]);
}

const RUST_TOTAL: &str = r"pub struct Cart { total: u32 }

const LIMIT: u32 = 10;

pub fn total(cart: &Cart, items: &[u32]) -> u32 {
    let total = items.iter().sum::<u32>();
    let capped = total.min(LIMIT);
    dbg!(total);
    cart.total + capped + total
}

fn other() -> u32 {
    let total = 1;
    total
}
";

#[test]
fn a_rust_binding_or_parameter_is_looked_up_in_its_function_only() {
    let file = OneFile::new("src/cart.rs", RUST_TOTAL);

    let binding = file.hover(7, "total.min");
    let parameter = file.hover(6, "items.iter");

    assert_eq!(definitions(&binding), [(DefinitionKind::Variable, 6)]);
    assert_eq!(
        &*binding.definitions[0].signature,
        "let total = items.iter().sum::<u32>();"
    );
    assert_eq!(usage_lines(&binding), [7, 8, 9]);
    assert_eq!(definitions(&parameter), [(DefinitionKind::Parameter, 5)]);
    assert_eq!(usage_lines(&parameter), [6]);
    assert_eq!(file.search.searches(), Vec::<String>::new());
}

#[test]
fn a_rust_function_constant_or_field_is_looked_up_by_name_in_the_repo() {
    let file = OneFile::new("src/cart.rs", RUST_TOTAL);

    for (line, at) in [(5, "total(cart"), (7, "LIMIT"), (9, "total +")] {
        assert_eq!(file.hover(line, at).reach, Reach::WholeRepo, "{at}");
    }
    assert_eq!(file.search.searches(), ["total", "LIMIT", "total"]);
}

const TS_TILL: &str = r#"import { price } from "./pricing";

export const total = (cart: Cart): number => cart.total();

export function pay(cart: Cart, { discount }: Options) {
  const sum = price(cart) - discount;
  return sum > 0 ? { sum } : this.total;
}

export class Till {
  total(): number {
    let total = 0;
    for (const item of this.items) {
      total += item.price;
    }
    return total;
  }
}
"#;

#[test]
fn a_typescript_variable_or_parameter_is_looked_up_in_its_function_only() {
    let file = OneFile::new("src/till.ts", TS_TILL);

    let cases = [
        (7, "sum >", (DefinitionKind::Variable, 6), vec![7]),
        (6, "cart)", (DefinitionKind::Parameter, 5), vec![6]),
        (6, "discount;", (DefinitionKind::Parameter, 5), vec![6]),
        (14, "total +=", (DefinitionKind::Variable, 12), vec![14, 16]),
        (14, "item.price", (DefinitionKind::Variable, 13), vec![14]),
    ];
    for (line, at, definition, usages) in cases {
        let lookup = file.hover(line, at);
        assert_eq!(definitions(&lookup), [definition], "{at}");
        assert_eq!(usage_lines(&lookup), usages, "{at}");
    }
    assert_eq!(file.search.searches(), Vec::<String>::new());
}

#[test]
fn a_typescript_import_property_or_module_level_name_is_looked_up_by_name_in_the_repo() {
    let file = OneFile::new("src/till.ts", TS_TILL);

    for (line, at) in [(6, "price("), (7, "total;"), (3, "total =")] {
        assert_eq!(file.hover(line, at).reach, Reach::WholeRepo, "{at}");
    }
    assert_eq!(file.search.searches(), ["price", "total", "total"]);
}

#[test]
fn php_this_and_properties_are_looked_up_by_name_in_the_repo() {
    let file = OneFile::new("app/Deliveries.php", PHP_DELIVERIES);

    let this = file.hover(9, "this->");
    let property = file.hover(9, "deliveryDate =");

    assert_eq!(this.reach, Reach::WholeRepo);
    assert_eq!(property.reach, Reach::WholeRepo);
    assert_eq!(file.search.searches(), ["this", "deliveryDate"]);
}
