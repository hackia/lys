use crate::chat::list_messages;
use crate::chat::send_message;
use crate::commit::author;
use crate::db::LYS_INIT;
use crate::db::{connect_lys, get_current_branch};
use crate::import::extract_repo_name;
use crate::utils::ko;
use crate::utils::ok;
use crate::utils::ok_merkle_hash;
use breathes::hooks::run_hooks;
use clap::value_parser;
use clap::{Arg, ArgAction, Command};
use inquire::Text;
use sqlite::State;
use std::env::current_dir;
use std::fs::File;
use std::fs::read_to_string;
use std::io::Error;
use std::path::MAIN_SEPARATOR_STR;
use std::path::Path;

pub mod chat;
pub mod commit;
pub mod crypto;
pub mod db;
pub mod import;
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
        .subcommand(Command::new("doctor").about("Check system health and permissions for lys"))
        .subcommand(Command::new("init").about("Initialize current directory"))
        .subcommand(Command::new("new").about("Create a new lys project"))
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
                    .value_parser(clap::value_parser!(String)),
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
                        .value_parser(clap::value_parser!(usize))
                        .default_value("1")
                        .help("Page number (default: 1)"),
                )
                .arg(
                    Arg::new("limit")
                        .short('n')
                        .long("limit")
                        .value_parser(clap::value_parser!(usize))
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
                        .value_parser(clap::value_parser!(i32))
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
                .subcommand(Command::new("list"))
                .subcommand(
                    Command::new("close").arg(
                        Arg::new("id")
                            .required(true)
                            .value_parser(clap::value_parser!(i64)),
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
            Command::new("web").about("Start the web interface").arg(
                Arg::new("port")
                    .short('p')
                    .default_value("3000")
                    .action(ArgAction::Set),
            ),
        )
}

fn perform_commit() -> Result<(), Error> {
    let current_dir = std::env::current_dir()?;
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
    let current_dir = std::env::current_dir()?;
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

fn new_project() -> Result<(), Error> {
    let mut project = String::new();
    while project.is_empty() {
        project.clear();
        project = Text::new("Name:")
            .prompt()
            .expect("failed to get name")
            .to_string();
        if (Path::new(project.as_str())).is_dir() {
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
        ok("project keys has been generated successsfully");
        ok("project created successsfully");
        Ok(())
    } else {
        Err(Error::other("failed to create the sqlite database"))
    }
}

fn summary() -> Result<(), Error> {
    let root_path = current_dir().expect("fail");
    let conn = connect_lys(root_path.as_path()).expect("failed");
    let contributors = db::get_unique_contributors(&conn).expect("aa");

    for contributor in &contributors {
        ok(contributor.as_str());
    }
    Ok(())
}
fn main() -> Result<(), Error> {
    let args = cli();
    let app = args.clone().get_matches();
    match app.subcommand() {
        Some(("new", _)) => new_project(),
        Some(("summary", _)) => summary(),
        Some(("prune", _)) => {
            let conn = db::connect_lys(Path::new(".")).expect("faield to connect to the database");
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
            rt.block_on(crate::web::start_server(".", port));
            Ok(())
        }
        Some(("import", sub_m)) => {
            let url = sub_m.get_one::<String>("url").unwrap();
            let depth = sub_m.get_one::<i32>("depth").copied();
            let only_recent = sub_m.get_flag("recent"); // Récupère le flag --recent
            let repo_name = import::extract_repo_name(url);
            let target_dir = std::env::current_dir()?.join(&repo_name);
            // On passe le nouveau paramètre à ta fonction
            import::import_from_git(url, &target_dir, depth, only_recent).expect("failed");
            ok("ready");
            Ok(())
        }
        Some(("doctor", _)) => {
            vcs::doctor().expect("system health degraded");
            Ok(())
        }
        Some(("mount", sub_args)) => {
            let target = sub_args.get_one::<String>("target").unwrap();
            let reference = sub_args.get_one::<String>("ref");
            let current_dir = std::env::current_dir()?;
            let conn = connect_lys(&current_dir).map_err(|e| Error::other(e.to_string()))?;
            vcs::mount_version(&conn, target, reference.map(|s| s.as_str()))
                .map_err(|e| Error::other(e.to_string()))
        }
        Some(("shell", sub_args)) => {
            let reference = sub_args.get_one::<String>("ref").map(|s| s.as_str());
            let current_dir = std::env::current_dir()?;
            let conn = connect_lys(&current_dir).map_err(|e| Error::other(e.to_string()))?;
            vcs::spawn_lys_shell(&conn, reference).map_err(|e| Error::other(e.to_string()))
        }
        Some(("init", _)) => {
            let current_dir = std::env::current_dir()?;
            let path_str = current_dir.to_str().unwrap();
            // Logique d'initialisation directe ici (copiée de new_project sans le prompt)
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

            let target_path = std::env::current_dir()?.join(&dir_name);

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
            let current_dir = std::env::current_dir()?;
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
                vcs::ls_tree(&conn, &root_hash, "").map_err(|e| Error::other(e.to_string()))?;
            } else {
                ok("repository empty. Commit something first!");
            }
            Ok(())
        }
        Some(("keygen", _)) => {
            let current_dir = std::env::current_dir()?;
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
            // On appelle la nouvelle signature
            vcs::log(&conn, page, limit).expect("failed to parse log");
            Ok(())
        }
        Some(("diff", _)) => {
            let current_dir = std::env::current_dir()?;
            let conn =
                connect_lys(current_dir.as_path()).map_err(|e| Error::other(e.to_string()))?;
            vcs::diff(&conn).map_err(|e| Error::other(e.to_string()))
        }
        Some(("restore", sub_matches)) => {
            let current_dir = std::env::current_dir()?;
            let conn =
                connect_lys(current_dir.as_path()).map_err(|e| Error::other(e.to_string()))?;

            let path = sub_matches.get_one::<String>("path").unwrap();
            vcs::restore(&conn, path).map_err(|e| Error::other(e.to_string()))
        }
        Some(("branch", sub_matches)) => {
            let current_dir = std::env::current_dir()?;
            let conn =
                connect_lys(current_dir.as_path()).map_err(|e| Error::other(e.to_string()))?;
            let name = sub_matches.get_one::<String>("name").unwrap();
            vcs::create_branch(&conn, name).map_err(|e| Error::other(e.to_string()))
        }
        Some(("checkout", sub_matches)) => {
            let current_dir = std::env::current_dir()?;
            let conn =
                connect_lys(current_dir.as_path()).map_err(|e| Error::other(e.to_string()))?;
            let name = sub_matches.get_one::<String>("name").unwrap();
            vcs::checkout(&conn, name).map_err(|e| Error::other(e.to_string()))
        }
        Some(("feat", sub_matches)) => {
            let current_dir = std::env::current_dir()?;
            let conn =
                connect_lys(current_dir.as_path()).map_err(|e| Error::other(e.to_string()))?;

            // On regarde la SOUS-commande (start ou finish)
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
            let current_dir = std::env::current_dir()?;
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
            let current_dir = std::env::current_dir()?;
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
            let current_dir = std::env::current_dir()?;
            let _conn =
                connect_lys(current_dir.as_path()).map_err(|e| Error::other(e.to_string()))?;
            let path = args.get_one::<String>("path").unwrap();
            vcs::sync(path)
        }
        Some(("web", args)) => {
            let current_dir = std::env::current_dir()?;
            let current_dir_str = current_dir.to_str().unwrap();
            if !Path::new(".silex").exists() {
                return Err(Error::other("Not a silex repository."));
            }

            let port: u16 = args
                .get_one::<String>("port")
                .unwrap()
                .parse()
                .unwrap_or(3000);
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(crate::web::start_server(current_dir_str, port));
            Ok(())
        }
        Some(("todo", sub)) => {
            let current_dir = std::env::current_dir()?;
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
            args.clone().print_help().expect("failed to print the help");
            Ok(())
        }
    }
}
