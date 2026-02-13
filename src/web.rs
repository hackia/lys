
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
use pulldown_cmark::{Options, Parser, html as cmark_html};
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
        .tabs { display: flex; border-bottom: 1px solid var(--border); margin-bottom: 20px; }
        .tab { padding: 8px 20px; cursor: pointer; border: 1px solid transparent; border-bottom: none; margin-bottom: -1px; border-radius: 4px 4px 0 0; font-size: 0.9em; font-weight: bold; }
        .tab.active { background: var(--bg); border-color: var(--border); color: var(--link); }
        .tab-content { display: none; }
        .tab-content.active { display: block; }
        .markdown-body { font-family: -apple-system, BlinkMacSystemFont, \"Segoe UI\", Helvetica, Arial, sans-serif; font-size: 16px; line-height: 1.5; word-wrap: break-word; }
        .markdown-body h1, .markdown-body h2, .markdown-body h3 { border-bottom: 1px solid var(--border); padding-bottom: 0.3em; }
        .markdown-body code { background-color: var(--code-bg); padding: 0.2em 0.4em; border-radius: 6px; font-family: monospace; }
        .markdown-body pre { padding: 16px; overflow: auto; line-height: 1.45; background-color: var(--code-bg); border-radius: 6px; }
        .markdown-body blockquote { padding: 0 1em; color: var(--meta); border-left: 0.25em solid var(--border); margin: 0; }
        .markdown-body ul, .markdown-body ol { padding-left: 2em; }
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
             <script>
               function openTab(evt, tabName) {{
                 var i;
                 var x = document.getElementsByClassName('tab-content');
                 for (i = 0; i < x.length; i++) {{
                   x[i].style.display = 'none';
                 }}
                 var tabs = document.getElementsByClassName('tab');
                 for (i = 0; i < tabs.length; i++) {{
                   tabs[i].className = tabs[i].className.replace(' active', '');
                 }}
                 document.getElementById(tabName).style.display = 'block';
                 evt.currentTarget.className += ' active';
               }}
               function copyToClipboard(elementId) {{
                 const element = document.getElementById(elementId);
                 const text = element.innerText || element.textContent;
                 navigator.clipboard.writeText(text).then(() => {{
                   const btn = event.target;
                   const originalText = btn.innerText;
                   btn.innerText = 'Copied!';
                   setTimeout(() => btn.innerText = originalText, 2000);
                 }});
               }}
             </script>
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
        let mut stmt = match conn.prepare("SELECT COUNT(*) FROM commits") {
            Ok(s) => s,
            Err(_) => return http_error(StatusCode::INTERNAL_SERVER_ERROR, "Failed to prepare count query"),
        };
        if let Ok(sqlite::State::Row) = stmt.next() {
            stmt.read(0).unwrap_or(0)
        } else {
            0
        }
    };
    let total_pages = (total_commits as f64 / per_page as f64).ceil() as i64;

    let contributors = crate::db::get_unique_contributors(&conn).unwrap_or_default();
    let contributor_names: Vec<String> = contributors.iter().map(|(n, _)| n.clone()).collect();

    let mut stats_tab = String::from("<div style='display: grid; grid-template-columns: 1fr 1fr; gap: 20px;'>");

    // Left: Author stats table
    stats_tab.push_str("<div><h3>Commits by Author</h3><table><thead><tr><th>Author</th><th>Commits</th></tr></thead><tbody>");
    for (name, count) in &contributors {
        stats_tab.push_str(&format!("<tr><td>{}</td><td>{}</td></tr>", html_escape(name), count));
    }
    stats_tab.push_str("</tbody></table></div>");

    // Right: Global stats
    stats_tab.push_str("<div><h3>Global Statistics</h3>");
    stats_tab.push_str(&format!(
        "<div style='background: var(--menu-bg); padding: 15px; border: 1px solid var(--border); border-radius: 4px;'>\
           <div style='display: grid; grid-template-columns: auto 1fr; gap: 10px 20px; font-size: 0.9em;'>\
             <strong>Total Commits:</strong> <span>{}</span>\
             <strong>Total Contributors:</strong> <span>{}</span>\
           </div>\
         </div>",
        total_commits, contributors.len()
    ));
    stats_tab.push_str("</div></div>");

    let query = "SELECT id, hash, author, message, timestamp FROM commits ORDER BY id DESC LIMIT ? OFFSET ?";
    let mut rows = String::new();

    let mut stmt = match conn.prepare(query) {
        Ok(s) => s,
        Err(_) => return http_error(StatusCode::INTERNAL_SERVER_ERROR, "Failed to query commits"),
    };
    if let Err(_) = stmt.bind((1, per_page as i64)) {
        return http_error(StatusCode::INTERNAL_SERVER_ERROR, "Failed to bind limit");
    }
    if let Err(_) = stmt.bind((2, offset as i64)) {
        return http_error(StatusCode::INTERNAL_SERVER_ERROR, "Failed to bind offset");
    }

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
                    <div class='age'>{} — {}</div>\
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

    let mut body = String::new();
    body.push_str("<div class='tabs'>");
    body.push_str("<div class='tab active' onclick=\"openTab(event, 'tab-log')\">Log</div>");
    body.push_str("<div class='tab' onclick=\"openTab(event, 'tab-contributors')\">Contributors</div>");
    body.push_str("<div class='tab' onclick=\"openTab(event, 'tab-stats')\">Stats</div>");
    body.push_str("</div>");

    body.push_str("<div id='tab-log' class='tab-content active'>");
    body.push_str("<h3 id='latest'>Latest Commits</h3>");
    body.push_str("<table>");
    body.push_str(&rows);
    body.push_str("</table>");
    body.push_str(&nav_html);
    body.push_str("</div>");

    body.push_str("<div id='tab-contributors' class='tab-content'>");
    body.push_str("<h3>Contributors</h3><ul>");
    for name in contributor_names {
        body.push_str(&format!("<li>{}</li>", html_escape(&name)));
    }
    body.push_str("</ul></div>");

    body.push_str("<div id='tab-stats' class='tab-content'>");
    body.push_str(&stats_tab);
    body.push_str("</div>");

    page(
        "Lys Repository",
        "",
        &body,
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

    // Search tags for this commit
    let mut tags = Vec::new();
    {
        let mut stmt_t = match conn.prepare("SELECT key FROM config WHERE key LIKE 'tag_%' AND value = ?") {
            Ok(s) => s,
            Err(_) => return http_error(StatusCode::INTERNAL_SERVER_ERROR, "Failed to prepare tags query"),
        };
        if stmt_t.bind((1, hash.as_str())).is_ok() {
            while let Ok(sqlite::State::Row) = stmt_t.next() {
                if let Ok(tag_key) = stmt_t.read::<String, _>(0) {
                    tags.push(tag_key.replace("tag_", ""));
                }
            }
        }
    }
    let tags_html = if tags.is_empty() { 
        String::new() 
    } else { 
        format!("<tr><td><b>tags</b></td><td>{}</td></tr>", 
            tags.iter().map(|t| format!("<span class='btn' style='margin-bottom:5px; background:var(--link); color:white; border:none;'>{}</span>", html_escape(t))).collect::<Vec<_>>().join(" ")) 
    };

    page(
        &format!("Commit {}", short_hash(&hash)),
        ".commit-info td { border: none; padding: 4px 12px; }",
        &format!(
            "<h3>Commit Details</h3>\
            <table class='commit-info' style='margin-bottom: 25px; width: 100%; border: 1px solid var(--border); background: var(--menu-bg);'>
               <tr><td><b>author</b></td><td>{}</td></tr>
               <tr><td><b>date</b></td><td>{} ({})</td></tr>
               <tr><td><b>commit</b></td><td class='hash'>{}</td></tr>
               {}
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
            tags_html,
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
        if let Err(_) = stmt.bind((1, current_tree_hash.as_str())) {
            return http_error(StatusCode::INTERNAL_SERVER_ERROR, "Failed to bind parent hash");
        }
        if let Err(_) = stmt.bind((2, *comp)) {
            return http_error(StatusCode::INTERNAL_SERVER_ERROR, "Failed to bind component name");
        }

        if let Ok(sqlite::State::Row) = stmt.next() {
            let mode: i64 = stmt.read("mode").unwrap_or(0);
            let is_dir = mode == 16384 || mode == 0o040000 || mode == 0o755;
            if is_dir {
                current_tree_hash = stmt.read("hash").unwrap_or_default();
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
    tree_html.push_str("<thead><tr><th style='width: 20px;'></th><th>Name</th><th style='text-align: right;'>Size</th></tr></thead>");
    let mut summary_html = String::new();
    if let Err(e) = render_tree_html_flat(&conn, commit_id, &path_str, &current_tree_hash, &mut tree_html, &mut summary_html) {
        return http_error(StatusCode::INTERNAL_SERVER_ERROR, &format!("Failed to render tree: {}", e));
    }

    // Special files detection and rendering
    let mut special_content = std::collections::BTreeMap::new();
    let special_files = [
        ("README", vec!["README.md", "README"]),
        ("LICENSE", vec!["LICENSE", "LICENSE.md"]),
        ("CODE_OF_CONDUCT", vec!["CODE_OF_CONDUCT.md"]),
        ("CONTRIBUTING", vec!["CONTRIBUTING.md"]),
    ];

    for (label, filenames) in special_files {
        for sf in filenames {
            let query = "SELECT hash FROM tree_nodes WHERE parent_tree_hash = ? AND name = ?";
            let mut stmt = match conn.prepare(query) {
                Ok(s) => s,
                Err(_) => continue,
            };
            if stmt.bind((1, current_tree_hash.as_str())).is_err() { continue; }
            if stmt.bind((2, sf)).is_err() { continue; }
            if let Ok(sqlite::State::Row) = stmt.next() {
                if let Ok(hash) = stmt.read::<String, _>(0) {
                    let bytes = get_raw_blob(&conn, &hash);
                    if let Ok(text) = String::from_utf8(bytes) {
                        let content_html = if sf.ends_with(".md") {
                            let mut options = Options::empty();
                            options.insert(Options::ENABLE_TABLES);
                            options.insert(Options::ENABLE_FOOTNOTES);
                            options.insert(Options::ENABLE_STRIKETHROUGH);
                            options.insert(Options::ENABLE_TASKLISTS);
                            let parser = Parser::new_ext(&text, options);
                            let mut html_output = String::new();
                            cmark_html::push_html(&mut html_output, parser);
                            format!("<div class='markdown-body'>{}</div>", html_output)
                        } else {
                            format!("<pre style='white-space: pre-wrap; font-family: sans-serif;'>{}</pre>", html_escape(&text))
                        };
                        special_content.insert(label.to_string(), content_html);
                        break; // Found one for this category
                    }
                }
            }
        }
    }

    let mut tabs_headers = String::new();
    let mut tabs_bodies = String::new();
    let active_tab = "FILES";

    // Files tab
    tabs_headers.push_str(&format!("<div class='tab {}' onclick='openTab(event, \"files-tab\")'>Files</div>", if active_tab == "FILES" { "active" } else { "" }));
    tabs_bodies.push_str(&format!("<div id='files-tab' class='tab-content {}'>{}<table class='tree-table'>{}</table></div>", if active_tab == "FILES" { "active" } else { "" }, summary_html, tree_html));

    // README tab
    if let Some(content) = special_content.get("README") {
        tabs_headers.push_str(&format!("<div class='tab {}' onclick='openTab(event, \"readme-tab\")'>README</div>", if active_tab == "README" { "active" } else { "" }));
        tabs_bodies.push_str(&format!("<div id='readme-tab' class='tab-content {}'>{}</div>", if active_tab == "README" { "active" } else { "" }, content));
    }

    // Other special tabs
    for (label, content) in &special_content {
        if label == "README" { continue; }
        let tab_id = format!("{}-tab", label.to_lowercase().replace("_", "-"));
        tabs_headers.push_str(&format!("<div class='tab' onclick='openTab(event, \"{}\")'>{}</div>", tab_id, label.replace("_", " ")));
        tabs_bodies.push_str(&format!("<div id='{}' class='tab-content'>{}</div>", tab_id, content));
    }

    let tabs_html = format!(
        "<div class='tabs'>{}</div>{}",
        tabs_headers, tabs_bodies
    );

    let script = "
        <script>
        function openTab(evt, tabName) {
            var i, tabcontent, tablinks;
            tabcontent = document.getElementsByClassName('tab-content');
            for (i = 0; i < tabcontent.length; i++) {
                tabcontent[i].style.display = 'none';
                tabcontent[i].classList.remove('active');
            }
            tablinks = document.getElementsByClassName('tab');
            for (i = 0; i < tablinks.length; i++) {
                tablinks[i].classList.remove('active');
            }
            document.getElementById(tabName).style.display = 'block';
            document.getElementById(tabName).classList.add('active');
            evt.currentTarget.classList.add('active');
        }
        </script>
    ";

    page(
        &format!("Tree - {}", short_hash(&commit_hash)),
        ".tree-table { width: 100%; border-collapse: collapse; font-family: monospace; border: none; }
         .tree-table td, .tree-table th { padding: 8px 12px; border: none; border-bottom: 1px solid var(--border); }
         .tree-table tr:last-child td { border-bottom: none; }
         .tree-table tr:hover { background: var(--hover-bg); }
         .icon { margin-right: 8px; }
         .dir { font-weight: bold; }
         .breadcrumbs { margin-bottom: 20px; font-family: monospace; background: var(--nav-bg); padding: 12px; border: 1px solid var(--border); border-radius: 4px; }",
        &format!(
            "<h3>Tree View</h3>\
             <div class='breadcrumbs'>{}</div>\
             {}\
             {}",
            breadcrumbs,
            tabs_html,
            script
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
    summary_out: &mut String,
) -> Result<(), Box<dyn std::error::Error>> {
    let query = "SELECT name, hash, mode, size FROM tree_nodes WHERE parent_tree_hash = ? ORDER BY name ASC";
    let mut stmt = conn.prepare(query)?;
    stmt.bind((1, tree_hash))?;

    let mut entries = Vec::new();
    while let Ok(sqlite::State::Row) = stmt.next() {
        entries.push((
            stmt.read::<String, _>("name")?,
            stmt.read::<String, _>("hash")?,
            stmt.read::<i64, _>("mode")?,
            stmt.read::<i64, _>("size")?,
        ));
    }

    // Trier : Dossiers d'abord, puis fichiers, par nom
    entries.sort_by(|a, b| {
        let a_is_dir = a.2 == 16384 || a.2 == 0o040000 || a.2 == 0o755;
        let b_is_dir = b.2 == 16384 || b.2 == 0o040000 || b.2 == 0o755;
        if a_is_dir != b_is_dir {
            b_is_dir.cmp(&a_is_dir)
        } else {
            a.0.cmp(&b.0)
        }
    });

    let mut dir_count = 0;
    let mut file_count = 0;
    let mut total_size = 0;

    for (_name, _hash, mode, size) in &entries {
        if *mode == 16384 || *mode == 0o040000 || *mode == 0o755 {
            dir_count += 1;
        } else {
            file_count += 1;
            total_size += size;
        }
    }

    let summary_html = format!(
        "<div style='margin-bottom: 15px; font-size: 0.85em; color: var(--meta);'>\
            Summary: {} directories, {} files ({} bytes)\
         </div>",
        dir_count, file_count, total_size
    );
    summary_out.push_str(&summary_html);

    for (name, hash, mode, size) in entries {
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

        let size_str = if is_dir {
            "-".to_string()
        } else {
            format!("{} B", size)
        };

        out.push_str(&format!(
            "<tr>\
                <td style='width: 20px;'><span class='icon'>{}</span></td>\
                <td>{}</td>\
                <td style='text-align: right; color: var(--meta); font-size: 0.8em;'>{}</td>\
             </tr>",
            icon,
            link,
            size_str
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
        if let Err(_) = stmt.bind((1, commit_id)) {
            return http_error(StatusCode::INTERNAL_SERVER_ERROR, "Failed to bind commit ID");
        }
        if let Ok(sqlite::State::Row) = stmt.next() {
            commit_hash = stmt.read("hash").unwrap_or_default();
            tree_hash = stmt.read("tree_hash").unwrap_or_default();
            let current_db_id: i64 = stmt.read("id").unwrap_or(0);

            if let Ok(mut stmt_p) = conn.prepare("SELECT tree_hash FROM commits WHERE id < ? ORDER BY id DESC LIMIT 1") {
                if stmt_p.bind((1, current_db_id)).is_ok() {
                    if let Ok(sqlite::State::Row) = stmt_p.next() {
                        parent_tree_hash = stmt_p.read(0).ok();
                    }
                }
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

    for (i, path) in paths.into_iter().enumerate() {
        let old_info = parent_state.get(path);
        let new_info = current_state.get(path);
        let diff_id = format!("diff-content-{}", i);

        match (old_info, new_info) {
            (Some((old_hash, _)), Some((new_hash, _))) if old_hash != new_hash => {
                // Modified
                let old_bytes = get_raw_blob(&conn, old_hash);
                let new_bytes = get_raw_blob(&conn, new_hash);
                diff_html.push_str(&format!(
                    "<div class='diff-file-header'>\
                       <button class='btn copy-btn' onclick='copyToClipboard(\"{}\")'>Copy</button>\
                       <strong>Modified: {}</strong>\
                     </div>", 
                    diff_id, path.display()
                ));
                diff_html.push_str(&format!("<div id='{}'>{}</div>", diff_id, render_diff(&old_bytes, &new_bytes, &mode)));
            }
            (None, Some((new_hash, _))) => {
                // Added
                let new_bytes = get_raw_blob(&conn, new_hash);
                diff_html.push_str(&format!(
                    "<div class='diff-file-header'>\
                       <button class='btn copy-btn' onclick='copyToClipboard(\"{}\")'>Copy</button>\
                       <strong>Added: {}</strong>\
                     </div>", 
                    diff_id, path.display()
                ));
                diff_html.push_str(&format!("<div id='{}'>{}</div>", diff_id, render_diff(&[], &new_bytes, &mode)));
            }
            (Some((old_hash, _)), None) => {
                // Deleted
                let old_bytes = get_raw_blob(&conn, old_hash);
                diff_html.push_str(&format!(
                    "<div class='diff-file-header'>\
                       <strong>Deleted: {}</strong>\
                     </div>", 
                    path.display()
                ));
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
         .btn-active { background: var(--link) !important; color: white !important; border-color: var(--link) !important; }
         .copy-btn { float: right; padding: 2px 8px; font-size: 0.8em; margin-top: -2px; cursor: pointer; }
         .diff-file-header { background: var(--header-bg); padding: 8px 12px; border: 1px solid var(--border); border-bottom: none; border-radius: 4px 4px 0 0; margin-top: 20px; font-family: monospace; }
         .diff-container { margin-top: 0 !important; border-top-left-radius: 0 !important; border-top-right-radius: 0 !important; }",
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
    if let Ok(mut stmt) = conn.prepare("SELECT content FROM store.blobs WHERE hash = ?") {
        if stmt.bind((1, hash)).is_ok() {
            if let Ok(sqlite::State::Row) = stmt.next() {
                if let Ok(content) = stmt.read::<Vec<u8>, _>(0) {
                    return crate::db::decompress(&content);
                }
            }
        }
    }
    Vec::new()
}

fn render_diff(old: &[u8], new: &[u8], mode: &str) -> String {
    let old_s = String::from_utf8_lossy(old);
    let new_s = String::from_utf8_lossy(new);
    
    // Si c'est binaire (approximatif)
    if old_s.contains('\0') || new_s.contains('\0') {
        return "<div class='diff-container'>(Binary file diff not shown)</div>".to_string();
    }

    if mode == "raw" {
        return format!("<div class='diff-container'><pre style='margin:0;'><code>{}</code></pre></div>", html_escape(&new_s));
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

        // On essaie de trouver le nom du fichier pour la coloration syntaxique
        let mut filename = String::new();
        let name_query = "SELECT name FROM tree_nodes WHERE hash = ? LIMIT 1";
        if let Ok(mut name_stmt) = conn.prepare(name_query) {
            if name_stmt.bind((1, hash.as_str())).is_ok() {
                if let Ok(sqlite::State::Row) = name_stmt.next() {
                    filename = name_stmt.read(0).unwrap_or_default();
                }
            }
        }

        // Decompress (falls back to raw if it's not zlib-compressed)
        let bytes = decompress(&content);

        const MAX_PREVIEW_BYTES: usize = 512 * 1024; // 512 KiB

        let mut body = String::new();
        body.push_str("<div style='margin-bottom: 20px;'><a href='javascript:history.back()'>&larr; Back</a></div>");
        body.push_str(&format!(
            "<p class='age' style='margin-bottom: 15px;'>file: <strong>{}</strong> — hash: <span class='hash'>{}</span> — size: {} bytes — <a href='/raw/{}'>Download raw</a></p>",
            html_escape(&filename),
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
                
                // Déterminer la classe de langage pour Prism de manière plus générique
                let extension = Path::new(&filename)
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .unwrap_or("");

                let lang_class = match extension {
                    "rs" => "language-rust",
                    "py" => "language-python",
                    "js" => "language-javascript",
                    "mjs" => "language-javascript",
                    "ts" => "language-typescript",
                    "c" => "language-c",
                    "cpp" | "cc" | "cxx" | "h" | "hpp" => "language-cpp",
                    "md" => "language-markdown",
                    "toml" => "language-toml",
                    "html" | "htm" => "language-html",
                    "css" => "language-css",
                    "sh" | "bash" | "run" | "zsh" => "language-bash",
                    "pl" | "pm" => "language-perl",
                    "rb" => "language-ruby",
                    "go" => "language-go",
                    "java" => "language-java",
                    "kt" | "kts" => "language-kotlin",
                    "php" => "language-php",
                    "sql" => "language-sql",
                    "yaml" | "yml" => "language-yaml",
                    "json" => "language-json",
                    "xml" => "language-xml",
                    "diff" => "language-diff",
                    "dockerfile" | "Dockerfile" => "language-docker",
                    "am" | "make" | "mak" => "language-makefile",
                    "lua" => "language-lua",
                    "swift" => "language-swift",
                    "dart" => "language-dart",
                    "elm" => "language-elm",
                    "ex" | "exs" => "language-elixir",
                    "erl" | "hrl" => "language-erlang",
                    "fs" | "fsx" => "language-fsharp",
                    "groovy" => "language-groovy",
                    "hs" => "language-haskell",
                    "nim" => "language-nim",
                    "scala" | "sc" => "language-scala",
                    "vim" => "language-vim",
                    "zig" => "language-zig",
                    _ => "language-none",
                };

                body.push_str(&format!("<pre class='line-numbers'><code class='{}'>{}</code></pre>", lang_class, html_escape(&text)));
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
    loop {
        match stmt.next() {
            Ok(sqlite::State::Row) => {
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
            _ => break,
        }
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

            if let Err(_) = stmt.bind((1, hash.as_str())) {
                return StatusCode::INTERNAL_SERVER_ERROR;
            }
            if let Err(_) = stmt.bind((2, &body[..])) {
                return StatusCode::INTERNAL_SERVER_ERROR;
            }
            if let Err(_) = stmt.bind((3, body.len() as i64)) {
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
