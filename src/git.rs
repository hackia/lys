use crate::db::{self, insert_tree_node};
use crate::utils::ok;
use crate::vcs;
use dashmap::DashSet;
use git2::build::RepoBuilder;
use git2::{FetchOptions, ObjectType, Oid, RemoteCallbacks, Repository};
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use rayon::prelude::*;
use std::path::Path;
use std::sync::Arc;
use std::sync::Mutex;
pub fn extract_repo_name(url: &str) -> String {
    let last_part = url.rsplit('/').next().unwrap_or("lys_repo");
    last_part
        .strip_suffix(".git")
        .unwrap_or(last_part)
        .to_string()
}

// Dans src/git.rs

fn build_vfs_tree_parallel(
    repo_path: &Path,          // Le dossier temporaire du clone (.git)
    target_dir: &Path,         // La racine de ton projet (où se trouve le vrai .lys)
    conn: &sqlite::Connection, // La connexion déjà ouverte vers ta DB saisonnière
    tree: &git2::Tree,
    parent_hash: &str,
    indexed: Arc<DashSet<String>>, // Cache thread-safe
    pb: &ProgressBar,
) -> Result<(), Box<dyn std::error::Error>> {
    let tree_id = tree.id().to_string();
    // 1. COLLECTE : On détache les données Git pour Rayon
    let entries: Vec<(Oid, String, ObjectType, i32)> = tree
        .iter()
        .map(|e| {
            (
                e.id(),
                e.name().unwrap_or("").to_string(),
                e.kind().unwrap_or(ObjectType::Any),
                e.filemode(),
            )
        })
        .collect();
    let db_lock = Mutex::new(());
    entries.par_iter().for_each(|(oid, _name, kind, _)| {
        if let ObjectType::Blob = kind {
            let h = oid.to_string();
            if !indexed.contains(&h) {
                if let Ok(local_repo) = Repository::open(repo_path) {
                    if let Ok(blob) = local_repo.find_blob(*oid) {
                        let content = blob.content();

                        // ON VERROUILLE UNIQUEMENT L'INSERTION
                        let _guard = db_lock.lock().unwrap();
                        match db::get_or_insert_blob_parallel(target_dir, &h, content) {
                            Ok(_) => {
                                indexed.insert(h);
                            }
                            Err(e) => eprintln!("Erreur store : {e}"),
                        }
                    }
                }
            }
        }
    });
    // 3. PHASE SÉQUENTIELLE : Construction de l'arborescence
    for (oid, name, kind, mode) in entries {
        let entry_hash = oid.to_string();
        pb.set_message(entry_hash.to_string());

        // FIX PERFORMANCE : On utilise la connexion 'conn' passée en paramètre
        // au lieu de réouvrir la DB à chaque fichier
        insert_tree_node(
            conn,
            parent_hash,
            &name,
            &entry_hash,
            mode as i64,
            None, // Le 'size' sera récupéré plus tard si besoin
        )?;

        if let ObjectType::Tree = kind {
            // Pour les dossiers, on descend récursivement
            if let Ok(local_repo) = Repository::open(repo_path) {
                let subtree = local_repo.find_tree(oid)?;
                build_vfs_tree_parallel(
                    repo_path,
                    target_dir,
                    conn,
                    &subtree,
                    &entry_hash,
                    Arc::clone(&indexed),
                    pb,
                )?;
            }
        }
    }

    indexed.insert(tree_id);
    Ok(())
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
                "{spinner:.white} [1/2] Git Fetch    [{bar:40.white}] {pos}/{len} objects ({msg})",
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
            .template("{spinner:.white} [2/2] Lys Import    [{bar:40.white}] {pos}/{len} commits ({msg})")?
            .progress_chars("=>-"));

        conn.execute("BEGIN TRANSACTION;")?;

        let mut commit_count = 0;

        let indexed_cache = Arc::new(DashSet::new()); // Pour chaque commit ou global
        // Dans src/git.rs - Fonction import_from_git
        for oid in commits_oids {
            let commit = repo.find_commit(oid)?;
            let tree = commit.tree()?;
            let tree_hash = tree.id().to_string();

            pb_lys.set_message(tree_hash.to_string());

            // UTILISE LA VERSION PARALLELE (et passe le chemin du repo, pas la connexion)
            build_vfs_tree_parallel(
                &temp_path, // repo_path
                target_dir,
                &conn,
                &tree,
                &tree_hash,
                Arc::clone(&indexed_cache),
                &pb_lys,
            )?;

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
