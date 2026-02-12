
use crate::db::decompress;
use crate::utils::ok;
use axum::{
    Router,
    body::Bytes,
    extract::{Path as UrlPath, Query, State},
    http::{HeaderMap, StatusCode, header},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
};
use chrono::{DateTime, Utc};
use serde::Deserialize;
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

#[derive(Deserialize)]
pub struct Pagination {
    pub page: Option<usize>,
}

#[derive(Deserialize)]
pub struct DiffParams {
    pub mode: Option<String>,
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

fn truncate_words(text: &str, limit: usize) -> String {
    let words: Vec<&str> = text.split_whitespace().collect();
    if words.len() <= limit {
        text.to_string()
    } else {
        words[..limit].join(" ") + "..."
    }
}

fn time_ago(timestamp: &str) -> String {
    let dt = match DateTime::parse_from_rfc3339(timestamp) {
        Ok(d) => d.with_timezone(&Utc),
        Err(_) => return String::new(),
    };
    let now = Utc::now();
    let diff = now.signed_duration_since(dt);

    if diff.num_seconds() < 60 {
        format!("{}s ago", diff.num_seconds())
    } else if diff.num_minutes() < 60 {
        format!("{}m ago", diff.num_minutes())
    } else if diff.num_hours() < 24 {
        format!("{}h ago", diff.num_hours())
    } else if diff.num_days() < 30 {
        format!("{}d ago", diff.num_days())
    } else if diff.num_days() < 365 {
        format!("{}mo ago", diff.num_days() / 30)
    } else {
        format!("{}y ago", diff.num_days() / 365)
    }
}

pub fn page(title: &str, style: &str, body: &str) -> Html<String> {
    const COMMON_STYLE: &str = "
        :root {
            --bg: #ffffff;
            --fg: #000000;
            --header-bg: #eeeeee;
            --menu-bg: #f8f8f8;
            --border: #cccccc;
            --link: #0000ee;
            --link-hover: #000088;
            --meta: #666666;
            --hover-bg: #f0f0f0;
            --table-header-bg: #eeeeee;
            --card-border: #cccccc;
            --nav-bg: #f8f8f8;
            --code-bg: #f8f8f8;
            --hash: #000000;
        }
        @media (prefers-color-scheme: dark) {
            :root {
                --bg: #1a1a1a;
                --fg: #e0e0e0;
                --header-bg: #2d2d2d;
                --menu-bg: #252525;
                --border: #444444;
                --link: #5c9eff;
                --link-hover: #80b3ff;
                --meta: #999999;
                --hover-bg: #333333;
                --table-header-bg: #2d2d2d;
                --card-border: #444444;
                --nav-bg: #252525;
                --code-bg: #2d2d2d;
                --hash: #ff9900;
            }
        }
        body { 
            font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, Helvetica, Arial, sans-serif; 
            margin: 0; padding: 0; 
            background: var(--bg); color: var(--fg); 
            line-height: 1.5;
        }
        #header { background: var(--header-bg); border-bottom: 1px solid var(--border); padding: 15px 20px; }
        #header h1 { margin: 0; font-size: 1.4em; }
        #header .repo-desc { color: var(--meta); font-size: 0.85em; margin-top: 4px; }
        #menu { background: var(--menu-bg); border-bottom: 1px solid var(--border); padding: 8px 20px; }
        #menu a { text-decoration: none; color: var(--fg); font-weight: bold; margin-right: 20px; font-size: 0.9em; }
        #menu a:hover { color: var(--link); }
        #content { padding: 25px 20px; max-width: 1200px; margin: 0 auto; }
        table { width: 100%; border-collapse: separate; border-spacing: 0; font-size: 0.9em; border: 1px solid var(--border); border-radius: 4px; overflow: hidden; margin-bottom: 20px; }
        th { background: var(--table-header-bg); text-align: left; padding: 10px; border-bottom: 1px solid var(--border); color: var(--fg); }
        td { padding: 10px; border-bottom: 1px solid var(--border); vertical-align: top; }
        tr:last-child td { border-bottom: none; }
        .hash { font-family: 'SFMono-Regular', Consolas, 'Liberation Mono', Menlo, monospace; color: var(--hash); font-weight: bold; }
        .age { color: var(--meta); font-size: 0.8em; margin-bottom: 4px; }
        .author { color: var(--fg); }
        tr:hover { background: var(--hover-bg); }
        a { color: var(--link); text-decoration: none; }
        a:hover { text-decoration: underline; color: var(--link-hover); }
        h3 { margin-top: 0; border-bottom: 1px solid var(--border); padding-bottom: 10px; margin-bottom: 15px; }
        pre { background: var(--code-bg); padding: 15px; border-radius: 4px; border: 1px solid var(--border); overflow-x: auto; font-family: monospace; font-size: 0.9em; }
        .btn { 
            display: inline-block; 
            background: var(--header-bg); 
            border: 1px solid var(--border); 
            padding: 4px 12px; 
            border-radius: 4px; 
            text-decoration: none; 
            color: var(--fg); 
            font-size: 0.85em; 
            margin-right: 10px; 
            font-weight: bold;
        }
        .btn:hover { background: var(--hover-bg); text-decoration: none; }
    ";

    Html(format!(
        "<!doctype html>\
         <html>\
           <head>\
             <meta charset='utf-8'>\
             <meta name='viewport' content='width=device-width, initial-scale=1'>\
             <title>{}</title>\
             <link rel='stylesheet' href='https://cdnjs.cloudflare.com/ajax/libs/prism/1.29.0/themes/prism-tomorrow.min.css' media='(prefers-color-scheme: dark)'>\
             <link rel='stylesheet' href='https://cdnjs.cloudflare.com/ajax/libs/prism/1.29.0/themes/prism.min.css' media='(prefers-color-scheme: light)'>\
             <style>{}{}</style>\
           </head>\
           <body>\
             <div id='header'>\
               <h1>Lys Repository</h1>\
               <div class='repo-desc'>A secure local-first vcs</div>\
             </div>\
             <div id='menu'>\
               <a href='/'>Summary</a>\
               <a href='/'>Log</a>\
               <a href='/rss'>RSS</a>\
             </div>\
             <div id='content'>{}</div>\
             <script src='https://cdnjs.cloudflare.com/ajax/libs/prism/1.29.0/components/prism-core.min.js'></script>\
             <script src='https://cdnjs.cloudflare.com/ajax/libs/prism/1.29.0/plugins/autoloader/prism-autoloader.min.js'></script>\
           </body>\
         </html>",
        html_escape(title),
        COMMON_STYLE,
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
        .route("/rss", get(serve_rss))
        .route("/commit/{id}", get(show_commit))
        .route("/commit/{id}/diff", get(show_commit_diff))
        .route("/commit/{id}/tree", get(show_commit_tree))
        .route("/commit/{id}/tree/{*path}", get(show_commit_tree))
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

pub async fn idx_commits(
    State(state): State<Arc<AppState>>,
    Query(pagination): Query<Pagination>,
) -> impl IntoResponse {
    let conn = match state.conn.lock() {
        Ok(g) => g,
        Err(_) => return http_error(StatusCode::INTERNAL_SERVER_ERROR, "DB lock poisoned"),
    };

    let page_num = pagination.page.unwrap_or(1).max(1);
    let per_page = 20;
    let offset = (page_num - 1) * per_page;

    // Stats
    let total_commits: i64 = {
        let mut stmt = conn.prepare("SELECT COUNT(*) FROM commits").unwrap();
        if let Ok(sqlite::State::Row) = stmt.next() {
            stmt.read(0).unwrap_or(0)
        } else {
            0
        }
    };
    let total_pages = (total_commits as f64 / per_page as f64).ceil() as i64;

    let contributors = crate::db::get_unique_contributors(&conn).unwrap_or_default();

    let stats_html = format!(
        "<div style='margin-bottom: 20px; background: var(--menu-bg); padding: 15px; border: 1px solid var(--border); border-radius: 4px;'>\
           <h3 style='margin-top: 0;'>Repository Summary</h3>\
           <div style='display: grid; grid-template-columns: auto 1fr; gap: 10px 20px; font-size: 0.9em;'>\
             <strong>Total Commits:</strong> <span>{}</span>\
             <strong>Contributors:</strong> <span>{}</span>\
             <strong>Current Page:</strong> <span>{} / {}</span>\
           </div>\
           <div style='margin-top: 15px;'>\
             <a href='/#latest' class='btn'>Jump to Latest Commits</a>\
           </div>\
         </div>",
        total_commits,
        contributors.join(", "),
        page_num,
        total_pages
    );

    let query = "SELECT id, hash, author, message, timestamp FROM commits ORDER BY id DESC LIMIT ? OFFSET ?";
    let mut rows = String::new();

    let mut stmt = match conn.prepare(query) {
        Ok(s) => s,
        Err(_) => return http_error(StatusCode::INTERNAL_SERVER_ERROR, "Failed to query commits"),
    };
    stmt.bind((1, per_page as i64)).unwrap();
    stmt.bind((2, offset as i64)).unwrap();

    while let Ok(sqlite::State::Row) = stmt.next() {
        let id: i64 = stmt.read("id").unwrap_or(0);
        let hash: String = stmt.read("hash").unwrap_or_default();
        let msg: String = stmt.read("message").unwrap_or_else(|_| String::from("(no message)"));
        let date: String = stmt.read("timestamp").unwrap_or_else(|_| String::from(""));
        let author: String = stmt.read("author").unwrap_or_else(|_| String::from("Unknown"));

        let first_line = msg.lines().next().unwrap_or("");
        let summary = truncate_words(first_line, 100);

        rows.push_str(&format!(
            "<tr>\
                <td colspan='4'>\
                    <div class='age'>{} ({})</div>\
                    <div style='margin: 5px 0;'>{}</div>\
                    <div class='author' style='font-size: 0.85em;'>{} — <a href='/commit/{id}' class='hash'>{}</a></div>\
                </td>\
             </tr>",
            html_escape(&date),
            time_ago(&date),
            html_escape(&summary),
            html_escape(&author),
            html_escape(short_hash(&hash)),
            id = id,
        ));
    }

    let nav_html = format!(
        "<div style='margin-top: 20px; font-size: 0.9em; padding: 10px; background: var(--nav-bg); border: 1px solid var(--border); border-radius: 4px;'>\
         <div style='margin-bottom: 10px;'>Page {} of {}</div>",
        page_num, total_pages
    );

    let mut links = Vec::new();
    if page_num > 1 {
        links.push(format!("<a href='/?page={}'>&laquo; Newer</a>", page_num - 1));
    }
    if (page_num as i64) < total_pages {
        links.push(format!("<a href='/?page={}'>Older &raquo;</a>", page_num + 1));
    }

    let mut nav_html = nav_html;
    if !links.is_empty() {
        nav_html.push_str(&links.join(" | "));
    }
    nav_html.push_str("</div>");

    page(
        "Lys Log",
        "",
        &format!(
            "{stats_html}\
             <h3 id='latest'>Latest Commits</h3>\
             <table>\
               {rows}\
             </table>\
             {nav}",
            stats_html = stats_html,
            rows = rows,
            nav = nav_html,
        ),
    )
    .into_response()
}

// 2. PAGE DE DÉTAIL : CONTENU D'UN COMMIT
async fn show_commit(
    State(state): State<Arc<AppState>>,
    UrlPath(commit_id): UrlPath<i64>,
) -> impl IntoResponse {
    let conn = match state.conn.lock() {
        Ok(g) => g,
        Err(_) => return http_error(StatusCode::INTERNAL_SERVER_ERROR, "DB lock poisoned"),
    };

    // Récupérer le tree_hash du commit
    let mut tree_hash = String::new();
    let mut title = String::from("Commit not found");
    let mut author = String::new();
    let mut date = String::new();
    let mut hash = String::new();

    {
        let mut stmt_c = match conn.prepare("SELECT message, hash, tree_hash, author, timestamp, id FROM commits WHERE id = ?") {
            Ok(s) => s,
            Err(_) => return http_error(StatusCode::INTERNAL_SERVER_ERROR, "Failed to query commit"),
        };
        if stmt_c.bind((1, commit_id)).is_ok() {
            if let Ok(sqlite::State::Row) = stmt_c.next() {
                title = stmt_c.read("message").unwrap_or_else(|_| String::from(""));
                hash = stmt_c.read("hash").unwrap_or_else(|_| String::from(""));
                tree_hash = stmt_c.read("tree_hash").unwrap_or_else(|_| String::from(""));
                author = stmt_c.read("author").unwrap_or_else(|_| String::from(""));
                date = stmt_c.read("timestamp").unwrap_or_else(|_| String::from(""));
            }
        }
    }

    if tree_hash.is_empty() {
        return http_error(StatusCode::NOT_FOUND, "Commit not found");
    }

    page(
        &format!("Commit {}", short_hash(&hash)),
        ".commit-info td { border: none; padding: 4px 12px; }",
        &format!(
            "<h3>Commit Details</h3>\
            <table class='commit-info' style='margin-bottom: 25px; width: 100%; border: 1px solid var(--border); background: var(--menu-bg);'>
               <tr><td><b>author</b></td><td>{}</td></tr>
               <tr><td><b>date</b></td><td>{} ({})</td></tr>
               <tr><td><b>commit</b></td><td class='hash'>{}</td></tr>
               <tr><td><b>tree</b></td><td class='hash'><a href='/commit/{}/tree'>{}</a></td></tr>
               <tr>
                 <td><b>actions</b></td>
                 <td>
                   <a href='/commit/{}/tree' class='btn'>Browse Tree</a>
                   <a href='/commit/{}/diff' class='btn'>View Diff</a>
                   <a href='/' class='btn'>Back to Log</a>
                 </td>
               </tr>
             </table>
             <div style='background: var(--code-bg); padding: 15px; border: 1px solid var(--border); border-radius: 4px; margin-bottom: 25px;'>\
               <pre style='margin: 0; white-space: pre-wrap; border: none; padding: 0;'>{}</pre>\
             </div>",
            html_escape(&author),
            html_escape(&date),
            time_ago(&date),
            html_escape(&hash),
            commit_id,
            html_escape(&tree_hash),
            commit_id,
            commit_id,
            html_escape(&title)
        ),
    )
    .into_response()
}

// 2.1. VUE ARBORESCENTE D'UN COMMIT
async fn show_commit_tree(
    State(state): State<Arc<AppState>>,
    UrlPath(params): UrlPath<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    let commit_id: i64 = match params.get("id").and_then(|id| id.parse().ok()) {
        Some(id) => id,
        None => return http_error(StatusCode::BAD_REQUEST, "Invalid commit ID"),
    };
    let path_str = params.get("path").cloned().unwrap_or_default();

    let conn = match state.conn.lock() {
        Ok(g) => g,
        Err(_) => return http_error(StatusCode::INTERNAL_SERVER_ERROR, "DB lock poisoned"),
    };

    // Récupérer le tree_hash du commit
    let mut root_tree_hash = String::new();
    let mut commit_hash = String::new();
    {
        let mut stmt_c = match conn.prepare("SELECT hash, tree_hash FROM commits WHERE id = ?") {
            Ok(s) => s,
            Err(_) => return http_error(StatusCode::INTERNAL_SERVER_ERROR, "Failed to query commit"),
        };
        if stmt_c.bind((1, commit_id)).is_ok() {
            if let Ok(sqlite::State::Row) = stmt_c.next() {
                commit_hash = stmt_c.read("hash").unwrap_or_else(|_| String::from(""));
                root_tree_hash = stmt_c.read("tree_hash").unwrap_or_else(|_| String::from(""));
            }
        }
    }

    if root_tree_hash.is_empty() {
        return http_error(StatusCode::NOT_FOUND, "Commit not found");
    }

    // Naviguer jusqu'au dossier spécifié par path_str
    let mut current_tree_hash = root_tree_hash;
    let components: Vec<&str> = path_str.split('/').filter(|s| !s.is_empty()).collect();

    for comp in &components {
        let query = "SELECT hash, mode FROM tree_nodes WHERE parent_tree_hash = ? AND name = ?";
        let mut stmt = match conn.prepare(query) {
            Ok(s) => s,
            Err(_) => return http_error(StatusCode::INTERNAL_SERVER_ERROR, "Failed to query tree nodes"),
        };
        stmt.bind((1, current_tree_hash.as_str())).unwrap();
        stmt.bind((2, *comp)).unwrap();

        if let Ok(sqlite::State::Row) = stmt.next() {
            let mode: i64 = stmt.read("mode").unwrap();
            let is_dir = mode == 16384 || mode == 0o040000 || mode == 0o755;
            if is_dir {
                current_tree_hash = stmt.read("hash").unwrap();
            } else {
                return http_error(StatusCode::BAD_REQUEST, "Path is not a directory");
            }
        } else {
            return http_error(StatusCode::NOT_FOUND, "Directory not found");
        }
    }

    // Générer les breadcrumbs
    let mut breadcrumbs = format!("<a href='/commit/{commit_id}/tree'>root</a>");
    let mut acc_path = String::new();
    for comp in &components {
        acc_path.push_str("/");
        acc_path.push_str(comp);
        breadcrumbs.push_str(&format!(" / <a href='/commit/{commit_id}/tree{acc_path}'>{}</a>", html_escape(comp)));
    }

    let mut tree_html = String::new();
    if let Err(e) = render_tree_html_flat(&conn, commit_id, &path_str, &current_tree_hash, &mut tree_html) {
        return http_error(StatusCode::INTERNAL_SERVER_ERROR, &format!("Failed to render tree: {}", e));
    }

    page(
        &format!("Tree - {}", short_hash(&commit_hash)),
        ".tree-table { width: 100%; border-collapse: collapse; font-family: monospace; border: none; }
         .tree-table td { padding: 8px 12px; border: none; border-bottom: 1px solid var(--border); }
         .tree-table tr:last-child td { border-bottom: none; }
         .tree-table tr:hover { background: var(--hover-bg); }
         .icon { margin-right: 8px; }
         .dir { font-weight: bold; }
         .breadcrumbs { margin-bottom: 20px; font-family: monospace; background: var(--nav-bg); padding: 12px; border: 1px solid var(--border); border-radius: 4px; }",
        &format!(
            "<h3>Tree View</h3>\
             <div class='breadcrumbs'>{}</div>\
             <table class='tree-table'>{}</table>",
            breadcrumbs,
            tree_html
        ),
    )
    .into_response()
}

fn render_tree_html_flat(
    conn: &Connection,
    commit_id: i64,
    current_path: &str,
    tree_hash: &str,
    out: &mut String,
) -> Result<(), Box<dyn std::error::Error>> {
    let query = "SELECT name, hash, mode FROM tree_nodes WHERE parent_tree_hash = ? ORDER BY mode DESC, name ASC";
    let mut stmt = conn.prepare(query)?;
    stmt.bind((1, tree_hash))?;

    while let Ok(sqlite::State::Row) = stmt.next() {
        let name: String = stmt.read("name")?;
        let hash: String = stmt.read("hash")?;
        let mode: i64 = stmt.read("mode")?;
        
        let is_dir = mode == 16384 || mode == 0o040000 || mode == 0o755;
        let icon = if is_dir { "📁" } else { "📄" };
        
        let link = if is_dir {
            let full_path = if current_path.is_empty() {
                name.clone()
            } else {
                format!("{}/{}", current_path, name)
            };
            format!("<a href='/commit/{commit_id}/tree/{}' class='dir'>{}</a>", full_path, html_escape(&name))
        } else {
            format!("<a href='/file/{}' class='file'>{}</a>", html_escape(&hash), html_escape(&name))
        };

        out.push_str(&format!(
            "<tr>\
                <td style='width: 20px;'><span class='icon'>{}</span></td>\
                <td>{}</td>\
             </tr>",
            icon,
            link
        ));
    }
    Ok(())
}

// 2.2. VUE DIFF D'UN COMMIT
async fn show_commit_diff(
    State(state): State<Arc<AppState>>,
    UrlPath(commit_id): UrlPath<i64>,
    Query(params): Query<DiffParams>,
) -> impl IntoResponse {
    let mode = params.mode.unwrap_or_else(|| "unified".to_string());
    let conn = match state.conn.lock() {
        Ok(g) => g,
        Err(_) => return http_error(StatusCode::INTERNAL_SERVER_ERROR, "DB lock poisoned"),
    };

    // 1. Infos du commit
    let mut tree_hash = String::new();
    let mut commit_hash = String::new();
    let mut parent_tree_hash: Option<String> = None;

    {
        let mut stmt = match conn.prepare("SELECT hash, tree_hash, id FROM commits WHERE id = ?") {
            Ok(s) => s,
            Err(_) => return http_error(StatusCode::INTERNAL_SERVER_ERROR, "Failed to query commit"),
        };
        stmt.bind((1, commit_id)).unwrap();
        if let Ok(sqlite::State::Row) = stmt.next() {
            commit_hash = stmt.read("hash").unwrap();
            tree_hash = stmt.read("tree_hash").unwrap();
            let current_db_id: i64 = stmt.read("id").unwrap();

            let mut stmt_p = conn.prepare("SELECT tree_hash FROM commits WHERE id < ? ORDER BY id DESC LIMIT 1").unwrap();
            stmt_p.bind((1, current_db_id)).unwrap();
            if let Ok(sqlite::State::Row) = stmt_p.next() {
                parent_tree_hash = Some(stmt_p.read(0).unwrap());
            }
        }
    }

    let mut current_state = std::collections::HashMap::new();
    let _ = crate::vcs::flatten_tree(&conn, &tree_hash, PathBuf::new(), &mut current_state);

    let mut parent_state = std::collections::HashMap::new();
    if let Some(ref ph) = parent_tree_hash {
        let _ = crate::vcs::flatten_tree(&conn, ph, PathBuf::new(), &mut parent_state);
    }

    let mut diff_html = String::new();
    let mut paths: Vec<_> = current_state.keys().collect();
    for p in parent_state.keys() {
        if !current_state.contains_key(p) {
            paths.push(p);
        }
    }
    paths.sort();
    paths.dedup();

    for path in paths {
        let old_info = parent_state.get(path);
        let new_info = current_state.get(path);

        match (old_info, new_info) {
            (Some((old_hash, _)), Some((new_hash, _))) if old_hash != new_hash => {
                // Modified
                let old_bytes = get_raw_blob(&conn, old_hash);
                let new_bytes = get_raw_blob(&conn, new_hash);
                diff_html.push_str(&format!("<div style='margin-top: 20px;'><strong>Modified: {}</strong></div>", path.display()));
                diff_html.push_str(&render_diff(&old_bytes, &new_bytes, &mode));
            }
            (None, Some((new_hash, _))) => {
                // Added
                let new_bytes = get_raw_blob(&conn, new_hash);
                diff_html.push_str(&format!("<div style='margin-top: 20px;'><strong>Added: {}</strong></div>", path.display()));
                diff_html.push_str(&render_diff(&[], &new_bytes, &mode));
            }
            (Some((old_hash, _)), None) => {
                // Deleted
                let old_bytes = get_raw_blob(&conn, old_hash);
                diff_html.push_str(&format!("<div style='margin-top: 20px;'><strong>Deleted: {}</strong></div>", path.display()));
                diff_html.push_str(&render_diff(&old_bytes, &[], &mode));
            }
            _ => {}
        }
    }

    page(
        &format!("Diff - {}", short_hash(&commit_hash)),
        ".diff-added { color: #28a745; background-color: #e6ffec; display: block; }
         .diff-deleted { color: #dc3545; background-color: #ffeef0; display: block; }
         .diff-equal { color: var(--fg); display: block; }
         .diff-container { font-family: monospace; font-size: 0.9em; white-space: pre-wrap; background: var(--code-bg); padding: 10px; border: 1px solid var(--border); border-radius: 4px; margin-top: 5px; }
         .diff-ss-table { width: 100%; border-collapse: collapse; table-layout: fixed; background: var(--code-bg); border: 1px solid var(--border); border-radius: 4px; font-family: monospace; font-size: 0.9em; }
         .diff-ss-table td { padding: 2px 5px; vertical-align: top; border: 1px solid var(--border); overflow-wrap: break-word; white-space: pre-wrap; }
         .diff-ss-left { width: 50%; }
         .diff-ss-right { width: 50%; }
         .diff-line-num { width: 40px; color: var(--meta); text-align: right; user-select: none; border-right: 1px solid var(--border); }
         .diff-ghost { color: var(--meta); background: var(--hover-bg); }
         .btn-active { background: var(--link) !important; color: white !important; border-color: var(--link) !important; }",
        &format!(
            "<div style='margin-bottom: 20px;'>\
               <a href='/commit/{}'>&larr; Back to Commit</a>\
               <div style='float: right;'>\
                 <a href='?mode=unified' class='btn {}'>Unified</a>\
                 <a href='?mode=side-by-side' class='btn {}'>Side-by-side</a>\
                 <a href='?mode=raw' class='btn {}'>Raw Content</a>\
               </div>\
             </div>\
             <h3>Changes for Commit {}</h3>\
             {}\
             {}",
            commit_id,
            if mode == "unified" { "btn-active" } else { "" },
            if mode == "side-by-side" { "btn-active" } else { "" },
            if mode == "raw" { "btn-active" } else { "" },
            short_hash(&commit_hash),
            if diff_html.is_empty() { "<p>No text changes.</p>" } else { "" },
            diff_html
        )
    ).into_response()
}

fn get_raw_blob(conn: &Connection, hash: &str) -> Vec<u8> {
    let mut stmt = conn.prepare("SELECT content FROM store.blobs WHERE hash = ?").unwrap();
    stmt.bind((1, hash)).unwrap();
    if let Ok(sqlite::State::Row) = stmt.next() {
        let content: Vec<u8> = stmt.read(0).unwrap();
        crate::db::decompress(&content)
    } else {
        Vec::new()
    }
}

fn render_diff(old: &[u8], new: &[u8], mode: &str) -> String {
    let old_s = String::from_utf8_lossy(old);
    let new_s = String::from_utf8_lossy(new);
    
    // Si c'est binaire (approximatif)
    if old_s.contains('\0') || new_s.contains('\0') {
        return "<div class='diff-container'>(Binary file diff not shown)</div>".to_string();
    }

    if mode == "raw" {
        return format!("<div class='diff-container'><pre><code>{}</code></pre></div>", html_escape(&new_s));
    }

    let diff = similar::TextDiff::from_lines(&old_s, &new_s);
    
    if mode == "side-by-side" {
        let mut out = String::from("<table class='diff-ss-table'>");
        for opcode in diff.grouped_ops(3) {
            for op in opcode {
                match op {
                    similar::DiffOp::Equal { old_index, new_index, len } => {
                        for i in 0..len {
                            let old_line = diff.old_slices()[old_index + i];
                            let new_line = diff.new_slices()[new_index + i];
                            out.push_str(&format!(
                                "<tr class='diff-equal'>\
                                    <td class='diff-line-num'>{}</td><td class='diff-ss-left'>{}</td>\
                                    <td class='diff-line-num'>{}</td><td class='diff-ss-right'>{}</td>\
                                 </tr>",
                                old_index + i + 1, html_escape(&old_line.to_string()),
                                new_index + i + 1, html_escape(&new_line.to_string())
                            ));
                        }
                    }
                    similar::DiffOp::Delete { old_index, old_len, .. } => {
                        for i in 0..old_len {
                            let old_line = diff.old_slices()[old_index + i];
                            out.push_str(&format!(
                                "<tr class='diff-deleted'>\
                                    <td class='diff-line-num'>{}</td><td class='diff-ss-left'>{}</td>\
                                    <td class='diff-line-num'></td><td class='diff-ss-right diff-ghost'></td>\
                                 </tr>",
                                old_index + i + 1, html_escape(&old_line.to_string())
                            ));
                        }
                    }
                    similar::DiffOp::Insert { new_index, new_len, .. } => {
                        for i in 0..new_len {
                            let new_line = diff.new_slices()[new_index + i];
                            out.push_str(&format!(
                                "<tr class='diff-added'>\
                                    <td class='diff-line-num'></td><td class='diff-ss-left diff-ghost'></td>\
                                    <td class='diff-line-num'>{}</td><td class='diff-ss-right'>{}</td>\
                                 </tr>",
                                new_index + i + 1, html_escape(&new_line.to_string())
                            ));
                        }
                    }
                    similar::DiffOp::Replace { old_index, old_len, new_index, new_len } => {
                        let common = old_len.min(new_len);
                        for i in 0..common {
                            let old_line = diff.old_slices()[old_index + i];
                            let new_line = diff.new_slices()[new_index + i];
                            out.push_str(&format!(
                                "<tr>\
                                    <td class='diff-line-num diff-deleted'>{}</td><td class='diff-ss-left diff-deleted'>{}</td>\
                                    <td class='diff-line-num diff-added'>{}</td><td class='diff-ss-right diff-added'>{}</td>\
                                 </tr>",
                                old_index + i + 1, html_escape(&old_line.to_string()),
                                new_index + i + 1, html_escape(&new_line.to_string())
                            ));
                        }
                        if old_len > common {
                            for i in common..old_len {
                                let old_line = diff.old_slices()[old_index + i];
                                out.push_str(&format!(
                                    "<tr class='diff-deleted'>\
                                        <td class='diff-line-num'>{}</td><td class='diff-ss-left'>{}</td>\
                                        <td class='diff-line-num'></td><td class='diff-ss-right diff-ghost'></td>\
                                     </tr>",
                                    old_index + i + 1, html_escape(&old_line.to_string())
                                ));
                            }
                        } else if new_len > common {
                            for i in common..new_len {
                                let new_line = diff.new_slices()[new_index + i];
                                out.push_str(&format!(
                                    "<tr class='diff-added'>\
                                        <td class='diff-line-num'></td><td class='diff-ss-left diff-ghost'></td>\
                                        <td class='diff-line-num'>{}</td><td class='diff-ss-right'>{}</td>\
                                     </tr>",
                                    new_index + i + 1, html_escape(&new_line.to_string())
                                ));
                            }
                        }
                    }
                }
            }
        }
        out.push_str("</table>");
        return out;
    }

    let mut out = String::from("<div class='diff-container'>");
    for change in diff.iter_all_changes() {
        let (sign, class) = match change.tag() {
            similar::ChangeTag::Delete => ("-", "diff-deleted"),
            similar::ChangeTag::Insert => ("+", "diff-added"),
            similar::ChangeTag::Equal => (" ", "diff-equal"),
        };
        out.push_str(&format!("<span class='{}'>{}{}</span>", class, sign, html_escape(&change.to_string())));
    }
    out.push_str("</div>");
    out
}

// 3. PAGE DE FICHIER : VOIR LE CONTENU
async fn show_file(
    State(state): State<Arc<AppState>>,
    UrlPath(hash): UrlPath<String>,
) -> impl IntoResponse {
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

        const MAX_PREVIEW_BYTES: usize = 512 * 1024; // 512 KiB

        let mut body = String::new();
        body.push_str("<div style='margin-bottom: 20px;'><a href='javascript:history.back()'>&larr; Back</a></div>");
        body.push_str(&format!(
            "<p class='age' style='margin-bottom: 15px;'>hash: <span class='hash'>{}</span> — size: {} bytes — <a href='/raw/{}'>Download raw</a></p>",
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
                body.push_str(&format!("<pre class='line-numbers'><code>{}</code></pre>", html_escape(&text)));
                page("File View", "", &body).into_response()
            }
            Err(_) => {
                body.push_str(
                    "<p><strong>Binary content</strong> (cannot render as UTF-8 text). Use <a href='/raw/",
                );
                body.push_str(&html_escape(&hash));
                body.push_str("'>Download raw</a>.</p>");
                page("File View", "", &body).into_response()
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

async fn serve_rss(
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    let conn = match state.conn.lock() {
        Ok(g) => g,
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "DB lock poisoned").into_response(),
    };

    // On récupère le nom du dossier actuel pour identifier le flux RSS
    let repo_name = std::env::current_dir()
        .ok()
        .and_then(|p| p.file_name().map(|s| s.to_string_lossy().into_owned()))
        .unwrap_or_else(|| "Lys".to_string());

    let query = "SELECT id, hash, author, message, timestamp FROM commits ORDER BY id DESC LIMIT 50";
    let mut stmt = match conn.prepare(query) {
        Ok(s) => s,
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "Failed to query commits").into_response(),
    };

    let mut items = String::new();
    while let Ok(sqlite::State::Row) = stmt.next() {
        let id: i64 = stmt.read("id").unwrap_or(0);
        let hash: String = stmt.read("hash").unwrap_or_default();
        let msg: String = stmt.read("message").unwrap_or_else(|_| String::from("(no message)"));
        let date_str: String = stmt.read("timestamp").unwrap_or_else(|_| String::from(""));
        let author: String = stmt.read("author").unwrap_or_else(|_| String::from("Unknown"));

        // RSS needs RFC822/2822 dates. Let's try to convert our RFC3339.
        let pub_date = match DateTime::parse_from_rfc3339(&date_str) {
            Ok(dt) => dt.to_rfc2822(),
            Err(_) => date_str.clone(),
        };

        let title = msg.lines().next().unwrap_or("Commit");

        items.push_str(&format!(
            "<item>\n\
                <title>{}</title>\n\
                <link>http://localhost:3000/commit/{}</link>\n\
                <description>{}</description>\n\
                <author>{}</author>\n\
                <pubDate>{}</pubDate>\n\
                <guid isPermaLink='false'>{}</guid>\n\
             </item>\n",
            html_escape(title),
            id,
            html_escape(&msg),
            html_escape(&author),
            pub_date,
            hash
        ));
    }

    let rss = format!(
        "<?xml version='1.0' encoding='UTF-8' ?>\n\
         <rss version='2.0'>\n\
         <channel>\n\
             <title>Lys Commits - {}</title>\n\
             <link>http://localhost:3000/</link>\n\
             <description>Latest commits from {} repository</description>\n\
             <language>en-us</language>\n\
             {}\n\
         </channel>\n\
         </rss>",
        html_escape(&repo_name),
        html_escape(&repo_name),
        items
    );

    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, header::HeaderValue::from_static("application/rss+xml"));

    (StatusCode::OK, headers, rss).into_response()
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
    match crate::crypto::verify_signature(root_path, &hash, signature) {
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
