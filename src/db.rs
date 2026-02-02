use chrono::Datelike;
use flate2::Compression;
use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;
use sqlite::{Connection, Error, State};
use std::fs::create_dir_all;
use std::io::prelude::*;
use std::path::Path;
use uuid::Uuid;

pub const SILEX_INIT: &str = "
    -- ====================================================================
    -- PARTIE STOCKAGE (store.db) - Données lourdes et immuables
    -- ====================================================================

    CREATE TABLE IF NOT EXISTS store.blobs (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        hash TEXT UNIQUE NOT NULL,      -- Hash Blake3 du contenu (Déduplication)
        content BLOB,                   -- Le contenu réel
        size INTEGER NOT NULL,
        mime_type TEXT                  -- Pour future UI / Stats
    );

    CREATE TABLE IF NOT EXISTS store.assets (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        uuid TEXT UNIQUE NOT NULL,      -- Identité stable du fichier (UUID)
        created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
        creator_id INTEGER              -- ID de l'auteur original (optionnel)
    );

    -- ====================================================================
    -- PARTIE HISTORIQUE (history_YYYY.db) - Métadonnées légères
    -- ====================================================================

    CREATE TABLE IF NOT EXISTS commits (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        hash TEXT UNIQUE NOT NULL,       -- Hash (Merkle) : métadonnées + parent
        parent_hash TEXT,                -- NULL si commit initial
        author TEXT NOT NULL,
        message TEXT NOT NULL,
        timestamp DATETIME DEFAULT CURRENT_TIMESTAMP,
        signature TEXT,
        FOREIGN KEY(parent_hash) REFERENCES commits(hash)
    );

    CREATE TABLE IF NOT EXISTS manifest (
        commit_id INTEGER NOT NULL,
        asset_id INTEGER NOT NULL,       -- Réfère à store.assets(id) (Géré par Rust)
        blob_id INTEGER NOT NULL,        -- Réfère à store.blobs(id) (Géré par Rust)

        file_path TEXT NOT NULL,         -- Le chemin à cet instant T
        permissions INTEGER DEFAULT 644,

        PRIMARY KEY (commit_id, asset_id),
        FOREIGN KEY (commit_id) REFERENCES commits(id)
        -- NOTE: Pas de FK vers store.* car SQLite ne supporte pas les FK inter-bases
    );

    CREATE TABLE IF NOT EXISTS branches (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        name TEXT UNIQUE NOT NULL,
        head_commit_id INTEGER NOT NULL,
        FOREIGN KEY (head_commit_id) REFERENCES commits(id)
    );

    CREATE TABLE IF NOT EXISTS tags (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        name TEXT UNIQUE NOT NULL,
        commit_id INTEGER NOT NULL,
        description TEXT,
        created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
        FOREIGN KEY (commit_id) REFERENCES commits(id)
    );

    CREATE TABLE IF NOT EXISTS config (
        key TEXT PRIMARY KEY,
        value TEXT NOT NULL
    );
-- ====================================================================
-- PARTIE 1 : MESSAGERIE ÉPHÉMÈRE (Autodestruction à 20h)
-- ====================================================================
CREATE TABLE IF NOT EXISTS ephemeral_messages (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    sender TEXT NOT NULL,
    content TEXT NOT NULL,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    -- La date d'expiration est calculée à l'insertion par l'app (prochain 20h)
    expires_at DATETIME NOT NULL, 
    is_read BOOLEAN DEFAULT 0
);

-- Index pour accélérer le nettoyage automatique
CREATE INDEX IF NOT EXISTS idx_messages_expire ON ephemeral_messages(expires_at);

-- ====================================================================
-- PARTIE 2 : TODO LIST INTÉGRÉE
-- ====================================================================
CREATE TABLE IF NOT EXISTS todos (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    title TEXT NOT NULL,
    description TEXT,
    status TEXT DEFAULT 'TODO', -- TODO, IN_PROGRESS, DONE
    assigned_to TEXT,           -- Peut être lié à un auteur de commit
    due_date DATETIME,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

-- Trigger pour mettre à jour updated_at automatiquement
CREATE TRIGGER IF NOT EXISTS update_todos_timestamp 
AFTER UPDATE ON todos
BEGIN
    UPDATE todos SET updated_at = CURRENT_TIMESTAMP WHERE id = OLD.id;
END;

-- ====================================================================
-- PARTIE 3 : STATS DE CO-OCCURRENCE (Fichiers modifiés ensemble)
-- ====================================================================
-- Cette table sert de cache pour éviter de scanner tous les commits à chaque fois
CREATE TABLE IF NOT EXISTS file_correlations (
    file_a TEXT NOT NULL,
    file_b TEXT NOT NULL,
    frequency INTEGER DEFAULT 1,
    last_seen_commit_id INTEGER,
    -- On force l'ordre alphabétique (file_a < file_b) pour éviter les doublons (A,B) et (B,A)
    PRIMARY KEY (file_a, file_b)
);

-- ====================================================================
-- PARTIE 4 : LOGS (Style Logstash)
-- ====================================================================
CREATE TABLE IF NOT EXISTS system_logs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    timestamp DATETIME DEFAULT CURRENT_TIMESTAMP,
    level TEXT NOT NULL,
    component TEXT NOT NULL,
    message TEXT NOT NULL,
    metadata JSON,
    trace_id TEXT
);        
-- ====================================================================
-- PARTIE 5 : CLASSEMENT DES CONTRIBUTEURS
-- ====================================================================
CREATE TABLE IF NOT EXISTS contributor_stats (
    author_name TEXT PRIMARY KEY,
    total_commits INTEGER DEFAULT 0,
    first_commit_at DATETIME,
    last_commit_at DATETIME,
    files_touched_count INTEGER DEFAULT 0,
    rank_score REAL DEFAULT 0.0 
);
    -- Initialisation par défaut (ignore si existe déjà)
    INSERT OR IGNORE INTO config (key, value) VALUES ('current_branch', 'main');
";

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

pub fn connect_silex(root_path: &Path) -> Result<Connection, sqlite::Error> {
    let db_dir = root_path.join(".silex/db");
    let store_path = db_dir.join("store.db");

    // 1. Calculer l'année en cours pour l'historique
    let current_year = chrono::Local::now().year();
    let history_path = db_dir.join(format!("history_{}.db", current_year));

    // Créer les dossiers si nécessaire
    create_dir_all(&db_dir).expect("failed to create the .silex/db directory");

    // 2. Ouvrir la connexion principale sur l'HISTORIQUE (ex: 2026)
    let conn = Connection::open(&history_path)?;

    // 3. Attacher le STOCKAGE (Blobs) sous l'alias 'store'
    // L'astuce est là : on exécute du SQL pour lier le 2ème fichier
    let attach_query = format!("ATTACH DATABASE '{}' AS store;", store_path.display());
    conn.execute(attach_query)?;

    // 4. Activer les performances (WAL + Foreign Keys)
    conn.execute("PRAGMA foreign_keys = ON;")?;
    conn.execute("PRAGMA journal_mode = WAL;")?;

    // Configurer le cache pour le store (très important pour les blobs)
    conn.execute("PRAGMA store.cache_size = -200000;")?; // ~200Mo cache

    Ok(conn)
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
    Ok(stmt_id.read(0)?)
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

// --- Helpers de compression ---
pub fn compress(data: &[u8]) -> Vec<u8> {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(data).expect("Failed to compress blob");
    encoder.finish().expect("Failed to finish compression")
}

pub fn decompress(data: &[u8]) -> Vec<u8> {
    let mut decoder = ZlibDecoder::new(data);
    let mut decoded = Vec::new();
    // Astuce : Si la décompression échoue (vieux fichier non compressé), on retourne le brut
    match decoder.read_to_end(&mut decoded) {
        Ok(_) => decoded,
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
        return Ok(stmt.read(0)?);
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
    Ok(stmt_id.read(0)?)
}
