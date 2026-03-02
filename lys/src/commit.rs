use crate::utils::run_hooks;
use chrono::Local;
use inquire::error::InquireResult;
use inquire::{Confirm, Editor, InquireError, Text};
#[cfg(unix)]
use nix::sys::utsname::uname;
#[cfg(unix)]
use nix::unistd::User;
use std::collections::BTreeMap;
use std::env::consts::ARCH;
use std::fmt::{Display, Formatter};
use std::io::Error;

pub const WHY_PROMPT: &str = "Explain the reason for this change";
pub const HOW_PROMPT: &str = "Details the changes";
pub const SUBJECT_PROMPT: &str = "Summary of changes";
pub const OUTCOME_PROMPT: &str = "Outcome of changes";

pub struct Log {
    pub author: String,
    pub message: String,
    pub at: String,
    pub signature: String,
    pub changes: Vec<(String, FileChange)>,
}

impl Display for Log {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let x = self.author.split("<").collect::<Vec<&str>>();
        let author = x[0].trim().to_string();
        writeln!(f, "\n{author} at {} ({})\n", self.at, self.signature)?;
        writeln!(f, "{}\n", self.message)?;

        if !self.changes.is_empty() {
            let mut root = Tree::default();
            for (path, change) in &self.changes {
                let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
                insert_into_tree(&mut root, &parts, change.clone());
            }
            print_tree(f, &root, "", true)?; // print from root
            writeln!(f)?;
        }
        Ok(())
    }
}

#[derive(Default)]
struct Tree {
    children: BTreeMap<String, Tree>,
    is_file: bool,
    change: Option<FileChange>,
}

fn insert_into_tree(node: &mut Tree, parts: &[&str], change: FileChange) {
    if parts.is_empty() {
        return;
    }
    let first = parts[0];
    let child = node
        .children
        .entry(first.to_string())
        .or_insert_with(Tree::default);
    if parts.len() == 1 {
        child.is_file = true;
        child.change = Some(change);
    } else {
        insert_into_tree(child, &parts[1..], change);
    }
}

fn print_tree(f: &mut Formatter<'_>, node: &Tree, prefix: &str, is_root: bool) -> std::fmt::Result {
    // For root, we don't print a name, only its children
    let len = node.children.len();
    let mut i = 0usize;
    for (name, child) in &node.children {
        i += 1;
        let is_last = i == len;
        let connector = if is_last { "└──" } else { "├──" };
        if child.is_file {
            // Affiche le marqueur et les compteurs
            let marker = match &child.change {
                Some(FileChange::Added { added, mode }) => {
                    let m = mode.map(|v| crate::vcs::format_mode(v)).unwrap_or_default();
                    format!("{m} + {added}")
                }
                Some(FileChange::Deleted { deleted, mode }) => {
                    let m = mode.map(|v| crate::vcs::format_mode(v)).unwrap_or_default();
                    format!("{m} - {deleted}")
                }
                Some(FileChange::Modified {
                    added,
                    deleted,
                    mode,
                }) => {
                    let m = mode.map(|v| crate::vcs::format_mode(v)).unwrap_or_default();
                    format!("{m} ~ +{added} -{deleted}")
                }
                _ => String::new(),
            };
            writeln!(f, "{prefix}{connector} {marker} {name}")?;
        } else {
            writeln!(f, "{prefix}{connector} {name}")?;
        }
        let new_prefix = if is_last {
            format!("{}    ", prefix)
        } else {
            format!("{}│   ", prefix)
        };
        print_tree(f, child, &new_prefix, false)?;
    }
    if is_root && len == 0 {
        // nothing to print
    }
    Ok(())
}
#[derive(Debug, Clone)]
pub enum FileChange {
    Added {
        added: usize,
        mode: Option<i64>,
    },
    Deleted {
        deleted: usize,
        mode: Option<i64>,
    },
    Modified {
        added: usize,
        deleted: usize,
        mode: Option<i64>,
    },
}

fn commit_justify(text: &str, width: usize) -> String {
    let words: Vec<&str> = text.split_whitespace().collect();
    let mut lines = Vec::new();
    let mut current_line = Vec::new();
    let mut current_len = 0;

    for word in words {
        if current_len + word.len() + current_line.len() > width {
            lines.push(format_line(&current_line, width, current_len));
            current_line.clear();
            current_len = 0;
        }
        current_line.push(word);
        current_len += word.len();
    }
    lines.push(current_line.join(" ")); // Dernière ligne alignée à gauche
    lines.join("\n")
}

fn format_line(words: &[&str], width: usize, current_len: usize) -> String {
    if words.len() == 1 {
        return words[0].to_string();
    }
    let total_spaces = width - current_len;
    let spaces_between = total_spaces / (words.len() - 1);
    let extra_spaces = total_spaces % (words.len() - 1);

    let mut result = String::new();
    for (i, word) in words.iter().enumerate() {
        result.push_str(word);
        if i < words.len() - 1 {
            let n = spaces_between + if i < extra_spaces { 1 } else { 0 };
            result.push_str(&" ".repeat(n));
        }
    }
    result
}

pub fn author() -> String {
    use crate::db::connect_lys;
    use std::path::Path;

    // 1. On tente de lire l'identité souveraine dans la config SQLite
    if let Ok(conn) = connect_lys(Path::new(".")) {
        let mut stmt = conn
            .prepare("SELECT value FROM config WHERE key = 'author'")
            .unwrap();
        if let Ok(sqlite::State::Row) = stmt.next() {
            if let Ok(val) = stmt.read::<String, _>(0) {
                if !val.trim().is_empty() {
                    return val;
                }
            }
        }
    }

    // 2. Fallback : Identité système originale si la DB est vide
    let u = std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "unknown".to_string());

    #[cfg(unix)]
    {
        // Sur Unix, on tente de récupérer le "Real Name" (GECOS)
        if let Ok(Some(user)) = User::from_name(u.as_str()) {
            let gecos = user.gecos.to_string_lossy().to_string();
            if !gecos.is_empty() {
                return gecos;
            }
        }
    }
    u
}
#[derive(Default, Debug, Clone)]
pub struct Commit {
    pub t: String,
    pub os: String,
    pub os_release: String,
    pub os_version: String,
    pub os_domain: String,
    pub machine: String,
    pub arch: String,
    pub summary: String,
    pub why: String,
    pub who: String,
    pub src: String,
    pub how: String,
    pub when: String,
    pub what: String,
    pub where_path: Vec<String>,
    pub outcome: String,
    pub impact: String,
    pub breaking_changes: String,
}

impl Display for Commit {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "{}", commit_justify(self.summary.as_str(), 82))?;
        writeln!(f, "\n{}", commit_justify(self.why.as_str(), 82))?;
        writeln!(f, "\n{}", commit_justify(self.how.as_str(), 82))?;
        writeln!(f, "\n{}", commit_justify(self.outcome.as_str(), 82))?;
        writeln!(
            f,
            "\nAuthor: {} Date: {} Os: {} {} ({})\n ",
            self.who, self.when, self.os, self.os_release, self.arch
        )?;
        Ok(())
    }
}
impl Commit {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    ///
    /// # Errors
    ///
    /// Bad user or cancel by user
    ///
    pub fn confirm(&mut self) -> InquireResult<&mut Self> {
        println!("{self}");
        if Confirm::new("Confirm commit?")
            .with_default(true)
            .prompt()?
        {
            Ok(self)
        } else {
            Err(InquireError::from(Error::other("commit aborted")))
        }
    }
    ///
    /// Commit the changes to the repository
    ///
    /// # Errors
    ///
    /// On bad user inputs
    ///
    pub fn commit(&mut self) -> InquireResult<&mut Self> {
        if run_hooks().is_ok() {
            return self
                .ask_summary()?
                .ask_why()?
                .ask_how()?
                .ask_benefits()?
                .human_and_system()?
                .confirm();
        }
        Err(InquireError::OperationCanceled)
    }
    ///
    /// # Errors
    ///
    /// On bad user inputs
    ///
    pub fn ask_summary(&mut self) -> InquireResult<&mut Self> {
        self.summary.clear();
        while self.summary.is_empty() {
            self.summary.clear();
            self.summary
                .push_str(Text::new("Commit summary:").prompt()?.as_str());
        }
        if self.summary.is_empty() {
            return Err(InquireError::from(Error::other("bad summary")));
        }
        Ok(self)
    }

    ///
    /// Why are you making these changes?
    ///
    /// # Errors
    ///
    /// On bad user inputs
    ///
    pub fn ask_why(&mut self) -> InquireResult<&mut Self> {
        self.why.clear();
        while self.why.is_empty() {
            self.why.clear();
            self.why
                .push_str(Editor::new(WHY_PROMPT).prompt()?.as_str());
        }
        if self.why.is_empty() {
            return Err(InquireError::from(Error::other("bad why")));
        }
        Ok(self)
    }

    ///
    /// Why are you making these changes?
    ///
    /// # Errors
    ///
    /// On bad user inputs
    ///
    pub fn ask_how(&mut self) -> InquireResult<&mut Self> {
        self.how.clear();
        while self.how.is_empty() {
            self.how.clear();
            self.how
                .push_str(Editor::new(HOW_PROMPT).prompt()?.as_str());
        }
        if self.why.is_empty() {
            return Err(InquireError::from(Error::other("bad why")));
        }
        Ok(self)
    }

    pub fn human_and_system(&mut self) -> InquireResult<&mut Self> {
        self.os.clear();
        self.os_version.clear();
        self.os_release.clear();
        self.os_domain.clear();
        self.machine.clear();
        self.arch.clear();
        self.who.clear();
        self.when.clear();
        self.arch.push_str(ARCH);
        self.when.push_str(
            Local::now()
                .format("%Y-%m-%d %H:%M:%S")
                .to_string()
                .as_str(),
        );
        #[cfg(unix)]
        {
            let o = uname().expect("failed");
            self.os
                .push_str(o.sysname().to_str().expect("").to_string().as_str());
            self.machine
                .push_str(o.machine().to_str().expect("").to_string().as_str());
            self.os_release
                .push_str(o.release().to_str().expect("").to_string().as_str());
            self.os_version
                .push_str(o.version().to_str().expect("").to_string().as_str());
            self.os_domain
                .push_str(o.nodename().to_str().expect("").to_string().as_str());
        }
        #[cfg(windows)]
        {
            let os_name = std::env::consts::OS;
            let os_release = std::env::var("OS").unwrap_or_else(|_| "Windows".to_string());
            let machine = std::env::var("COMPUTERNAME").unwrap_or_default();
            let domain = std::env::var("USERDOMAIN").unwrap_or_default();
            self.os.push_str(os_name);
            self.machine.push_str(machine.as_str());
            self.os_release.push_str(os_release.as_str());
            self.os_version.push_str(os_release.as_str());
            self.os_domain.push_str(domain.as_str());
        }
        self.who.push_str(author().as_str());
        Ok(self)
    }

    ///
    /// What code resolve
    ///
    /// # Errors
    ///
    /// On bad user inputs
    ///
    pub fn ask_benefits(&mut self) -> InquireResult<&mut Self> {
        self.outcome.clear();
        while self.outcome.is_empty() {
            self.outcome.clear();
            self.outcome
                .push_str(Editor::new(OUTCOME_PROMPT).prompt()?.as_str());
        }
        if self.outcome.is_empty() {
            return Err(InquireError::from(Error::other("bad benefits")));
        }
        Ok(self)
    }
}
