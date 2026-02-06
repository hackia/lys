use crate::db;
use crate::vcs;
use dashmap::DashSet;
use git2::build::RepoBuilder;
use git2::{ObjectType, Oid};
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use rayon::prelude::*;
use std::path::Path;
use std::sync::{Arc, Mutex};

/// Explore un arbre Git et insère les objets dans Lys de manière parallèle.
fn build_vfs_tree_parallel(
    repo: &Mutex<git2::Repository>,
    target_dir: &Path,
    conn: &sqlite::Connection,
    store_conn: &Mutex<sqlite::Connection>,
    tree_oid: Oid,
    parent_hash: &str,
    indexed: Arc<DashSet<String>>,
    pb: &ProgressBar,
) -> Result<(), Box<dyn std::error::Error>> {
    let tree_hash_str = tree_oid.to_string();

    // OPTIMISATION: Si ce dossier a déjà été scanné (Merkle), on ne descend pas
    if indexed.contains(&tree_hash_str) {
        return Ok(());
    }

    // 1. Extraction des entrées de l'arbre (Séquentiel sous verrou court)
    let entries: Vec<(Oid, String, ObjectType, i32)> = {
        let repo_guard = repo.lock().unwrap();
        let tree = repo_guard.find_tree(tree_oid)?;
        tree.iter()
            .map(|e| {
                (
                    e.id(),
                    e.name().unwrap_or("").to_string(),
                    e.kind().unwrap_or(ObjectType::Any),
                    e.filemode(),
                )
            })
            .collect()
    };

    // 2. Traitement des Blobs en parallèle
    entries.par_iter().for_each(|(oid, _name, kind, _)| {
        if let ObjectType::Blob = kind {
            let h = oid.to_string();
            if !indexed.contains(&h) {
                // Lecture du contenu (Verrou repo)
                let content = {
                    let repo_guard = repo.lock().unwrap();
                    repo_guard
                        .find_blob(*oid)
                        .map(|b| b.content().to_vec())
                        .ok()
                };

                if let Some(data) = content {
                    // Insertion en base (Verrou store)
                    let store_guard = store_conn.lock().unwrap();
                    let _ = db::insert_blob_with_conn(&store_guard, &h, &data);
                    indexed.insert(h);
                }
            }
        }
    });

    // 3. Traitement récursif des répertoires (Séquentiel)
    for (oid, name, kind, mode) in entries {
        let entry_hash = oid.to_string();
        pb.set_message(format!("Indexing {}", &entry_hash[..7]));

        db::insert_tree_node(conn, parent_hash, &name, &entry_hash, mode as i64, None)?;

        if let ObjectType::Tree = kind {
            build_vfs_tree_parallel(
                repo,
                target_dir,
                conn,
                store_conn,
                oid,
                &entry_hash,
                Arc::clone(&indexed),
                pb,
            )?;
        }
    }
    indexed.insert(tree_hash_str);
    Ok(())
}

pub fn import_from_git(
    git_url: &str,
    target_dir: &Path,
    depth: Option<i32>,
) -> Result<(), Box<dyn std::error::Error>> {
    let m = MultiProgress::new();
    let pb_git = m.add(ProgressBar::new(0));
    pb_git.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.green} {msg}")
            .unwrap(),
    );
    pb_git.set_message("Cloning git repository...");

    let temp_path = target_dir.join("temp_git_import");
    if temp_path.exists() {
        std::fs::remove_dir_all(&temp_path)?;
    }

    // Clonage et mise en Mutex immédiate
    let repo_raw = RepoBuilder::new().clone(git_url, &temp_path)?;
    let repo = Mutex::new(repo_raw);
    pb_git.finish_with_message("Git clone complete");

    let conn = db::connect_lys(target_dir)?;
    let store_db_path = target_dir.join(".lys/db/store.db");
    let store_conn = Mutex::new(sqlite::open(store_db_path)?);

    // OPTIMISATION SQL: On booste les perfs pour l'import (pas de synchro disque immédiate)
    conn.execute("PRAGMA synchronous = OFF; PRAGMA journal_mode = MEMORY;")?;
    {
        let s = store_conn.lock().unwrap();
        s.execute("PRAGMA synchronous = OFF; PRAGMA journal_mode = MEMORY;")?;
    }

    // Analyse de l'historique
    let (commits_oids, pb_lys) = {
        let repo_guard = repo.lock().unwrap();
        let mut revwalk = repo_guard.revwalk()?;
        revwalk.push_head()?;
        revwalk.set_sorting(git2::Sort::TOPOLOGICAL | git2::Sort::REVERSE)?;

        let mut oids: Vec<Oid> = revwalk.filter_map(|id| id.ok()).collect();
        if let Some(d) = depth {
            let start = oids.len().saturating_sub(d as usize);
            oids = oids[start..].to_vec();
        }

        let pb = m.add(ProgressBar::new(oids.len() as u64));
        pb.set_style(
            ProgressStyle::default_bar()
                .template("{spinner:.green} [{bar:40.white}] {pos}/{len} {msg}")
                .unwrap()
                .progress_chars("=>-"),
        );
        (oids, pb)
    };

    conn.execute("BEGIN TRANSACTION;")?;
    let indexed_cache = Arc::new(DashSet::new());
    for oid in commits_oids {
        let (tree_oid, author, message, time) = {
            let repo_guard = repo.lock().unwrap();
            let commit = repo_guard.find_commit(oid)?;
            (
                commit.tree_id(),
                commit.author().name().unwrap_or("Unknown").to_string(),
                commit.message().unwrap_or("").to_string(),
                commit.time().seconds(),
            )
        };

        build_vfs_tree_parallel(
            &repo,
            target_dir,
            &conn,
            &store_conn,
            tree_oid,
            &tree_oid.to_string(),
            Arc::clone(&indexed_cache),
            &pb_lys,
        )?;

        vcs::commit_manual(&conn, &message, &author, time, &tree_oid.to_string())?;
        pb_lys.inc(1);
    }

    // Mise à jour de la branche principale
    let last_commit_query = "SELECT id FROM commits ORDER BY id DESC LIMIT 1";
    let mut stmt = conn.prepare(last_commit_query)?;
    if let Ok(sqlite::State::Row) = stmt.next() {
        let last_id: i64 = stmt.read(0)?;
        let mut br_stmt = conn
            .prepare("INSERT OR REPLACE INTO branches (name, head_commit_id) VALUES ('main', ?)")?;
        br_stmt.bind((1, last_id))?;
        br_stmt.next()?;
    }

    conn.execute("COMMIT;")?;
    pb_lys.finish_with_message("Lys import complete");

    // Nettoyage et checkout final
    std::fs::remove_dir_all(&temp_path)?;
    vcs::checkout_head(&conn, target_dir)?;

    Ok(())
}

pub fn extract_repo_name(url: &str) -> String {
    url.split('/')
        .last()
        .unwrap_or("new_repo")
        .trim_end_matches(".git")
        .to_string()
}
