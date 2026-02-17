use clap::{Arg, ArgAction, Command};
use plan::{EXT, Plan, wrap};
use std::fs::File;
use std::io::{BufReader, Error, ErrorKind};
use std::path::PathBuf;

fn cli() -> Command {
    Command::new(env!("CARGO_PKG_NAME"))
        .about(env!("CARGO_PKG_DESCRIPTION"))
        .version(env!("CARGO_PKG_VERSION"))
        .author(env!("CARGO_PKG_AUTHORS"))
        .subcommand(
            Command::new("new").about("Create a new plan").arg(
                Arg::new("name")
                    .required(true)
                    .index(1)
                    .action(ArgAction::Set),
            ),
        )
        .subcommand(
            Command::new("push")
                .about("Append a plan to a cube")
                .arg(
                    Arg::new("cube")
                        .required(true)
                        .index(1)
                        .action(ArgAction::Set),
                )
                .arg(
                    Arg::new("plan")
                        .required(true)
                        .index(2)
                        .action(ArgAction::Set),
                ),
        )
        .subcommand(
            Command::new("inspect").about("Inspect a plan").arg(
                Arg::new("plan")
                    .required(true)
                    .index(1)
                    .action(ArgAction::Set),
            ),
        )
        .subcommand(
            Command::new("audit").about("audit a plan").arg(
                Arg::new("plan")
                    .required(true)
                    .index(1)
                    .action(ArgAction::Set),
            ),
        )
        .subcommand(Command::new("tree").about("Show plan structure"))
        .subcommand(
            Command::new("run").about("Run the plan").arg(
                Arg::new("plan")
                    .required(true)
                    .index(1)
                    .action(ArgAction::Set),
            ),
        )
        .subcommand(
            Command::new("wrap")
                .about("the plan")
                .arg(
                    Arg::new("directory")
                        .action(ArgAction::Set)
                        .required(true)
                        .index(1),
                )
                .arg(
                    Arg::new("id")
                        .required(true)
                        .index(2)
                        .action(ArgAction::Set),
                ),
        )
        .subcommand(
            Command::new("import")
                .about("Import the plan")
                .arg(
                    Arg::new("archive")
                        .required(true)
                        .index(1)
                        .action(ArgAction::Set),
                )
                .arg(
                    Arg::new("id")
                        .required(true)
                        .index(2)
                        .action(ArgAction::Set),
                ),
        )
}

fn main() -> std::io::Result<()> {
    let matches = cli().get_matches();
    match matches.subcommand() {
        Some(("wrap", sub)) => {
            let id = sub
                .get_one::<String>("id")
                .expect("required id arg missing");
            let directory = sub
                .get_one::<String>("directory")
                .expect("required directory arg missing");
            let path = PathBuf::from(directory);

            if !path.exists() {
                return Err(Error::new(ErrorKind::NotFound, "directory not found"));
            }

            println!("Packaging...");

            // 2. Appel de ta logique métier (lib.rs)
            let plan = wrap(path, id);

            // 3. Sauvegarde sur le disque (Persistance)
            // On crée un fichier JSON pour l'instant (facile à lire)
            let filename = format!("{id}.{EXT}");
            let file = File::create(&filename)?;

            if serde_json::to_writer_pretty(file, &plan).is_err() {
                return Err(Error::other("Failed to write to file"));
            }
            Ok(())
        }
        Some(("inspect", sub)) => {
            let path_str = sub
                .get_one::<String>("plan")
                .expect("failed to get plan name&");
            let file = File::open(path_str).expect("Fichier introuvable");
            let reader = BufReader::new(file);

            // Désérialisation pour lire le plan
            let plan: Result<Plan, _> = serde_json::from_reader(reader);

            match plan {
                Ok(p) => {
                    println!("Inspection of the plan : {}", p.id());
                    println!("Layers : {}", p.layers().len());
                    if let Some(first_layer) = p.layers().first() {
                        println!("Layer files : {}", first_layer.changes.len());
                        return Ok(());
                    }
                    Err(Error::other("Layers is empty"))
                }
                Err(e) => Err(e.into()),
            }
        }
        Some(("audit", _sub)) => Ok(()),
        Some(("tree", _sub)) => Ok(()),
        Some(("import", _sub)) => Ok(()),
        _ => {
            cli().print_help().expect("Failed to print help");
            Ok(())
        }
    }
}
