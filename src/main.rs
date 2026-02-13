use crate::Language::{C, CSharp, Cpp, D, Haskell, Js, Php, Python, Rust, Typescript};
use crate::chat::list_messages;
use crate::chat::send_message;
use crate::commit::author;
use crate::db::LYS_INIT;
use crate::db::{connect_lys, get_current_branch};
use crate::import::extract_repo_name;
use crate::utils::ko;
use crate::utils::ok;
use crate::utils::ok_merkle_hash;
use crate::utils::run_hooks;
use clap::value_parser;
use clap::{Arg, ArgAction, Command};
use inquire::{Select, Text};
use sqlite::State;
use std::env::current_dir;
use std::fmt::Display;
use std::fs::File;
use std::fs::read_to_string;
use std::io::{Error, Write};
use std::path::MAIN_SEPARATOR_STR;
use std::path::Path;
use std::process::{Command as Cmd, Stdio};
use crate::shell::Shell;

pub mod chat;
pub mod commit;
pub mod crypto;
pub mod db;
pub mod import;
mod mount;
pub mod shell;
pub mod todo;
pub mod tree;
pub mod utils;
pub mod vcs;
pub mod web;

fn cli() -> Command {
    Command::new(env!("CARGO_PKG_NAME"))
        .about(env!("CARGO_PKG_DESCRIPTION"))
        .author("Saigo Ekitae <saigoekitae@gmail.com>")
        .version(env!("CARGO_PKG_VERSION"))
        .subcommand(Command::new("init").about("Initialize current directory"))
        .subcommand(Command::new("new").about("Create a new lys project"))
        .subcommand(
            Command::new("verify")
                .about("Check repository integrity and missing blobs")
                .arg(
                    Arg::new("deep")
                        .long("deep")
                        .action(ArgAction::SetTrue)
                        .help("Recalculate Blake3 checksums for every blob (Slower but safer)"),
                ),
        )
        .subcommand(Command::new("summary").about("Show working directory infos"))
        .subcommand(Command::new("status").about("Show changes in working directory"))
        .subcommand(Command::new("push").about("Push local commits to a remote architect"))
        .subcommand(Command::new("pull").about("Pull commits from a remote architect"))
        .subcommand(
            Command::new("prune").about(
                "Maintain repository health by removing old history and reclaiming disk space.",
            ),
        )
        .subcommand(
            Command::new("shell")
                .about("Open a temporary shell with the code mounted")
                .arg(
                    Arg::new("ref")
                        .help("Reference to mount (default: HEAD)")
                        .required(false)
                        .action(ArgAction::Set),
                ),
        )
        .subcommand(
            Command::new("mount")
                .about("Mount a specific version or the current head to a directory")
                .arg(
                    Arg::new("target")
                        .help("The mount point (e.g., /mnt/lys_project)")
                        .required(true)
                        .action(ArgAction::Set),
                )
                .arg(
                    Arg::new("ref")
                        .short('r')
                        .long("ref")
                        .help("Branch, tag or commit hash to mount (default: current HEAD)")
                        .action(ArgAction::Set),
                ),
        )
        .subcommand(
            Command::new("tree").about("Show repository").arg(
                Arg::new("color")
                    .help("colorize tree or not")
                    .required(false)
                    .default_value("false")
                    .value_parser(value_parser!(String)),
            ),
        )
        .subcommand(
            Command::new("import")
                .about("Import a Git repository into Lys")
                .arg(Arg::new("url").required(true).help("Git repository URL"))
                .arg(
                    Arg::new("depth")
                        .long("depth")
                        .value_parser(value_parser!(i32))
                        .help("Number of commits to import"),
                )
                .arg(
                    Arg::new("recent")
                        .long("recent")
                        .action(ArgAction::SetTrue)
                        .help("Only import the last 2 years of history (Lean mode)"),
                ),
        )
        .subcommand(
            Command::new("keygen").about("Generate Ed25519 identity keys for signing commits"),
        )
        .subcommand(
            Command::new("serve")
                .about("Start the Silex Node (Daemon) to receive atoms")
                .arg(Arg::new("port").short('p').default_value("3000")),
        )
        .subcommand(Command::new("audit").about("Verify integrity of commit signatures"))
        .subcommand(
            Command::new("log")
                .about("Show commit logs")
                .arg(
                    Arg::new("page")
                        .short('p')
                        .long("page")
                        .value_parser(value_parser!(usize))
                        .default_value("1")
                        .help("Page number (default: 1)"),
                )
                .arg(
                    Arg::new("limit")
                        .short('n')
                        .long("limit")
                        .value_parser(value_parser!(usize))
                        .default_value("120") // Ta demande spécifique
                        .help("Number of commits per page"),
                ),
        )
        .subcommand(Command::new("diff").about("Show changes between working tree and last commit"))
        .subcommand(
            Command::new("clone")
                .about("Clone a Git repository into a new lys repository")
                .arg(
                    Arg::new("url")
                        .required(true)
                        .help("The git URL (https://...)")
                        .action(ArgAction::Set),
                )
                // Optionnel : permettre de forcer un nom de dossier différent
                .arg(
                    Arg::new("name")
                        .required(false)
                        .help("Target directory name"),
                )
                .arg(
                    Arg::new("depth")
                        .long("depth")
                        .short('d')
                        .value_parser(value_parser!(i32))
                        .help("Truncate history to the specified number of commits"),
                ),
        )
        .subcommand(Command::new("health").about("Check the source code"))
        .subcommand(
            Command::new("todo")
                .about("Manage project tasks")
                .subcommand(
                    Command::new("add")
                        .arg(Arg::new("title").required(true))
                        .arg(Arg::new("user").short('u').help("Assign to user"))
                        .arg(
                            Arg::new("due")
                                .short('d')
                                .long("due")
                                .help("Due date (YYYY-MM-DD)"),
                        ),
                )
                .subcommand(
                    Command::new("start").arg(
                        Arg::new("id")
                            .required(true)
                            .value_parser(value_parser!(i64)),
                    ),
                )
                .subcommand(Command::new("list"))
                .subcommand(
                    Command::new("close").arg(
                        Arg::new("id")
                            .required(true)
                            .value_parser(value_parser!(i64)),
                    ),
                ),
        )
        .subcommand(Command::new("commit").about("Record changes to the repository"))
        .subcommand(
            Command::new("restore")
                .about("Discard changes in working directory")
                .arg(
                    Arg::new("path")
                        .help("The file to restore")
                        .required(true)
                        .action(ArgAction::Set),
                ),
        )
        .subcommand(
            Command::new("chat")
                .about("Chat with the team")
                .subcommand(
                    Command::new("send").arg(
                        Arg::new("message")
                            .required(true)
                            .action(ArgAction::Set)
                            .help("message to send"),
                    ),
                )
                .subcommand(Command::new("list").about("list messages")),
        )
        .subcommand(
            Command::new("sync")
                .about("Backup repository to a destination (USB, Drive...)")
                .arg(
                    Arg::new("path")
                        .required(true)
                        .action(ArgAction::Set)
                        .help("Destination path"),
                ),
        )
        .subcommand(
            Command::new("branch")
                .about("Create a new branch")
                .arg(Arg::new("name").required(true).action(ArgAction::Set)),
        )
        .subcommand(
            Command::new("checkout")
                .about("Switch branches or restore working tree files")
                .arg(Arg::new("name").required(true).action(ArgAction::Set)),
        )
        .subcommand(
            Command::new("feat")
                .about("Manage feature branches")
                .subcommand(
                    Command::new("start")
                        .about("Start a new feature")
                        .arg(Arg::new("name").required(true).action(ArgAction::Set)),
                )
                .subcommand(
                    Command::new("finish")
                        .about("Merge and close a feature")
                        .arg(Arg::new("name").required(true).action(ArgAction::Set)),
                ),
        )
        .subcommand(
            Command::new("hotfix")
                .about("Manage hotfix branches")
                .subcommand(
                    Command::new("start")
                        .about("Start a critical fix from main")
                        .arg(Arg::new("name").required(true).action(ArgAction::Set)),
                )
                .subcommand(
                    Command::new("finish")
                        .about("Apply fix to main and close")
                        .arg(Arg::new("name").required(true).action(ArgAction::Set)),
                ),
        )
        .subcommand(
            Command::new("tag")
                .about("Manage version tags")
                .subcommand(
                    Command::new("create")
                        .about("Create a new tag at HEAD")
                        .arg(Arg::new("name").required(true).action(ArgAction::Set))
                        .arg(
                            Arg::new("message")
                                .short('m')
                                .help("Description")
                                .action(ArgAction::Set),
                        ),
                )
                .subcommand(Command::new("list").about("List all tags")),
        )
        .subcommand(
            Command::new("web")
                .about("Start the web interface")
                .arg(
                    Arg::new("port")
                        .short('p')
                        .default_value("3000")
                        .action(ArgAction::Set),
                )
                .arg(
                    Arg::new("spotify")
                        .short('s')
                        .long("spotify")
                        .help("Music URL (Spotify or YouTube Music) to display on the home page")
                        .action(ArgAction::Set),
                )
                .arg(
                    Arg::new("video")
                        .short('v')
                        .long("video")
                        .help("YouTube Video URL to display as banner on the home page")
                        .action(ArgAction::Set),
                )
                .arg(
                    Arg::new("banner")
                        .short('b')
                        .long("banner")
                        .help("Image URL to display as banner on the home page")
                        .action(ArgAction::Set),
                )
                .arg(
                    Arg::new("title")
                        .long("title")
                        .help("Custom site title to display in header and browser tab")
                        .action(ArgAction::Set),
                )
                .arg(
                    Arg::new("subtitle")
                        .long("subtitle")
                        .help("Custom subtitle/description to display under the header title")
                        .action(ArgAction::Set),
                )
                .arg(
                    Arg::new("footer")
                        .long("footer")
                        .help("Custom footer HTML to display at the bottom of pages")
                        .action(ArgAction::Set),
                )
                .arg(
                    Arg::new("homepage")
                        .long("homepage")
                        .help("URL to the project's homepage")
                        .action(ArgAction::Set),
                )
                .arg(
                    Arg::new("documentation")
                        .long("documentation")
                        .help("URL to the project's documentation")
                        .action(ArgAction::Set),
                ),
        )
        .subcommand(
            Command::new("spotify")
                .about("Set the Music album/track to display on the home page")
                .arg(Arg::new("url").required(true).help("Music URL (Spotify or YouTube Music)")),
        )
        .subcommand(
            Command::new("video")
                .about("Set the YouTube video banner to display on the home page")
                .arg(Arg::new("url").required(true).help("YouTube Video URL")),
        )
        .subcommand(
            Command::new("banner")
                .about("Set the image banner to display on the home page")
                .arg(Arg::new("url").required(true).help("Image URL")),
        )
}

fn perform_commit() -> Result<(), Error> {
    let current_dir = current_dir()?;
    let current_dir_str = current_dir.to_str().unwrap();

    if !Path::new(".lys").exists() {
        return Err(Error::other("Not a lys repository."));
    }

    let connection =
        connect_lys(Path::new(current_dir_str)).map_err(|e| Error::other(e.to_string()))?;

    // On récupère le message depuis les arguments CLI
    let message = commit::Commit::new()
        .commit()
        .expect("commit fail")
        .to_string();

    vcs::commit(&connection, message.as_str(), author().as_str())
        .map_err(|e| Error::other(e.to_string()))?;

    Ok(())
}
pub fn check_status() -> Result<(), Error> {
    let current_dir = current_dir()?;
    let current_dir_str = current_dir.to_str().unwrap();
    if !Path::new(&format!("{MAIN_SEPARATOR_STR}.lys")).exists() && !Path::new(".lys").exists() {
        return Err(Error::other("Not a lys repository."));
    }

    let connection =
        connect_lys(Path::new(current_dir_str)).map_err(|e| Error::other(e.to_string()))?;
    vcs::status(
        &connection,
        current_dir_str,
        get_current_branch(&connection)
            .expect("failed to get current branch")
            .as_str(),
    )
    .map_err(|e| Error::other(e.to_string()))?;
    Ok(())
}

#[derive(Clone, Ord, Eq, PartialEq, PartialOrd, Debug)]
enum Language {
    Rust,
    Python,
    Haskell,
    CSharp,
    C,
    D,
    Cpp,
    Php,
    Js,
    Typescript,
}
impl Language {
    fn all() -> Vec<Language> {
        let mut x = vec![
            Rust, Python, Haskell, CSharp, C, D, Cpp, Php, Js, Typescript,
        ];
        x.sort_unstable();
        x
    }
    fn get_language_name(&self) -> &'static str {
        match self {
            Rust => "Rust",
            Python => "Python",
            Haskell => "Haskell",
            CSharp => "CSharp",
            C => "C",
            D => "D",
            Cpp => "Cpp",
            Php => "Php",
            Js => "JavaScript",
            Typescript => "TypeScript",
        }
    }
}
impl Display for Language {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.get_language_name())
    }
}
fn new_project() -> Result<(), Error> {
    let mut project = String::new();
    let supported_languages = Language::all();
    while project.is_empty() {
        project.clear();
        project = Text::new("Project name:")
            .prompt()
            .expect("failed to get name")
            .to_string();
        if Path::new(project.as_str()).is_dir() {
            ko("project already exist");
            project.clear();
        }
    }
    if connect_lys(Path::new(project.as_str()))
        .expect("failed to get the connexion")
        .execute(LYS_INIT)
        .is_ok()
    {
        File::create_new(format!("{project}{MAIN_SEPARATOR_STR}syl").as_str())
            .expect("failed to create file");
        ok("syl file created successfully");
        crypto::generate_keypair(Path::new(project.as_str())).expect("failed to generate keys");
        ok("project keys has been generated successfully");
        File::create_new(format!("{project}{MAIN_SEPARATOR_STR}README.md").as_str())
            .expect("failed to create readme file");
        ok("README.md created successfully");

        let main_language = Select::new("Select the main language:", supported_languages)
            .prompt()
            .expect("Failed to select language");
        match main_language {
            D => {
                Cmd::new("dub")
                    .arg("init")
                    .current_dir(project.as_str())
                    .spawn()
                    .expect("Failed to create dub project")
                    .wait()
                    .expect("Failed to wait for dub init");
                let mut syl = File::create(format!("{project}{MAIN_SEPARATOR_STR}syl").as_str())
                    .expect("failed to open syl file");
                syl.write_all(b"target\n").expect("Failed to ignore target");
                syl.write_all(b"breathes\n")
                    .expect("Failed to ignore target");
                syl.sync_all().expect("Failed to sync");
                ok("syl file updated successfully");
                ok("dub project created successfully");
            }
            Rust => {
                let p = Select::new("create a bin or a lib :", vec!["bin", "lib"])
                    .prompt()
                    .expect("Failed to select project type");
                ok(format!("creating a {} project", p.to_lowercase().replace(" ", "")).as_str());
                if p == "bin" {
                    Cmd::new("cargo")
                        .arg("init")
                        .arg("--vcs")
                        .arg("none")
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .current_dir(project.as_str())
                        .spawn()
                        .expect("failed to init cargo project");
                } else {
                    Cmd::new("cargo")
                        .arg("init")
                        .arg("--lib")
                        .arg("--vcs")
                        .arg("none")
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .current_dir(project.as_str())
                        .spawn()
                        .expect("Failed to init cargo project");
                }
                ok("Cargo project created successfully");
                let mut syl = File::create(format!("{project}{MAIN_SEPARATOR_STR}syl").as_str())
                    .expect("failed to open syl file");
                syl.write_all(b"target\n").expect("Failed to ignore target");
                syl.write_all(b"breathes\n")
                    .expect("Failed to ignore target");
                syl.sync_all().expect("Failed to sync");
                ok("syl file updated successfully");
            }
            Python => {
                Cmd::new("python3")
                    .arg("-m")
                    .arg("venv")
                    .arg(format!("{project}{MAIN_SEPARATOR_STR}.venv"))
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .current_dir(project.as_str())
                    .spawn()
                    .expect("Failed to create .venv")
                    .wait()
                    .expect("Failed to wait for venv creation");
                ok("venv created successfully");
                File::create(format!("{project}{MAIN_SEPARATOR_STR}main.py").as_str())
                    .expect("Failed to create main.py file");
                ok("main.py file created successfully");
                File::create(format!("{project}{MAIN_SEPARATOR_STR}requirements.txt").as_str())
                    .expect("Failed to create requirements file");
                ok("requirements.txt file created successfully");
                File::create(format!("{project}{MAIN_SEPARATOR_STR}README.md").as_str())
                    .expect("Failed to create README.md file");
                ok("README.md file created successfully");
                let mut syl = File::create(format!("{project}{MAIN_SEPARATOR_STR}syl").as_str())
                    .expect("failed to open syl file");
                syl.write_all(b"breathes\n")
                    .expect("Failed to ignore target");
                syl.write_all(b"__pycache__/\n")
                    .expect("Failed to ignore target");
                syl.write_all(b".venv\n").expect("Failed to ignore target");
                syl.write_all(b"*.pyc\n").expect("Failed to ignore target");
                syl.write_all(b"node_modules/\n")
                    .expect("Failed to ignore target");
                syl.sync_all().expect("Failed to sync");
                ok("syl file updated successfully");
            }
            Haskell => {
                Cmd::new("cabal")
                    .arg("init")
                    .current_dir(project.as_str())
                    .spawn()
                    .expect("Failed to create cabal project");
                ok("cabal project created successfully");
                File::create(format!("{project}{MAIN_SEPARATOR_STR}README.md").as_str())
                    .expect("Failed to create README.md file");
                ok("README.md file created successfully");
            }
            CSharp => {
                let x = Select::new(
                    "select the project type :",
                    vec!["console", "blazor", "blazor", "wpf", "classlib", "mstest"],
                )
                .prompt()
                .expect("Failed to select project type");
                Cmd::new("dotnet")
                    .arg("new")
                    .arg(x.to_lowercase().replace(" ", ""))
                    .arg("--output")
                    .arg(project.as_str())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                    .expect("Failed to create dotnet project");
                ok("dotnet project created successfully");
                File::create(format!("{project}{MAIN_SEPARATOR_STR}README.md").as_str())
                    .expect("Failed to create README.md file");
            }
            C | Cpp => {
                File::create(format!("{project}{MAIN_SEPARATOR_STR}README.md").as_str())
                    .expect("Failed to create README.md file");
                ok("README.md file created successfully");
                File::create(format!("{project}{MAIN_SEPARATOR_STR}CMakeLists.txt").as_str())
                    .expect("Failed to create CMakeLists.txt file");
                ok("CMakeLists.txt file created successfully");
                let mut syl = File::create(format!("{project}{MAIN_SEPARATOR_STR}syl").as_str())
                    .expect("failed to open syl file");
                syl.write_all(b"breathes\n")
                    .expect("Failed to ignore target");
                syl.write_all(b"build\n").expect("Failed to ignore target");
                syl.write_all(b"*.o\n").expect("Failed to ignore target");
                syl.write_all(b"*.a\n").expect("Failed to ignore target");
                syl.write_all(b"*.dll\n").expect("Failed to ignore target");
                syl.write_all(b"*.dll\n").expect("Failed to ignore target");
                syl.write_all(b".cmake/\n")
                    .expect("Failed to ignore target");
                syl.write_all(b"CMakeFiles/\n")
                    .expect("Failed to ignore target");
                syl.write_all(b"cmake_install.cmake\n")
                    .expect("Failed to ignore target");
                syl.write_all(b"CMakefiles\n")
                    .expect("Failed to ignore target");
                syl.write_all(b"Makefile\n")
                    .expect("Failed to ignore target");
                syl.write_all(b"*.so\n").expect("Failed to ignore target");
                syl.write_all(b"cmake-build-debug")
                    .expect("Failed to ignore target");
                syl.write_all(b"install_manifest.txt\n")
                    .expect("Failed to ignore target");
                syl.write_all(b"*_include.cmake\n")
                    .expect("Failed to ignore target");
                syl.write_all(b"*.pdb\n").expect("Failed to ignore target");
                syl.sync_all().expect("Failed to sync");
                ok("syl file updated successfully");
            }
            Php => {
                Cmd::new("composer")
                    .arg("init")
                    .current_dir(project.as_str())
                    .spawn()
                    .expect("Failed to create laravel project")
                    .wait()
                    .expect("Failed to wait for composer init");
                ok("composer.json created successfully");
                let mut syl = File::create(format!("{project}{MAIN_SEPARATOR_STR}syl").as_str())
                    .expect("failed to open syl file");
                syl.write_all(b"breathes\n")
                    .expect("Failed to ignore target");
                syl.write_all(b"vendor\n").expect("Failed to ignore target");
                syl.write_all(b"node_modules/\n")
                    .expect("Failed to ignore target");
                syl.sync_all().expect("Failed to sync");
                ok("syl file updated successfully");
            }
            Js => {
                Cmd::new("npm")
                    .arg("init")
                    .current_dir(project.as_str())
                    .spawn()
                    .expect("Failed to create npm project")
                    .wait()
                    .expect("Failed to wait for npm init");
                ok("package.json created successfully");
                let mut syl = File::create(format!("{project}{MAIN_SEPARATOR_STR}syl").as_str())
                    .expect("failed to open syl file");
                syl.write_all(b"breathes\n")
                    .expect("Failed to ignore target");
                syl.write_all(b"build\n").expect("Failed to ignore target");
                syl.write_all(b"node_modules/\n")
                    .expect("Failed to ignore target");
                syl.sync_all().expect("Failed to sync");
                ok("syl file updated successfully");
            }
            Typescript => {
                Cmd::new("npm")
                    .arg("init")
                    .current_dir(project.as_str())
                    .spawn()
                    .expect("Failed to create npm project")
                    .wait()
                    .expect("Failed to wait for npm init");
                ok("package.json created successfully");
                Cmd::new("tsc")
                    .arg("--init")
                    .current_dir(project.as_str())
                    .spawn()
                    .expect("Failed to init typescript");
                ok("tsconfig.json created successfully");
                let mut syl = File::create(format!("{project}{MAIN_SEPARATOR_STR}syl").as_str())
                    .expect("failed to open syl file");
                syl.write_all(b"breathes\n")
                    .expect("Failed to ignore target");
                syl.write_all(b"build\n").expect("Failed to ignore target");
                syl.write_all(b"node_modules/\n")
                    .expect("Failed to ignore target");
                syl.sync_all().expect("Failed to sync");
                ok("syl file updated successfully");
            }
        }
        ok("Project created successfully");
        Ok(())
    } else {
        Err(Error::other("Failed to create the sqlite database"))
    }
}

fn summary() -> Result<(), Error> {
    let root_path = current_dir().expect("Failed to get current directory");
    let conn = connect_lys(root_path.as_path()).expect("Failed to connect to database");
    let contributors = db::get_unique_contributors(&conn).expect("Failed to get contributors");

    for (contributor, count) in &contributors {
        ok(format!("{} ({} commits)", contributor, count).as_str());
    }
    Ok(())
}
pub fn execute_matches(app: clap::ArgMatches) -> Result<(), Error> {
    match app.subcommand() {
        Some(("new", _)) => new_project(),
        Some(("verify", args)) => {
            let deep = args.get_flag("deep"); // On récupère le flag
            let current_dir = current_dir()?;
            let conn = connect_lys(&current_dir).map_err(|e| Error::other(e.to_string()))?;
            db::verify(&conn, deep).map_err(|e| Error::other(e.to_string()))?;
            Ok(())
        }
        Some(("summary", _)) => summary(),
        Some(("prune", _)) => {
            let conn = connect_lys(Path::new(".")).expect("failed to connect to the database");
            let ans = inquire::Confirm::new("Are you sure you want to prune the repository?")
                .with_help_message("This action will PERMANENTLY delete all commits older than 2 years and reclaim disk space.")
                .with_default(false)
                .prompt();
            match ans {
                Ok(true) => {
                    // Lancement de la fonction de nettoyage que nous avons codée
                    db::prune(&conn).expect("failed to prune");
                }
                Ok(false) => println!("Prune operation cancelled."),
                Err(_) => println!("Error during confirmation. Operation aborted."),
            }
            Ok(())
        }
        Some(("serve", args)) => {
            let port: u16 = args
                .get_one::<String>("port")
                .unwrap()
                .parse()
                .unwrap_or(3000);
            let rt = tokio::runtime::Runtime::new()?;
            // On lance le serveur sur le répertoire actuel
            rt.block_on(web::start_server(".", port));
            Ok(())
        }
        Some(("import", sub_m)) => {
            let url = sub_m.get_one::<String>("url").unwrap();
            let depth = sub_m.get_one::<i32>("depth").copied();
            let only_recent = sub_m.get_flag("recent"); // Récupère le flag --recent
            let repo_name = extract_repo_name(url);
            let target_dir = current_dir()?.join(&repo_name);
            // On passe le nouveau paramètre à ta fonction
            import::import_from_git(url, &target_dir, depth, only_recent).expect("failed");
            ok("ready");
            Ok(())
        }
        Some(("mount", sub_args)) => {
            let target = sub_args.get_one::<String>("target").unwrap();
            let reference = sub_args.get_one::<String>("ref");
            let current_dir = current_dir()?;
            let conn = connect_lys(&current_dir).map_err(|e| Error::other(e.to_string()))?;
            vcs::mount_version(&conn, target, reference.map(|s| s.as_str()))
                .map_err(|e| Error::other(e.to_string()))
        }
        Some(("shell", sub_args)) => {
            let reference = sub_args.get_one::<String>("ref").map(|s| s.as_str());
            let current_dir = current_dir()?;
            let conn = connect_lys(&current_dir).map_err(|e| Error::other(e.to_string()))?;
            vcs::spawn_lys_shell(&conn, reference).map_err(|e| Error::other(e.to_string()))
        }
        Some(("init", _)) => {
            let current_dir = current_dir()?;
            let path_str = current_dir.to_str().unwrap();
            if connect_lys(Path::new(path_str))
                .expect("fail")
                .execute(LYS_INIT)
                .is_ok()
            {
                ok("Initialized empty lys repository");
                Ok(())
            } else {
                Err(Error::other("Failed to init"))
            }
        }
        Some(("clone", args)) => {
            let url = args.get_one::<String>("url").unwrap();

            // Récupération de la depth
            let depth = args.get_one::<i32>("depth").copied();

            // 1. Déterminer le nom du dossier (soit l'arg, soit déduit de l'URL)
            let dir_name = if let Some(name) = args.get_one::<String>("name") {
                name.clone()
            } else {
                extract_repo_name(url)
            };

            let target_path = current_dir()?.join(&dir_name);

            // 2. Vérifier si ça existe déjà pour ne pas écraser
            if target_path.exists() {
                ko("already exist");
                return Ok(());
            }

            // 3. Créer le dossier
            ok("Creation of the repository");
            std::fs::create_dir(&target_path)?;
            // Appel avec le nouveau paramètre
            import::import_from_git(url, &target_path, depth, false).expect("failed to clone");
            ok("ready");
            Ok(())
        }
        Some(("tree", _)) => {
            let current_dir = current_dir()?;
            let conn = connect_lys(&current_dir).map_err(|e| Error::other(e.to_string()))?;

            // 1. On récupère la branche actuelle
            let branch = get_current_branch(&conn).expect("failed to get branch");

            // 2. On récupère le tree_hash associé au HEAD de cette branche
            let query = "SELECT c.tree_hash FROM branches b JOIN commits c ON b.head_commit_id = c.id WHERE b.name = ?";
            let mut stmt = conn
                .prepare(query)
                .map_err(|e| Error::other(e.to_string()))?;
            stmt.bind((1, branch.as_str())).unwrap();

            if let Ok(State::Row) = stmt.next() {
                let root_hash: String = stmt.read(0).unwrap();
                ok_merkle_hash(root_hash.as_str());
                let lines =
                    vcs::ls_tree(&conn, &root_hash, "").map_err(|e| Error::other(e.to_string()))?;

                if let Some(mut child) = vcs::start_pager() {
                    if let Some(mut stdin) = child.stdin.take() {
                        let output = lines.join("\n");
                        let _ = stdin.write_all(output.as_bytes());
                        drop(stdin);
                        let _ = child.wait();
                    } else {
                        println!("{}", lines.join("\n"));
                    }
                } else {
                    println!("{}", lines.join("\n"));
                }
            } else {
                ok("repository empty. Commit something first!");
            }
            Ok(())
        }
        Some(("keygen", _)) => {
            let current_dir = current_dir()?;
            crypto::generate_keypair(&current_dir).expect("failed to create keys");
            ok("keys generated successfully");
            Ok(())
        }
        Some(("status", _)) => check_status(),
        Some(("chat", sub)) => {
            let sender = std::env::var("USER").expect("USER must be defined");
            let conn = connect_lys(Path::new(".")).expect("failed to connect to the database");
            match sub.subcommand() {
                Some(("send", arg)) => {
                    let message = arg
                        .get_one::<String>("message")
                        .expect("failed to get message");
                    send_message(&conn, sender.as_str(), message.as_str())
                        .expect("failed to send message");
                    Ok(())
                }
                Some(("list", _)) => match list_messages(&conn) {
                    Ok(messages) => {
                        if messages.is_empty() {
                            ok("chat messages is empty.");
                            Ok(())
                        } else {
                            for message in &messages {
                                println!(
                                    "{}",
                                    format_args!("\n{}\n{}\n", message.content, message.sender)
                                );
                            }
                            Ok(())
                        }
                    }
                    Err(_) => Err(Error::other("Failed to read messages")),
                },
                _ => Ok(()),
            }
        }
        Some(("audit", _)) => {
            let conn = connect_lys(Path::new(".")).expect("failed to connect to the databaase");
            if crypto::audit(&conn).expect("failed to connect to the database") {
                Ok(())
            } else {
                Err(Error::other("audit detect failure"))
            }
        }
        Some(("health", _)) => {
            if run_hooks().is_ok() {
                ok("code can be commited");
            } else {
                ko("code must not be commited");
            }
            Ok(())
        }
        Some(("commit", _)) => {
            if read_to_string("syl")
                .expect("missing syl file")
                .trim()
                .is_empty()
            {
                return Err(Error::other(
                    "syl content cannot be empty ignore content before commit.",
                ));
            }
            perform_commit()
        }
        Some(("log", args)) => {
            let page = *args.get_one::<usize>("page").unwrap();
            let limit = *args.get_one::<usize>("limit").unwrap();
            let conn = connect_lys(Path::new(".")).expect("failed to connect to the database");
            vcs::log(&conn, page, limit).expect("failed to parse log");
            Ok(())
        }
        Some(("diff", _)) => {
            let current_dir = current_dir()?;
            let conn =
                connect_lys(current_dir.as_path()).map_err(|e| Error::other(e.to_string()))?;
            vcs::diff(&conn).map_err(|e| Error::other(e.to_string()))
        }
        Some(("restore", sub_matches)) => {
            let current_dir = current_dir()?;
            let conn =
                connect_lys(current_dir.as_path()).map_err(|e| Error::other(e.to_string()))?;

            let path = sub_matches.get_one::<String>("path").unwrap();
            vcs::restore(&conn, path).map_err(|e| Error::other(e.to_string()))
        }
        Some(("branch", sub_matches)) => {
            let current_dir = current_dir()?;
            let conn =
                connect_lys(current_dir.as_path()).map_err(|e| Error::other(e.to_string()))?;
            let name = sub_matches.get_one::<String>("name").unwrap();
            vcs::create_branch(&conn, name).map_err(|e| Error::other(e.to_string()))
        }
        Some(("checkout", sub_matches)) => {
            let current_dir = current_dir()?;
            let conn =
                connect_lys(current_dir.as_path()).map_err(|e| Error::other(e.to_string()))?;
            let name = sub_matches.get_one::<String>("name").unwrap();
            vcs::checkout(&conn, name).map_err(|e| Error::other(e.to_string()))
        }
        Some(("feat", sub_matches)) => {
            let current_dir = current_dir()?;
            let conn =
                connect_lys(current_dir.as_path()).map_err(|e| Error::other(e.to_string()))?;

            match sub_matches.subcommand() {
                Some(("start", args)) => {
                    let name = args.get_one::<String>("name").unwrap();
                    vcs::feature_start(&conn, name).map_err(|e| Error::other(e.to_string()))
                }
                Some(("finish", args)) => {
                    let name = args.get_one::<String>("name").unwrap();
                    vcs::feature_finish(&conn, name).map_err(|e| Error::other(e.to_string()))
                }
                _ => {
                    ok("Please specify 'start' or 'finish'.");
                    Ok(())
                }
            }
        }
        Some(("hotfix", sub_matches)) => {
            let current_dir = current_dir()?;
            let conn =
                connect_lys(current_dir.as_path()).map_err(|e| Error::other(e.to_string()))?;

            match sub_matches.subcommand() {
                Some(("start", args)) => {
                    let name = args.get_one::<String>("name").unwrap();
                    vcs::hotfix_start(&conn, name).map_err(|e| Error::other(e.to_string()))
                }
                Some(("finish", args)) => {
                    let name = args.get_one::<String>("name").unwrap();
                    vcs::hotfix_finish(&conn, name).map_err(|e| Error::other(e.to_string()))
                }
                _ => {
                    ok("please specify 'start' or 'finish'");
                    Ok(())
                }
            }
        }
        Some(("tag", sub_matches)) => {
            let current_dir = current_dir()?;
            let conn =
                connect_lys(current_dir.as_path()).map_err(|e| Error::other(e.to_string()))?;

            match sub_matches.subcommand() {
                Some(("create", args)) => {
                    let name = args.get_one::<String>("name").unwrap();
                    let msg = args.get_one::<String>("message").map(|s| s.as_str());
                    vcs::tag_create(&conn, name, msg)
                }
                Some(("list", _)) => vcs::tag_list(&conn),
                _ => {
                    ok("Please use 'create' or 'list'.");
                    Ok(())
                }
            }
        }
        Some(("sync", args)) => {
            let current_dir = current_dir()?;
            let _conn =
                connect_lys(current_dir.as_path()).map_err(|e| Error::other(e.to_string()))?;
            let path = args.get_one::<String>("path").unwrap();
            vcs::sync(path)
        }
        Some(("web", args)) => {
            let current_dir = current_dir()?;
            let current_dir_str = current_dir.to_str().unwrap();
            if !Path::new(".lys").exists() {
                return Err(Error::other("Not a lys repository."));
            }

            let conn = connect_lys(&current_dir).map_err(|e| Error::other(e.to_string()))?;

            if let Some(spotify_url) = args.get_one::<String>("spotify") {
                let mut stmt = conn
                    .prepare("INSERT OR REPLACE INTO config (key, value) VALUES ('spotify_url', ?)")
                    .map_err(|e| Error::other(e.to_string()))?;
                stmt.bind((1, spotify_url.as_str()))
                    .map_err(|e| Error::other(e.to_string()))?;
                stmt.next().map_err(|e| Error::other(e.to_string()))?;
                ok("Music URL updated");
            }

            if let Some(video_url) = args.get_one::<String>("video") {
                let mut stmt = conn
                    .prepare("INSERT OR REPLACE INTO config (key, value) VALUES ('video_banner_url', ?)")
                    .map_err(|e| Error::other(e.to_string()))?;
                stmt.bind((1, video_url.as_str()))
                    .map_err(|e| Error::other(e.to_string()))?;
                stmt.next().map_err(|e| Error::other(e.to_string()))?;
                ok("Video banner URL updated");
            }

            if let Some(banner_url) = args.get_one::<String>("banner") {
                let mut stmt = conn
                    .prepare("INSERT OR REPLACE INTO config (key, value) VALUES ('banner_url', ?)")
                    .map_err(|e| Error::other(e.to_string()))?;
                stmt.bind((1, banner_url.as_str()))
                    .map_err(|e| Error::other(e.to_string()))?;
                stmt.next().map_err(|e| Error::other(e.to_string()))?;
                ok("Image banner URL updated");
            }

            if let Some(title) = args.get_one::<String>("title") {
                let mut stmt = conn
                    .prepare("INSERT OR REPLACE INTO config (key, value) VALUES ('web_title', ?)")
                    .map_err(|e| Error::other(e.to_string()))?;
                stmt.bind((1, title.as_str())).map_err(|e| Error::other(e.to_string()))?;
                stmt.next().map_err(|e| Error::other(e.to_string()))?;
                ok("Web title updated");
            }

            if let Some(subtitle) = args.get_one::<String>("subtitle") {
                let mut stmt = conn
                    .prepare("INSERT OR REPLACE INTO config (key, value) VALUES ('web_subtitle', ?)")
                    .map_err(|e| Error::other(e.to_string()))?;
                stmt.bind((1, subtitle.as_str())).map_err(|e| Error::other(e.to_string()))?;
                stmt.next().map_err(|e| Error::other(e.to_string()))?;
                ok("Web subtitle updated");
            }

            if let Some(footer) = args.get_one::<String>("footer") {
                let footer_content = if Path::new(footer).exists() && Path::new(footer).is_file() {
                    read_to_string(footer).unwrap_or_else(|_| footer.clone())
                } else {
                    footer.clone()
                };
                let mut stmt = conn
                    .prepare("INSERT OR REPLACE INTO config (key, value) VALUES ('web_footer', ?)")
                    .map_err(|e| Error::other(e.to_string()))?;
                stmt.bind((1, footer_content.as_str())).map_err(|e| Error::other(e.to_string()))?;
                stmt.next().map_err(|e| Error::other(e.to_string()))?;
                ok("Web footer updated");
            }

            if let Some(homepage) = args.get_one::<String>("homepage") {
                let mut stmt = conn
                    .prepare("INSERT OR REPLACE INTO config (key, value) VALUES ('web_homepage', ?)")
                    .map_err(|e| Error::other(e.to_string()))?;
                stmt.bind((1, homepage.as_str())).map_err(|e| Error::other(e.to_string()))?;
                stmt.next().map_err(|e| Error::other(e.to_string()))?;
                ok("Web homepage URL updated");
            }

            if let Some(documentation) = args.get_one::<String>("documentation") {
                let mut stmt = conn
                    .prepare("INSERT OR REPLACE INTO config (key, value) VALUES ('web_documentation', ?)")
                    .map_err(|e| Error::other(e.to_string()))?;
                stmt.bind((1, documentation.as_str())).map_err(|e| Error::other(e.to_string()))?;
                stmt.next().map_err(|e| Error::other(e.to_string()))?;
                ok("Web documentation URL updated");
            }

            let port: u16 = args
                .get_one::<String>("port")
                .unwrap()
                .parse()
                .unwrap_or(3000);
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(web::start_server(current_dir_str, port));
            Ok(())
        }
        Some(("spotify", args)) => {
            let url = args.get_one::<String>("url").unwrap();
            let current_dir = current_dir()?;
            let conn = connect_lys(&current_dir).map_err(|e| Error::other(e.to_string()))?;
            let mut stmt = conn
                .prepare("INSERT OR REPLACE INTO config (key, value) VALUES ('spotify_url', ?)")
                .map_err(|e| Error::other(e.to_string()))?;
            stmt.bind((1, url.as_str()))
                .map_err(|e| Error::other(e.to_string()))?;
            stmt.next().map_err(|e| Error::other(e.to_string()))?;
            ok("Music URL updated for web interface");
            Ok(())
        }
        Some(("video", args)) => {
            let url = args.get_one::<String>("url").unwrap();
            let current_dir = current_dir()?;
            let conn = connect_lys(&current_dir).map_err(|e| Error::other(e.to_string()))?;
            let mut stmt = conn
                .prepare("INSERT OR REPLACE INTO config (key, value) VALUES ('video_banner_url', ?)")
                .map_err(|e| Error::other(e.to_string()))?;
            stmt.bind((1, url.as_str()))
                .map_err(|e| Error::other(e.to_string()))?;
            stmt.next().map_err(|e| Error::other(e.to_string()))?;
            ok("Video banner URL updated for web interface");
            Ok(())
        }
        Some(("banner", args)) => {
            let url = args.get_one::<String>("url").unwrap();
            let current_dir = current_dir()?;
            let conn = connect_lys(&current_dir).map_err(|e| Error::other(e.to_string()))?;
            let mut stmt = conn
                .prepare("INSERT OR REPLACE INTO config (key, value) VALUES ('banner_url', ?)")
                .map_err(|e| Error::other(e.to_string()))?;
            stmt.bind((1, url.as_str()))
                .map_err(|e| Error::other(e.to_string()))?;
            stmt.next().map_err(|e| Error::other(e.to_string()))?;
            ok("Image banner URL updated for web interface");
            Ok(())
        }
        Some(("todo", sub)) => {
            let current_dir = current_dir()?;
            let conn =
                connect_lys(current_dir.as_path()).expect("failed to connect to the database");
            match sub.subcommand() {
                Some(("add", args)) => {
                    let title = args.get_one::<String>("title").unwrap();
                    let user = args.get_one::<String>("user").map(|s| s.as_str());
                    let due = args.get_one::<String>("due").map(|s| s.as_str());
                    todo::add_todo(&conn, title, user, due).expect("failed to add todo");
                    Ok(())
                }
                Some(("start", args)) => {
                    let id = args.get_one::<i64>("id").unwrap();
                    todo::start_todo(&conn, *id).expect("failed to start todo");
                    Ok(())
                }
                Some(("list", _)) => {
                    todo::list_todos(&conn).map_err(|e| Error::other(e.to_string()))
                }
                Some(("close", args)) => {
                    let id = args.get_one::<i64>("id").unwrap();
                    todo::complete_todo(&conn, *id).expect("failed to complete todo");
                    Ok(())
                }
                _ => Ok(()),
            }
        }
        _ => {
            Shell::new().run()
        }
    }
}

fn main() -> Result<(), Error> {
    let args = cli();
    let app = args.clone().try_get_matches();
    match app {
        Ok(matches) => execute_matches(matches),
        Err(e) => {
            if std::env::args().len() == 1 {
                Shell::new().run().map_err(|e| Error::other(e.to_string()))
            } else {
                e.exit();
            }
        }
    }
}
