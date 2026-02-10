use chrono::{DateTime, Local};
use content_inspector::{ContentType, inspect};
use crossterm::style::Stylize;
use ignore::{DirEntry, WalkBuilder};
use std::collections::HashMap;
use std::fs::Metadata;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

struct TreeNode {
    path: PathBuf,
    entry: Option<DirEntry>,
    children: HashMap<String, TreeNode>,
}

impl TreeNode {
    fn new(path: PathBuf, entry: Option<DirEntry>) -> Self {
        Self {
            path,
            entry,
            children: HashMap::new(),
        }
    }
    fn add_child(&mut self, components: &[&std::ffi::OsStr], entry: DirEntry) {
        if let Some((first, rest)) = components.split_first() {
            let key = first.to_string_lossy().to_string();
            let node = self
                .children
                .entry(key)
                .or_insert_with(|| TreeNode::new(PathBuf::from(first), None));

            if rest.is_empty() {
                node.entry = Some(entry);
            } else {
                node.add_child(rest, entry);
            }
        }
    }
}

pub fn scan_and_print_tree(root_path: &Path, max_level: Option<u32>, color: Option<bool>) {
    println!();
    let walker = WalkBuilder::new(root_path)
        .hidden(false)
        .add_custom_ignore_filename("syl")
        .standard_filters(true)
        .threads(4)
        .build();

    let mut root = TreeNode::new(root_path.to_path_buf(), None);
    let mut file_count = 0;
    let mut dir_count = 0;

    for result in walker {
        match result {
            Ok(entry) => {
                let path = entry.path();
                if let Ok(relative) = path.strip_prefix(root_path) {
                    let components: Vec<_> = relative.iter().collect();
                    if !components.is_empty() {
                        if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                            dir_count += 1;
                        } else {
                            file_count += 1;
                        }
                        root.add_child(&components, entry.clone());
                    }
                }
            }
            Err(err) => eprintln!("Erreur scan: {err}"),
        }
    }
    print_node(&root, "", true, 0, max_level, color);

    println!("\nSummary: {dir_count} directories, {file_count} files\n");
}

fn print_node(
    node: &TreeNode,
    prefix: &str,
    is_last: bool,
    current_level: u32,
    max_level: Option<u32>,
    color: Option<bool>,
) {
    if let Some(limit) = max_level
        && current_level > limit
    {
        return;
    }

    if let Some(entry) = &node.entry {
        let metadata = entry.metadata().ok();
        let (mode_with_type, size, m_date, c_date) =
            extract_metadata(metadata.as_ref(), color, entry);

        // Détermination du type de contenu
        let content_type = get_content_category(entry, color);

        let connector = if is_last { "└──" } else { "├──" };
        let file_name = entry.file_name().to_string_lossy();

        let display_name = if entry.path().is_dir() {
            if color.is_some() && color.expect("a") {
                file_name.blue().to_string()
            } else {
                file_name.to_string()
            }
        } else if file_name == "syl" || file_name == ".lys" {
            if color.is_some() && color.expect("a") {
                file_name.yellow().to_string()
            } else {
                file_name.to_string()
            }
        } else if color.is_some() && color.expect("a") {
            file_name.green().to_string()
        } else {
            file_name.to_string()
        };

        if color.is_some() && color.expect("") {
            println!(
                "{content_type:<25} {mode_with_type:<50} {c_date:<25} {m_date:<25} {size:>25} {prefix}{connector} {display_name}"
            );
        } else {
            println!(
                "{content_type:<8} {mode_with_type:<10} {c_date:<15} {m_date:<15} {size:>10} {prefix}{connector} {display_name}"
            );
        }
    }

    let mut children: Vec<_> = node.children.values().collect();
    children.sort_by(|a, b| {
        let a_is_dir = a
            .entry
            .as_ref()
            .map(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
            .unwrap_or_else(|| !a.children.is_empty());
        let b_is_dir = b
            .entry
            .as_ref()
            .map(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
            .unwrap_or_else(|| !b.children.is_empty());
        if a_is_dir == b_is_dir {
            a.path.cmp(&b.path)
        } else {
            b_is_dir.cmp(&a_is_dir)
        }
    });

    for (i, child) in children.iter().enumerate() {
        let is_last_child = i == children.len() - 1;
        let child_prefix = if node.entry.is_none() {
            "".to_string()
        } else if is_last {
            format!("{}    ", prefix)
        } else {
            format!("{}│   ", prefix)
        };

        print_node(
            child,
            &child_prefix,
            is_last_child,
            current_level + 1,
            max_level,
            color,
        );
    }
}

fn get_content_category(entry: &DirEntry, color: Option<bool>) -> String {
    if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
        if color.is_some() && color.expect("a") {
            return format!("{} {} {}", "[".white(), "DIR".green(), "]".white());
        }
        return format!("{} {} {}", "[", "DIR", "]");
    }
    let path = entry.path();
    let file_name = entry.file_name().to_string_lossy();

    // 1. Lecture rapide du début du fichier pour savoir si c'est du texte
    let file = std::fs::File::open(path).ok();
    let mut buffer = [0u8; 1024];
    let is_text = if let Some(mut f) = file {
        use std::io::Read;
        let n = f.read(&mut buffer).unwrap_or(0);
        inspect(&buffer[..n]) != ContentType::BINARY
    } else {
        true
    };

    // 2. Logique de classification
    let tag = if !is_text {
        "BIN" // Binaire
    } else {
        let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
        match ext.to_lowercase().as_str() {
            "rs" | "py" | "c" | "cpp" | "go" | "js" => "SRC",
            "toml" | "json" | "yaml" | "yml" => "CFG",
            "md" | "txt" => "DOC",
            "log" => "LOG",
            "sh" | "fish" | "bash" => "SHL",
            _ => {
                if file_name.to_lowercase().contains("license") {
                    "LIC"
                } else if file_name.to_lowercase().contains("lock") {
                    "LCK"
                } else if file_name.to_lowercase().eq("syl") {
                    "IGN"
                } else {
                    "TXT"
                }
            }
        }
    };
    if color.is_some() && color.expect("") {
        format!("{} {} {}", "[".white(), tag.green(), "]".white())
    } else {
        format!("{} {} {}", "[", tag, "]")
    }
}

fn extract_metadata(
    meta: Option<&Metadata>,
    color: Option<bool>,
    entry: &DirEntry,
) -> (String, String, String, String) {
    let type_char = if let Some(ft) = entry.file_type() {
        if ft.is_dir() {
            "d"
        } else if ft.is_symlink() {
            "l"
        } else {
            "f"
        }
    } else {
        "?"
    };

    match meta {
        Some(m) => {
            if color.is_some() && color.expect("failed to get color option") {
                let mode_str = format!(
                    "{} {}",
                    type_char.cyan(),
                    format_permissions(m.permissions().mode(), type_char, color)
                );
                let size = if m.is_dir() {
                    "-".to_string()
                } else {
                    human_bytes(m.len())
                };

                let m_date: DateTime<Local> =
                    m.modified().unwrap_or(std::time::SystemTime::now()).into();
                let c_date: DateTime<Local> = m
                    .created()
                    .unwrap_or(m.modified().unwrap_or(std::time::SystemTime::now()))
                    .into();

                (
                    mode_str.to_string().green().to_string(),
                    size.to_string().yellow().to_string(),
                    m_date
                        .format("%Y-%m-%d %H:%M")
                        .to_string()
                        .cyan()
                        .to_string(),
                    c_date
                        .format("%Y-%m-%d %H:%M")
                        .to_string()
                        .blue()
                        .to_string(),
                )
            } else {
                let mode_str = format!(
                    "{} {}",
                    type_char,
                    format_permissions(m.permissions().mode(), type_char, color)
                );
                let size = if m.is_dir() {
                    "-".to_string()
                } else {
                    human_bytes(m.len())
                };

                let m_date: DateTime<Local> =
                    m.modified().unwrap_or(std::time::SystemTime::now()).into();
                let c_date: DateTime<Local> = m
                    .created()
                    .unwrap_or(m.modified().unwrap_or(std::time::SystemTime::now()))
                    .into();

                (
                    mode_str.to_string(),
                    size.to_string().to_string(),
                    m_date.format("%Y-%m-%d %H:%M").to_string(),
                    c_date.format("%Y-%m-%d %H:%M").to_string(),
                )
            }
        }
        None => (
            format!("{} ????", type_char),
            "?".to_string(),
            "?".to_string(),
            "?".to_string(),
        ),
    }
}

fn format_permissions(mode: u32, type_char: &str, color: Option<bool>) -> String {
    let user = (mode >> 6) & 0o7;
    let group = (mode >> 3) & 0o7;
    let other = mode & 0o7;
    format!(
        "{} {} {}",
        fmt_triplet(user, type_char, color),
        fmt_triplet(group, type_char, color),
        fmt_triplet(other, type_char, color)
    )
}

fn fmt_triplet(val: u32, type_char: &str, color: Option<bool>) -> String {
    if color.is_some() && color.expect("a") {
        let cyan = |x: &str| -> String { format!("{}", x.cyan()) };
        let blue = |x: &str| -> String { format!("{}", x.blue()) };
        let green = |x: &str| -> String { format!("{}", x.green()) };
        let grey = |x: &str| -> String { format!("{}", x.grey()) };

        let r = |x: bool| -> String { if x { cyan("r") } else { grey("-") } };
        let w = |x: bool| -> String { if x { blue("w") } else { grey("-") } };
        let x = |x: bool| -> String { if x { green("x") } else { grey("-") } };
        let xx = |x: bool| -> String { if x { grey("x") } else { grey("-") } };

        let rwx = |a: bool, b: bool, c: bool, t: &str| -> (String, String, String) {
            if t.eq("f") {
                (r(a), w(b), x(c))
            } else {
                (r(a), w(b), xx(c))
            }
        };
        let (r, w, x) = rwx(val & 4 != 0, val & 2 != 0, val & 1 != 0, type_char);
        format!("{r}{w}{x}")
    } else {
        let cyan = |x: &str| -> String { x.to_string() };
        let blue = |x: &str| -> String { x.to_string() };
        let green = |x: &str| -> String { x.to_string() };
        let grey = |x: &str| -> String { x.to_string() };

        let r = |x: bool| -> String { if x { cyan("r") } else { grey("-") } };
        let w = |x: bool| -> String { if x { blue("w") } else { grey("-") } };
        let x = |x: bool| -> String { if x { green("x") } else { grey("-") } };
        let xx = |x: bool| -> String { if x { grey("x") } else { grey("-") } };

        let rwx = |a: bool, b: bool, c: bool, t: &str| -> (String, String, String) {
            if t.eq("f") {
                (r(a), w(b), x(c))
            } else {
                (r(a), w(b), xx(c))
            }
        };
        let (r, w, x) = rwx(val & 4 != 0, val & 2 != 0, val & 1 != 0, type_char);
        format!("{r}{w}{x}")
    }
}
fn human_bytes(size: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB"];
    let mut s = size as f64;
    let mut unit_idx = 0;
    while s >= 1024.0 && unit_idx < UNITS.len() - 1 {
        s /= 1024.0;
        unit_idx += 1;
    }
    format!("{:.1} {}", s, UNITS[unit_idx])
}
