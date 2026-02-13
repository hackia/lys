use crate::cli;
use crossterm::execute;
use crossterm::style::Stylize;
use std::io::{self, Write};

pub struct Shell;

impl Shell {
    pub fn new() -> Self {
        Shell
    }

    pub fn run(&self) -> Result<(), io::Error> {
        let stdin = io::stdin();
        let mut stdout = io::stdout();
        execute!(
            stdout,
            crossterm::terminal::Clear(crossterm::terminal::ClearType::All),
            crossterm::cursor::MoveTo(0, 0)
        )?;
        execute!(stdout, crossterm::terminal::EnterAlternateScreen)?;
        loop {
            // Affichage du prompt
            // lys en vert, > en blanc
            print!("{}", "lys".green());
            print!("{}", "> ".white());
            stdout.flush()?;

            let mut input = String::new();
            if stdin.read_line(&mut input)? == 0 {
                // EOF (Ctrl+D)
                println!();
                break;
            }

            let input = input.trim();
            if input.is_empty() {
                continue;
            }

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

            // Pour supporter les commandes de lys, on peut réutiliser le parser clap.
            // On divise l'entrée en arguments.
            let args = match shlex::split(input) {
                Some(args) => args,
                None => {
                    eprintln!("Invalid input");
                    continue;
                }
            };

            // On ajoute "lys" comme premier argument pour satisfaire clap
            let mut full_args = vec!["lys".to_string()];
            full_args.extend(args);

            // On tente de parser et d'exécuter
            let matches = cli().try_get_matches_from(full_args);

            match matches {
                Ok(matches) => {
                    // Ici on devrait exécuter la commande.
                    // Comme la logique d'exécution est dans main(), on va devoir la refactoriser
                    // ou appeler main avec ces matches.
                    // Pour l'instant, signalons qu'on a trouvé un match.
                    if let Err(e) = crate::execute_matches(matches) {
                        eprintln!("Error: {}", e);
                    }
                }
                Err(e) => {
                    println!("{}", e);
                }
            }
        }
        execute!(stdout, crossterm::terminal::LeaveAlternateScreen)?;
        Ok(())
    }
}
