use crate::db::get_current_branch;
use crate::utils::commit_created;
use crate::utils::ko;
use crate::utils::ok;
use crate::utils::ok_status;
use crate::utils::ok_tag;
use glob::GlobError;
use glob::glob;
use ignore::DirEntry;
use nix::sys::wait::waitpid;
use nix::unistd::{ForkResult, execvp, fork};
use similar::{ChangeTag, TextDiff};
use sqlite::Connection;
use sqlite::Error;
use sqlite::State;
use std::collections::{HashMap, HashSet};
use std::ffi::CString;
use std::fmt::Debug;
use std::fs;
use std::fs::File;
use std::fs::create_dir_all;
use std::io::Error as IoError; // On renomme pour clarifier
use std::io::Write;
use std::io::{Read, Result as IoResult};
use std::path::{Path, PathBuf};
use tabled::{Table, Tabled};
use uuid::Uuid;
#[derive(Tabled)]
struct LogEntry {
    #[tabled(rename = "Hash")]
    hash: String,
    #[tabled(rename = "Author")]
    author: String,
    #[tabled(rename = "Message")]
    message: String,
    #[tabled(rename = "Date")]
    date: String,
}

#[derive(Debug)]
pub enum FileStatus {
    New(PathBuf),           // N'existe pas en base -> Nouvel Asset
    Modified(PathBuf, i64), // Existe mais hash différent -> Même Asset
    Deleted(PathBuf, i64),  // Existe en base mais plus sur disque
    Unchanged,
}

pub struct ManifestEntry {
    path: String,
    blob_id: i64,
    asset_id: i64,
    perm: i64,
}

pub fn sync(destination_path: &str) -> Result<(), IoError> {
    let files: Vec<Result<PathBuf, GlobError>> = glob("./.lys/db/*.db").expect("a").collect();
    let x = Path::new(destination_path);
    create_dir_all(format!("{destination_path}/.lys/db"))?;
    if x.exists() {
        for file in files.iter().flatten() {
            let z = file.file_name().expect("failed to get filename");
            fs::copy(
                file.as_path()
                    .to_str()
                    .expect("failed to get file path")
                    .to_string()
                    .as_str(),
                x.join(format!(".lys/db/{}", z.display()).as_str()),
            )?;
            ok(z.to_str()
                .expect("failed to get filename")
                .to_string()
                .as_str());
        }
    }
    ok("Backup complete");
    Ok(())
}

#[cfg(target_os = "freebsd")]
use nix::mount::{MntFlags, unmount};

#[cfg(target_os = "freebsd")]
pub fn umount(path: &str) -> Result<(), String> {
    // On convertit le chemin pour l'appel système
    let p = std::path::Path::new(path);

    // Sur FreeBSD, on utilise unmount avec MntFlags
    unmount(p, MntFlags::empty()).map_err(|e| format!("Échec du démontage de {} : {}", path, e))?;

    crate::utils::ok(&format!("Démontage de {} réussi", path));
    Ok(())
}

pub fn spawn_lys_shell(conn: &sqlite::Connection, reference: Option<&str>) -> Result<(), String> {
    let temp_mount = format!("/tmp/lys_shell_{}", uuid::Uuid::new_v4().simple());
    let mount_path = std::path::Path::new(&temp_mount);

    // 1. On monte le code (on réutilise ta commande qui fonctionne !)
    mount_version(conn, &temp_mount, reference).expect("failed to mount version");

    // 2. Préparation du message d'accueil (Saison + Messages + TODOs)
    let season = crate::db::Season::current(); //
    let user = crate::commit::author(); //

    println!("\nLYS SHELL - Season: {season} | User: {user}");
    println!("🚀 Entrée dans le shell éphémère. Tapez 'exit' pour quitter.");

    // 3. Gestion du processus Shell
    match unsafe { fork() } {
        Ok(ForkResult::Parent { child }) => {
            // Le parent attend que l'utilisateur quitte le shell
            waitpid(child, None).ok();

            // 4. Nettoyage automatique (Lest et démontage)
            println!("\nNettoyage du shell...");
            umount(&temp_mount)
                .map_err(|e| println!("Erreur nettoyage : {}", e))
                .ok();
            std::fs::remove_dir_all(mount_path).ok();
            ok("Shell Lys fermé proprement.");
        }

        Ok(ForkResult::Child) => {
            // Le fils remplace son processus par un shell (ex: fish ou bash)
            let shell = CString::new("fish").unwrap(); // Ou ton shell préféré
            let args = [shell.clone()];

            // On change le répertoire de travail vers le montage
            std::env::set_current_dir(mount_path).ok();

            execvp(&shell, &args).map_err(|e| e.to_string())?;
        }
        Err(e) => return Err(format!("Fork failed: {e}")),
    }

    Ok(())
}
// Dans src/vcs.rs

pub fn mount_version(
    conn: &Connection,
    target_path: &str,
    reference: Option<&str>,
) -> Result<(), sqlite::Error> {
    let target = Path::new(target_path);

    // 1. Créer le point de montage s'il n'existe pas
    if !target.exists() {
        fs::create_dir_all(target).expect("Failed to create mount point");
    }

    // 2. Déterminer quel commit monter
    let commit_id = if let Some(r) = reference {
        // On réutilise tes fonctions existantes pour trouver le hash
        get_commit_id_by_hash(conn, r)?
    } else {
        let branch = get_current_branch(conn)?;
        let (id, _) = get_branch_head_info(conn, &branch)?;
        id
    };

    let commit_id = commit_id.expect("Reference not found");

    // 3. Préparer le dossier "source" (Cache interne de lys)
    let cache_source = format!(".lys/mounts/{}", commit_id);
    let cache_path = Path::new(&cache_source);

    if !cache_path.exists() {
        fs::create_dir_all(cache_path).expect("Failed to create cache dir");
        // Ici on réutilise ta logique de checkout_head mais vers le cache
        reconstruct_to_path(conn, commit_id, cache_path)?;
    }

    #[cfg(target_os = "linux")]
    {
        use nix::mount::{MsFlags as MountFlags, mount};
        // Code spécifique à Linux
        mount(
            Some(cache_path),
            target_path,
            Some("none"),
            MountFlags::MS_BIND | MountFlags::MS_RDONLY,
            None::<&str>,
        )
        .expect("failed to mount");
    }

    #[cfg(target_os = "freebsd")]
    {
        use nix::mount::{MntFlags, Nmount};
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;

        // 1. On prépare les données (on "matérialise" en CString)
        // On doit les garder dans des variables pour qu'elles ne soient pas dropées
        let k_type = CString::new("fstype").unwrap();
        let v_type = CString::new("nullfs").unwrap();

        let k_dest = CString::new("fspath").unwrap();
        let v_dest = CString::new(target.as_os_str().as_bytes()).unwrap();

        let k_from = CString::new("from").unwrap();
        let v_from = CString::new(cache_path.as_os_str().as_bytes()).unwrap();

        // 2. On configure nmount
        let mut nm = Nmount::new();
        // On passe des références (&k_type) pour ne pas déplacer (move) les variables
        nm.str_opt(&k_type, &v_type);
        nm.str_opt(&k_dest, &v_dest);
        nm.str_opt(&k_from, &v_from);

        // 3. L'appel système
        nm.nmount(MntFlags::MNT_RDONLY).map_err(|e| sqlite::Error {
            code: Some(1),
            message: Some(format!("Erreur nmount: {}", e)),
        })?;
    }

    ok(&format!("Monté sur {}", target.display()).as_str());
    Ok(())
}

// Helper pour reconstruire les fichiers sans toucher au working dir actuel
fn reconstruct_to_path(
    conn: &Connection,
    commit_id: i64,
    dest: &Path,
) -> Result<(), sqlite::Error> {
    let query = "SELECT m.file_path, b.content FROM manifest m JOIN store.blobs b ON m.blob_id = b.id WHERE m.commit_id = ?";
    let mut stmt = conn.prepare(query)?;
    stmt.bind((1, commit_id))?;

    while let Ok(State::Row) = stmt.next() {
        let path_str: String = stmt.read(0)?;
        let raw_content: Vec<u8> = stmt.read(1)?;
        let content = crate::db::decompress(&raw_content); //
        let full_path = dest.join(&path_str);

        if let Some(parent) = full_path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(full_path, content).unwrap();
    }
    Ok(())
}

pub fn checkout_head(conn: &Connection, root_path: &Path) -> Result<(), sqlite::Error> {
    ok("Bulding...");

    // 1. Trouver le dernier commit (HEAD)
    // On prend le plus grand ID, ce qui correspond au dernier inséré lors de l'import
    let query_head = "SELECT id FROM commits ORDER BY id DESC LIMIT 1";
    let mut stmt = conn.prepare(query_head)?;

    let head_id: i64 = if let Ok(State::Row) = stmt.next() {
        stmt.read(0)?
    } else {
        return Ok(()); // Pas de commits, rien à faire
    };

    // 2. Récupérer la liste des fichiers pour ce commit (Manifeste + Blobs)
    // On joint manifest et blobs pour avoir le chemin ET le contenu
    let query_files = "
        SELECT m.file_path, b.content 
        FROM manifest m
        JOIN store.blobs b ON m.blob_id = b.id
        WHERE m.commit_id = ?
    ";

    let mut stmt_files = conn.prepare(query_files)?;
    stmt_files.bind((1, head_id))?;

    while let Ok(State::Row) = stmt_files.next() {
        let path_str: String = stmt_files.read(0)?;
        let raw_content: Vec<u8> = stmt_files.read(1)?;
        let content = crate::db::decompress(&raw_content);

        let full_path = root_path.join(&path_str);

        // Créer les dossiers parents si nécessaire (ex: src/ui/...)
        if let Some(parent) = full_path.parent() {
            fs::create_dir_all(parent).unwrap(); // Ignorer erreur si existe déjà
        }

        // Écrire le fichier
        let mut file = File::create(full_path).unwrap();
        file.write_all(&content).unwrap();
    }
    ok("Clonned");
    Ok(())
}

// Dans src/vcs.rs

pub fn commit_manual(
    conn: &Connection,
    message: &str,
    author: &str,
    timestamp: i64,
) -> Result<i64, sqlite::Error> {
    // 1. On récupère le hash du dernier commit inséré (le parent) pour la chaîne cryptographique
    // Cela permet de lier mathématiquement ce commit au précédent
    let query_last = "SELECT hash FROM commits ORDER BY id DESC LIMIT 1";
    let mut stmt_last = conn.prepare(query_last)?;

    let parent_hash = if let Ok(State::Row) = stmt_last.next() {
        stmt_last.read::<String, _>(0)?
    } else {
        String::from("") // Premier commit (Genesis)
    };

    // 2. CORRECTION : On calcule un VRAI hash Blake3
    // On mélange : Parent + Auteur + Message + Date
    let commit_data = format!("{}{}{}{}", parent_hash, author, message, timestamp);
    let silex_hash = blake3::hash(commit_data.as_bytes()).to_hex().to_string();

    // 3. Insertion propre
    let query = "INSERT INTO commits (hash, parent_hash, author, message, timestamp) VALUES (?, ?, ?, ?, datetime(?, 'unixepoch'))";
    let mut stmt = conn.prepare(query)?;

    stmt.bind((1, silex_hash.as_str()))?; // On utilise le hash calculé

    // On lie le parent (pour que l'arbre soit valide)
    if parent_hash.is_empty() {
        stmt.bind((2, Option::<&str>::None))?;
    } else {
        stmt.bind((2, Some(parent_hash.as_str())))?;
    }

    stmt.bind((3, author))?;
    stmt.bind((4, message))?;
    stmt.bind((5, timestamp))?;
    stmt.next()?;

    // On retourne l'ID
    let id_query = "SELECT last_insert_rowid()";
    let mut stmt_id = conn.prepare(id_query)?;
    stmt_id.next()?;
    Ok(stmt_id.read(0)?)
}

pub fn tag_create(conn: &Connection, name: &str, message: Option<&str>) -> Result<(), IoError> {
    // 1. On récupère le commit actuel (HEAD)
    let current_branch = get_current_branch(conn).expect("failed to get current branch");

    let (head_id, head_hash) =
        get_branch_head_info(conn, &current_branch).map_err(|e| IoError::other(e.to_string()))?;

    if head_id.is_none() {
        return Err(IoError::other(
            "Cannot tag an empty branch. Commit something first.",
        ));
    }

    // 2. On insère le tag
    let query = "INSERT INTO tags (name, commit_id, description) VALUES (?, ?, ?)";
    let mut stmt = conn
        .prepare(query)
        .map_err(|e| IoError::other(e.to_string()))?;

    stmt.bind((1, name)).unwrap();
    stmt.bind((2, head_id.unwrap())).unwrap();
    stmt.bind((3, message)).unwrap();

    match stmt.next() {
        Ok(_) => ok(&format!(
            "Tag '{name}' created on commit {}",
            &head_hash[0..7]
        )),
        Err(_) => return Err(IoError::other(format!("Tag '{name}' already exists."))),
    }
    Ok(())
}

pub fn tag_list(conn: &Connection) -> Result<(), IoError> {
    // On joint avec la table commits pour afficher le hash correspondant
    let query = "
        SELECT t.name, t.description, t.created_at, c.hash
        FROM tags t
        JOIN commits c ON t.commit_id = c.id
        ORDER BY t.name
    ";
    let mut stmt = conn
        .prepare(query)
        .map_err(|e| IoError::other(e.to_string()))?;

    let mut count = 0;
    while let Ok(State::Row) = stmt.next() {
        let name: String = stmt.read("name").unwrap();
        let desc: Option<String> = stmt.read("description").unwrap_or(None);
        let hash: String = stmt.read("hash").unwrap();
        let date: String = stmt.read("created_at").unwrap();
        let desc_str = desc.unwrap_or_else(|| String::from("no description"));
        ok_tag(
            name.as_str(),
            desc_str.as_str(),
            date.as_str(),
            hash.as_str(),
        );
        count += 1;
    }
    if count == 0 {
        ok("no tags yet");
    }
    Ok(())
}

// --- GESTION GIT FLOW (OPTIMISÉE) ---

pub fn hotfix_start(conn: &Connection, name: &str) -> Result<(), Error> {
    let branch_name = format!("hotfix/{name}");
    let source_branch = "main"; // CONTRAINTE : Un hotfix part toujours de la prod

    // 1. On vérifie qu'on part bien de 'main' pour avoir la base saine
    let (main_id, _) = get_branch_head_info(conn, source_branch)?;
    if main_id.is_none() {
        return Err(Error {
            code: Some(1),
            message: Some(String::from("No main branches has been founded")),
        });
    }

    // 2. On crée la branche manuellement (sans utiliser create_branch qui utilise HEAD)
    let query = "INSERT INTO branches (name, head_commit_id) VALUES (?, ?)";
    let mut stmt = conn.prepare(query)?;
    stmt.bind((1, branch_name.as_str()))?;
    stmt.bind((2, main_id.unwrap()))?;

    match stmt.next() {
        Ok(_) => {} // Création OK
        Err(_) => {
            return Err(Error {
                code: Some(1),
                message: Some(String::from("hotfix already exist")),
            });
        }
    }

    // 3. On bascule dessus
    checkout(conn, &branch_name)?;

    ok(&format!(
        "Hotfix started: Switched to '{branch_name}' from 'main'"
    ));
    Ok(())
}

pub fn hotfix_finish(conn: &Connection, name: &str) -> Result<(), Error> {
    // C'est la même logique que feature_finish, mais sémantiquement distinct
    let hotfix_branch = format!("hotfix/{name}");
    let target_branch = "main";

    let (hf_head_id, _) = get_branch_head_info(conn, &hotfix_branch)?;
    if hf_head_id.is_none() {
        return Err(Error {
            code: Some(1),
            message: Some(String::from("hotfix not exist")),
        });
    }

    ok(format!("Switching to '{target_branch}' to apply hotfix...").as_str());
    checkout(conn, target_branch)?;

    // Fast-Forward Merge
    let query = "UPDATE branches SET head_commit_id = ? WHERE name = ?";
    let mut stmt = conn.prepare(query)?;
    stmt.bind((1, hf_head_id.unwrap()))?;
    stmt.bind((2, target_branch))?;
    stmt.next()?;

    ok("Hotfix applied to main");

    // Nettoyage
    let delete_query = "DELETE FROM branches WHERE name = ?";
    let mut del_stmt = conn.prepare(delete_query)?;
    del_stmt.bind((1, hotfix_branch.as_str()))?;
    del_stmt.next()?;
    ok(&format!("Hotfix '{name}' finished and branch deleted."));
    Ok(())
}

pub fn feature_start(conn: &Connection, name: &str) -> Result<(), Error> {
    // 1. Standardisation du nom : feature/nom
    let branch_name = format!("feature/{name}");

    create_branch(conn, &branch_name)?;

    // 3. On bascule dessus immédiatement (Optimisation UX)
    checkout(conn, &branch_name)?;

    ok(&format!("Flow started: You are now on '{branch_name}'"));
    Ok(())
}

pub fn feature_finish(conn: &Connection, name: &str) -> Result<(), Error> {
    let feat_branch = format!("feature/{name}");
    let target_branch = "main";

    // 1. Sécurité : On vérifie que la branche feature existe
    let (feat_head_id, _) = get_branch_head_info(conn, &feat_branch)?;
    if feat_head_id.is_none() {
        return Err(Error {
            code: Some(1),
            message: Some(String::from("main branch not exist")),
        });
    }

    // 2. On bascule sur 'main' pour préparer la fusion
    ok(format!("Switching to '{target_branch}' to merge changes...").as_str());
    checkout(conn, target_branch)?;

    // 3. LE FAST-FORWARD (L'optimisation ultime)
    // Au lieu de calculer un diff, on déplace juste le pointeur de main sur la tête de la feature
    let query = "UPDATE branches SET head_commit_id = ? WHERE name = ?";
    let mut stmt = conn.prepare(query)?;
    stmt.bind((1, feat_head_id.unwrap()))?;
    stmt.bind((2, target_branch))?;
    stmt.next()?;

    ok("Fast-forward merge complete");

    // 4. Nettoyage : On supprime la branche temporaire
    let delete_query = "DELETE FROM branches WHERE name = ?";
    let mut del_stmt = conn.prepare(delete_query)?;
    del_stmt.bind((1, feat_branch.as_str()))?;
    del_stmt.next()?;

    ok(&format!("Feature '{name}' finished and branch deleted."));
    Ok(())
}

pub fn create_branch(conn: &Connection, new_branch_name: &str) -> Result<(), Error> {
    // 1. On récupère la branche actuelle et son commit ID
    let current_branch = get_current_branch(conn).expect("failed to get current branch");
    let (head_id, _) = get_branch_head_info(conn, &current_branch)?;

    if let Some(id) = head_id {
        // 2. On insère la nouvelle étiquette pointant vers le MEME commit
        let query = "INSERT INTO branches (name, head_commit_id) VALUES (?, ?)";
        let mut stmt = conn.prepare(query)?;
        stmt.bind((1, new_branch_name))?;
        stmt.bind((2, id))?;

        match stmt.next() {
            Ok(_) => ok(&format!("Branch '{new_branch_name}' created.")),
            Err(_) => ko(format!("Error: branch '{new_branch_name}' already exists.").as_str()),
        }
    } else {
        ok("Cannot branch from an empty repository. Commit something first.");
    }
    Ok(())
}

pub fn checkout(conn: &Connection, target_ref: &str) -> Result<(), Error> {
    // 1. VÉRIFICATION DE SÉCURITÉ
    let current_dir = std::env::current_dir().unwrap();
    let current_branch = get_current_branch(conn).unwrap_or("DETACHED".to_string());

    // Si on est déjà dessus (et que ce n'est pas un checkout forcé sur un hash), on skip
    if current_branch == target_ref {
        ok(&format!("Already on '{target_ref}'"));
        return Ok(());
    }

    let status_list = status(conn, current_dir.to_str().unwrap(), &current_branch)?;
    if !status_list.is_empty() {
        ok("Your changes would be overwritten by checkout.");
        ok("Please commit your changes or stash them first.");
        return Ok(());
    }

    // 2. PRÉPARATION DES DONNÉES (C'est ici qu'on change la logique !)
    let (current_head_id, _) = get_branch_head_info(conn, &current_branch)?;

    // A. Est-ce une BRANCHE ?
    let (branch_head_id, _) = get_branch_head_info(conn, target_ref)?;

    // B. Sinon, est-ce un HASH (Time Travel) ?
    let target_head_id = if branch_head_id.is_some() {
        branch_head_id
    } else {
        get_commit_id_by_hash(conn, target_ref)?
    };

    // Si introuvable ni en branche, ni en commit
    if target_head_id.is_none() {
        return Err(Error {
            code: Some(1),
            message: Some(format!(
                "Reference '{target_ref}' (branch or commit) not found."
            )),
        });
    }
    // On charge les deux manifestes en mémoire pour comparer
    let current_files = get_manifest_map(conn, current_head_id)?;
    let target_files = get_manifest_map(conn, target_head_id)?;
    ok(format!("Switched to branch '{target_ref}'").as_str());

    // 3. MISE À JOUR DU DISQUE (Différentiel)

    // A. Gérer les AJOUTS et MODIFICATIONS (Target vs Current)
    for (path, (target_hash, _)) in &target_files {
        let should_write = match current_files.get(path) {
            Some((current_hash, _)) => current_hash != target_hash, // Modifié
            None => true,                                           // Nouveau fichier
        };

        if should_write {
            // On récupère le contenu binaire depuis le store
            if let Some(content) = get_blob_bytes_by_hash(conn, target_hash)?
                && let Some(parent) = Path::new(path).parent()
            {
                std::fs::create_dir_all(parent).expect("failed to create directory");
                std::fs::write(path, content).expect("failed to write content");
            }
        }
    }

    // B. Gérer les SUPPRESSIONS (Ce qui est dans Current mais plus dans Target)
    for path in current_files.keys() {
        if !target_files.contains_key(path) && Path::new(path).exists() {
            std::fs::remove_file(path).expect("failed to remove the file");
            // Optionnel : Supprimer les dossiers vides parents
        }
    }

    // 4. METTRE À JOUR LA CONFIGURATION
    // ... LE RESTE DE LA FONCTION (BOUCLES FOR) RESTE IDENTIQUE ...
    // ... (Partie 3: MISE À JOUR DU DISQUE) ...

    // 4. METTRE À JOUR LA CONFIGURATION (Ajustement final)
    let query = "INSERT INTO config (key, value) VALUES ('current_branch', ?) 
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value";
    let mut stmt = conn.prepare(query)?;

    if branch_head_id.is_some() {
        // C'est une vraie branche
        stmt.bind((1, target_ref))?;
    } else {
        ok(format!("You are in 'Detached HEAD' state (viewing commit {target_ref}).").as_str());
        stmt.bind((1, "DETACHED"))?;
    }
    stmt.next()?;
    Ok(())
}

// Récupère tout le manifeste d'un commit sous forme de HashMap facile à comparer
fn get_manifest_map(
    conn: &Connection,
    commit_id: Option<i64>,
) -> Result<HashMap<String, (String, i64)>, Error> {
    let mut map = HashMap::new();
    if let Some(id) = commit_id {
        let query = "
            SELECT m.file_path, b.hash, m.asset_id 
            FROM manifest m
            JOIN store.blobs b ON m.blob_id = b.id
            WHERE m.commit_id = ?
        ";
        let mut stmt = conn.prepare(query)?;
        stmt.bind((1, id))?;
        while let Ok(State::Row) = stmt.next() {
            let path: String = stmt.read("file_path")?;
            let hash: String = stmt.read("hash")?;
            let asset_id: i64 = stmt.read("asset_id")?;
            map.insert(path, (hash, asset_id));
        }
    }
    Ok(map)
}

// Récupère les octets via le hash (plus rapide que via le path)
fn get_blob_bytes_by_hash(conn: &Connection, hash: &str) -> Result<Option<Vec<u8>>, Error> {
    let query = "SELECT content FROM store.blobs WHERE hash = ?";
    let mut stmt = conn.prepare(query)?;
    stmt.bind((1, hash))?;
    if let Ok(State::Row) = stmt.next() {
        let raw: Vec<u8> = stmt.read("content")?;
        Ok(Some(crate::db::decompress(&raw)))
    } else {
        Ok(None)
    }
}

// Helper pour récupérer le contenu BRUT (bytes) d'un fichier dans le HEAD
// C'est vital pour restaurer des images ou des exécutables sans corruption UTF-8
fn get_blob_bytes(conn: &Connection, branch: &str, path: &Path) -> Result<Option<Vec<u8>>, Error> {
    let relative_path = path.strip_prefix(".").unwrap_or(path).to_string_lossy();

    let query = "
        SELECT b.content 
        FROM branches br
        JOIN manifest m ON m.commit_id = br.head_commit_id
        JOIN store.blobs b ON m.blob_id = b.id
        WHERE br.name = ? AND m.file_path = ?
    ";

    let mut stmt = conn.prepare(query)?;
    stmt.bind((1, branch))?;
    stmt.bind((2, relative_path.as_ref()))?;

    if let Ok(State::Row) = stmt.next() {
        let raw_content: Vec<u8> = stmt.read("content")?;
        let content = crate::db::decompress(&raw_content);
        Ok(Some(content))
    } else {
        Ok(None) // Le fichier n'existe pas dans le HEAD
    }
}

pub fn restore(conn: &Connection, path_str: &str) -> Result<(), Error> {
    let path = Path::new(path_str);
    let branch = get_current_branch(conn).expect("failed to get current branch");
    // 1. On cherche le contenu original dans la BDD
    match get_blob_bytes(conn, &branch, path)? {
        Some(content) => {
            // 2. Le fichier existe dans le HEAD, on l'écrase sur le disque
            std::fs::write(path, content).expect("failed to restore");
            ok(&format!("Restored '{}' from HEAD.", path.display()));
        }
        None => {
            ko(format!(
                "Error: File '{}' does not exist in the last commit.",
                path.display()
            )
            .as_str());
        }
    }
    Ok(())
}

pub fn diff(conn: &Connection) -> Result<(), Error> {
    let current_dir = std::env::current_dir().unwrap();
    let current_dir_str = current_dir.to_str().unwrap();
    let branch = get_current_branch(conn).expect("failed to get current branch");
    // 1. On récupère les changements (on réutilise ta logique de status)
    let changes = status(conn, current_dir_str, &branch)?;

    if changes.is_empty() {
        return Ok(());
    }

    for change in changes {
        match change {
            FileStatus::Modified(path, _) => {
                println!("\n\x1b[1;33mDiff: {}\x1b[0m", path.display());
                println!("\x1b[90m==================================================\x1b[0m");

                // A. Lire le nouveau contenu sur le disque
                let new_content = match std::fs::read_to_string(&path) {
                    Ok(c) => c,
                    Err(_) => {
                        println!("(Binary or unreadable file)");
                        continue;
                    }
                };

                // B. Récupérer l'ancien contenu depuis la BDD (via le Hash du HEAD)
                let old_content = get_file_content_from_head(conn, &branch, &path)?;

                // C. Calculer et afficher le Diff
                let diff = TextDiff::from_lines(&old_content, &new_content);

                for change in diff.iter_all_changes() {
                    let (sign, color) = match change.tag() {
                        ChangeTag::Delete => ("-", "\x1b[31m"), // Rouge
                        ChangeTag::Insert => ("+", "\x1b[32m"), // Vert
                        ChangeTag::Equal => (" ", "\x1b[0m"),   // Blanc
                    };
                    print!("{}{}{}\x1b[0m", color, sign, change);
                }
            }
            FileStatus::New(path) => {
                println!(
                    "\n\x1b[1;32mNew File: {}\x1b[0m (All content is new)",
                    path.display()
                );
            }
            FileStatus::Deleted(path, _) => {
                println!("\n\x1b[1;31mDeleted File: {}\x1b[0m", path.display());
            }
            _ => {}
        }
    }
    Ok(())
}

// Helper pour récupérer le contenu textuel d'un blob depuis le HEAD
fn get_file_content_from_head(
    conn: &Connection,
    branch: &str,
    path: &Path,
) -> Result<String, Error> {
    let relative_path = path.strip_prefix(".").unwrap_or(path).to_string_lossy();

    // 1. Trouver le hash du fichier dans le commit HEAD
    let query = "
        SELECT b.content 
        FROM branches br
        JOIN manifest m ON m.commit_id = br.head_commit_id
        JOIN store.blobs b ON m.blob_id = b.id
        WHERE br.name = ? AND m.file_path = ?
    ";

    let mut stmt = conn.prepare(query)?;
    stmt.bind((1, branch))?;
    stmt.bind((2, relative_path.as_ref()))?;

    if let Ok(State::Row) = stmt.next() {
        // Attention : On suppose ici que c'est du texte (UTF-8)
        let raw_content_blob: Vec<u8> = stmt.read("content")?;
        let content_blob = crate::db::decompress(&raw_content_blob);
        match String::from_utf8(content_blob) {
            Ok(s) => Ok(s),
            Err(_) => Ok(String::from("(Binary content)")),
        }
    } else {
        Ok(String::new()) // Fichier introuvable (ne devrait pas arriver si Modified)
    }
}

pub fn log(conn: &Connection, page: usize, per_page: usize) -> Result<(), sqlite::Error> {
    // Calcul de l'offset (Page 1 = Offset 0)
    let offset = (page - 1) * per_page;

    // Requête avec LIMIT et OFFSET
    let query = "
        SELECT hash, author, message, timestamp 
        FROM commits 
        ORDER BY timestamp DESC 
        LIMIT ? OFFSET ?";

    let mut stmt = conn.prepare(query)?;
    stmt.bind((1, per_page as i64))?;
    stmt.bind((2, offset as i64))?;

    let mut logs = Vec::new();
    while let Ok(State::Row) = stmt.next() {
        // On tronque le hash pour l'affichage (7 premiers chars)
        let full_hash: String = stmt.read(0)?;
        let short_hash = if full_hash.len() > 7 {
            full_hash[0..7].to_string()
        } else {
            full_hash
        };
        logs.push(LogEntry {
            hash: short_hash,
            author: stmt.read(1)?,
            message: stmt.read(2)?,
            date: stmt.read(3)?,
        });
    }

    if logs.is_empty() {
        if page == 1 {
            ok("please commit first");
        } else {
            ok(format!("No commits on {page} page.").as_str());
        }
    } else {
        let x = logs.len();
        println!("{}", Table::new(&logs));
        if x >= 120 {
            ok(format!(
                "\nPage {page} ({}/{per_page} commits). Use --page {} for see the suite.",
                x,
                page + 1
            )
            .as_str());
        }
    }
    Ok(())
}

pub fn files() -> Vec<String> {
    let mut all: Vec<String> = Vec::new();
    let walk = ignore::WalkBuilder::new(".")
        .standard_filters(true)
        .threads(4)
        .add_custom_ignore_filename("syl")
        .hidden(true)
        .build();
    let files = walk.collect::<Vec<Result<DirEntry, ignore::Error>>>();
    for file in files.iter().flatten() {
        if file.path().ends_with(".") {
            continue;
        }
        all.push(
            file.path()
                .strip_prefix("./")
                .expect("failed to strip prefix")
                .to_str()
                .expect("failed to get path")
                .to_string(),
        );
    }
    all
}

pub fn commit(conn: &Connection, message: &str, author: &str) -> Result<(), Error> {
    let current_branch = get_current_branch(conn).expect("failed to get current branch");
    let (parent_id, parent_hash) = get_branch_head_info(conn, &current_branch)?;

    // 1. Charger l'état parent complet (ID + Hash)
    let parent_state = get_parent_asset_map(conn, parent_id, &parent_hash)?;

    let root_path = ".";
    let walk = ignore::WalkBuilder::new(".")
        .add_custom_ignore_filename("syl")
        .threads(4)
        .standard_filters(true)
        .build();

    let mut new_manifest: Vec<ManifestEntry> = Vec::new();
    let mut changes_count = 0; // Compteur de modifs

    // ON OUVRE LA TRANSACTION MAIS ON NE VALIDE PAS TOUT DE SUITE
    conn.execute("BEGIN TRANSACTION;")?;

    for result in walk {
        let entry = match result {
            Ok(e) => e,
            Err(_) => continue,
        };
        let path = entry.path();

        if path.components().any(|c| c.as_os_str() == ".lys") || path.is_dir() {
            continue;
        }

        let content_hash = match calculate_hash(path) {
            Ok(h) => h,
            Err(_) => continue,
        };

        let metadata = std::fs::metadata(path).expect("failed to get metadata");
        let blob_id = ensure_blob_exists(conn, &content_hash, path, metadata.len())?;

        let relative_path = path
            .strip_prefix("./")
            .unwrap_or(path)
            .to_string_lossy()
            .to_string();

        // --- DÉTECTION INTELLIGENTE ---
        let asset_id = match parent_state.get(&relative_path) {
            Some((id, old_hash)) => {
                // Le fichier existait déjà : a-t-il changé ?
                if old_hash != &content_hash {
                    changes_count += 1; // MODIFIED
                }
                *id
            }
            None => {
                changes_count += 1; // NEW FILE
                create_new_asset(conn, author)?
            }
        };

        new_manifest.push(ManifestEntry {
            path: relative_path,
            blob_id,
            asset_id,
            perm: if metadata.permissions().readonly() {
                444
            } else {
                644
            },
        });
    }

    // --- DÉTECTION DES SUPPRESSIONS ---
    // Si un fichier était dans le parent mais n'est pas dans le nouveau manifest -> DELETED
    for old_path in parent_state.keys() {
        if !new_manifest.iter().any(|e| e.path == *old_path) {
            changes_count += 1;
        }
    }

    // --- LE VERDICT ---
    if changes_count == 0 {
        conn.execute("ROLLBACK;")?; // On annule tout, on ne touche pas à la DB
        ok("Nothing to commit, working tree clean."); // Message clair
        return Ok(());
    }

    // Si on arrive ici, c'est qu'il y a des changements -> ON ÉCRIT !

    // (Le reste du code reste identique : calcul hash, insert commits, insert manifest...)
    let timestamp = chrono::Utc::now().to_rfc3339();
    let commit_hash_input = format!("{parent_hash}{author}{message}{timestamp}");
    let commit_hash = blake3::hash(commit_hash_input.as_bytes())
        .to_hex()
        .to_string();

    let signature = match crate::crypto::sign_message(Path::new(root_path), commit_hash.as_str()) {
        Ok(sig) => sig,
        Err(_) => String::from("UNSIGNED"),
    };

    let query_commit = "INSERT INTO commits (hash, parent_hash, author, message, timestamp, signature) VALUES (?, ?, ?, ?, ?, ?) RETURNING id;";
    let mut stmt = conn.prepare(query_commit)?;
    stmt.bind((1, commit_hash.as_str()))?;
    stmt.bind((
        2,
        if parent_hash.is_empty() {
            None
        } else {
            Some(parent_hash.as_str())
        },
    ))?;
    stmt.bind((3, author))?;
    stmt.bind((4, message))?;
    stmt.bind((5, timestamp.as_str()))?;
    stmt.bind((6, signature.as_str()))?;
    stmt.next()?;
    let new_commit_id: i64 = stmt.read("id")?;

    let query_manifest = "INSERT INTO manifest (commit_id, asset_id, blob_id, file_path, permissions) VALUES (?, ?, ?, ?, ?)";
    let mut stmt_m = conn.prepare(query_manifest)?;

    for entry in new_manifest {
        stmt_m.reset()?;
        stmt_m.bind((1, new_commit_id))?;
        stmt_m.bind((2, entry.asset_id))?;
        stmt_m.bind((3, entry.blob_id))?;
        stmt_m.bind((4, entry.path.as_str()))?;
        stmt_m.bind((5, entry.perm))?;
        stmt_m.next()?;
    }

    let query_upsert = "INSERT INTO branches (name, head_commit_id) VALUES (?, ?) ON CONFLICT(name) DO UPDATE SET head_commit_id = excluded.head_commit_id";
    let mut stmt_b = conn.prepare(query_upsert)?;
    stmt_b.bind((1, current_branch.as_str()))?;
    stmt_b.bind((2, new_commit_id))?;
    stmt_b.next()?;

    drop(stmt);
    drop(stmt_m);
    drop(stmt_b);

    conn.execute("COMMIT;")?; // Validation finale
    commit_created(commit_hash[0..7].to_string().as_str());
    Ok(())
}

// On met à jour get_branch_head_info pour chercher dans 'old' si besoin
fn get_branch_head_info(conn: &Connection, branch: &str) -> Result<(Option<i64>, String), Error> {
    // 1. Base actuelle
    let query = "SELECT c.id, c.hash FROM branches b JOIN commits c ON b.head_commit_id = c.id WHERE b.name = ?";
    let mut stmt = conn.prepare(query)?;
    stmt.bind((1, branch))?;

    if let Ok(State::Row) = stmt.next() {
        return Ok((Some(stmt.read("id")?), stmt.read("hash")?));
    }

    // 2. Repli sur la base 'old' (la "Dernière Base Connue")
    let query_old = "SELECT c.hash FROM old.branches b JOIN old.commits c ON b.head_commit_id = c.id WHERE b.name = ?";
    if let Ok(mut stmt_old) = conn.prepare(query_old) {
        stmt_old.bind((1, branch))?;
        if let Ok(State::Row) = stmt_old.next() {
            // On renvoie l'ID à None (car l'ID de 'old' n'existe pas ici) mais le HASH pour le chaînage
            return Ok((None, stmt_old.read("hash")?));
        }
    }

    Ok((None, String::new()))
}

// On met à jour get_parent_asset_map pour charger le manifest depuis 'old'
fn get_parent_asset_map(
    conn: &Connection,
    parent_id: Option<i64>,
    parent_hash: &str,
) -> Result<HashMap<String, (i64, String)>, Error> {
    let mut map = HashMap::new();

    if let Some(pid) = parent_id {
        // Parent dans la base actuelle
        let query = "SELECT m.file_path, m.asset_id, b.hash FROM manifest m JOIN store.blobs b ON m.blob_id = b.id WHERE m.commit_id = ?";
        let mut stmt = conn.prepare(query)?;
        stmt.bind((1, pid))?;
        while let Ok(State::Row) = stmt.next() {
            map.insert(
                stmt.read("file_path")?,
                (stmt.read("asset_id")?, stmt.read("hash")?),
            );
        }
    } else if !parent_hash.is_empty() {
        // Parent dans la base 'old' (Reconsolidation)
        let query_old = "
            SELECT m.file_path, m.asset_id, b.hash 
            FROM old.manifest m 
            JOIN store.blobs b ON m.blob_id = b.id 
            JOIN old.commits c ON m.commit_id = c.id
            WHERE c.hash = ?";
        if let Ok(mut stmt_old) = conn.prepare(query_old) {
            stmt_old.bind((1, parent_hash))?;
            while let Ok(State::Row) = stmt_old.next() {
                map.insert(
                    stmt_old.read("file_path")?,
                    (stmt_old.read("asset_id")?, stmt_old.read("hash")?),
                );
            }
        }
    }
    Ok(map)
}

fn ensure_blob_exists(conn: &Connection, hash: &str, path: &Path, size: u64) -> Result<i64, Error> {
    // 1. Vérifier si le blob existe déjà (DÉDUPLICATION)
    let check_query = "SELECT id FROM store.blobs WHERE hash = ?"; // Note le 'store.' !
    let mut stmt = conn.prepare(check_query)?;
    stmt.bind((1, hash))?;

    if let Ok(State::Row) = stmt.next() {
        return stmt.read("id");
    }

    // 2. Si non, on l'insère
    // Attention : lire tout le fichier en RAM pour l'insérer en BLOB peut être lourd.
    // Pour l'instant on fait simple :
    let content = std::fs::read(path).expect("failed to read file");
    let compressed_content = crate::db::compress(&content);
    let insert_query = "
        INSERT INTO store.blobs (hash, content, size) 
        VALUES (?, ?, ?) 
        RETURNING id";
    let mut stmt_ins = conn.prepare(insert_query)?;
    stmt_ins.bind((1, hash))?;
    stmt_ins.bind((2, &compressed_content[..]))?; // Bind byte array
    stmt_ins.bind((3, size as i64))?;

    stmt_ins.next()?;
    stmt_ins.read("id")
}

fn create_new_asset(conn: &Connection, _creator: &str) -> Result<i64, Error> {
    let new_uuid = Uuid::new_v4().to_string();
    let query = "INSERT INTO store.assets (uuid) VALUES (?) RETURNING id";
    let mut stmt = conn.prepare(query)?;
    stmt.bind((1, new_uuid.as_str()))?;
    stmt.next()?;
    stmt.read("id")
}

pub fn get_head_state(
    conn: &Connection,
    branch: &str,
) -> Result<HashMap<PathBuf, (String, i64)>, sqlite::Error> {
    let mut state_map = HashMap::new();
    let query_head = "SELECT head_commit_id FROM branches WHERE name = ?";
    let mut statement = conn.prepare(query_head)?;
    statement.bind((1, branch))?;

    let head_commit_id = if let Ok(State::Row) = statement.next() {
        statement.read::<i64, _>("head_commit_id")?
    } else {
        return Ok(state_map); // Pas de commit, repo vide
    };

    // 2. Récupérer le manifest de ce commit
    let query_manifest = "
        SELECT m.file_path, b.hash, m.asset_id 
        FROM manifest m
        JOIN store.blobs b ON m.blob_id = b.id
        WHERE m.commit_id = ?
    ";
    let mut statement = conn.prepare(query_manifest)?;
    statement.bind((1, head_commit_id))?;

    while let Ok(State::Row) = statement.next() {
        let path_str: String = statement.read("file_path")?;
        let hash: String = statement.read("hash")?;
        let asset_id: i64 = statement.read("asset_id")?;

        state_map.insert(PathBuf::from(path_str), (hash, asset_id));
    }
    Ok(state_map)
}

pub fn status(conn: &Connection, root_path: &str, branch: &str) -> Result<Vec<FileStatus>, Error> {
    let db_state = get_head_state(conn, branch).expect("failed to get db state");
    let mut changes = Vec::new();
    let mut files_on_disk: HashSet<PathBuf> = HashSet::new();
    let walk = ignore::WalkBuilder::new(root_path)
        .add_custom_ignore_filename("syl")
        .threads(4)
        .standard_filters(true)
        .build()
        .flatten()
        .collect::<Vec<DirEntry>>();

    for path in &walk {
        if path.path().components().any(|c| c.as_os_str() == ".lys") || path.path().is_dir() {
            continue;
        }

        let relative_path = path
            .path()
            .strip_prefix(root_path)
            .expect("failed to get relative path")
            .to_path_buf();
        files_on_disk.insert(relative_path.clone());

        let current_hash = match calculate_hash(path.path()) {
            Ok(h) => h,
            Err(_) => continue, // On ignore les fichiers illisibles (ou on log un warning)
        };
        // Comparaison
        match db_state.get(&relative_path) {
            Some((db_hash, asset_id)) => {
                if *db_hash != current_hash {
                    changes.push(FileStatus::Modified(relative_path, *asset_id));
                }
            }
            None => {
                // Le fichier n'est pas dans le manifest -> New
                changes.push(FileStatus::New(relative_path));
            }
        }
    }
    for (path, (_, asset_id)) in db_state {
        if !files_on_disk.contains(&path) {
            changes.push(FileStatus::Deleted(path, asset_id));
        }
    }
    if changes.is_empty() {
        ok("No changes detected. Working tree is clean.");
    } else {
        for change in &changes {
            ok_status(change);
        }
    }
    Ok(changes)
}

pub fn calculate_hash(path: &Path) -> IoResult<String> {
    let mut file = File::open(path)?;
    let mut hasher = blake3::Hasher::new();
    let mut buffer = [0; 1024]; // Buffer de lecture

    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(hex::encode(hasher.finalize().as_bytes()))
}

fn get_commit_id_by_hash(conn: &Connection, partial_hash: &str) -> Result<Option<i64>, Error> {
    // On cherche un hash qui COMMENCE par la chaîne donnée (LIKE 'abc%')
    let query = "SELECT id FROM commits WHERE hash LIKE ? || '%' LIMIT 1";
    let mut stmt = conn.prepare(query)?;
    stmt.bind((1, partial_hash))?;

    if let Ok(State::Row) = stmt.next() {
        Ok(Some(stmt.read("id")?))
    } else {
        Ok(None)
    }
}
