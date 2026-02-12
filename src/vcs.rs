use crate::commit::{FileChange, Log};
use crate::db::get_current_branch;
use crate::utils::commit_created;
use crate::utils::ko;
use crate::utils::ok;
use crate::utils::ok_merkle_hash;
use crate::utils::ok_status;
use crate::utils::ok_tag;
use anyhow::Error;
use glob::GlobError;
use glob::glob;
use ignore::DirEntry;
use indicatif::{ProgressBar, ProgressStyle};
use miniz_oxide::inflate;
#[cfg(target_os = "linux")]
use nix::mount::umount;
use nix::sys::wait::waitpid;
use nix::unistd::{ForkResult, execvp, fork};
use similar::{ChangeTag, TextDiff};
use sqlite::Connection;
use sqlite::State;
use std::collections::BTreeMap;
use std::collections::{HashMap, HashSet};
use std::ffi::CString;
use std::fmt::Debug;
use std::fs::File;
use std::fs::copy;
use std::fs::create_dir_all;
use std::fs::remove_dir_all;
use std::io::Error as IoError;
use std::io::Write;
use std::io::{Read, Result as IoResult};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub enum Node {
    File { hash: String, mode: u32 },
    Directory { children: BTreeMap<String, Node> },
}

#[derive(Debug)]
pub enum FileStatus {
    New(PathBuf),           // N'existe pas en base -> Nouvel Asset
    Modified(PathBuf, i64), // Existe mais hash différent -> Même Asset
    Deleted(PathBuf, i64),  // Existe en base mais plus sur disque
    Unchanged,
}

pub fn push_atoms(conn: &Connection, remote_url: &str) -> Result<(), Box<dyn std::error::Error>> {
    // 1. Lister les hashes que tu possèdes
    let mut stmt = conn.prepare("SELECT hash FROM blobs")?;

    // 2. Préparer une requête (ex: avec reqwest) pour envoyer chaque blob
    let client = reqwest::blocking::Client::new();

    while let Ok(State::Row) = stmt.next() {
        let hash: String = stmt.read(0)?;

        // On récupère le contenu brut (déjà compressé en zlib dans ta DB)
        // fetch_blob utilise déjà le chemin vers .lys/db/store.db
        let content = fetch_blob(Path::new("."), &hash)?;

        // 3. Le transfert : On envoie le hash et le binaire
        let res = client
            .post(format!("{remote_url}/upload/{hash}"))
            .body(content)
            .send()?;

        if res.status().is_success() {
            ok(format!("Atom {} sent", &hash[0..7]).as_str());
        }
    }
    Ok(())
}

pub fn sync(destination_path: &str) -> Result<(), IoError> {
    let files: Vec<Result<PathBuf, GlobError>> = glob("./.lys/db/*.db").expect("a").collect();
    let total_files = files.len();

    // Création de la barre
    let pb = ProgressBar::new(total_files as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template(
                "{spinner:.white} [{elapsed_precise}] [{bar:40.white}] {pos}/{len} ({eta}) {msg}",
            )
            .expect("style fail")
            .progress_chars("=>-"),
    );

    let x = Path::new(destination_path);
    create_dir_all(format!("{destination_path}/.lys/db"))?;
    if x.exists() {
        for file in files.iter().flatten() {
            let z = file.file_name().expect("failed to get filename");
            pb.set_message(format!("Syncing {}", z.to_string_lossy()));

            copy(
                file.as_path().to_str().expect("failed to get file path"),
                x.join(format!(".lys/db/{}", z.display())),
            )?;

            pb.inc(1); // On avance la barre
        }
    }
    pb.finish_with_message("Backup complete");
    Ok(())
}

// Dans src/vcs.rs - Version optimisée
pub fn fetch_blob_with_conn(
    conn: &Connection,
    hash: &str,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut stmt = conn.prepare("SELECT content FROM blobs WHERE hash = ?")?;
    stmt.bind((1, hash))?;

    if let Ok(State::Row) = stmt.next() {
        let compressed: Vec<u8> = stmt.read(0)?;
        let decompressed = inflate::decompress_to_vec_zlib(&compressed)
            .map_err(|e| format!("Erreur de décompression pour {hash}: {e:?}"))?;
        return Ok(decompressed);
    }
    Err(format!("Blob {hash} not found").into())
}

/// Va chercher un blob en utilisant un chemin absolu ou calculé
pub fn fetch_blob(repo_root: &Path, hash: &str) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    // Construction propre du chemin : repo_root + .lys/db/store.db
    let db_path = repo_root.join(".lys").join("db").join("store.db");

    // Vérification dxe survie : est-ce que le fichier existe vraiment ?
    if !db_path.exists() {
        return Err(format!("Fatal error no found the db at : {db_path:?}").into());
    }

    // On ouvre la connexion avec le chemin blindé
    let conn = sqlite::open(&db_path)?;

    conn.execute("PRAGMA busy_timeout = 5000;")?;
    // Petite optimisation pour la lecture seule
    conn.execute("PRAGMA query_only = ON;")?;

    let mut stmt = conn.prepare("SELECT content FROM blobs WHERE hash = ?")?;
    stmt.bind((1, hash))?;

    if let Ok(State::Row) = stmt.next() {
        let compressed: Vec<u8> = stmt.read(0)?;
        let decompressed =
            inflate::decompress_to_vec_zlib(&compressed).map_err(|e| format!("{hash}: {e:?}"))?;

        return Ok(decompressed);
    }

    Err(format!("Blob {hash} not found in the store {db_path:?}").into())
}

fn restore_tree(
    conn: &Connection,
    tree_hash: &str,
    current_path: &Path,
    repo_root: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    // On cherche tous les enfants de ce dossier/tree
    let mut stmt =
        conn.prepare("SELECT name, hash, mode FROM tree_nodes WHERE parent_tree_hash = ?")?;
    stmt.bind((1, tree_hash))?;

    let mut nodes = Vec::new();
    while let Ok(State::Row) = stmt.next() {
        nodes.push((
            stmt.read::<String, _>(0)?,
            stmt.read::<String, _>(1)?,
            stmt.read::<i64, _>(2)?,
        ));
    }

    for (name, hash, mode) in nodes {
        let path = current_path.join(&name);

        // 16384 est le mode Git pour un dossier (040000 en octal)
        if mode == 16384 || mode == 0o040000 {
            create_dir_all(&path)?;
            // Récursion : on va chercher les fichiers DANS ce dossier
            restore_tree(conn, &hash, &path, repo_root)?;
        } else {
            // C'est un fichier : on l'extrait de store.db
            if let Ok(content) = fetch_blob_with_conn(conn, &hash) {
                // Création du dossier parent au cas où
                if let Some(parent) = path.parent() {
                    create_dir_all(parent)?;
                }
                let mut f = File::create(&path)?;
                f.write_all(&content)?;
                f.sync_data()?;
                // Sur FreeBSD/Linux, on peut même remettre les droits d'exécution !
                #[cfg(unix)]
                {
                    use std::fs::Permissions;

                    f.set_permissions(Permissions::from_mode(0o755)).expect("");

                    if mode == 33261 {
                        // Exécutable
                        f.set_permissions(Permissions::from_mode(0o755))?;
                    }
                }
            }
        }
    }
    Ok(())
}
// Dans src/vcs.rs

#[cfg(target_os = "freebsd")]
pub fn doctor() -> Result<(), String> {
    use std::process::Command;

    // 1. Vérification du dossier .lys
    if Path::new(".lys").exists() {
        ok("Database .lys detected");
    } else {
        ko("Not a lys repository.");
    }

    // 2. Vérification de vfs.usermount (FreeBSD spécifique)
    let output = Command::new("sysctl")
        .arg("-n")
        .arg("vfs.usermount")
        .output()
        .map_err(|_| "failed to read sysctl")?;

    let usermount = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if usermount == "1" {
        ok("vfs.usermount eq 1 (User mount authorized).");
    } else {
        ko("vfs.usermount eq 0. run sudo sysctl vfs.usermount=1 !");
    }

    // 3. Vérification des permissions sur /tmp (pour le shell)
    if File::open("/tmp").expect("").metadata().is_ok() {
        ok("The /tmp dir is accessible for the ephemeral operations.");
    }

    // 4. Vérification du cache de montage
    let cache_path = Path::new(".lys/mounts");
    if !cache_path.exists() {
        ok("The cache will be created by 'lys mount'.");
    } else {
        ok("Cache ready to use.");
    }
    ok("The system ready");
    Ok(())
}

pub fn ls_tree(conn: &Connection, tree_hash: &str, prefix: &str) -> Result<Vec<String>, Error> {
    let mut lines = Vec::new();
    ls_tree_recursive(conn, tree_hash, prefix, &mut lines)?;
    Ok(lines)
}

fn ls_tree_recursive(
    conn: &Connection,
    tree_hash: &str,
    prefix: &str,
    lines: &mut Vec<String>,
) -> Result<(), Error> {
    // On récupère tous les enfants directs de ce hash de dossier
    let query =
        "SELECT name, hash, mode FROM tree_nodes WHERE parent_tree_hash = ? ORDER BY name ASC";
    let mut stmt = conn.prepare(query)?;
    stmt.bind((1, tree_hash))?;

    // On stocke les résultats pour gérer la récursion après l'affichage
    let mut entries = Vec::new();
    while let Ok(State::Row) = stmt.next() {
        entries.push((
            stmt.read::<String, _>("name")?,
            stmt.read::<String, _>("hash")?,
            stmt.read::<i64, _>("mode")?,
        ));
    }

    let count = entries.len();
    for (i, (name, hash, mode)) in entries.into_iter().enumerate() {
        let is_last = i == count - 1;
        let connector = if is_last { "└── " } else { "├── " };

        lines.push(format!(
            "{} [ {} ] {}{}",
            format_mode(mode),
            &hash[0..7],
            prefix,
            connector.to_string() + &name,
        ));

        // Si le hash possède lui-même des enfants dans tree_nodes, c'est un dossier
        if is_directory(conn, &hash)? {
            let new_prefix = if is_last {
                format!("{}    ", prefix)
            } else {
                format!("{}│   ", prefix)
            };
            ls_tree_recursive(conn, &hash, &new_prefix, lines)?;
        }
    }
    Ok(())
}

pub fn checkout_head(
    conn: &Connection,
    root_path: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let query = "SELECT tree_hash FROM commits ORDER BY id DESC LIMIT 1";
    let mut stmt = conn.prepare(query)?;

    if let Ok(State::Row) = stmt.next() {
        let tree_hash: String = stmt.read(0)?;
        // On passe root_path à restore_tree
        restore_tree(conn, &tree_hash, root_path, root_path)?;
    }
    Ok(())
}

fn get_manifest_map(
    conn: &Connection,
    commit_id: Option<i64>,
) -> Result<HashMap<String, (String, i64)>, Error> {
    let mut map = HashMap::new();
    if let Some(id) = commit_id {
        // On récupère le tree_hash du commit spécifique
        let query = "SELECT tree_hash FROM commits WHERE id = ?";
        let mut stmt = conn.prepare(query)?;
        stmt.bind((1, id))?;

        if let Ok(State::Row) = stmt.next() {
            let tree_hash: String = stmt.read(0)?;
            let mut path_map = HashMap::new();
            // On utilise ton flatten_tree pour obtenir l'état complet
            flatten_tree(conn, &tree_hash, PathBuf::new(), &mut path_map)?;
            // Conversion PathBuf -> String pour rester compatible avec la logique de checkout
            for (p, (h, a)) in path_map {
                map.insert(p.to_string_lossy().to_string(), (h, a));
            }
        }
    }
    Ok(map)
}

fn get_blob_bytes(conn: &Connection, branch: &str, path: &Path) -> Result<Option<Vec<u8>>, Error> {
    // 1. On récupère l'état complet du HEAD via l'arbre Merkle
    let state = get_head_state(conn, branch).expect("failed");
    // On nettoie le chemin pour la recherche dans la map
    let relative_path = path.strip_prefix("./").unwrap_or(path).to_path_buf();

    if let Some((hash, _)) = state.get(&relative_path) {
        // 2. Si trouvé, on récupère les octets via le hash
        return get_blob_bytes_by_hash(conn, hash);
    }
    Ok(None)
}

fn get_file_content_from_head(
    conn: &Connection,
    branch: &str,
    path: &Path,
) -> Result<String, Error> {
    match get_blob_bytes(conn, branch, path)? {
        Some(content) => match String::from_utf8(content) {
            Ok(s) => Ok(s),
            Err(_) => Ok(String::from("(Binary content)")),
        },
        None => Ok(String::new()),
    }
}
// Helper pour savoir si un hash est un dossier (présent en tant que parent)
fn is_directory(conn: &Connection, hash: &str) -> Result<bool, Error> {
    let query = "SELECT 1 FROM tree_nodes WHERE parent_tree_hash = ? LIMIT 1";
    let mut stmt = conn.prepare(query)?;
    stmt.bind((1, hash))?;
    Ok(matches!(stmt.next(), Ok(State::Row)))
}

pub fn format_mode(mode: i64) -> String {
    if mode == 16384 || mode == 0o040000 || mode == 0o755 {
        "d".to_string()
    } else {
        "f".to_string()
    }
}

#[cfg(target_os = "freebsd")]
use nix::mount::{MntFlags, unmount};

#[cfg(target_os = "freebsd")]
pub fn umount(path: &str) -> Result<(), String> {
    // On convertit le chemin pour l'appel système
    let p = std::path::Path::new(path);

    // Sur FreeBSD, on utilise unmount avec MntFlags
    unmount(p, MntFlags::empty()).map_err(|e| format!("umount of the path {path} failed : {e}"))?;

    ok(&format!("Unmounted: {path}"));
    Ok(())
}

pub fn spawn_lys_shell(conn: &Connection, reference: Option<&str>) -> Result<(), String> {
    let temp_mount = format!("/tmp/{}", uuid::Uuid::new_v4().simple());
    let mount_path = Path::new(&temp_mount);

    create_dir_all(mount_path).map_err(|e| e.to_string())?;
    if let Err(e) = mount_version(conn, &temp_mount, reference) {
        let _ = remove_dir_all(mount_path);
        return Err(format!("Mount error: {e}"));
    }

    // 2. Préparation du message d'accueil (Saison + Messages + TODOs)
    let season = crate::db::Season::current(); //
    let user = crate::commit::author(); //

    let (shell, s) = if cfg!(target_os = "linux") {
        (CString::new("bash").expect(""), "bash")
    } else {
        (CString::new("tcsh").expect(""), "tcsh")
    };
    ok(format!("Season: {season} User: {user} Shell: {s}").as_str());
    ok("Enter exit to quit");

    // 3. Gestion du processus Shell
    match unsafe { fork() } {
        Ok(ForkResult::Parent { child }) => {
            // Le parent attend que l'utilisateur quitte le shell
            waitpid(child, None).ok();
            println!();
            ok("Clean the shell");
            // 4. Nettoyage automatique (Lest et démontage)
            umount(temp_mount.as_str())
                .map_err(|e| println!("Error: {e}"))
                .ok();
            remove_dir_all(mount_path).ok();
            ok("Shell lys successfully cleaned.");
        }
        // Dans src/vcs.rs, dans ForkResult::Child
        Ok(ForkResult::Child) => {
            // On récupère le chemin absolu du projet actuel (Base Terre)
            let project_root = std::env::current_dir().expect("failed to get current dir");

            unsafe {
                std::env::set_var(
                    "LYS_PROJECT_ROOT",
                    project_root
                        .to_str()
                        .expect("failed to get project root path"),
                );
            }

            let args = [shell.clone()];
            // On change le répertoire de travail vers le montage
            std::env::set_current_dir(mount_path).ok();
            execvp(&shell, &args).map_err(|e| e.to_string())?;
        }
        Err(e) => return Err(anyhow::anyhow!("Fork failed: {e}").to_string()),
    }

    Ok(())
}

pub fn mount_version(
    conn: &Connection,
    target_path: &str,
    reference: Option<&str>,
) -> Result<(), Error> {
    let tree_hash = if let Some(r) = reference {
        // Recherche par hash partiel de commit
        let query = "SELECT tree_hash FROM commits WHERE hash LIKE ? || '%' LIMIT 1";
        let mut stmt = conn.prepare(query)?;
        stmt.bind((1, r))?;
        if let Ok(State::Row) = stmt.next() {
            stmt.read::<String, _>(0)?
        } else {
            return Err(anyhow::anyhow!("Commit not founded"));
        }
    } else {
        // Sinon HEAD de la branche actuelle
        let branch = get_current_branch(conn)?;
        let query = "SELECT c.tree_hash FROM branches b JOIN commits c ON b.head_commit_id = c.id WHERE b.name = ?";
        let mut stmt = conn.prepare(query)?;
        stmt.bind((1, branch.as_str()))?;
        if let Ok(State::Row) = stmt.next() {
            stmt.read::<String, _>(0)?
        } else {
            return Err(anyhow::anyhow!("Branch empty"));
        }
    };

    // 2. Préparation du cache interne (Identifié par le tree_hash pour déduplication)
    let cache_source = format!(".lys/mounts/{}", &tree_hash[0..12]);
    let cache_path = Path::new(&cache_source);

    if !cache_path.exists() {
        ok_merkle_hash(&tree_hash[0..7]);
        reconstruct_to_path(conn, &tree_hash, cache_path)?;
    }

    // 3. Appel au noyau (Linux/FreeBSD)
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
        let target = Path::new(target_path);
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
            message: Some(format!("nmount error: {e}")),
        })?;
    }
    ok(format!(
        "Version {} monted successfully on {}",
        &tree_hash[0..7],
        target_path
    )
    .as_str());
    Ok(())
}

fn reconstruct_to_path(
    conn: &Connection,
    tree_hash: &str,
    dest: &Path,
) -> Result<(), sqlite::Error> {
    // 1. On s'assure que le dossier de destination existe
    if !dest.exists() {
        create_dir_all(dest).unwrap();
    }

    // 2. On lance l'extraction récursive
    extract_tree_recursive(conn, tree_hash, dest)?;

    Ok(())
}

fn extract_tree_recursive(
    conn: &Connection,
    tree_hash: &str,
    current_dest: &Path,
) -> Result<(), sqlite::Error> {
    // On récupère les enfants et on joint avec store.blobs pour avoir le contenu
    let query = "
        SELECT tn.name, tn.hash, tn.mode, b.content 
        FROM tree_nodes tn
        LEFT JOIN store.blobs b ON tn.hash = b.hash
        WHERE tn.parent_tree_hash = ?";

    let mut stmt = conn.prepare(query)?;
    stmt.bind((1, tree_hash))?;

    let mut entries = Vec::new();
    while let Ok(State::Row) = stmt.next() {
        entries.push((
            stmt.read::<String, _>("name")?,
            stmt.read::<String, _>("hash")?,
            stmt.read::<i64, _>("mode")?,
            stmt.read::<Option<Vec<u8>>, _>("content")?,
        ));
    }

    for (name, hash, mode, content) in entries {
        let full_path = current_dest.join(name);

        if mode == 0o755 {
            // C'est un dossier
            create_dir_all(&full_path).unwrap();
            extract_tree_recursive(conn, &hash, &full_path)?;
        } else if let Some(raw_data) = content {
            // C'est un fichier : on décompresse et on écrit
            let decoded = crate::db::decompress(&raw_data);
            let mut f = File::create(full_path).expect("");
            f.write_all(&decoded).expect("a");
            f.sync_all().expect("a");
        }
    }
    Ok(())
}

pub fn commit_manual(
    conn: &Connection,
    message: &str,
    author: &str,
    timestamp: i64,
    tree_hash: &str, // Ajout du paramètre
) -> Result<i64, sqlite::Error> {
    let query_last = "SELECT hash FROM commits ORDER BY id DESC LIMIT 1";
    let mut stmt_last = conn.prepare(query_last)?;
    let parent_hash = if let Ok(State::Row) = stmt_last.next() {
        stmt_last.read::<String, _>(0)?
    } else {
        String::from("")
    };

    let commit_data = format!("{parent_hash}{author}{message}{timestamp}{tree_hash}");
    let lys_hash = blake3::hash(commit_data.as_bytes()).to_hex().to_string();

    // AJOUT DE tree_hash DANS LA REQUÊTE
    let query = "INSERT INTO commits (hash, parent_hash, tree_hash, author, message, timestamp) 
                 VALUES (?, ?, ?, ?, ?, datetime(?, 'unixepoch'))";
    let mut stmt = conn.prepare(query)?;
    stmt.bind((1, lys_hash.as_str()))?;
    stmt.bind((
        2,
        if parent_hash.is_empty() {
            None
        } else {
            Some(parent_hash.as_str())
        },
    ))?;
    stmt.bind((3, tree_hash))?; // Bind de la valeur
    stmt.bind((4, author))?;
    stmt.bind((5, message))?;
    stmt.bind((6, timestamp))?;
    stmt.next()?;

    let id_query = "SELECT last_insert_rowid()";
    let mut stmt_id = conn.prepare(id_query)?;
    stmt_id.next()?;
    stmt_id.read(0)
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

pub fn hotfix_start(conn: &Connection, name: &str) -> Result<(), Error> {
    let branch_name = format!("hotfix/{name}");
    let source_branch = "main"; // CONTRAINTE : Un hotfix part toujours de la prod

    // 1. On vérifie qu'on part bien de 'main' pour avoir la base saine
    let (main_id, _) = get_branch_head_info(conn, source_branch)?;
    if main_id.is_none() {
        return Err(anyhow::anyhow!("No main branches has been founded").into());
    }

    // 2. On crée la branche manuellement (sans utiliser create_branch qui utilise HEAD)
    let query = "INSERT INTO branches (name, head_commit_id) VALUES (?, ?)";
    let mut stmt = conn.prepare(query)?;
    stmt.bind((1, branch_name.as_str()))?;
    stmt.bind((2, main_id.unwrap()))?;

    match stmt.next() {
        Ok(_) => {
            // 3. On bascule dessus
            checkout(conn, &branch_name)?;

            ok(&format!(
                "Hotfix started: Switched to '{branch_name}' from 'main'"
            ));
            Ok(())
        } // Création OK
        Err(_) => Err(anyhow::anyhow!("hotfix already exist")),
    }
}

pub fn hotfix_finish(conn: &Connection, name: &str) -> Result<(), Error> {
    // C'est la même logique que feature_finish, mais sémantiquement distinct
    let hotfix_branch = format!("hotfix/{name}");
    let target_branch = "main";

    let (hf_head_id, _) = get_branch_head_info(conn, &hotfix_branch)?;
    if hf_head_id.is_none() {
        return Err(anyhow::anyhow!("hotfix not exist"));
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
        return Err(anyhow::anyhow!("main branch not exist"));
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
    let current_dir = std::env::current_dir().expect("failed to get current dir");
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
        return Err(anyhow::anyhow!(
            "Reference '{target_ref}' (branch or commit) not found."
        ));
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
                create_dir_all(parent).expect("failed to create directory");
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
    let current_dir = std::env::current_dir().expect("failed to get current dir");
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

pub fn count_lines(content: &[u8]) -> usize {
    match String::from_utf8(content.to_vec()) {
        Ok(s) => {
            if s.is_empty() {
                0
            } else {
                s.lines().count()
            }
        }
        Err(_) => 0,
    }
}

pub fn count_line_changes(old: &[u8], new: &[u8]) -> (usize, usize) {
    let old_s = String::from_utf8(old.to_vec()).unwrap_or_default();
    let new_s = String::from_utf8(new.to_vec()).unwrap_or_default();
    let diff = TextDiff::from_lines(&old_s, &new_s);
    let mut added = 0usize;
    let mut deleted = 0usize;
    for change in diff.iter_all_changes() {
        match change.tag() {
            ChangeTag::Insert => added += 1,
            ChangeTag::Delete => deleted += 1,
            _ => {}
        }
    }
    (added, deleted)
}

pub fn log(conn: &Connection, page: usize, per_page: usize) -> Result<(), sqlite::Error> {
    // Calcul de l'offset (Page 1 = Offset 0)
    let offset = (page - 1) * per_page;

    // Get total number of commits
    let mut count_stmt = conn.prepare("SELECT COUNT(*) FROM commits")?;
    let total_commits: i64 = if let Ok(State::Row) = count_stmt.next() {
        count_stmt.read(0)?
    } else {
        0
    };
    let total_pages = (total_commits as f64 / per_page as f64).ceil() as usize;

    // Requête avec LIMIT et OFFSET (inclut tree_hash pour construire l'arborescence)
    let query = "
        SELECT hash, author, message, timestamp, tree_hash 
        FROM commits 
        ORDER BY timestamp DESC 
        LIMIT ? OFFSET ?";

    let mut stmt = conn.prepare(query)?;
    stmt.bind((1, per_page as i64))?;
    stmt.bind((2, offset as i64))?;

    let mut rendered = Vec::new();
    let mut idx_in_page = 0usize;
    while let Ok(State::Row) = stmt.next() {
        // On tronque le hash pour l'affichage (7 premiers chars)
        let full_hash: String = stmt.read(0)?;
        let short_hash = if full_hash.len() > 7 {
            full_hash[0..7].to_string()
        } else {
            full_hash
        };

        let tree_hash: String = stmt.read(4)?;
        // Construit la liste des fichiers à partir du tree courant
        let mut new_state: HashMap<PathBuf, (String, i64)> = HashMap::new();
        flatten_tree(conn, &tree_hash, PathBuf::new(), &mut new_state)?;

        // Récupère le tree du parent (commit suivant dans l'ordre DESC par timestamp)
        let global_offset = offset + idx_in_page + 1; // +1 pour le parent suivant
        let mut parent_tree: Option<String> = None;
        if let Ok(mut parent_stmt) =
            conn.prepare("SELECT tree_hash FROM commits ORDER BY timestamp DESC LIMIT 1 OFFSET ?")
        {
            parent_stmt.bind((1, global_offset as i64))?;
            if let Ok(State::Row) = parent_stmt.next() {
                parent_tree = Some(parent_stmt.read(0)?);
            }
        }
        let mut old_state: HashMap<PathBuf, (String, i64)> = HashMap::new();
        if let Some(pth) = parent_tree.as_deref() {
            flatten_tree(conn, pth, PathBuf::new(), &mut old_state)?;
        }

        // Calcule les changements
        let mut changes: Vec<(String, FileChange)> = Vec::new();

        // Ajouts et modifications
        for (path, (new_hash, new_mode)) in &new_state {
            if let Some((old_hash, _)) = old_state.get(path) {
                if old_hash != new_hash {
                    // Modified
                    let old_bytes = get_blob_bytes_by_hash(conn, old_hash)
                        .ok()
                        .flatten()
                        .unwrap_or_default();
                    let new_bytes = get_blob_bytes_by_hash(conn, new_hash)
                        .ok()
                        .flatten()
                        .unwrap_or_default();
                    let (a, d) = count_line_changes(&old_bytes, &new_bytes);
                    changes.push((
                        path.to_string_lossy().to_string(),
                        FileChange::Modified {
                            added: a,
                            deleted: d,
                            mode: Some(*new_mode),
                        },
                    ));
                } // else unchanged -> ignore
            } else {
                // Added
                let nb = if let Some(bytes) = get_blob_bytes_by_hash(conn, new_hash).ok().flatten()
                {
                    count_lines(&bytes)
                } else {
                    0
                };
                changes.push((
                    path.to_string_lossy().to_string(),
                    FileChange::Added {
                        added: nb,
                        mode: Some(*new_mode),
                    },
                ));
            }
        }
        // Suppressions
        for (path, (old_hash, old_mode)) in &old_state {
            if !new_state.contains_key(path) {
                let nb = if let Some(bytes) = get_blob_bytes_by_hash(conn, old_hash).ok().flatten()
                {
                    count_lines(&bytes)
                } else {
                    0
                };
                changes.push((
                    path.to_string_lossy().to_string(),
                    FileChange::Deleted {
                        deleted: nb,
                        mode: Some(*old_mode),
                    },
                ));
            }
        }
        // Trie pour affichage stable
        changes.sort_by(|a, b| a.0.cmp(&b.0));

        let log = Log {
            author: stmt.read(1)?,
            at: stmt.read(3)?,
            message: stmt.read(2)?,
            signature: short_hash,
            changes,
        };
        rendered.push(log.to_string());
        idx_in_page += 1;
    }

    if rendered.is_empty() {
        if page == 1 {
            ok("please commit first");
        } else {
            ok(format!("No commits on {page} page.").as_str());
        }
    } else {
        if let Some(mut child) = start_pager() {
            if let Some(mut stdin) = child.stdin.take() {
                let output = rendered.join("\n");
                let _ = stdin.write_all(output.as_bytes());
                // Drop stdin to close it, so pager knows we're done
                drop(stdin);
                let _ = child.wait();
            } else {
                println!("{}", rendered.join("\n"));
            }
        } else {
            println!("{}", rendered.join("\n"));
        }

        let x = rendered.len();
        println!();
        let mut footer = format!("Page {page}/{total_pages} ({x}/{per_page} commits).");
        if page < total_pages {
            footer.push_str(&format!(" Next: --page {}", page + 1));
        }
        if page > 1 {
            footer.push_str(" Prev: --page 1");
        }
        if total_pages > 1 && page != total_pages {
            footer.push_str(&format!(" Last: --page {total_pages}"));
        }
        ok(footer.as_str());
        println!("\n");
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

fn insert_into_tree(root: &mut Node, path: &Path, hash: String, mode: u32) {
    let mut current = root;

    // On parcourt chaque composant du chemin (ex: ["src", "ui", "main.rs"])
    for component in path.components() {
        let name = component.as_os_str().to_string_lossy().to_string();

        // On descend dans l'arbre. Si le dossier n'existe pas, on le crée.
        if let Node::Directory { children } = current {
            current = children.entry(name).or_insert_with(|| Node::Directory {
                children: BTreeMap::new(),
            });
        }
    }

    // Une fois arrivé au bout du chemin, on remplace le nœud par le fichier réel
    *current = Node::File { hash, mode };
}

fn store_tree_recursive(
    conn: &Connection,
    _name: &str,
    node: &Node,
) -> Result<String, sqlite::Error> {
    match node {
        // Si c'est un fichier, on retourne juste son hash (déjà calculé)
        Node::File { hash, .. } => Ok(hash.clone()),

        // Si c'est un dossier, on doit traiter ses enfants
        Node::Directory { children } => {
            let mut hasher = blake3::Hasher::new();
            let mut children_data = Vec::new();

            for (name, child_node) in children {
                // Appel récursif pour obtenir le hash de l'enfant
                let child_hash = store_tree_recursive(conn, name, child_node)?;

                let mode = match child_node {
                    Node::File { mode, .. } => *mode,
                    Node::Directory { .. } => 0o755, // Mode par défaut pour les répertoires
                };

                // On nourrit le hash du dossier avec les données de l'enfant (Nom + Hash)
                hasher.update(name.as_bytes());
                hasher.update(child_hash.as_bytes());

                children_data.push((name, child_hash, mode));
            }

            // Le hash final du dossier est le résultat de la combinaison de ses enfants
            let dir_hash = hasher.finalize().to_hex().to_string();

            // On enregistre chaque enfant dans la table tree_nodes
            // parent_tree_hash est le hash du dossier que nous venons de calculer
            for (name, hash, mode) in children_data {
                crate::db::insert_tree_node(
                    conn,
                    &dir_hash,
                    name,
                    &hash,
                    mode as i64,
                    None, // On pourra passer l'env Nix ici plus tard
                )?;
            }
            Ok(dir_hash)
        }
    }
}

pub fn commit(conn: &Connection, message: &str, author: &str) -> Result<(), Error> {
    // 1. On scanne et on construit l'arbre en mémoire (Bottom-up)
    let mut root_tree = Node::Directory {
        children: BTreeMap::new(),
    };
    let walk = ignore::WalkBuilder::new(".")
        .threads(4)
        .add_custom_ignore_filename("syl")
        .standard_filters(true)
        .build();

    for result in walk.flatten() {
        let path = result.path();
        if path.is_dir() || path.components().any(|c| c.as_os_str() == ".lys") {
            continue;
        }

        let relative = path.strip_prefix("./").unwrap_or(path);
        let content = std::fs::read(path).expect("failed to read file");
        let content_hash = blake3::hash(&content).to_hex().to_string();
        let metadata = std::fs::metadata(path).expect("failed to get metadata");

        // On insère le blob dans la base de données
        crate::db::insert_blob_with_conn(conn, &content_hash, &content)
            .expect("failed to insert blob");

        // Insertion du fichier dans notre structure d'arbre en mémoire
        insert_into_tree(
            &mut root_tree,
            relative,
            content_hash,
            metadata.permissions().mode(),
        );
    }

    // 2. On calcule les hashes de chaque dossier et on insère dans SQLite
    // Le hash du dossier racine (root) sera notre tree_hash pour le commit
    conn.execute("BEGIN TRANSACTION;")?;
    let root_hash = store_tree_recursive(conn, "ROOT", &root_tree)?;

    // 3. Création du commit avec le lien vers l'arbre racine
    let timestamp = chrono::Utc::now().to_rfc3339();
    let commit_hash = blake3::hash(format!("{root_hash}{author}{message}").as_bytes())
        .to_hex()
        .to_string();

    let query_commit =
        "INSERT INTO commits (hash, tree_hash, author, message, timestamp) VALUES (?, ?, ?, ?, ?)";
    let mut stmt = conn.prepare(query_commit)?;
    stmt.bind((1, commit_hash.as_str()))?;
    stmt.bind((2, root_hash.as_str()))?; // Lien crucial vers tree_nodes
    stmt.bind((3, author))?;
    stmt.bind((4, message))?;
    stmt.bind((5, timestamp.as_str()))?;
    stmt.next()?;

    // 4. On enregistre l'opération dans l'OpLog pour le Undo
    let log_query = "INSERT INTO operations_log (operation_type, view_state) VALUES ('commit', ?)";
    let mut log_stmt = conn.prepare(log_query)?;
    log_stmt.bind((1, format!("{{\"head\": \"{commit_hash}\"}}").as_str()))?;
    log_stmt.next()?;

    let id_query = "SELECT last_insert_rowid()";
    let mut stmt_id = conn.prepare(id_query)?;
    stmt_id.next()?;
    let commit_id: i64 = stmt_id.read(0)?;

    // On récupère la branche actuelle et on met à jour son pointeur HEAD
    let branch = get_current_branch(conn)?;
    let update_branch = "INSERT INTO branches (name, head_commit_id) VALUES (?, ?) 
                         ON CONFLICT(name) DO UPDATE SET head_commit_id = excluded.head_commit_id";
    let mut stmt_br = conn.prepare(update_branch)?;
    stmt_br.bind((1, branch.as_str()))?;
    stmt_br.bind((2, commit_id))?;
    stmt_br.next()?;

    conn.execute("COMMIT;")?;
    commit_created(&commit_hash[0..7]);
    Ok(())
}

pub fn get_head_state(
    conn: &Connection,
    branch: &str,
) -> Result<HashMap<PathBuf, (String, i64)>, sqlite::Error> {
    let mut state_map = HashMap::new();

    // On va chercher le tree_hash du dernier commit de la branche
    let query = "
        SELECT c.tree_hash 
        FROM branches b 
        JOIN commits c ON b.head_commit_id = c.id 
        WHERE b.name = ?";
    let mut stmt = conn.prepare(query)?;
    stmt.bind((1, branch))?;

    if let Ok(State::Row) = stmt.next() {
        let root_hash: String = stmt.read(0)?;
        // On "aplatit" l'arbre Merkle pour obtenir une liste de fichiers utilisable
        flatten_tree(conn, &root_hash, PathBuf::new(), &mut state_map)?;
    }

    Ok(state_map)
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

pub fn flatten_tree(
    conn: &Connection,
    tree_hash: &str,
    current_path: PathBuf,
    state: &mut HashMap<PathBuf, (String, i64)>,
) -> Result<(), sqlite::Error> {
    let query = "SELECT name, hash, mode FROM tree_nodes WHERE parent_tree_hash = ?";
    let mut stmt = conn.prepare(query)?;
    stmt.bind((1, tree_hash))?;

    let mut entries = Vec::new();
    while let Ok(State::Row) = stmt.next() {
        entries.push((
            stmt.read::<String, _>("name")?,
            stmt.read::<String, _>("hash")?,
            stmt.read::<i64, _>("mode")?,
        ));
    }

    for (name, hash, mode) in entries {
        let path = current_path.join(name);
        if mode == 0o755 || mode == 0o040000 || mode == 16384 {
            // C'est un répertoire
            flatten_tree(conn, &hash, path, state)?;
        } else {
            // On stocke le fichier avec son hash et son mode
            state.insert(path, (hash, mode));
        }
    }
    Ok(())
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

pub fn start_pager() -> Option<std::process::Child> {
    if !std::io::IsTerminal::is_terminal(&std::io::stdout()) {
        return None;
    }

    let pager_cmd = std::env::var("PAGER").unwrap_or_else(|_| "less".to_string());
    let mut cmd = std::process::Command::new(&pager_cmd);

    if pager_cmd == "less" {
        cmd.arg("-F").arg("-X").arg("-R");
    }

    cmd.stdin(std::process::Stdio::piped()).spawn().ok()
}

pub fn is_file_in_state(path: &Path, tree_hash: &str, conn: &Connection) -> bool {
    let mut state = HashMap::new();
    if flatten_tree(conn, tree_hash, PathBuf::new(), &mut state).is_ok() {
        state.contains_key(path)
    } else {
        false
    }
}

pub fn get_commit_id_by_hash(conn: &Connection, partial_hash: &str) -> Result<Option<i64>, Error> {
    // On cherche un hash qui COMMENCE par la chaîne donnée (LIKE 'abc%')
    let query = "SELECT id FROM commits WHERE hash LIKE ? || '%' LIMIT 1";
    let mut stmt = conn.prepare(query)?;
    stmt.bind((1, partial_hash))?;

    if let Ok(State::Row) = stmt.next() {
        Ok(stmt.read("id")?)
    } else {
        Ok(None)
    }
}
