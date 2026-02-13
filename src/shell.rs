use crate::cli;
use crossterm::execute;
use crossterm::style::Stylize;
use rustyline::error::ReadlineError;
use rustyline::DefaultEditor;
use std::io;
use std::path::PathBuf;

pub struct Shell;

impl Shell {
    pub fn new() -> Self {
        Shell
    }

    fn history_path() -> Option<PathBuf> {
        #[allow(deprecated)]
        std::env::home_dir().map(|mut p| {
            p.push(".lys-history");
            p
        })
    }

    pub fn run(&self) -> Result<(), io::Error> {
        let mut rl = DefaultEditor::new().map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        
        let history_path = Self::history_path();
        if let Some(ref path) = history_path {
            if path.exists() {
                let _ = rl.load_history(path);
            }
        }

        let mut stdout = io::stdout();
        execute!(
            stdout,
            crossterm::terminal::Clear(crossterm::terminal::ClearType::All),
            crossterm::cursor::MoveTo(0, 0)
        )?;
        
        loop {
            // Prompt avec lys en vert et > en blanc
            let prompt = format!("{}{}", "lys".green(), "> ".white());
            
            let readline = rl.readline(&prompt);
            match readline {
                Ok(line) => {
                    let input = line.trim();
                    if input.is_empty() {
                        continue;
                    }

                    let _ = rl.add_history_entry(input);

                    if input == "exit" || input == "quit" {
                        break;
                    } else if input == "clear" {
                        execute!(
                            stdout,
                            crossterm::terminal::Clear(crossterm::terminal::ClearType::All),
                            crossterm::cursor::MoveTo(0, 0)
                        )?;
                        continue;
                    }

                    // Pour le shell local, on exécute directement les matches
                    // pour garder l'interactivité (TTY, couleurs, pager).
                    if let Some(args) = shlex::split(input) {
                        let mut full_args = vec!["lys".to_string()];
                        full_args.extend(args);
                        let matches = cli().try_get_matches_from(full_args);
                        match matches {
                            Ok(m) => {
                                if let Err(e) = crate::execute_matches(m) {
                                    eprintln!("{}", format!("Error: {}", e).red());
                                }
                            }
                            Err(e) => {
                                println!("{}", e);
                            }
                        }
                    } else {
                        println!("{}", "Invalid input".red());
                    }
                },
                Err(ReadlineError::Interrupted) => {
                    // Ctrl-C
                    continue;
                },
                Err(ReadlineError::Eof) => {
                    // Ctrl-D
                    println!();
                    break;
                },
                Err(err) => {
                    println!("Error: {:?}", err);
                    break;
                }
            }
        }

        if let Some(ref path) = history_path {
            let _ = rl.save_history(path);
        }

        Ok(())
    }

    pub fn execute_command(&self, input: &str) -> String {
        use std::io::Read;
        use std::os::fd::AsRawFd;
        use std::fs::File;

        // On divise l'entrée en arguments.
        let args = match shlex::split(input) {
            Some(args) => args,
            None => {
                return "Invalid input".to_string();
            }
        };

        // On ajoute "lys" comme premier argument pour satisfaire clap
        let mut full_args = vec!["lys".to_string()];
        full_args.extend(args);

        // On tente de parser
        let matches = cli().try_get_matches_from(full_args);

        match matches {
            Ok(matches) => {
                let temp_file = match tempfile::NamedTempFile::new() {
                    Ok(f) => f,
                    Err(e) => return format!("Failed to create temp file: {}", e),
                };
                
                let stdout_fd = 1;
                let old_stdout = unsafe { nix::libc::dup(stdout_fd) };

                // Rediriger stdout vers le fichier temporaire
                unsafe {
                    nix::libc::dup2(temp_file.as_raw_fd(), stdout_fd);
                    std::env::set_var("LYS_WEB_TERMINAL", "1");
                }

                // Exécution de la commande
                let res = crate::execute_matches(matches);

                // Restaurer stdout
                unsafe {
                    std::env::remove_var("LYS_WEB_TERMINAL");
                    nix::libc::dup2(old_stdout, stdout_fd);
                    nix::libc::close(old_stdout);
                }

                // Lire le contenu du fichier temporaire
                let mut captured_output = String::new();
                if let Ok(mut f) = File::open(temp_file.path()) {
                    f.read_to_string(&mut captured_output).ok();
                }

                if let Err(e) = res {
                    format!("{}Error: {}", captured_output, e)
                } else {
                    captured_output
                }
            }
            Err(e) => {
                e.to_string()
            }
        }
    }

    pub fn complete_command(&self, input: &str) -> Vec<String> {
        let app = cli();
        let args = shlex::split(input).unwrap_or_default();
        
        // Si l'entrée est vide ou se termine par un espace, on cherche les sous-commandes possibles
        // Sinon on cherche les sous-commandes qui commencent par le dernier mot.
        let last_word = if input.ends_with(' ') || input.is_empty() {
            ""
        } else {
            args.last().map(|s| s.as_str()).unwrap_or("")
        };

        let mut suggestions = Vec::new();

        // Pour l'instant, on se concentre sur les noms de sous-commandes du premier niveau
        for cmd in app.get_subcommands() {
            let name = cmd.get_name();
            if name.starts_with(last_word) {
                suggestions.push(name.to_string());
            }
        }

        suggestions.sort();
        suggestions
    }
}
