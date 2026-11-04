use globset::{Glob, GlobBuilder, GlobSet, GlobSetBuilder};

const BUILT_IN_GENERATED: &[&str] = &[
    "composer.lock",
    "package-lock.json",
    "yarn.lock",
    "pnpm-lock.yaml",
    "Cargo.lock",
    "*.snap",
    "*.min.js",
    "*.min.css",
];

const CUT_PER_HUNK: &[&str] = &[".yml", ".yaml", ".json", ".xml", ".md", ".markdown"];

/// Formats without symbols: each hunk is its own piece.
pub fn is_cut_per_hunk(path: &str) -> bool {
    let path = path.to_ascii_lowercase();
    CUT_PER_HUNK
        .iter()
        .any(|extension| path.ends_with(extension))
}

/// Built-in list plus the root `.gitattributes` `linguist-generated` rules.
/// Nested `.gitattributes` files are not read.
pub struct GeneratedFiles {
    built_in: GlobSet,
    attributes: Vec<AttributeRule>,
}

struct AttributeRule {
    pattern: GlobSet,
    generated: bool,
}

#[expect(
    clippy::expect_used,
    reason = "BUILT_IN_GENERATED is fixed in this file, so every pattern is a valid glob"
)]
fn built_in_generated() -> GlobSet {
    let mut built_in = GlobSetBuilder::new();
    for pattern in BUILT_IN_GENERATED {
        built_in.add(Glob::new(&format!("**/{pattern}")).expect("valid built-in glob"));
    }
    built_in.build().expect("valid built-in globs")
}

impl GeneratedFiles {
    pub fn new(root_gitattributes: &str) -> Self {
        Self {
            built_in: built_in_generated(),
            attributes: root_gitattributes
                .lines()
                .filter_map(AttributeRule::parse)
                .collect(),
        }
    }

    pub fn contains(&self, path: &str) -> bool {
        let from_attributes = self
            .attributes
            .iter()
            .rev()
            .find(|rule| rule.pattern.is_match(path))
            .map(|rule| rule.generated);
        from_attributes.unwrap_or_else(|| self.built_in.is_match(path))
    }
}

impl AttributeRule {
    fn parse(line: &str) -> Option<Self> {
        let mut words = line.split_whitespace();
        let pattern = words.next().filter(|pattern| !pattern.starts_with('#'))?;
        let generated = words.rev().find_map(|attribute| match attribute {
            "linguist-generated" | "linguist-generated=true" => Some(true),
            "-linguist-generated" | "!linguist-generated" | "linguist-generated=false" => {
                Some(false)
            }
            _ => None,
        })?;
        Some(Self {
            pattern: Self::glob(pattern)?,
            generated,
        })
    }

    fn glob(pattern: &str) -> Option<GlobSet> {
        let pattern = pattern.trim_end_matches('/');
        let anchored = pattern.trim_start_matches('/');
        let glob = if pattern.contains('/') {
            anchored.to_owned()
        } else {
            format!("**/{pattern}")
        };
        let mut set = GlobSetBuilder::new();
        for glob in [glob.clone(), format!("{glob}/**")] {
            set.add(
                GlobBuilder::new(&glob)
                    .literal_separator(true)
                    .build()
                    .ok()?,
            );
        }
        set.build().ok()
    }
}
