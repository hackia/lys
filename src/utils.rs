use std::io::stdout;

use crossterm::{
    execute,
    style::{Print, Stylize},
    terminal::size,
};

use crate::vcs::FileStatus;

pub fn ok(description: &str) {
    let (x, _) = size().expect("failed to get term size");

    let padding = x - "[ ok ]".len() as u16 - description.len() as u16 - 7;

    let _ = execute!(
        stdout(),
        Print(" * ".green().bold()),
        Print(description),
        Print(" ".repeat(padding as usize)),
        Print(" [ ".white().bold()),
        Print("ok".green().bold()),
        Print(" ]\n".white().bold()),
    );
}

pub fn ko(description: &str) {
    let (x, _) = size().expect("failed to get term size");

    let padding = x - "[ ko ]".len() as u16 - description.len() as u16 - 7;

    let _ = execute!(
        stdout(),
        Print(" ! ".red().bold()),
        Print(description),
        Print(" ".repeat(padding as usize)),
        Print(" [ ".white().bold()),
        Print("ko".red().bold()),
        Print(" ]\n".white().bold()),
    );
}
pub fn ok_status(verb: &FileStatus) {
    match verb {
        FileStatus::New(p) => {
            println!(
                "{}",
                format_args!("\x1b[1;32m+\x1b[1;37m {}\x1b[0m", p.display())
            );
        }
        FileStatus::Modified(p, _) => {
            println!(
                "{}",
                format_args!("\x1b[1;33m~\x1b[1;37m {}\x1b[0m", p.display())
            );
        }
        FileStatus::Deleted(p, _) => {
            println!(
                "{}",
                format_args!("\x1b[1;31m-\x1b[1;37m {}\x1b[0m", p.display())
            );
        }
        _ => {}
    }
}

pub fn ok_tag(tag: &str, description: &str, date: &str, _hash: &str) {
    let (x, _) = size().expect("failed to get term size");

    let padding = x - tag.len() as u16 - description.len() as u16 - date.len() as u16 - 9;
    let _ = execute!(
        stdout(),
        Print(" * ".green().bold()),
        Print(date.blue().bold()),
        Print(" "),
        Print(description.cyan().bold()),
        Print(" ".repeat(padding as usize)),
        Print(" [ ".white().bold()),
        Print(tag.green().bold()),
        Print(" ]\n".white().bold()),
    );
}

pub fn ok_audit_commit(hash: &str) {
    let (x, _) = size().expect("failed to get term size");

    let description = " Signature is valid ";
    let padding = x - hash.len() as u16 - description.len() as u16 - 7;

    let _ = execute!(
        stdout(),
        Print(" *".green().bold()),
        Print(description),
        Print(" ".repeat(padding as usize)),
        Print(" [ ".white().bold()),
        Print(hash.green().bold()),
        Print(" ]\n".white().bold()),
    );
}

pub fn commit_created(hash: &str) {
    let (x, _) = size().expect("failed to get term size");

    let description = " Commited successfully ";
    let padding = x - hash.len() as u16 - description.len() as u16 - 7;

    let _ = execute!(
        stdout(),
        Print(" *".green().bold()),
        Print(description),
        Print(" ".repeat(padding as usize)),
        Print(" [ ".white().bold()),
        Print(hash.green().bold()),
        Print(" ]\n".white().bold()),
    );
}

pub fn ko_audit_commit(hash: &str) {
    let (x, _) = size().expect("failed to get term size");

    let description = " Signature is unvalid ";
    let padding = x - hash.len() as u16 - description.len() as u16 - 7;

    let _ = execute!(
        stdout(),
        Print(" !".red().bold()),
        Print(description),
        Print(" ".repeat(padding as usize)),
        Print(" [ ".white().bold()),
        Print(hash.red().bold()),
        Print(" ]\n".white().bold()),
    );
}

pub fn hooks(c: fn()) {
    if breathes::hooks::run_hooks().is_ok() {
        c();
    }
}
