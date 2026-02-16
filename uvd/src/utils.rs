use std::fs::read_to_string;
use std::io::stdout;
use std::path::Path;
use std::process::Command;

use crossterm::{
    execute,
    style::{Print, Stylize},
    terminal::size,
};

use crate::vcs::FileStatus;

pub fn ok(description: &str) {
    let x = term_width();

    // 1. Calcul de la largeur réelle des caractères UTF-8
    let desc_width = description.chars().count();

    // 2. Définition des symboles et labels
    let icon = " * "; // Symbole UTF-8 (Checkmark)
    let status_label = "ok";
    let brackets = (" [ ", " ] "); // Délimiteurs UTF-8 élégants

    // 3. Calcul du padding sécurisé
    // On retire la largeur de l'icone (3), du label (2), des brackets (6) et des espaces

    let occupied_width = (desc_width + 10) as u16;
    let padding = x.saturating_sub(occupied_width);
    let _ = execute!(
        stdout(),
        // Icône en vert brillant
        Print(icon.green().bold()),
        Print(description),
        // Remplissage dynamique
        Print(" ".repeat(padding as usize)),
        // Bloc de statut avec délimiteurs UTF-8
        Print(brackets.0.white().bold()),
        Print(status_label.green().bold()),
        Print(brackets.1.trim_end().white().bold()),
        Print("\n"),
    );
}

pub fn ok_merkle_hash(h: &str) {
    let x = term_width();

    let padding = x.saturating_sub(h.chars().count() as u16);
    let _ = execute!(
        stdout(),
        Print("m"),
        Print(" ".repeat(padding as usize)),
        Print(" [ "),
        Print(h),
        Print(" ]\n")
    );
}

pub fn ko(description: &str) {
    let x = term_width();
    // 1. Calcul de la largeur réelle des caractères UTF-8
    let desc_width = description.chars().count();

    // 3. Calcul du padding sécurisé
    // On retire la largeur de l'icone (3), du label (2), des brackets (6) et des espaces

    let occupied_width = (desc_width + 10) as u16;
    let padding = x.saturating_sub(occupied_width);
    let _ = execute!(
        stdout(),
        Print(" ! ".red().bold()),
        Print(description),
        Print(" ".repeat(padding as usize)),
        Print(" [ ".white().bold()),
        Print("ko".red().bold()),
        Print(" ]\n".trim_end().white().bold()),
        Print("\n"),
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
    let x = term_width();

    let padding = x.saturating_sub(
        tag.chars().count() as u16
            + description.chars().count() as u16
            + date.chars().count() as u16
            + 8,
    );
    let _ = execute!(
        stdout(),
        Print(" * ".green().bold()),
        Print(date.blue().bold()),
        Print(" "),
        Print(description.cyan().bold()),
        Print(" ".repeat(padding as usize)),
        Print(" [ ".white().bold()),
        Print(tag.green().bold()),
        Print(" ]\n".trim_end().white().bold()),
        Print("\n"),
    );
}

pub fn ok_audit_commit(hash: &str) {
    let x = term_width();

    let description = " Signature verified ";
    let padding =
        x.saturating_sub(hash.chars().count() as u16 + description.chars().count() as u16 + 7);

    let _ = execute!(
        stdout(),
        Print(" *".green().bold()),
        Print(description),
        Print(" ".repeat(padding as usize)),
        Print(" [ ".white().bold()),
        Print(hash.green().bold()),
        Print(" ]\n".trim_end().white().bold()),
        Print("\n"),
    );
}

pub fn commit_created(hash: &str) {
    let x = term_width();

    let description = " Committed successfully ";
    let padding =
        x.saturating_sub(hash.chars().count() as u16 + description.chars().count() as u16 + 7);

    let _ = execute!(
        stdout(),
        Print(" *".green().bold()),
        Print(description),
        Print(" ".repeat(padding as usize)),
        Print(" [ ".white().bold()),
        Print(hash.green().bold()),
        Print(" ]\n".trim_end().white().bold()),
        Print("\n"),
    );
}

pub fn ko_audit_commit(hash: &str) {
    let x = term_width();

    let description = " Signature verification failed ";
    let padding =
        x.saturating_sub(hash.chars().count() as u16 + description.chars().count() as u16 + 6);

    let _ = execute!(
        stdout(),
        Print(" !".red().bold()),
        Print(description),
        Print(" ".repeat(padding as usize)),
        Print(" [ ".white().bold()),
        Print(hash.red().bold()),
        Print(" ]\n".trim_end().white().bold()),
        Print("\n"),
    );
}

pub fn run_hooks() -> Result<(), Box<dyn std::error::Error>> {
    let lys_file = Path::new("lys");
    if !lys_file.exists() {
        return Ok(());
    }

    let content = read_to_string(lys_file)?;
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        ok(&format!("Running hook: {}", line));

        let status = if cfg!(target_os = "windows") {
            Command::new("cmd").args(["/C", line]).status()?
        } else {
            Command::new("sh").args(["-c", line]).status()?
        };

        if !status.success() {
            return Err(format!("Hook failed: {}", line).into());
        }
    }

    Ok(())
}

fn term_width() -> u16 {
    size().map(|(w, _)| w).unwrap_or(80)
}
