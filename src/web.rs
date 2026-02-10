use crate::crypto::verify_signature;
use crate::db::decompress;
use crate::utils::ok;
use axum::{
    Router,
    body::Bytes,
    extract::{Path as UrlPath, State},
    http::{HeaderMap, StatusCode},
    response::{Html, IntoResponse, Response},
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

// -----------------------------
// Small, reusable helpers
// -----------------------------
fn short_hash(s: &str) -> &str {
    s.get(..7).unwrap_or(s)
}

fn html_escape(s: &str) -> String {
    // Minimal escaping to prevent injection in our handcrafted HTML.
    // If you later add more HTML, consider a templating engine.
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
    out
}

pub fn page(title: &str, style: &str, body: &str) -> Html<String> {
    Html(format!(
        "<!doctype html>\
         <html>\
           <head>\
             <meta charset='utf-8'>\
             <meta name='viewport' content='width=device-width, initial-scale=1'>\
             <title>{}</title>\
             <style>{}</style>\
           </head>\
           <body>{}</body>\
         </html>",
        html_escape(title),
        style,
        body
    ))
}

fn http_error(status: StatusCode, msg: &str) -> Response {
    (status, page("Error", "body{font-family:sans-serif;max-width:800px;margin:auto;padding:20px}", &format!(
        "<h2>Error</h2><p>{}</p><p><a href='/'>Back</a></p>",
        html_escape(msg)
    )))
        .into_response()
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
        .route("/raw/{hash}", get(download_raw)) // <-- new: reliable way to view binary / huge files
        .route("/upload/{hash}", post(upload_atom))
        .with_state(shared_state);

    let addr = SocketAddr::from(([127, 0, 0, 1], port));

    ok(format!("Server running at https://{addr}").as_str());
    ok("Press Ctrl+C to stop.");

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

pub async fn idx_commits(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    const STYLE: &str = "\
        body{font-family:sans-serif;max-width:900px;margin:auto;padding:20px;background:#f4f4f4}\
        .card{background:white;padding:15px;margin-bottom:10px;border-radius:8px;box-shadow:0 2px 5px rgba(0,0,0,0.08)}\
        a{text-decoration:none;color:#222}\
        a:hover{text-decoration:underline}\
        .hash{color:#e67e22;font-family:monospace}\
        .meta{color:#555}\
    ";

    let conn = match state.conn.lock() {
        Ok(g) => g,
        Err(_) => return http_error(StatusCode::INTERNAL_SERVER_ERROR, "DB lock poisoned"),
    };

    let query = "SELECT id, hash, author, message, timestamp FROM commits ORDER BY id DESC";
    let mut cards = String::new();

    let mut stmt = match conn.prepare(query) {
        Ok(s) => s,
        Err(_) => return http_error(StatusCode::INTERNAL_SERVER_ERROR, "Failed to query commits"),
    };

    while let Ok(sqlite::State::Row) = stmt.next() {
        let id: i64 = match stmt.read("id") {
            Ok(v) => v,
            Err(_) => continue,
        };
        let hash: String = match stmt.read("hash") {
            Ok(v) => v,
            Err(_) => continue,
        };
        let msg: String = stmt.read("message").unwrap_or_else(|_| String::from("(no message)"));
        let date: String = stmt.read("timestamp").unwrap_or_else(|_| String::from(""));
        let author: String = stmt.read("author").unwrap_or_else(|_| String::from("Unknown"));

        cards.push_str(&format!(
            "<div class='card'>\
                <h3><a href='/commit/{id}'>{msg}</a></h3>\
                <p class='meta'><strong>{author}</strong> — <span class='hash'>{hash}</span></p>\
                <small class='meta'>{date}</small>\
             </div>",
            id = id,
            msg = html_escape(&msg),
            author = html_escape(&author),
            hash = html_escape(short_hash(&hash)),
            date = html_escape(&date),
        ));
    }
    page(
        "Silex Log",
        STYLE,
        &format!(
            "<h1>Silex Repository</h1>\
             <p class='meta'>Latest commits:</p>\
             {cards}",
        ),
    ).into_response()
}

// 2. PAGE DE DÉTAIL : CONTENU D'UN COMMIT
async fn show_commit(
    State(state): State<Arc<AppState>>,
    UrlPath(commit_id): UrlPath<i64>,
) -> impl IntoResponse {
    const STYLE: &str = "\
        body{font-family:sans-serif;max-width:1000px;margin:auto;padding:20px}\
        table{width:100%;border-collapse:collapse;margin-top:12px}\
        td,th{padding:10px;border-bottom:1px solid #ddd;text-align:left;vertical-align:top}\
        .hash{font-family:monospace;color:#e67e22}\
        a{text-decoration:none}\
        a:hover{text-decoration:underline}\
        .actions a{margin-right:10px}\
    ";

    let conn = match state.conn.lock() {
        Ok(g) => g,
        Err(_) => return http_error(StatusCode::INTERNAL_SERVER_ERROR, "DB lock poisoned"),
    };

    // Récupérer les infos du commit
    let mut title = String::from("Commit not found");
    {
        let mut stmt_c = match conn.prepare("SELECT message, hash FROM commits WHERE id = ?") {
            Ok(s) => s,
            Err(_) => return http_error(StatusCode::INTERNAL_SERVER_ERROR, "Failed to query commit"),
        };
        if stmt_c.bind((1, commit_id)).is_ok() {
            if let Ok(sqlite::State::Row) = stmt_c.next() {
                let msg: String = stmt_c.read("message").unwrap_or_else(|_| String::from(""));
                let hash: String = stmt_c.read("hash").unwrap_or_else(|_| String::from(""));
                title = format!("{} ({})", msg, short_hash(&hash));
            }
        }
    }

    // Récupérer les fichiers
    let query = "\
        SELECT m.file_path, b.hash, b.size \
        FROM manifest m \
        JOIN store.blobs b ON m.blob_id = b.id \
        WHERE m.commit_id = ? \
        ORDER BY m.file_path";

    let mut rows = String::new();
    let mut stmt = match conn.prepare(query) {
        Ok(s) => s,
        Err(_) => return http_error(StatusCode::INTERNAL_SERVER_ERROR, "Failed to query files"),
    };

    if stmt.bind((1, commit_id)).is_ok() {
        while let Ok(sqlite::State::Row) = stmt.next() {
            let path: String = stmt.read("file_path").unwrap_or_else(|_| String::from(""));
            let hash: String = stmt.read("hash").unwrap_or_else(|_| String::from(""));
            let size: i64 = stmt.read("size").unwrap_or(0);

            rows.push_str(&format!(
                "<tr>\
                    <td>{}</td>\
                    <td>{} bytes</td>\
                    <td class='actions'>\
                      <a href='/file/{}'>View</a>\
                      <a href='/raw/{}'>Download</a>\
                    </td>\
                 </tr>",
                html_escape(&path),
                size,
                html_escape(&hash),
                html_escape(&hash),
            ));
        }
    }

    page(
        &format!("Commit {}", commit_id),
        STYLE,
        &format!(
            "<a href='/'>&larr; Back to Log</a>\
             <h2>{}</h2>\
             <table>\
               <tr><th>File Path</th><th>Size</th><th>Actions</th></tr>\
               {}\
             </table>",
            html_escape(&title),
            rows
        ),
    )
    .into_response()
}

// 3. PAGE DE FICHIER : VOIR LE CONTENU
async fn show_file(
    State(state): State<Arc<AppState>>,
    UrlPath(hash): UrlPath<String>,
) -> impl IntoResponse {
    const STYLE: &str = "\
        body{font-family:sans-serif;max-width:1000px;margin:auto;padding:20px}\
        pre{background:#f4f4f4;padding:20px;border:1px solid #ddd;overflow-x:auto;white-space:pre}\
        .hash{font-family:monospace;color:#e67e22}\
        .meta{color:#555}\
        code{font-family:ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, 'Liberation Mono', 'Courier New', monospace}\
    ";

    let conn = match state.conn.lock() {
        Ok(g) => g,
        Err(_) => return http_error(StatusCode::INTERNAL_SERVER_ERROR, "DB lock poisoned"),
    };

    let query = "SELECT content, size FROM store.blobs WHERE hash = ?";
    let mut stmt = match conn.prepare(query) {
        Ok(s) => s,
        Err(_) => return http_error(StatusCode::INTERNAL_SERVER_ERROR, "Failed to query blob"),
    };

    if stmt.bind((1, hash.as_str())).is_err() {
        return http_error(StatusCode::BAD_REQUEST, "Invalid hash parameter");
    }

    if let Ok(sqlite::State::Row) = stmt.next() {
        let content: Vec<u8> = match stmt.read("content") {
            Ok(v) => v,
            Err(_) => return http_error(StatusCode::INTERNAL_SERVER_ERROR, "Failed to read blob"),
        };
        let original_size: i64 = stmt.read("size").unwrap_or(0);

        // Decompress (falls back to raw if it's not zlib-compressed)
        let bytes = decompress(&content);

        // Bugfix class: huge/binary files were “displayed” as garbage or could freeze the browser.
        // Strategy:
        //  - If not UTF-8: show a friendly message and provide download link
        //  - If UTF-8 but huge: truncate preview
        const MAX_PREVIEW_BYTES: usize = 512 * 1024; // 512 KiB

        let mut body = String::new();
        body.push_str("<a href='javascript:history.back()'>&larr; Back</a>");
        body.push_str(&format!(
            "<p class='meta'>hash: <span class='hash'>{}</span> — size: {} bytes — <a href='/raw/{}'>Download raw</a></p>",
            html_escape(&hash),
            original_size.max(0),
            html_escape(&hash),
        ));

        match String::from_utf8(bytes) {
            Ok(mut text) => {
                let truncated = text.len() > MAX_PREVIEW_BYTES;
                if truncated {
                    text.truncate(MAX_PREVIEW_BYTES);
                    text.push_str("\n\n[... truncated preview ...]");
                }
                body.push_str(&format!("<pre><code>{}</code></pre>", html_escape(&text)));
                page("File View", STYLE, &body).into_response()
            }
            Err(_) => {
                body.push_str(
                    "<p><strong>Binary content</strong> (cannot render as UTF-8 text). Use <a href='/raw/",
                );
                body.push_str(&html_escape(&hash));
                body.push_str("'>Download raw</a>.</p>");
                page("File View", STYLE, &body).into_response()
            }
        }
    } else {
        http_error(StatusCode::NOT_FOUND, "File not found")
    }
}

// New: raw download endpoint (fixes “display” for binary files and huge blobs)
async fn download_raw(
    State(state): State<Arc<AppState>>,
    UrlPath(hash): UrlPath<String>,
) -> impl IntoResponse {
    let conn = match state.conn.lock() {
        Ok(g) => g,
        Err(_) => return http_error(StatusCode::INTERNAL_SERVER_ERROR, "DB lock poisoned"),
    };

    let query = "SELECT content FROM store.blobs WHERE hash = ?";
    let mut stmt = match conn.prepare(query) {
        Ok(s) => s,
        Err(_) => return http_error(StatusCode::INTERNAL_SERVER_ERROR, "Failed to query blob"),
    };

    if stmt.bind((1, hash.as_str())).is_err() {
        return http_error(StatusCode::BAD_REQUEST, "Invalid hash parameter");
    }

    if let Ok(sqlite::State::Row) = stmt.next() {
        let content: Vec<u8> = match stmt.read("content") {
            Ok(v) => v,
            Err(_) => return http_error(StatusCode::INTERNAL_SERVER_ERROR, "Failed to read blob"),
        };

        let bytes = decompress(&content);

        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::CONTENT_TYPE,
            axum::http::HeaderValue::from_static("application/octet-stream"),
        );
        // Avoid header injection: keep filename simple.
        let filename = format!("lys-{}.bin", short_hash(&hash));
        let cd = format!("attachment; filename=\"{}\"", filename);
        if let Ok(v) = axum::http::HeaderValue::from_str(&cd) {
            headers.insert(axum::http::header::CONTENT_DISPOSITION, v);
        }

        (StatusCode::OK, headers, bytes).into_response()
    } else {
        http_error(StatusCode::NOT_FOUND, "File not found")
    }
}

async fn upload_atom(
    State(state): State<Arc<AppState>>,
    UrlPath(hash): UrlPath<String>,
    headers: HeaderMap,
    body: Bytes,
) -> StatusCode {
    // 1. On récupère la signature envoyée par le client dans l'entête
    let signature = match headers.get("X-Silex-Signature").and_then(|s| s.to_str().ok()) {
        Some(s) if !s.is_empty() => s,
        _ => return StatusCode::UNAUTHORIZED, // Pas de signature = Rejet
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
            let conn = match state.conn.lock() {
                Ok(g) => g,
                Err(_) => return StatusCode::INTERNAL_SERVER_ERROR,
            };

            let query =
                "INSERT OR IGNORE INTO store.blobs (hash, content, size) VALUES (?, ?, ?)";
            let mut stmt = match conn.prepare(query) {
                Ok(s) => s,
                Err(_) => return StatusCode::INTERNAL_SERVER_ERROR,
            };

            if stmt.bind((1, hash.as_str())).is_err() {
                return StatusCode::INTERNAL_SERVER_ERROR;
            }
            if stmt.bind((2, &body[..])).is_err() {
                return StatusCode::INTERNAL_SERVER_ERROR;
            }
            if stmt.bind((3, body.len() as i64)).is_err() {
                return StatusCode::INTERNAL_SERVER_ERROR;
            }

            match stmt.next() {
                Ok(_) => StatusCode::CREATED,
                Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
            }
        }
        _ => StatusCode::FORBIDDEN, // Signature invalide !
    }
}
