use guided_review::language::Language;
use guided_review::line_number::LineNumber;
use guided_review::navigation::declaration_end;

fn end_of(language: Language, source: &str, line: usize) -> Option<u32> {
    declaration_end(language, source, LineNumber::try_from(line).unwrap()).map(LineNumber::get)
}

#[test]
fn a_php_interface_with_its_brace_on_the_next_line_ends_at_the_closing_brace() {
    let source =
        "<?php\n\nnamespace Shop;\n\ninterface Priced\n{\n    public function price(): int;\n}\n";

    assert_eq!(end_of(Language::Php, source, 5), Some(8));
}

#[test]
fn a_php_class_with_its_brace_on_the_next_line_ends_at_the_closing_brace() {
    let source = "<?php\n\nfinal class Cart extends Basket implements Priced\n{\n    public function total(): int\n    {\n        return 1;\n    }\n}\n";

    assert_eq!(end_of(Language::Php, source, 3), Some(9));
}

#[test]
fn a_php_method_with_its_brace_on_its_own_line_ends_at_the_closing_brace() {
    let source = "<?php\n\nclass Cart\n{\n    public function total(): int\n    {\n        return 1;\n    }\n}\n";

    assert_eq!(end_of(Language::Php, source, 5), Some(8));
}

#[test]
fn a_php_method_with_a_signature_over_many_lines_ends_at_the_closing_brace() {
    let source = "<?php\n\nclass Cart\n{\n    public function total(\n        int $a,\n        int $b,\n    ): int\n    {\n        return 1;\n    }\n}\n";

    assert_eq!(end_of(Language::Php, source, 5), Some(11));
}

#[test]
fn a_php_interface_method_ends_at_its_semicolon() {
    let source = "<?php\n\ninterface Priced\n{\n    public function price(): int;\n}\n";

    assert_eq!(end_of(Language::Php, source, 5), Some(5));
}

#[test]
fn a_rust_fn_with_a_where_clause_ends_at_the_closing_brace() {
    let source = "pub fn total<T>(items: &[T]) -> u32\nwhere\n    T: Priced,\n{\n    1\n}\n";

    assert_eq!(end_of(Language::Rust, source, 1), Some(6));
}

#[test]
fn a_rust_impl_with_a_where_clause_ends_at_the_closing_brace() {
    let source = "impl<T> Cart<T>\nwhere\n    T: Priced,\n{\n    fn total(&self) -> u32 {\n        1\n    }\n}\n";

    assert_eq!(end_of(Language::Rust, source, 1), Some(8));
}

#[test]
fn a_go_func_with_a_signature_over_many_lines_ends_at_the_closing_brace() {
    let source = "package shop\n\nfunc Total(\n\ta int,\n\tb int,\n) int {\n\treturn a + b\n}\n";

    assert_eq!(end_of(Language::Go, source, 3), Some(8));
}

#[test]
fn a_go_struct_type_ends_at_the_closing_brace() {
    let source = "package shop\n\ntype Cart struct {\n\tTotal int\n}\n";

    assert_eq!(end_of(Language::Go, source, 3), Some(5));
}

#[test]
fn a_typescript_interface_with_its_brace_on_the_next_line_ends_at_the_closing_brace() {
    let source = "export interface Priced\n{\n    price(): number;\n}\n";

    assert_eq!(end_of(Language::TypeScript, source, 1), Some(4));
}

#[test]
fn a_typescript_function_with_a_signature_over_many_lines_ends_at_the_closing_brace() {
    let source = "export function total(\n    a: number,\n    b: number,\n): number\n{\n    return a + b;\n}\n";

    assert_eq!(end_of(Language::TypeScript, source, 1), Some(7));
}

#[test]
fn a_python_def_with_a_signature_over_many_lines_ends_at_its_last_indented_line() {
    let source = "def total(\n    a,\n    b,\n):\n    x = a\n    return x + b\n\n\nprint(1)\n";

    assert_eq!(end_of(Language::Python, source, 1), Some(6));
}

#[test]
fn a_python_class_ends_at_the_last_line_of_its_indented_block() {
    let source = "class Cart:\n    def total(self):\n        return 1\n\n\nprint(1)\n";

    assert_eq!(end_of(Language::Python, source, 1), Some(3));
}

#[test]
fn a_php_interface_under_an_attribute_ends_at_the_closing_brace() {
    let source = "<?php\n\n#[Entity]\ninterface Priced\n{\n    public function price(): int;\n}\n";

    assert_eq!(end_of(Language::Php, source, 4), Some(7));
}

#[test]
fn a_php_class_under_an_attribute_ends_at_the_closing_brace() {
    let source = "<?php\n\n#[Entity]\nfinal class Cart\n{\n    public function total(): int\n    {\n        return 1;\n    }\n}\n";

    assert_eq!(end_of(Language::Php, source, 4), Some(10));
}

#[test]
fn a_php_method_under_an_attribute_ends_at_the_closing_brace() {
    let source = "<?php\n\nclass Cart\n{\n    #[Route('/total')]\n    public function total(): int\n    {\n        return 1;\n    }\n}\n";

    assert_eq!(end_of(Language::Php, source, 6), Some(9));
}

#[test]
fn a_php_default_value_holding_a_brace_does_not_end_the_search_for_the_body() {
    let source = "<?php\n\nclass Cart\n{\n    #[Route('/total')]\n    public function total(\n        string $open = '{',\n    ): int\n    {\n        return 1;\n    }\n}\n";

    assert_eq!(end_of(Language::Php, source, 6), Some(11));
}

#[test]
fn a_typescript_class_under_a_decorator_ends_at_the_closing_brace() {
    let source =
        "@Injectable()\nexport class Cart\n{\n    total() {\n        return 1;\n    }\n}\n";

    assert_eq!(end_of(Language::TypeScript, source, 2), Some(7));
}

#[test]
fn a_php_abstract_method_under_an_attribute_ends_at_its_semicolon() {
    let source = "<?php\n\nabstract class Cart\n{\n    #[Route('/total')]\n    abstract public function total(): int;\n\n    public function other(): int\n    {\n        return 1;\n    }\n}\n";

    assert_eq!(end_of(Language::Php, source, 6), Some(6));
}
