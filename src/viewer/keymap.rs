use std::collections::{BTreeMap, HashMap};

use super::chord::Chord;
use super::shortcuts::Command;

/// The keys each command answers to: its defaults, unless the reviewer's `[keys]` name others.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Keymap {
    changed: HashMap<Command, Vec<Chord>>,
}

impl Keymap {
    /// Every way to press it; the first is the one a button shows.
    pub fn chords(&self, command: Command) -> &[Chord] {
        self.changed
            .get(&command)
            .map_or_else(|| command.default_chords(), Vec::as_slice)
    }

    pub fn first_chord(&self, command: Command) -> Option<Chord> {
        self.chords(command).first().copied()
    }

    pub fn is_changed(&self, command: Command) -> bool {
        self.changed.contains_key(&command)
    }

    fn clashes(&self) -> Vec<KeyProblem> {
        let mut clashes = Vec::new();
        for (at, first) in Command::ALL.iter().enumerate() {
            for second in Command::ALL.iter().skip(at + 1) {
                if !first.scope().overlaps(second.scope()) {
                    continue;
                }
                for chord in self.chords(*first) {
                    if self.chords(*second).contains(chord) {
                        clashes.push(KeyProblem::Clash {
                            chord: *chord,
                            commands: [*first, *second],
                        });
                    }
                }
            }
        }
        clashes
    }
}

/// One entry of `[keys]` as it is written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WrittenKeys {
    /// One text or a list of them, each meant as a chord.
    Texts(Vec<String>),
    /// Neither a text nor a list of texts: the value as TOML writes it.
    NotText(String),
}

/// What the reviewer's `[keys]` table says, entry by entry, by the name it is written under.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KeyChanges {
    entries: BTreeMap<String, WrittenKeys>,
}

impl KeyChanges {
    pub fn new(entries: impl IntoIterator<Item = (String, WrittenKeys)>) -> Self {
        Self {
            entries: entries.into_iter().collect(),
        }
    }

    pub fn entries(&self) -> impl Iterator<Item = (&str, &WrittenKeys)> {
        self.entries
            .iter()
            .map(|(name, written)| (name.as_str(), written))
    }

    pub fn written(&self, name: &str) -> Option<&WrittenKeys> {
        self.entries.get(name)
    }

    /// The keys the entry for this command gives it, while there is one that can be read.
    pub fn chords(&self, command: Command) -> Option<Vec<Chord>> {
        chords_of(self.entries.get(command.config_name())?).ok()
    }

    pub fn unknown_names(&self) -> impl Iterator<Item = &str> {
        self.entries
            .keys()
            .map(String::as_str)
            .filter(|name| Command::of_config_name(name).is_none())
    }

    /// The command's keys from now on; keys equal to its defaults leave no entry.
    pub fn set(&mut self, command: Command, chords: &[Chord]) {
        if chords == command.default_chords() {
            self.reset(command);
            return;
        }
        self.entries.insert(
            command.config_name().to_owned(),
            WrittenKeys::Texts(chords.iter().copied().map(Chord::keystroke).collect()),
        );
    }

    pub fn reset(&mut self, command: Command) {
        self.remove(command.config_name());
    }

    pub fn remove(&mut self, name: &str) {
        self.entries.remove(name);
    }

    /// What the good lines make, and what is wrong with the rest. Commands that share a key go
    /// back to their defaults, again and again until the defaults they fall back to clash with
    /// no one.
    pub fn resolve(&self) -> ResolvedKeymap {
        let mut unreadable = Vec::new();
        let mut unknown = Vec::new();
        let mut changed = HashMap::new();
        for (name, written) in &self.entries {
            let Some(command) = Command::of_config_name(name) else {
                unknown.push(KeyProblem::UnknownCommand(name.clone()));
                continue;
            };
            match chords_of(written) {
                Ok(chords) => {
                    changed.insert(command, chords);
                }
                Err(written) => unreadable.push((command, written)),
            }
        }
        unreadable.sort_by_key(|(command, _)| Command::ALL.iter().position(|it| it == command));
        let mut keymap = Keymap { changed };
        let mut clashes = Vec::new();
        loop {
            let found = keymap.clashes();
            if found.is_empty() {
                break;
            }
            for clash in &found {
                if let KeyProblem::Clash { commands, .. } = clash {
                    for command in commands {
                        keymap.changed.remove(command);
                    }
                }
            }
            clashes.extend(found);
        }
        let problems = unreadable
            .into_iter()
            .map(|(command, written)| KeyProblem::NotAKey { command, written })
            .chain(clashes)
            .chain(unknown)
            .collect();
        ResolvedKeymap { keymap, problems }
    }

    /// The keymap only while nothing is wrong, which is what Settings needs to allow Save.
    pub fn keymap(&self) -> Result<Keymap, Vec<KeyProblem>> {
        let ResolvedKeymap { keymap, problems } = self.resolve();
        if problems.is_empty() {
            Ok(keymap)
        } else {
            Err(problems)
        }
    }
}

/// The keymap the good lines of `[keys]` make, and what was wrong with the rest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedKeymap {
    pub keymap: Keymap,
    pub problems: Vec<KeyProblem>,
}

impl ResolvedKeymap {
    /// What to tell the reviewer once when the app opens with something wrong in `[keys]`.
    pub fn notice(&self) -> Option<String> {
        match self.problems.len() {
            0 => None,
            1 => Some(
                "1 key in config.toml cannot be used and keeps its default. Settings shows it."
                    .to_owned(),
            ),
            many => Some(format!(
                "{many} keys in config.toml cannot be used and keep their defaults. Settings shows them."
            )),
        }
    }
}

/// The first text that is not a chord, when one is not.
fn chords_of(written: &WrittenKeys) -> Result<Vec<Chord>, String> {
    match written {
        WrittenKeys::Texts(texts) => texts
            .iter()
            .map(|text| Chord::parse(text).ok_or_else(|| text.clone()))
            .collect(),
        WrittenKeys::NotText(value) => Err(value.clone()),
    }
}

/// Why the `[keys]` table cannot be used as it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyProblem {
    UnknownCommand(String),
    NotAKey {
        command: Command,
        written: String,
    },
    /// The commands in table order.
    Clash {
        chord: Chord,
        commands: [Command; 2],
    },
}

impl KeyProblem {
    pub fn message(&self) -> String {
        match self {
            Self::UnknownCommand(name) => format!("{name} is not a command"),
            Self::NotAKey { written, .. } => format!("{written} is not a key"),
            Self::Clash {
                chord,
                commands: [first, second],
            } => format!(
                "{} is taken by both “{}” and “{}”",
                chord.keystroke(),
                first.label(),
                second.label()
            ),
        }
    }
}
