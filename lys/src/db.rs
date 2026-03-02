use chrono::{Datelike, Local};
use sqlite::{Connection, Error, State};
use std::fmt::Display;
use std::fs::create_dir_all;
use std::path::Path;
use std::path::PathBuf;
use uuid::Uuid;

use crate::utils::ok;

pub const LYS_INIT: &str = "CREATE TABLE IF NOT EXISTS tree_nodes (
        parent_tree_hash TEXT,
        name TEXT,
        hash TEXT,
        mode INTEGER,
        size INTEGER,
        nix_env_hash TEXT,
        PRIMARY KEY (parent_tree_hash, name)
    ) WITHOUT ROWID;
    CREATE INDEX IF NOT EXISTS idx_tree_nodes_parent ON tree_nodes(parent_tree_hash);
 CREATE TABLE IF NOT EXISTS todos (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    title TEXT NOT NULL,
    status TEXT DEFAULT 'TODO',
    assigned_to TEXT,
    due_date DATETIME,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP -- Ajouté pour le tri
);   
    -- ====================================================================
    -- PARTIE 1 : STOCKAGE ADRESSABLE (store.db)
    -- ====================================================================
    CREATE TABLE IF NOT EXISTS store.blobs (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        hash TEXT UNIQUE NOT NULL,      -- Hash Blake3
        content BLOB,                   -- Compressé en Zlib côté Rust
        size INTEGER NOT NULL,
        mime_type TEXT
    );

    CREATE TABLE IF NOT EXISTS store.assets (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        uuid TEXT UNIQUE NOT NULL,      -- Identité stable (UUID)
        created_at DATETIME DEFAULT CURRENT_TIMESTAMP
    );

    -- ====================================================================
    -- PARTIE 2 : INDEXATION HIÉRARCHIQUE (VFS Optimized)
    -- remplace le manifest à plat pour permettre le montage performant
    -- ====================================================================

    -- ====================================================================
    -- PARTIE 3 : HISTORIQUE ET ÉVOLUTION
    -- ====================================================================
    CREATE TABLE IF NOT EXISTS commits (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        hash TEXT UNIQUE NOT NULL,       -- Merkle Root
        parent_hash TEXT,
        tree_hash TEXT NOT NULL,         -- Hash du 'tree' racine du commit
        author TEXT NOT NULL,
        message TEXT NOT NULL,
        timestamp DATETIME DEFAULT CURRENT_TIMESTAMP,
        signature TEXT,
        nix_env_hash TEXT                -- Reproductibilité totale
    );
    CREATE INDEX IF NOT EXISTS idx_commits_timestamp ON commits(timestamp);
    CREATE INDEX IF NOT EXISTS idx_commits_hash ON commits(hash);

    -- Journal d'opérations (Style Jujutsu) pour le Undo/Redo
    CREATE TABLE IF NOT EXISTS operations_log (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        operation_type TEXT NOT NULL,    -- 'commit', 'checkout', 'reset'
        view_state JSON NOT NULL,        -- État complet des refs au moment T
        timestamp DATETIME DEFAULT CURRENT_TIMESTAMP
    );

    CREATE TABLE IF NOT EXISTS branches (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        name TEXT UNIQUE NOT NULL,
        head_commit_id INTEGER NOT NULL,
        FOREIGN KEY (head_commit_id) REFERENCES commits(id)
    );

    CREATE TABLE IF NOT EXISTS manifest (
        commit_id INTEGER NOT NULL,
        asset_id INTEGER NOT NULL,
        blob_id INTEGER NOT NULL,
        file_path TEXT NOT NULL
    );
    CREATE INDEX IF NOT EXISTS idx_manifest_commit ON manifest(commit_id);
    CREATE INDEX IF NOT EXISTS idx_manifest_path ON manifest(file_path);

    -- ====================================================================
    -- PARTIE 4 : OUTILS COLLABORATIFS ET SYSTÈME
    -- ====================================================================
    CREATE TABLE IF NOT EXISTS ephemeral_messages (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        sender TEXT NOT NULL,
        content TEXT NOT NULL,
        created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
        expires_at DATETIME NOT NULL
    );
    CREATE TABLE IF NOT EXISTS config (
        key TEXT PRIMARY KEY,
        value TEXT NOT NULL
    );

    INSERT OR IGNORE INTO config (key, value) VALUES ('current_branch', 'main');
";

#[derive(Default)]
pub struct CommitQuery {
    pub author: Option<String>,
    pub message: Option<String>,
    pub file: Option<String>,
    pub after: Option<String>,
    pub before: Option<String>,
    pub branch: Option<String>,
    pub tag: Option<String>,
    pub hash_prefix: Option<String>,
}

pub struct CommitQueryResult {
    pub id: i64,
    pub hash: String,
    pub author: String,
    pub message: String,
    pub timestamp: String,
}

pub fn list_branches(conn: &Connection) -> Vec<String> {
    let mut out = Vec::new();
    let mut stmt = match conn.prepare("SELECT name FROM branches ORDER BY name") {
        Ok(s) => s,
        Err(_) => return out,
    };
    while let Ok(State::Row) = stmt.next() {
        if let Ok(name) = stmt.read::<String, _>(0) {
            out.push(name);
        }
    }
    out
}

pub fn list_tags(conn: &Connection) -> Vec<String> {
    let mut out = Vec::new();
    if let Ok(mut stmt) = conn.prepare("SELECT name FROM tags ORDER BY name") {
        while let Ok(State::Row) = stmt.next() {
            if let Ok(name) = stmt.read::<String, _>(0) {
                out.push(name);
            }
        }
        if !out.is_empty() {
            return out;
        }
    }

    let mut stmt = match conn.prepare("SELECT key FROM config WHERE key LIKE 'tag_%' ORDER BY key")
    {
        Ok(s) => s,
        Err(_) => return out,
    };
    while let Ok(State::Row) = stmt.next() {
        if let Ok(key) = stmt.read::<String, _>(0) {
            if let Some(name) = key.strip_prefix("tag_") {
                out.push(name.to_string());
            }
        }
    }
    out
}

pub fn tag_hash(conn: &Connection, tag: &str) -> Option<String> {
    if let Ok(mut stmt) = conn
        .prepare("SELECT c.hash FROM tags t JOIN commits c ON t.commit_id = c.id WHERE t.name = ?")
    {
        if stmt.bind((1, tag)).is_ok() {
            if let Ok(State::Row) = stmt.next() {
                if let Ok(hash) = stmt.read::<String, _>(0) {
                    return Some(hash);
                }
            }
        }
    }

    let key = format!("tag_{tag}");
    let mut stmt = conn
        .prepare("SELECT value FROM config WHERE key = ?")
        .ok()?;
    stmt.bind((1, key.as_str())).ok()?;
    if let Ok(State::Row) = stmt.next() {
        stmt.read::<String, _>(0).ok()
    } else {
        None
    }
}

pub fn branch_head_id(conn: &Connection, branch: &str) -> Result<Option<i64>, Error> {
    let query =
        "SELECT c.id FROM branches b JOIN commits c ON b.head_commit_id = c.id WHERE b.name = ?";
    let mut stmt = conn.prepare(query)?;
    stmt.bind((1, branch))?;
    if let Ok(State::Row) = stmt.next() {
        Ok(Some(stmt.read::<i64, _>(0)?))
    } else {
        Ok(None)
    }
}

pub fn query_commits(
    conn: &Connection,
    query: &CommitQuery,
    page: usize,
    limit: usize,
) -> Result<(Vec<CommitQueryResult>, i64), Error> {
    let mut clauses: Vec<String> = Vec::new();
    let mut params: Vec<String> = Vec::new();
    let mut cte_sql = String::new();
    let mut cte_params: Vec<String> = Vec::new();
    let mut from_clause = String::from("commits");

    if let Some(branch) = query
        .branch
        .as_ref()
        .map(|v| v.trim())
        .filter(|v| !v.is_empty())
    {
        if let Some(head_id) = branch_head_id(conn, branch)? {
            cte_sql = String::from(
                "WITH RECURSIVE branch_commits(id, hash, parent_hash) AS ( \
                 SELECT id, hash, parent_hash FROM commits WHERE id = ? \
                 UNION ALL \
                 SELECT c.id, c.hash, c.parent_hash FROM commits c \
                 JOIN branch_commits bc ON c.hash = bc.parent_hash \
                 ) ",
            );
            cte_params.push(head_id.to_string());
            from_clause = String::from("commits JOIN branch_commits bc ON commits.id = bc.id");
        } else {
            return Ok((Vec::new(), 0));
        }
    }

    if let Some(tag) = query
        .tag
        .as_ref()
        .map(|v| v.trim())
        .filter(|v| !v.is_empty())
    {
        if let Some(hash) = tag_hash(conn, tag) {
            clauses.push("hash = ?".to_string());
            params.push(hash);
        } else {
            return Ok((Vec::new(), 0));
        }
    }

    if let Some(prefix) = query
        .hash_prefix
        .as_ref()
        .map(|v| v.trim())
        .filter(|v| !v.is_empty())
    {
        clauses.push("hash LIKE ?".to_string());
        params.push(format!("{prefix}%"));
    }

    if let Some(author) = query
        .author
        .as_ref()
        .map(|v| v.trim())
        .filter(|v| !v.is_empty())
    {
        clauses.push("author LIKE ? COLLATE NOCASE".to_string());
        params.push(format!("%{author}%"));
    }
    if let Some(message) = query
        .message
        .as_ref()
        .map(|v| v.trim())
        .filter(|v| !v.is_empty())
    {
        clauses.push("message LIKE ? COLLATE NOCASE".to_string());
        params.push(format!("%{message}%"));
    }
    if let Some(file) = query
        .file
        .as_ref()
        .map(|v| v.trim())
        .filter(|v| !v.is_empty())
    {
        clauses.push("EXISTS (SELECT 1 FROM manifest WHERE manifest.commit_id = commits.id AND file_path LIKE ? COLLATE NOCASE)".to_string());
        params.push(format!("%{file}%"));
    }
    if let Some(after) = query
        .after
        .as_ref()
        .map(|v| v.trim())
        .filter(|v| !v.is_empty())
    {
        clauses.push("timestamp >= ?".to_string());
        params.push(after.to_string());
    }
    if let Some(before) = query
        .before
        .as_ref()
        .map(|v| v.trim())
        .filter(|v| !v.is_empty())
    {
        clauses.push("timestamp <= ?".to_string());
        params.push(before.to_string());
    }

    let where_clause = if clauses.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", clauses.join(" AND "))
    };

    let count_sql = if cte_sql.is_empty() {
        format!("SELECT COUNT(*) FROM {from_clause}{where_clause}")
    } else {
        format!("{cte_sql}SELECT COUNT(*) FROM {from_clause}{where_clause}")
    };
    let mut count_stmt = conn.prepare(count_sql)?;
    let mut bind_idx = 1;
    for value in cte_params.iter() {
        count_stmt.bind((bind_idx, value.as_str()))?;
        bind_idx += 1;
    }
    for value in params.iter() {
        count_stmt.bind((bind_idx, value.as_str()))?;
        bind_idx += 1;
    }
    let total = if let Ok(State::Row) = count_stmt.next() {
        count_stmt.read::<i64, _>(0).unwrap_or(0)
    } else {
        0
    };

    let offset = (page.saturating_sub(1) * limit) as i64;
    let data_sql = if cte_sql.is_empty() {
        format!(
            "SELECT id, hash, author, message, timestamp FROM {from_clause}{where_clause} ORDER BY id DESC LIMIT ? OFFSET ?"
        )
    } else {
        format!(
            "{cte_sql}SELECT id, hash, author, message, timestamp FROM {from_clause}{where_clause} ORDER BY id DESC LIMIT ? OFFSET ?"
        )
    };
    let mut stmt = conn.prepare(data_sql)?;
    let mut bind_idx = 1;
    for value in cte_params.iter() {
        stmt.bind((bind_idx, value.as_str()))?;
        bind_idx += 1;
    }
    for value in params.iter() {
        stmt.bind((bind_idx, value.as_str()))?;
        bind_idx += 1;
    }
    stmt.bind((bind_idx, limit as i64))?;
    stmt.bind((bind_idx + 1, offset))?;

    let mut rows = Vec::new();
    while let Ok(State::Row) = stmt.next() {
        rows.push(CommitQueryResult {
            id: stmt.read::<i64, _>("id").unwrap_or(0),
            hash: stmt.read::<String, _>("hash").unwrap_or_default(),
            author: stmt.read::<String, _>("author").unwrap_or_default(),
            message: stmt.read::<String, _>("message").unwrap_or_default(),
            timestamp: stmt.read::<String, _>("timestamp").unwrap_or_default(),
        });
    }

    Ok((rows, total))
}

pub fn commit_files_preview(
    conn: &Connection,
    commit_id: i64,
    max_files: usize,
) -> Result<(Vec<String>, i64), Error> {
    let mut count_stmt = conn.prepare("SELECT COUNT(*) FROM manifest WHERE commit_id = ?")?;
    count_stmt.bind((1, commit_id))?;
    let total = if let Ok(State::Row) = count_stmt.next() {
        count_stmt.read::<i64, _>(0).unwrap_or(0)
    } else {
        0
    };

    let mut stmt = conn.prepare(
        "SELECT file_path FROM manifest WHERE commit_id = ? ORDER BY file_path ASC LIMIT ?",
    )?;
    stmt.bind((1, commit_id))?;
    stmt.bind((2, max_files as i64))?;

    let mut files = Vec::new();
    while let Ok(State::Row) = stmt.next() {
        files.push(stmt.read::<String, _>(0).unwrap_or_default());
    }

    Ok((files, total))
}

pub fn insert_tree_node(
    conn: &Connection,
    parent_hash: &str,
    name: &str,
    child_hash: &str,
    mode: i64,
    size: Option<i64>, // Utilise size ici
) -> Result<(), Error> {
    let query = "INSERT OR IGNORE INTO tree_nodes (parent_tree_hash, name, hash, mode, size) VALUES (?, ?, ?, ?, ?)";
    let mut stmt = conn.prepare(query)?;
    stmt.bind((1, parent_hash))?;
    stmt.bind((2, name))?;
    stmt.bind((3, child_hash))?;
    stmt.bind((4, mode))?;
    stmt.bind((5, size.unwrap_or(0)))?; // Bind de la taille réelle
    stmt.next()?;
    Ok(())
}

pub fn get_or_insert_blob_parallel(
    repo_root: &Path,
    hash: &str, // On ajoute le paramètre hash
    content: &[u8],
) -> Result<(), Error> {
    let db_path = repo_root.join(".lys/db/store.db");
    let conn = sqlite::open(db_path)?;
    conn.execute("PRAGMA journal_mode = WAL; PRAGMA busy_timeout = 5000;")?;

    let compressed = compress(content);
    let mut stmt =
        conn.prepare("INSERT OR IGNORE INTO store.blobs (hash, content, size) VALUES (?, ?, ?)")?;
    stmt.bind((1, hash))?; // On utilise le hash passé (le SHA1 de Git)
    stmt.bind((2, &compressed[..]))?;
    stmt.bind((3, content.len() as i64))?;
    stmt.next()?;
    Ok(())
}
// 2. Correction de l'insertion pour inclure la colonne 'size'
pub fn get_current_branch(conn: &Connection) -> Result<String, Error> {
    let query = "SELECT value FROM config WHERE key = 'current_branch'";
    let mut statement = conn.prepare(query)?;

    if let Ok(State::Row) = statement.next() {
        let branch_name: String = statement.read("value")?;
        Ok(branch_name)
    } else {
        // Fallback si la config est cassée, mais ça ne devrait pas arriver
        Ok(String::from("main"))
    }
}

pub enum Season {
    Winter,
    Spring,
    Summer,
    Autumn,
}

impl Season {
    pub fn current() -> Self {
        match Local::now().month() {
            1..=3 => Self::Winter,
            4..=6 => Self::Spring,
            7..=9 => Self::Summer,
            _ => Self::Autumn,
        }
    }

    pub fn before() -> Self {
        match Local::now().month() {
            1..=3 => Self::Autumn,
            4..=6 => Self::Winter,
            7..=9 => Self::Spring,
            _ => Self::Summer,
        }
    }
    // Calcule la saison précédente et l'année correspondante
    pub fn previous(&self, current_year: i32) -> (Self, i32) {
        match self {
            Self::Winter => (Self::Autumn, current_year - 1),
            Self::Spring => (Self::Winter, current_year),
            Self::Summer => (Self::Spring, current_year),
            Self::Autumn => (Self::Summer, current_year),
        }
    }
    pub fn after() -> Self {
        match Local::now().month() {
            1..=3 => Self::Spring,
            4..=6 => Self::Summer,
            7..=9 => Self::Autumn,
            _ => Self::Winter,
        }
    }
}

impl Display for Season {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Winter => write!(f, "winter"),
            Self::Spring => write!(f, "spring"),
            Self::Summer => write!(f, "summer"),
            Self::Autumn => write!(f, "autumn"),
        }
    }
}

// Dans src/db.rs

pub fn verify(conn: &Connection, deep: bool) -> Result<(), Box<dyn std::error::Error>> {
    ok("Starting repository integrity verification...");
    if deep {
        ok("Deep mode enabled: Recalculating all checksums...");
    }

    // On récupère les objets référencés par les commits (ignorant les dossiers)
    let query = "SELECT DISTINCT hash, name FROM tree_nodes WHERE mode != 16384 AND mode != 16400 AND mode != 49152";
    let mut stmt = conn.prepare(query)?;

    let mut missing = 0;
    let mut corrupted = 0;
    let mut total = 0;

    while let Ok(State::Row) = stmt.next() {
        total += 1;
        let expected_hash: String = stmt.read(0)?;
        let name: String = stmt.read(1)?;

        // On récupère le contenu pour vérifier l'existence
        let mut check_stmt =
            conn.prepare("SELECT content FROM store.blobs WHERE hash = ? LIMIT 1")?;
        check_stmt.bind((1, expected_hash.as_str()))?;

        if let Ok(State::Row) = check_stmt.next() {
            if deep {
                // VERIFICATION PROFONDE : On décompresse et on rehache
                let compressed: Vec<u8> = check_stmt.read(0)?;
                let decompressed = decompress(&compressed); // Ta fonction de décompression

                let actual_hash = blake3::hash(&decompressed).to_hex().to_string();

                if actual_hash != expected_hash {
                    corrupted += 1;
                    crate::utils::ko(&format!(
                        "CORRUPTED: Hash mismatch for '{name}' (Expected: {}, Actual: {})",
                        &expected_hash[..7],
                        &actual_hash[..7]
                    ));
                }
            }
        } else {
            missing += 1;
            crate::utils::ko(&format!(
                "MISSING: Data for '{name}' (hash: {})",
                &expected_hash[..7]
            ));
        }
    }

    // Rapport final
    if missing == 0 && corrupted == 0 {
        ok(&format!(
            "Verification success! All {total} objects are intact.",
        ));
    } else {
        crate::utils::ko(&format!(
            "Integrity report: {missing} missing, {corrupted} corrupted / {total} total",
        ));
    }
    Ok(())
}
pub fn connect_lys(root_path: &Path) -> Result<Connection, Error> {
    let db_dir = root_path.join(".lys/db");
    let store_path = db_dir.join("store.db");

    let s = Season::current();
    let current_year = Local::now().year();
    let history_dir = db_dir.join(format!("{current_year}/{s}"));
    let db_full_path = history_dir.join(format!("{s}.db"));

    if std::env::var("LYS_SHELL").is_err() {
        create_dir_all(&history_dir).expect("failed to create the .lys/db directory");
    }
    let conn = Connection::open(db_full_path.to_str().unwrap())?;
    conn.execute("PRAGMA temp_store = MEMORY;")?;
    conn.execute("PRAGMA cache_size = -64000;")?;
    conn.execute("PRAGMA busy_timeout = 5000;")?;
    conn.execute("PRAGMA mmap_size = 30000000000;")?;
    // --- CORRECTION : ATTACHER LE STORE EN PREMIER ---
    conn.execute(format!(
        "ATTACH DATABASE '{}' AS store;",
        store_path.display()
    ))?;

    if conn.execute("SELECT 1 FROM tree_nodes LIMIT 1;").is_err() {
        conn.execute(LYS_INIT)?;
    }
    // 2.5 RESET DES TODOS
    let _ = crate::todo::check_and_reset_todos(&conn);
    // 3. RECONSOLIDATION DYNAMIQUE
    if let Some(prev_db) = find_latest_db(&db_dir, &db_full_path) {
        let attach_query = format!("ATTACH DATABASE '{}' AS old;", prev_db.display());
        conn.execute(attach_query)?;
    }
    // Performances
    conn.execute("PRAGMA foreign_keys = ON;")?;
    conn.execute("PRAGMA journal_mode = WAL;")?;
    Ok(conn)
}

// Cherche récursivement la base .db la plus récente dans .lys/db
fn find_latest_db(db_root: &Path, current_path: &Path) -> Option<PathBuf> {
    let pattern = format!("{}/**/*.db", db_root.display());
    let mut dbs: Vec<PathBuf> = glob::glob(&pattern)
        .ok()?
        .filter_map(|res| res.ok())
        .filter(|path| path != current_path && !path.to_string_lossy().contains("store.db"))
        .collect();
    // On trie par date de modification (la plus récente d'abord)
    dbs.sort_by(|a, b| {
        let time_a = a.metadata().and_then(|m| m.modified()).ok();
        let time_b = b.metadata().and_then(|m| m.modified()).ok();
        time_b.cmp(&time_a)
    });
    dbs.into_iter().next()
}

// Crée une nouvelle identité de fichier (Asset)
pub fn create_asset(conn: &Connection) -> Result<i64, Error> {
    let new_uuid = Uuid::new_v4().to_string();
    let query = "INSERT INTO store.assets (uuid) VALUES (?)";
    let mut stmt = conn.prepare(query)?;
    stmt.bind((1, new_uuid.as_str()))?;
    stmt.next()?;

    // On retourne l'ID de la ligne insérée
    let id_query = "SELECT last_insert_rowid()";
    let mut stmt_id = conn.prepare(id_query)?;
    stmt_id.next()?;
    stmt_id.read(0)
}

// Lie un Commit + Asset + Blob dans le Manifeste
pub fn insert_manifest_entry(
    conn: &Connection,
    commit_id: i64,
    asset_id: i64,
    blob_id: i64,
    path: &str,
) -> Result<(), Error> {
    let query =
        "INSERT INTO manifest (commit_id, asset_id, blob_id, file_path) VALUES (?, ?, ?, ?)";
    let mut stmt = conn.prepare(query)?;
    stmt.bind((1, commit_id))?;
    stmt.bind((2, asset_id))?;
    stmt.bind((3, blob_id))?;
    stmt.bind((4, path))?;
    stmt.next()?;
    Ok(())
}
pub fn compress(data: &[u8]) -> Vec<u8> {
    zstd::encode_all(data, 0).expect("Failed to compress blob with zstd")
}

pub fn decompress(data: &[u8]) -> Vec<u8> {
    match zstd::decode_all(data) {
        Ok(decoded) => decoded,
        Err(_) => data.to_vec(),
    }
}
// Modifie ta fonction get_or_insert_blob pour compresser
pub fn get_or_insert_blob(conn: &Connection, content: &[u8]) -> Result<i64, Error> {
    // 1. On calcule le hash sur le contenu ORIGINAL (pour que le hash reste stable)
    let hash = blake3::hash(content).to_string();

    // 2. Vérif existence... (inchangé)
    let check_query = "SELECT id FROM store.blobs WHERE hash = ?";
    let mut stmt = conn.prepare(check_query)?;
    stmt.bind((1, hash.as_str()))?;
    if let Ok(State::Row) = stmt.next() {
        return stmt.read(0);
    }

    // 3. Compression avant insertion !
    let compressed_content = compress(content); // <--- LA MAGIE EST ICI

    let insert_query = "INSERT INTO store.blobs (hash, content, size) VALUES (?, ?, ?)";
    let mut stmt_ins = conn.prepare(insert_query)?;
    stmt_ins.bind((1, hash.as_str()))?;
    stmt_ins.bind((2, &compressed_content[..]))?; // On stocke le compressé
    stmt_ins.bind((3, content.len() as i64))?; // On garde la taille originale pour info
    stmt_ins.next()?;

    // ... retour ID (inchangé)
    let id_query = "SELECT last_insert_rowid()";
    let mut stmt_id = conn.prepare(id_query)?;
    stmt_id.next()?;
    stmt_id.read(0)
}

pub fn get_unique_contributors(conn: &Connection) -> Result<Vec<(String, i64)>, Error> {
    let query = "SELECT author, COUNT(*) as commit_count FROM commits GROUP BY author ORDER BY commit_count DESC";
    let mut stmt = conn.prepare(query)?;

    let mut contributors = Vec::new();
    while let Ok(State::Row) = stmt.next() {
        contributors.push((stmt.read::<String, _>(0)?, stmt.read::<i64, _>(1)?));
    }
    Ok(contributors)
}

// Dans src/db.rs
pub fn insert_blob_with_conn(conn: &Connection, hash: &str, content: &[u8]) -> Result<(), Error> {
    let compressed = compress(content); // Ta fonction de compression existante
    let insert = |sql: &str| -> Result<(), Error> {
        let mut stmt = conn.prepare(sql)?;
        stmt.bind((1, hash))?;
        stmt.bind((2, &compressed[..]))?;
        stmt.bind((3, content.len() as i64))?;
        stmt.next()?;
        Ok(())
    };

    match insert("INSERT OR IGNORE INTO store.blobs (hash, content, size) VALUES (?, ?, ?)") {
        Ok(()) => Ok(()),
        Err(e) => {
            let msg = e.to_string();
            if msg.contains("store.blobs") || msg.contains("unknown database store") {
                insert("INSERT OR IGNORE INTO blobs (hash, content, size) VALUES (?, ?, ?)")
            } else {
                Err(e)
            }
        }
    }
}

// À ajouter dans src/db.rs
pub fn prune_orphans(conn: &Connection) -> Result<usize, Error> {
    conn.execute("PRAGMA busy_timeout = 5000;")?;
    // 1. On compte combien on va supprimer pour informer l'utilisateur
    let count_query =
        "SELECT COUNT(*) FROM store.blobs WHERE hash NOT IN (SELECT DISTINCT hash FROM tree_nodes)";
    let mut stmt = conn.prepare(count_query)?;
    stmt.next()?;
    let count: i64 = stmt.read(0)?;

    if count > 0 {
        // 2. On effectue la suppression réelle
        conn.execute(
            "DELETE FROM store.blobs WHERE hash NOT IN (SELECT DISTINCT hash FROM tree_nodes)",
        )?;

        ok("Please wait");
        // 3. Optionnel : On libère l'espace disque sur le fichier .db (VACUUM)
        // Attention : VACUUM peut être lent sur de très grosses bases
        conn.execute("VACUUM;")?;
    }

    Ok(count as usize)
}

pub fn prune(conn: &Connection) -> Result<(), Box<dyn std::error::Error>> {
    ok("Starting prune");

    conn.execute("BEGIN TRANSACTION;")?;

    // 1. Supprimer les vieux commits (Plus vieux que 2 ans)
    // On utilise la fonction datetime d'SQLite pour cibler la colonne timestamp
    let del_commits = "DELETE FROM commits WHERE timestamp < datetime('now', '-2 years');";
    conn.execute(del_commits)?;

    // 2. Créer une table temporaire pour lister les hashes à CONSERVER
    // On utilise un Merkle Tree récursif pour trouver tous les descendants des commits restants
    conn.execute("CREATE TEMP TABLE live_hashes(hash TEXT PRIMARY KEY);")?;

    // A. On commence par les racines (tree_hash) des commits survivants
    conn.execute("INSERT OR IGNORE INTO live_hashes (hash) SELECT tree_hash FROM commits;")?;

    // B. Propagation récursive : on cherche tous les fichiers et sous-dossiers liés
    // On boucle jusqu'à ce que le nombre de hashes vivants n'évolue plus
    loop {
        let count_before = {
            let mut stmt = conn.prepare("SELECT COUNT(*) FROM live_hashes")?;
            stmt.next()?;
            stmt.read::<i64, _>(0)?
        };

        // On insère les enfants des dossiers déjà marqués comme vivants
        conn.execute(
            "
            INSERT OR IGNORE INTO live_hashes (hash)
            SELECT hash FROM tree_nodes
            WHERE parent_tree_hash IN (SELECT hash FROM live_hashes);
        ",
        )?;

        let count_after = {
            let mut stmt = conn.prepare("SELECT COUNT(*) FROM live_hashes")?;
            stmt.next()?;
            stmt.read::<i64, _>(0)?
        };

        if count_before == count_after {
            break;
        }
    }

    // 3. Nettoyage de la structure (tree_nodes)
    // On supprime les dossiers qui n'ont plus de parent vivant
    conn.execute(
        "DELETE FROM tree_nodes WHERE parent_tree_hash NOT IN (SELECT hash FROM live_hashes);",
    )?;

    // 4. Nettoyage des données binaires (store.blobs)
    let before_blobs = {
        let mut stmt = conn.prepare("SELECT COUNT(*) FROM store.blobs")?;
        stmt.next()?;
        stmt.read::<i64, _>(0)?
    };

    // On supprime les contenus qui ne sont plus référencés par aucun nœud vivant
    conn.execute("DELETE FROM store.blobs WHERE hash NOT IN (SELECT hash FROM live_hashes);")?;

    let after_blobs = {
        let mut stmt = conn.prepare("SELECT COUNT(*) FROM store.blobs")?;
        stmt.next()?;
        stmt.read::<i64, _>(0)?
    };

    conn.execute("COMMIT;")?;
    ok(format!("Blobs deleted : {}", before_blobs - after_blobs).as_str());

    // 5. Compression physique de la base de données
    ok("Optimisation");
    conn.execute("VACUUM;")?;
    Ok(())
}
