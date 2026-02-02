use crate::db;
use crate::utils::ok;
use crate::vcs;
use git2::{FetchOptions, ObjectType, Repository, Tree, build::RepoBuilder};
use std::collections::HashMap;
use std::path::Path;

pub fn extract_repo_name(url: &str) -> String {
    // 1. On coupe par les "/" et on prend le dernier morceau
    let last_part = url.rsplit('/').next().unwrap_or("silex_repo");

    // 2. On retire le ".git" si présent à la fin
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
    ok(format!("Clonage de {git_url} (Depth: {:?})...", depth.unwrap_or(0)).as_str());
    let temp_path = target_dir.join("temp_git_import");
    if temp_path.exists() {
        std::fs::remove_dir_all(&temp_path)?;
    }

    // --- CONFIGURATION DU CLONE (Shallow) ---
    let mut fetch_options = FetchOptions::new();

    // Si une profondeur est spécifiée, on limite l'historique (ex: 100 derniers commits)
    if let Some(d) = depth {
        fetch_options.depth(d);
    }

    let mut builder = RepoBuilder::new();
    builder.fetch_options(fetch_options);

    // On lance le clone avec les options
    let repo = builder.clone(git_url, &temp_path)?;
    // ----------------------------------------

    let mut revwalk = repo.revwalk()?;
    revwalk.set_sorting(git2::Sort::TOPOLOGICAL | git2::Sort::REVERSE)?;
    revwalk.push_head()?;

    let conn = db::connect_silex(target_dir).expect("failed to connect to the database");
    conn.execute(db::SILEX_INIT)?;
    let mut path_to_asset: HashMap<String, i64> = HashMap::new();
    ok("importing history...");

    for commit_oid in revwalk {
        let oid = commit_oid?;
        let commit = repo.find_commit(oid)?;
        let tree = commit.tree()?;

        // Transaction pour optimiser la vitesse (indispensable)
        // Dans src/git.rs (dans la boucle)

        conn.execute("BEGIN TRANSACTION")?;

        // 1. Création du Commit Silex
        let author = commit.author().name().unwrap_or("Unknown").to_string();
        let message = commit.message().unwrap_or("").to_string();
        let time = commit.time().seconds();
        let original_git_hash = commit.id().to_string();
        let full_message = format!("{}\n\n[Git-Import: {}]", message, original_git_hash);
        let silex_commit_id = vcs::commit_manual(&conn, &full_message, &author, time)?;

        // 2. Parcours récursif de l'arbre Git
        walk_git_tree(&repo, &tree, "", &mut |path, content| {
            // A. Gestion de l'Asset ID (Identité)
            let asset_id = if let Some(id) = path_to_asset.get(&path) {
                *id // Le fichier existe déjà, on garde son ID
            } else {
                // Nouveau fichier -> Nouvel Asset
                let new_id = db::create_asset(&conn).expect("Failed to create asset");
                path_to_asset.insert(path.clone(), new_id);
                new_id
            };

            // B. Gestion du Blob (Contenu)
            let blob_id = db::get_or_insert_blob(&conn, content).expect("Failed to insert blob");

            // C. Liaison dans le Manifeste
            db::insert_manifest_entry(&conn, silex_commit_id, asset_id, blob_id, &path)
                .expect("Failed to update manifest");
        })?;

        conn.execute("COMMIT")?;
    }

    // Nettoyage
    std::fs::remove_dir_all(&temp_path)?;
    ok("temps files removed");
    vcs::checkout_head(&conn, target_dir).expect("failed to build repository");
    ok("Import finnished");
    Ok(())
}

// Fonction récursive pour parcourir l'arbre Git
fn walk_git_tree<F>(
    repo: &Repository,
    tree: &Tree,
    prefix: &str,
    callback: &mut F,
) -> Result<(), git2::Error>
where
    F: FnMut(String, &[u8]),
{
    for entry in tree.iter() {
        let name = entry.name().unwrap_or("unnamed");
        let path = if prefix.is_empty() {
            name.to_string()
        } else {
            format!("{prefix}/{name}")
        };

        match entry.kind() {
            Some(ObjectType::Blob) => {
                let object = entry.to_object(repo)?;
                let blob = object.as_blob().unwrap();
                callback(path, blob.content());
            }
            Some(ObjectType::Tree) => {
                let object = entry.to_object(repo)?;
                let subtree = object.as_tree().unwrap();
                walk_git_tree(repo, subtree, &path, callback)?;
            }
            _ => {} // On ignore les submodules ou autres bizarreries pour l'instant
        }
    }
    Ok(())
}
