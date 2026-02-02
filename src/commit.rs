use breathes::hooks::run_hooks;
use chrono::Local;
use inquire::error::InquireResult;
use inquire::{Confirm, Editor, InquireError, Text};
use nix::sys::utsname::uname;
use nix::unistd::User;
use std::env::consts::ARCH;
use std::fmt::{Display, Formatter};
use std::fs::{File, read_to_string, remove_file};
use std::io::{Error, Write};

pub const WHY_PROMPT: &str = "Explain the reason for this change";
pub const WHY_FILENAME: &str = "why.txt";

pub const HOW_PROMPT: &str = "Details the changes";
pub const HOW_FILENAME: &str = "how.txt";

pub const WHERE_FILENAME: &str = "where.txt";

pub const SUBJECT_PROMPT: &str = "Summary of changes";
pub const SUBJECT_FILENAME: &str = "subject.txt";

pub const OUTCOME_PROMPT: &str = "Outcome of changes";
pub const OUTCOME_FILENAME: &str = "outcome.txt";

pub const IMPACT_PROMPT: &str = "Consequences of changes";
pub const IMPACT_FILENAME: &str = "impact.txt";

fn justify_paragraph(text: &str, width: usize) -> String {
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

fn file_create(p: &str, content: &str) -> Result<(), Error> {
    let mut f = File::create(p)?;
    f.write(content.as_bytes())?;
    f.sync_all()?;
    Ok(())
}

fn read_file(p: &str) -> String {
    let x = read_to_string(p).expect("failed to read");
    justify_paragraph(x.as_str(), 82)
}
pub fn author() -> String {
    let u = std::env::var("USER").expect("USER must be defined");
    if let Ok(Some(user)) = User::from_name(std::env::var("USER").expect("a").as_str()) {
        return user.gecos.to_string_lossy().to_string();
    }
    u.to_string()
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
        writeln!(f, "\n{}", read_file(SUBJECT_FILENAME))?;
        writeln!(f, "\n{}", read_file(WHY_FILENAME))?;
        writeln!(f, "\n{}", read_file(HOW_FILENAME))?;
        writeln!(f, "\n{}", read_file(OUTCOME_FILENAME))?;
        writeln!(
            f,
            "\nAuthor: {} Date: {} Os: {} {} ({})\n ",
            self.who, self.when, self.os, self.os_release, self.arch
        )?;
        remove_file(OUTCOME_FILENAME).expect("a");
        remove_file(HOW_FILENAME).expect("a");
        remove_file(WHY_FILENAME).expect("a");
        remove_file(SUBJECT_FILENAME).expect("a");
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
        file_create(SUBJECT_FILENAME, self.summary.as_str())?;
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
        file_create(WHY_FILENAME, &self.why)?;
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
        file_create(HOW_FILENAME, self.how.as_str())?;
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
        file_create(OUTCOME_FILENAME, &self.outcome).expect("failed");
        Ok(self)
    }
}
