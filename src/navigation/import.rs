//! What a file imports: each name it brings in and the module it names, so a definition in that
//! module can be told apart from others of the same name.

use crate::file_path::FilePath;
use crate::language::Language;

/// One name a file brings in, with the module the import names it from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Import {
    /// As the module calls it, not the alias the file gives it.
    pub name: String,
    from: Module,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Module {
    /// `crate::cart`, `super::cart`, `other_crate::cart`, by segment.
    Rust(Vec<String>),
    /// `App\Models`, by segment.
    PhpNamespace(Vec<String>),
    /// `./cart`, `../shop/cart`, `@/shop/cart`
    TypeScript(String),
}

impl Import {
    pub fn all_in(language: Language, text: &str) -> Vec<Self> {
        match language {
            Language::Rust => rust_imports(text),
            Language::Php => php_imports(text),
            Language::TypeScript | Language::Tsx => typescript_imports(text),
        }
    }

    /// The module segments a Rust import names, `crate`, `self` and `super` left out.
    pub fn module_names(&self) -> impl Iterator<Item = &str> {
        let segments = match &self.from {
            Module::Rust(segments) => segments.as_slice(),
            Module::PhpNamespace(_) | Module::TypeScript(_) => &[],
        };
        segments
            .iter()
            .map(String::as_str)
            .filter(|segment| !matches!(*segment, "crate" | "self" | "super"))
    }

    /// Whether the import, read in `from`, names the module of the file at `path`.
    pub fn leads_to(&self, from: &FilePath, path: &FilePath) -> bool {
        match &self.from {
            Module::Rust(segments) => rust_leads_to(segments, from, path),
            Module::PhpNamespace(namespace) => php_leads_to(namespace, path),
            Module::TypeScript(module) => typescript_leads_to(module, from, path),
        }
    }
}

fn segments_of(path: &str) -> Vec<&str> {
    path.split('/')
        .filter(|segment| !segment.is_empty())
        .collect()
}

/// `src/shop/cart/mod.rs` and `src/shop/cart.rs` give `src`, `shop`, `cart`; `src/lib.rs` gives `src`.
fn rust_module_of(path: &FilePath) -> Vec<&str> {
    let mut segments = segments_of(path.as_str());
    if let Some(file) = segments.pop() {
        let stem = file.strip_suffix(".rs").unwrap_or(file);
        if !matches!(stem, "mod" | "lib" | "main") {
            segments.push(stem);
        }
    }
    segments
}

/// The module itself, or a module right inside it whose items it re-exports.
fn is_module_or_child(module: &[&str], wanted: &[&str]) -> bool {
    module == wanted || (module.len() == wanted.len() + 1 && module.starts_with(wanted))
}

fn rust_leads_to(segments: &[String], from: &FilePath, path: &FilePath) -> bool {
    let module = rust_module_of(path);
    let Some((first, rest)) = segments.split_first() else {
        return false;
    };
    let rest: Vec<&str> = rest.iter().map(String::as_str).collect();
    match first.as_str() {
        "self" | "super" => {
            let mut wanted = rust_module_of(from);
            let mut relative = segments.iter().map(String::as_str).peekable();
            while let Some(step) = relative.next_if(|step| matches!(*step, "self" | "super")) {
                if step == "super" {
                    wanted.pop();
                }
            }
            wanted.extend(relative);
            is_module_or_child(&module, &wanted)
        }
        // A crate's paths start at its `src`, which is all a path says of where the crate is.
        _ => {
            let in_crate = match module.iter().rposition(|segment| *segment == "src") {
                Some(src) => module.get(src + 1..).unwrap_or_default(),
                None => module.as_slice(),
            };
            is_module_or_child(in_crate, &rest)
        }
    }
}

/// `App\Models` leads to a file in `app/Models`, or in `src/Models` with the root namespace left off.
fn php_leads_to(namespace: &[String], path: &FilePath) -> bool {
    let mut folders = segments_of(path.as_str());
    folders.pop();
    let ends_with = |wanted: &[String]| {
        !wanted.is_empty()
            && folders.len() >= wanted.len()
            && folders
                .iter()
                .rev()
                .zip(wanted.iter().rev())
                .all(|(folder, segment)| folder.eq_ignore_ascii_case(segment))
    };
    ends_with(namespace) || namespace.get(1..).is_some_and(ends_with)
}

fn typescript_leads_to(module: &str, from: &FilePath, path: &FilePath) -> bool {
    let file = without_code_extension(path.as_str());
    let module = without_code_extension(module);
    if module.starts_with('.') {
        let mut resolved = segments_of(from.as_str());
        resolved.pop();
        for step in segments_of(module) {
            match step {
                "." => {}
                ".." => {
                    resolved.pop();
                }
                name => resolved.push(name),
            }
        }
        let resolved = resolved.join("/");
        return file == resolved || file.strip_suffix("/index") == Some(resolved.as_str());
    }
    let module = module
        .strip_prefix("@/")
        .or_else(|| module.strip_prefix("~/"))
        .unwrap_or(module);
    let is_module = |file: &str| {
        file == module
            || file
                .strip_suffix(module)
                .is_some_and(|before| before.ends_with('/'))
    };
    is_module(file) || file.strip_suffix("/index").is_some_and(is_module)
}

fn without_code_extension(text: &str) -> &str {
    const EXTENSIONS: [&str; 8] = [".ts", ".tsx", ".mts", ".cts", ".js", ".jsx", ".mjs", ".cjs"];
    EXTENSIONS
        .iter()
        .find_map(|extension| text.strip_suffix(extension))
        .unwrap_or(text)
}

/// The body of each `use` statement, behind any visibility: `pub(crate) use a::b;` gives `a::b`.
fn rust_use_statements(text: &str) -> Vec<String> {
    let mut statements = Vec::new();
    let mut statement: Option<String> = None;
    for line in text.lines() {
        let line = line.trim();
        let body = if let Some(open) = statement.as_mut() {
            open.push(' ');
            open.push_str(line);
            open
        } else {
            let Some(rest) = without_visibility(line).strip_prefix("use ") else {
                continue;
            };
            statement.insert(rest.to_owned())
        };
        if let Some((tree, _)) = body.split_once(';') {
            statements.push(tree.to_owned());
            statement = None;
        }
    }
    statements
}

/// `pub`, `pub(crate)`, `pub(super)`, `pub(in a::b)` left off.
fn without_visibility(line: &str) -> &str {
    let Some(rest) = line.strip_prefix("pub") else {
        return line;
    };
    if let Some(scoped) = rest.strip_prefix('(') {
        return scoped
            .split_once(')')
            .map_or(line, |(_, after)| after.trim_start());
    }
    if rest.starts_with(char::is_whitespace) {
        rest.trim_start()
    } else {
        line
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Token<'a> {
    Name(&'a str),
    Path,
    Open,
    Close,
    Comma,
    Glob,
}

fn rust_tokens(tree: &str) -> Vec<Token<'_>> {
    let mut tokens = Vec::new();
    let mut rest = tree;
    while let Some(first) = rest.chars().next() {
        let (token, length) = match first {
            '{' => (Some(Token::Open), 1),
            '}' => (Some(Token::Close), 1),
            ',' => (Some(Token::Comma), 1),
            '*' => (Some(Token::Glob), 1),
            ':' if rest.starts_with("::") => (Some(Token::Path), 2),
            c if c.is_alphanumeric() || c == '_' => {
                let length = rest
                    .find(|c: char| !(c.is_alphanumeric() || c == '_'))
                    .unwrap_or(rest.len());
                (rest.get(..length).map(Token::Name), length)
            }
            other => (None, other.len_utf8()),
        };
        tokens.extend(token);
        rest = rest.get(length..).unwrap_or_default();
    }
    tokens
}

fn rust_imports(text: &str) -> Vec<Import> {
    let mut imports = Vec::new();
    for statement in rust_use_statements(text) {
        let tokens = rust_tokens(&statement);
        let mut at = 0;
        read_use_tree(&tokens, &mut at, &[], &mut imports);
    }
    imports
}

/// One use tree from `at`: `a::b`, `a::b as c`, `a::{b, c::d}`, `a::*`.
fn read_use_tree(tokens: &[Token], at: &mut usize, prefix: &[String], imports: &mut Vec<Import>) {
    let mut path = prefix.to_vec();
    loop {
        match tokens.get(*at) {
            Some(Token::Open) => {
                *at += 1;
                while !matches!(tokens.get(*at), Some(Token::Close) | None) {
                    let before = *at;
                    read_use_tree(tokens, at, &path, imports);
                    if tokens.get(*at) == Some(&Token::Comma) || *at == before {
                        *at += 1;
                    }
                }
                *at += 1;
                return;
            }
            Some(Token::Name(name)) => {
                *at += 1;
                path.push((*name).to_owned());
                if tokens.get(*at) == Some(&Token::Path) {
                    *at += 1;
                    continue;
                }
                if tokens.get(*at) == Some(&Token::Name("as")) {
                    *at += 2;
                }
                let Some(mut name) = path.pop() else { return };
                if name == "self" {
                    match path.pop() {
                        Some(module) => name = module,
                        None => return,
                    }
                }
                imports.push(Import {
                    name,
                    from: Module::Rust(path),
                });
                return;
            }
            Some(Token::Glob) => {
                *at += 1;
                return;
            }
            _ => return,
        }
    }
}

/// `use App\Models\Order;`, `use App\{Cart, Item as Line};`, `use function App\price;`
fn php_imports(text: &str) -> Vec<Import> {
    let mut imports = Vec::new();
    for statement in text
        .lines()
        .filter_map(|line| line.trim().strip_prefix("use "))
    {
        let statement = statement.trim_end_matches(';');
        let statement = statement
            .strip_prefix("function ")
            .or_else(|| statement.strip_prefix("const "))
            .unwrap_or(statement);
        let (prefix, items): (&str, Vec<&str>) = match statement.split_once('{') {
            Some((prefix, group)) => (prefix, group.trim_end_matches('}').split(',').collect()),
            None => ("", vec![statement]),
        };
        for item in items {
            let item = item.split(" as ").next().unwrap_or(item).trim();
            let mut namespace: Vec<String> = prefix
                .split('\\')
                .chain(item.split('\\'))
                .map(str::trim)
                .filter(|segment| !segment.is_empty())
                .map(str::to_owned)
                .collect();
            if let Some(name) = namespace.pop() {
                imports.push(Import {
                    name,
                    from: Module::PhpNamespace(namespace),
                });
            }
        }
    }
    imports
}

/// `import { Cart as C, type Item } from "./cart"`, `import Cart from "./cart"`
fn typescript_imports(text: &str) -> Vec<Import> {
    let mut imports = Vec::new();
    let mut statement = String::new();
    for line in text.lines() {
        if statement.is_empty() && !line.trim_start().starts_with("import ") {
            continue;
        }
        statement.push_str(line);
        statement.push(' ');
        let Some((clause, module)) = statement.rsplit_once(" from ") else {
            if statement.trim_start().starts_with("import \"")
                || statement.trim_start().starts_with("import '")
            {
                statement.clear();
            }
            continue;
        };
        let module = module
            .trim()
            .trim_end_matches(';')
            .trim_matches(['"', '\'']);
        let clause = clause.trim_start().trim_start_matches("import").trim();
        let clause = clause.trim_prefix("type ");
        for part in clause.split([',', '{', '}']) {
            let part = part.trim().trim_prefix("type ");
            let name = part.split(" as ").next().unwrap_or(part).trim();
            if !name.is_empty() && !name.starts_with('*') {
                imports.push(Import {
                    name: name.to_owned(),
                    from: Module::TypeScript(module.to_owned()),
                });
            }
        }
        statement.clear();
    }
    imports
}
