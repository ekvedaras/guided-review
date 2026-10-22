#[allow(dead_code)]
mod support;

use support::{FakeGitHub, FixtureRepo, GuidedReview};

const PR: &str = "https://github.com/acme/shop/pull/7";

/// The piece lines the outline prints for one file that the PR adds.
fn pieces_of_added_file(path: &str, content: &str) -> Vec<String> {
    let repo = FixtureRepo::cloned_from("acme/shop");
    repo.write("README.md", "# Shop\n");
    let base = repo.commit("base");
    repo.write(path, content);
    let head = repo.commit("head");
    let github = FakeGitHub::serving_pull("acme/shop", 7, &base, &head);

    let run = GuidedReview::new(&repo, &github).run(&["outline", PR]);

    run.stdout_with(&[(&base, "<base>"), (&head, "<head>")])
        .lines()
        .filter(|line| {
            line.starts_with("  ")
                && !line.starts_with("  #(top)")
                && !line.trim_start().starts_with("subjects:")
        })
        .map(|line| line.trim_start().to_string())
        .collect()
}

#[test]
fn ts_functions_classes_and_methods_are_pieces() {
    let pieces = pieces_of_added_file(
        "src/cart.ts",
        r#"export function total(items: number[]): number {
  return items.length;
}

export abstract class Cart {
  add(item: string): void {
    console.log(item);
  }

  #secret(): void {}
}

interface Priced {
  price: number;
}
"#,
    );

    assert_eq!(
        pieces,
        [
            "#total  function  added  +4 −0  function total(items: number[]): number",
            "#Cart  class  added  +5 −0  abstract class Cart",
            "#Cart::add  method  added  +3 −0  add(item: string): void",
            "#Priced  interface  added  +3 −0  interface Priced",
        ]
    );
}

#[test]
fn ts_type_aliases_and_enums_are_pieces() {
    let pieces = pieces_of_added_file(
        "src/types.ts",
        r#"export type Money = {
  cents: number;
};

enum Status {
  Open,
  Closed,
}
"#,
    );

    assert_eq!(
        pieces,
        [
            "#Money  type  added  +4 −0  type Money = { cents: number; }",
            "#Status  enum  added  +4 −0  enum Status",
        ]
    );
}

#[test]
fn tsx_arrow_components_are_pieces_and_other_arrow_functions_are_functions() {
    let pieces = pieces_of_added_file(
        "src/Cart.tsx",
        r#"export const CartView = ({ items }: Props) => {
  const label = () => "inner";
  return <ul>{items.length}</ul>;
};

const format = (cents: number): string => `${cents}`;
"#,
    );

    assert_eq!(
        pieces,
        [
            "#CartView  component  added  +5 −0  const CartView = ({ items }: Props) =>",
            "#format  function  added  +1 −0  const format = (cents: number): string =>",
        ]
    );
}

#[test]
fn ts_tests_are_pieces_nested_under_their_describe() {
    let pieces = pieces_of_added_file(
        "src/cart.test.ts",
        r#"describe("Cart", () => {
  it("adds items", () => {
    expect(1).toBe(1);
  });

  test("removes items", () => {
    expect(2).toBe(2);
  });
});
"#,
    );

    assert_eq!(
        pieces,
        [
            "#Cart  test  added  +3 −0  describe(\"Cart\", () =>",
            "#Cart::adds items  test  added  +3 −0  it(\"adds items\", () =>",
            "#Cart::removes items  test  added  +3 −0  test(\"removes items\", () =>",
        ]
    );
}

#[test]
fn php_enums_and_consts_are_pieces() {
    let pieces = pieces_of_added_file(
        "app/Status.php",
        r#"<?php

const LIMIT = 10;

enum Status: string
{
    case Open = 'open';

    const DEFAULT = self::Open;

    public function label(): string
    {
        return 'x';
    }
}
"#,
    );

    assert_eq!(
        pieces,
        [
            "#LIMIT  const  added  +1 −0  const LIMIT = 10",
            "#Status  enum  added  +6 −0  enum Status: string",
            "#Status::DEFAULT  const  added  +1 −0  const DEFAULT = self::Open",
            "#Status::label  method  added  +4 −0  public function label(): string",
        ]
    );
}
