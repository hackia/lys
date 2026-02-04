use git2::build::RepoBuilder;
use git2::{FetchOptions, ObjectType, Oid, RemoteCallbacks, Repository, Tree};
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use std::collections::HashSet;
use std::path::Path;

use crate::db;
use crate::utils::ok;
use crate::vcs;

pub fn extract_repo_name(url: &str) -> String {
    let last_part = url.rsplit('/').next().unwrap_or("lys_repo");
    last_part
        .strip_suffix(".git")
        .unwrap_or(last_part)
        .to_string()
}

pub fn import_from_git(
    git_url: &str,
    target_dir: &Path,
    depth: Option<i32>,
) -> Result<(), Box<dyn std::error::Error>> {
    let m = MultiProgress::new();

    // --- BARRE 1 : CLONAGE GIT ---
    let pb_git = m.add(ProgressBar::new(0));
    pb_git.set_style(
        ProgressStyle::default_bar()
            .template(
                "{spinner:.white} [1/2] Git Fetch:    [{bar:40.white}] {pos}/{len} objects ({msg})",
            )?
            .progress_chars("=>-"),
    );

    let mut callbacks = RemoteCallbacks::new();
    callbacks.transfer_progress(|stats| {
        pb_git.set_length(stats.total_objects() as u64);
        pb_git.set_position(stats.received_objects() as u64);
        pb_git.set_message("Downloading...");
        true
    });

    let mut fetch_options = FetchOptions::new();
    fetch_options.remote_callbacks(callbacks);
    if let Some(d) = depth {
        fetch_options.depth(d);
    }

    let temp_path = target_dir.join("temp_git_import");
    if temp_path.exists() {
        std::fs::remove_dir_all(&temp_path)?;
    }

    let repo = RepoBuilder::new()
        .fetch_options(fetch_options)
        .clone(git_url, &temp_path)?;

    pb_git.finish_with_message("Git clone complete");

    // --- PRÉPARATION LYS ---
    let conn = db::connect_lys(target_dir).expect("Failed to connect to Lys");
    let mut indexed_cache = HashSet::new();

    {
        let mut revwalk = repo.revwalk()?;
        revwalk.push_head()?;
        // TOPOLOGICAL + REVERSE : On importe du plus vieux au plus récent
        revwalk.set_sorting(git2::Sort::TOPOLOGICAL | git2::Sort::REVERSE)?;

        let mut commits_oids: Vec<Oid> = revwalk.filter_map(|id| id.ok()).collect();
        if let Some(d) = depth {
            let len = commits_oids.len();
            if len > d as usize {
                commits_oids = commits_oids.split_off(len - d as usize);
            }
        }

        // --- BARRE 2 : IMPORTATION LYS ---
        let pb_lys = m.add(ProgressBar::new(commits_oids.len() as u64));
        pb_lys.set_style(ProgressStyle::default_bar()
            .template("{spinner:.white} [2/2] Lys Import:   [{bar:40.white}] {pos}/{len} commits ({msg})")?
            .progress_chars("=>-"));

        conn.execute("BEGIN TRANSACTION;")?;

        let mut commit_count = 0;
        for oid in commits_oids {
            let commit = repo.find_commit(oid)?;
            let tree = commit.tree()?;
            let tree_hash = tree.id().to_string();

            pb_lys.set_message(format!("{:.7}", tree_hash));

            // Indexation optimisée (VFS)
            build_vfs_tree(&conn, &repo, &tree, "ROOT", &mut indexed_cache)?;

            // Création du commit manuel
            let lys_commit_id = vcs::commit_manual(
                &conn,
                commit.message().unwrap_or("Import"),
                commit.author().name().unwrap_or("Git"),
                commit.time().seconds(),
                &tree_hash, // Assure-toi que vcs.rs accepte ce 5ème paramètre !
            )?;

            // Mise à jour de la branche main
            let query_branch = "INSERT INTO branches (name, head_commit_id) VALUES ('main', ?) 
                                ON CONFLICT(name) DO UPDATE SET head_commit_id = excluded.head_commit_id";
            let mut stmt_b = conn.prepare(query_branch)?;
            stmt_b.bind((1, lys_commit_id))?;
            stmt_b.next()?;

            // --- BATCHING : On commit toutes les 500 entrées pour la performance ---
            commit_count += 1;
            if commit_count % 500 == 0 {
                conn.execute("COMMIT; BEGIN TRANSACTION;")?;
            }

            pb_lys.inc(1);
        }

        conn.execute("COMMIT;")?;
        pb_lys.finish_with_message("History imported successfully");
    }

    // Nettoyage final
    std::fs::remove_dir_all(&temp_path)?;
    ok("Temporary Git files removed");

    // Checkout final pour avoir les fichiers sur disque
    vcs::checkout_head(&conn, target_dir).ok();

    ok("Repository ready!");
    Ok(())
}

fn build_vfs_tree(
    conn: &sqlite::Connection,
    repo: &Repository,
    tree: &Tree,
    parent_hash: &str,
    indexed: &mut HashSet<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let tree_id = tree.id().to_string();

    // Si on a déjà indexé ce dossier (et qu'on n'est pas à la racine), on skip
    if indexed.contains(&tree_id) && parent_hash != "ROOT" {
        return Ok(());
    }

    for entry in tree.iter() {
        let name = entry.name().unwrap_or("unnamed");
        let entry_hash = entry.id().to_string();
        let mode = entry.filemode() as i64;

        // Insertion hiérarchique
        db::insert_tree_node(conn, parent_hash, name, &entry_hash, mode, None)?;

        match entry.kind() {
            Some(ObjectType::Tree) => {
                let subtree = repo.find_tree(entry.id())?;
                build_vfs_tree(conn, repo, &subtree, &entry_hash, indexed)?;
            }
            Some(ObjectType::Blob) => {
                // On n'insère le blob que s'il est inconnu
                if !indexed.contains(&entry_hash) {
                    let blob = repo.find_blob(entry.id())?;
                    db::get_or_insert_blob(conn, blob.content())?;
                    indexed.insert(entry_hash);
                }
            }
            _ => {}
        }
    }
    indexed.insert(tree_id);
    Ok(())
}
