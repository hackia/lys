use crate::crypto::verify_signature;
use crate::db::decompress;
use crate::utils::ok;
use axum::{
    Router,
    body::Bytes,
    extract::{Path as UrlPath, State},
    http::{HeaderMap, StatusCode},
    response::Html,
    routing::{get, post},
};
use sqlite::Connection;
use std::net::SocketAddr;
use std::path::Path;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
// On a besoin de partager la connexion BDD entre les threads du serveur
// SQLite n'est pas "Thread Safe" par défaut, on le met dans un Mutex
pub struct AppState {
    pub conn: Mutex<Connection>,
}

pub async fn start_server(repo_path: &str, port: u16) {
    let path = PathBuf::from(repo_path);

    // On ouvre une connexion dédiée au serveur web
    let conn = crate::db::connect_lys(&path).expect("Failed to connect to DB");

    let shared_state = Arc::new(AppState {
        conn: Mutex::new(conn),
    });

    let app = Router::new()
        .route("/", get(idx_commits))
        .route("/commit/{id}", get(show_commit))
        .route("/file/{hash}", get(show_file))
        // --- NOUVELLE ROUTE POUR LE TRANSFERT ---
        .route("/upload/{hash}", post(upload_atom))
        .with_state(shared_state);

    let addr = SocketAddr::from(([127, 0, 0, 1], port));

    ok(format!("Server running at http://{addr}\x1b").as_str());
    ok("Press Ctrl+C to stop.");

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

pub async fn idx_commits(State(state): State<Arc<AppState>>) -> Html<String> {
    let conn = state.conn.lock().unwrap();
    let query = "SELECT id, hash, author, message, timestamp FROM commits ORDER BY id DESC";

    let mut html = String::from(
        "<html><head><title>Silex Log</title><style>body{font-family:sans-serif;max-width:800px;margin:auto;padding:20px;background:#f4f4f4} .card{background:white;padding:15px;margin-bottom:10px;border-radius:5px;box-shadow:0 2px 5px rgba(0,0,0,0.1)} a{text-decoration:none;color:#333} .hash{color:#e67e22;font-family:monospace}</style></head><body><h1>Silex Repository</h1>",
    );

    if let Ok(mut stmt) = conn.prepare(query) {
        while let Ok(sqlite::State::Row) = stmt.next() {
            let id: i64 = stmt.read("id").unwrap();
            let hash: String = stmt.read("hash").unwrap();
            let msg: String = stmt.read("message").unwrap();
            let date: String = stmt.read("timestamp").unwrap();
            let author: String = stmt.read("author").unwrap();

            html.push_str(&format!(
                "<div class='card'>
                    <h3><a href='/commit/{}'>{}</a></h3>
                    <p><strong>{}</strong> - <span class='hash'>{}</span></p>
                    <small>{}</small>
                </div>",
                id,
                msg,
                author,
                &hash[0..7],
                date
            ));
        }
    }
    html.push_str("</body></html>");
    Html(html)
}

// 2. PAGE DE DÉTAIL : CONTENU D'UN COMMIT
async fn show_commit(
    State(state): State<Arc<AppState>>,
    UrlPath(commit_id): UrlPath<i64>,
) -> Html<String> {
    let conn = state.conn.lock().unwrap();

    // Récupérer les infos du commit
    let mut title = String::new();
    let mut stmt_c = conn
        .prepare("SELECT message, hash FROM commits WHERE id = ?")
        .unwrap();
    stmt_c.bind((1, commit_id)).unwrap();
    if let Ok(sqlite::State::Row) = stmt_c.next() {
        let msg: String = stmt_c.read("message").unwrap();
        let hash: String = stmt_c.read("hash").unwrap();
        title = format!("{} ({})", msg, &hash[0..7]);
    }

    // Récupérer les fichiers
    let query = "
        SELECT m.file_path, b.hash, b.size 
        FROM manifest m 
        JOIN store.blobs b ON m.blob_id = b.id 
        WHERE m.commit_id = ?
        ORDER BY m.file_path";

    let mut html = format!(
        "<html><head><title>Commit {}</title><style>body{{font-family:sans-serif;max-width:800px;margin:auto;padding:20px}} table{{width:100%;border-collapse:collapse}} td,th{{padding:10px;border-bottom:1px solid #ddd;text-align:left}}</style></head><body><h2>{}</h2><a href='/'>&larr; Back to Log</a><br><br><table><tr><th>File Path</th><th>Size</th><th>Actions</th></tr>",
        commit_id, title
    );

    if let Ok(mut stmt) = conn.prepare(query) {
        stmt.bind((1, commit_id)).unwrap();
        while let Ok(sqlite::State::Row) = stmt.next() {
            let path: String = stmt.read("file_path").unwrap();
            let hash: String = stmt.read("hash").unwrap();
            let size: i64 = stmt.read("size").unwrap();

            html.push_str(&format!(
                "<tr>
                    <td>{}</td>
                    <td>{} bytes</td>
                    <td><a href='/file/{}'>View Content</a></td>
                </tr>",
                path, size, hash
            ));
        }
    }
    html.push_str("</table></body></html>");
    Html(html)
}

// 3. PAGE DE FICHIER : VOIR LE CONTENU
async fn show_file(
    State(state): State<Arc<AppState>>,
    UrlPath(hash): UrlPath<String>,
) -> Html<String> {
    let conn = state.conn.lock().unwrap();
    let query = "SELECT content FROM store.blobs WHERE hash = ?";
    let mut stmt = conn.prepare(query).unwrap();
    stmt.bind((1, hash.as_str())).unwrap();

    if let Ok(sqlite::State::Row) = stmt.next() {
        let content: Vec<u8> = stmt.read("content").unwrap();
        // On essaie de convertir en UTF-8 pour l'afficher
        let text = String::from_utf8(decompress(&content))
            .unwrap_or_else(|_| String::from("[Binary Content - Cannot Display]"));

        return Html(format!(
            "<html><head><title>File View</title></head><body>
            <a href='javascript:history.back()'>&larr; Back</a>
            <pre style='background:#f4f4f4;padding:20px;border:1px solid #ddd;overflow-x:auto'>{}</pre>
            </body></html>", 
            text.replace("<", "&lt;").replace(">", "&gt;") // Sécurité basique XSS
        ));
    }
    Html(String::from("File not found"))
}

async fn upload_atom(
    State(state): State<Arc<AppState>>,
    UrlPath(hash): UrlPath<String>,
    headers: HeaderMap,
    body: Bytes,
) -> StatusCode {
    // 1. On récupère la signature envoyée par le client dans l'entête
    let signature = match headers.get("X-Silex-Signature") {
        Some(s) => s.to_str().unwrap_or(""),
        None => return StatusCode::UNAUTHORIZED, // Pas de signature = Rejet
    };

    // 2. Vérification de la signature (Souveraineté)
    // On utilise la clé publique stockée localement sur le serveur
    let root_path = Path::new(".");
    match verify_signature(root_path, &hash, signature) {
        Ok(true) => {
            // 3. Vérification de l'intégrité (Sanctité du Numérateur)
            let actual_hash = blake3::hash(&body).to_hex().to_string();
            if actual_hash != hash {
                return StatusCode::BAD_REQUEST; // Le contenu a été modifié !
            }

            // 4. Stockage dans la base SQLite
            let conn = state.conn.lock().unwrap();
            let query = "INSERT OR IGNORE INTO store.blobs (hash, content, size) VALUES (?, ?, ?)";
            let mut stmt = conn.prepare(query).unwrap();
            stmt.bind((1, hash.as_str())).unwrap();
            stmt.bind((2, &body[..])).unwrap();
            stmt.bind((3, body.len() as i64)).unwrap();

            match stmt.next() {
                Ok(_) => StatusCode::CREATED,
                Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
            }
        }
        _ => StatusCode::FORBIDDEN, // Signature invalide !
    }
}
