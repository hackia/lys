use crate::db::decompress;
use crate::utils::ok;
use axum::{
    Router,
    body::Bytes,
    extract::{
        Path as UrlPath, Query, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::{HeaderMap, StatusCode, header},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
};
use chrono::{DateTime, Utc};
use dashmap::DashMap;
use once_cell::sync::OnceCell;
use pulldown_cmark::{Options, Parser, html as cmark_html};
use serde::Deserialize;
use sqlite::Connection;
use std::net::SocketAddr;
use std::path::Path;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;

pub struct Session {
    pub history: Mutex<Vec<String>>,
    pub tx: broadcast::Sender<String>,
}

pub struct AppState {
    pub conn: Mutex<Connection>,
    pub sessions: DashMap<String, Arc<Session>>,
    pub chat_tx: broadcast::Sender<String>,
}

static WEB_TITLE: OnceCell<String> = OnceCell::new();
static WEB_SUBTITLE: OnceCell<String> = OnceCell::new();
static WEB_FOOTER: OnceCell<String> = OnceCell::new();
static WEB_HOMEPAGE: OnceCell<String> = OnceCell::new();
static WEB_DOCUMENTATION: OnceCell<String> = OnceCell::new();

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
#[derive(Deserialize)]
struct CommitForm {
    summary: String,
    why: String,
    how: String,
    outcome: String,
}

#[derive(Deserialize)]
struct EditorForm {
    content: String,
}

#[derive(Deserialize)]
struct NewFileForm {
    path: String,
}

#[derive(Deserialize)]
struct TodoForm {
    title: String,
    assigned_to: Option<String>,
    due_date: Option<String>,
}

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

fn get_spotify_embed_url(url: &str) -> Option<String> {
    if url.contains("spotify.com/embed/") {
        return Some(url.to_string());
    }

    // Support tracks, albums, playlists
    // format: https://open.spotify.com/album/4EFDM5bjlaF1xx3sNjutFE?utm_source=generator
    let base_url = url.split('?').next()?;
    if let Some(pos) = base_url.find("spotify.com/") {
        let path = &base_url[pos + 12..];
        return Some(format!("https://open.spotify.com/embed/{}", path));
    }
    None
}

fn get_youtube_embed_url(url: &str) -> Option<String> {
    if url.contains("youtube.com/embed/") {
        return Some(url.to_string());
    }

    // Support music.youtube.com and youtube.com
    // music.youtube.com/watch?v=ID...
    // youtube.com/watch?v=ID...
    // youtube.com/v/ID
    // youtu.be/ID

    if url.contains("youtu.be/") {
        let id = url.split("youtu.be/").nth(1)?.split('?').next()?;
        return Some(format!("https://www.youtube.com/embed/{}", id));
    }

    if let Some(pos) = url.find("v=") {
        let id = &url[pos + 2..].split('&').next()?;
        return Some(format!("https://www.youtube.com/embed/{}", id));
    }

    if url.contains("/v/") {
        let id = url.split("/v/").nth(1)?.split('?').next()?;
        return Some(format!("https://www.youtube.com/embed/{}", id));
    }

    None
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
        #content { padding: 30px 25px; max-width: 1300px; margin: 0 auto; }
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
        .tabs { display: flex; border-bottom: 2px solid var(--border); margin-bottom: 20px; gap: 5px; }
        .tab { padding: 10px 25px; cursor: pointer; border: 1px solid transparent; border-bottom: none; margin-bottom: -2px; border-radius: 6px 6px 0 0; font-size: 0.95em; font-weight: 600; color: var(--meta); transition: all 0.2s; }
        .tab:hover { background: var(--hover-bg); color: var(--fg); }
        .tab.active { background: var(--bg); border: 2px solid var(--border); border-bottom: 2px solid var(--bg); color: var(--link); }
        .tab-content { display: none; }
        .tab-content.active { display: block; }
        .markdown-body { font-family: -apple-system, BlinkMacSystemFont, \"Segoe UI\", Helvetica, Arial, sans-serif; font-size: 16px; line-height: 1.5; word-wrap: break-word; }
        .markdown-body h1, .markdown-body h2, .markdown-body h3 { border-bottom: 1px solid var(--border); padding-bottom: 0.3em; }
        .markdown-body code { background-color: var(--code-bg); padding: 0.2em 0.4em; border-radius: 6px; font-family: monospace; }
        .markdown-body pre { padding: 16px; overflow: auto; line-height: 1.45; background-color: var(--code-bg); border-radius: 6px; }
        .markdown-body blockquote { padding: 0 1em; color: var(--meta); border-left: 0.25em solid var(--border); margin: 0; }
        .markdown-body ul, .markdown-body ol { padding-left: 2em; }
    ";

    let site_title = WEB_TITLE
        .get()
        .map(String::as_str)
        .unwrap_or("Lys Repository");
    let site_subtitle = WEB_SUBTITLE
        .get()
        .map(String::as_str)
        .unwrap_or("A secure local-first vcs");
    let site_footer = WEB_FOOTER.get().map(String::as_str).unwrap_or("");
    let site_homepage = WEB_HOMEPAGE.get().map(String::as_str).unwrap_or("");
    let site_documentation = WEB_DOCUMENTATION.get().map(String::as_str).unwrap_or("");

    let mut menu_links =
        String::from("<a href='/'>Summary</a><a href='/'>Log</a><a href='/rss'>RSS</a><a href='/editor'>Editor</a><a href='/commit/new'>Commit</a><a href='/todo'>Todo</a><a href='/chat'>Chat</a>");
    if !site_homepage.is_empty() {
        menu_links.push_str(&format!(
            "<a href='{}' target='_blank'>Homepage</a>",
            html_escape(site_homepage)
        ));
    }
    if !site_documentation.is_empty() {
        menu_links.push_str(&format!(
            "<a href='{}' target='_blank'>Documentation</a>",
            html_escape(site_documentation)
        ));
    }

    let footer_html = if site_footer.is_empty() {
        String::from(
            "<div id='footer' style='padding:20px; border-top:1px solid var(--border); margin:30px 25px 0 25px;'><small>&copy; 2026 Lys Inc.</small></div>",
        )
    } else {
        format!(
            "<div id='footer' style='padding:20px; border-top:1px solid var(--border); margin:30px 25px 0 25px;'>{}</div>",
            site_footer
        )
    };

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
               <h1>{}</h1>\
               <div class='repo-desc'>{}</div>\
             </div>\
             <div id='menu'>\
               {}\
             </div>\
             <div id='content'>{}</div>\
             {}\
             <script src='https://cdnjs.cloudflare.com/ajax/libs/prism/1.29.0/components/prism-core.min.js'></script>\
             <script src='https://cdnjs.cloudflare.com/ajax/libs/prism/1.29.0/plugins/autoloader/prism-autoloader.min.js'></script>\
             <script>
               function loadPage(event, pageNum) {{
                 event.preventDefault();
                 const logTab = document.getElementById('tab-log');
                 logTab.style.opacity = '0.5';
                 fetch('/api/commits?page=' + pageNum)
                   .then(response => response.text())
                   .then(html => {{
                     logTab.innerHTML = html;
                     logTab.style.opacity = '1';
                     window.history.pushState({{}}, '', '/?page=' + pageNum);
                   }})
                   .catch(err => {{
                     console.error('Failed to load commits:', err);
                     logTab.style.opacity = '1';
                   }});
               }}
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
           </body>
         </html>",
        html_escape(title),
        COMMON_STYLE,
        style,
        html_escape(site_title),
        html_escape(site_subtitle),
        menu_links,
        body,
        footer_html,
    ))
}

fn http_error(status: StatusCode, msg: &str) -> Response {
    (
        status,
        page(
            "Error",
            "body{font-family:sans-serif;max-width:800px;margin:auto;padding:20px}",
            &format!(
                "<h2>Error</h2><p>{}</p><p><a href='/'>Back</a></p>",
                html_escape(msg)
            ),
        ),
    )
        .into_response()
}

async fn ws_handler(ws: WebSocketUpgrade, State(state): State<Arc<AppState>>) -> impl IntoResponse {
    ws.on_upgrade(|socket| handle_socket(socket, state))
}

async fn handle_socket(mut socket: WebSocket, state: Arc<AppState>) {
    let shell = crate::shell::Shell::new();
    let mut current_session_id = "default".to_string();

    // S'assurer que la session par défaut existe
    {
        if !state.sessions.contains_key(&current_session_id) {
            let (tx, _) = broadcast::channel(100);
            state.sessions.insert(
                current_session_id.clone(),
                Arc::new(Session {
                    history: Mutex::new(Vec::new()),
                    tx,
                }),
            );
        }
    }

    let mut rx = {
        let session = state.sessions.get(&current_session_id).unwrap();

        // Envoyer l'historique au nouveau client
        let history = {
            let h = session.history.lock().unwrap();
            h.clone()
        };
        for line in history.iter() {
            let _ = socket.send(Message::Text(line.clone().into())).await;
        }

        session.tx.subscribe()
    };

    // Welcome message
    let _ = socket
        .send(Message::Text(
            "\r\n--- Attached to Lys Session: default ---\r\n"
                .to_string()
                .into(),
        ))
        .await;

    let mut socket_tx = state.sessions.get(&current_session_id).unwrap().tx.clone();

    loop {
        tokio::select! {
            result = rx.recv() => {
                if let Ok(msg) = result {
                    if socket.send(Message::Text(msg.into())).await.is_err() {
                        break;
                    }
                }
            }
            msg = socket.recv() => {
                if let Some(Ok(msg)) = msg {
                    match msg {
                        Message::Text(text) => {
                            let input = text.trim();

                            // Gérer les requêtes d'auto-complétion (format: "complete:command")
                            if input.starts_with("complete:") {
                                let cmd_to_complete = &input[9..];
                                let suggestions = shell.complete_command(cmd_to_complete);
                                if !suggestions.is_empty() {
                                    let resp = format!("suggestions:{}", suggestions.join(","));
                                    let _ = socket.send(Message::Text(resp.into())).await;
                                }
                                continue;
                            }

                            if input.is_empty() {
                                // Just prompt
                                let _ = socket.send(Message::Text("lys> ".to_string().into())).await;
                                continue;
                            }

                            if input == "exit" || input == "quit" {
                                break;
                            }

                            if input == "clear" {
                                // Effacer le terminal côté client
                                let _ = socket.send(Message::Text("\x1b[2J\x1b[H".to_string().into())).await;
                                let _ = socket.send(Message::Text("lys> ".to_string().into())).await;
                                continue;
                            }

                            if input.starts_with("session ") {
                                let parts: Vec<&str> = input.split_whitespace().collect();
                                if parts.len() > 1 {
                                    let new_id = parts[1].to_string();
                                    // Switch session
                                    current_session_id = new_id;
                                    if !state.sessions.contains_key(&current_session_id) {
                                        let (tx, _) = broadcast::channel(100);
                                        state.sessions.insert(current_session_id.clone(), Arc::new(Session {
                                            history: Mutex::new(Vec::new()),
                                            tx,
                                        }));
                                    }
                                    let session = state.sessions.get(&current_session_id).unwrap();
                                    rx = session.tx.subscribe();
                                    socket_tx = session.tx.clone();

                                    let _ = socket.send(Message::Text(format!("\r\n--- Switched to Session: {} ---\r\n", current_session_id).into())).await;

                                    {
                                        let history = {
                                            let h = session.history.lock().unwrap();
                                            h.clone()
                                        };
                                        for line in history.iter() {
                                            let _ = socket.send(Message::Text(line.clone().into())).await;
                                        }
                                    }
                                    continue;
                                }
                            }

                            if input == "sessions" {
                                let mut list = String::from("\r\nActive sessions:\r\n");
                                for s in state.sessions.iter() {
                                    list.push_str(&format!("- {}\r\n", s.key()));
                                }
                                let _ = socket.send(Message::Text(list.into())).await;
                                let _ = socket.send(Message::Text("lys> ".to_string().into())).await;
                                continue;
                            }

                            // Exécution de la commande
                            let output = shell.execute_command(input);
                            if !output.is_empty() {
                                let formatted_output = output.replace("\n", "\r\n");
                                let full_output = format!("lys> {}\r\n{}", input, formatted_output);

                                // Ajouter à l'historique et diffuser
                                {
                                    let session = state.sessions.get(&current_session_id).unwrap();
                                    let mut history = session.history.lock().unwrap();
                                    history.push(full_output.clone());
                                    let _ = socket_tx.send(full_output);
                                }
                            } else {
                                let prompt_line = format!("lys> {}\r\n", input);
                                let session = state.sessions.get(&current_session_id).unwrap();
                                let mut history = session.history.lock().unwrap();
                                history.push(prompt_line.clone());
                                let _ = socket_tx.send(prompt_line);
                            }
                        }
                        Message::Close(_) => break,
                        _ => (),
                    }
                } else {
                    break;
                }
            }
        }
    }
}

pub async fn start_server(repo_path: &str, port: u16) {
    let path = PathBuf::from(repo_path);

    // On ouvre une connexion dédiée au serveur web
    let conn = crate::db::connect_lys(&path).expect("Failed to connect to DB");

    // Initialize site-wide options (title, subtitle, footer) from config once
    {
        // Helper to read a single key
        let read_key = |key: &str| -> Option<String> {
            let mut stmt = conn
                .prepare("SELECT value FROM config WHERE key = ?")
                .ok()?;
            stmt.bind((1, key)).ok()?;
            if let Ok(sqlite::State::Row) = stmt.next() {
                stmt.read::<String, _>(0).ok()
            } else {
                None
            }
        };
        if let Some(t) = read_key("web_title") {
            let _ = WEB_TITLE.set(t);
        } else {
            let _ = WEB_TITLE.set("Lys Repository".to_string());
        }
        if let Some(st) = read_key("web_subtitle") {
            let _ = WEB_SUBTITLE.set(st);
        } else {
            let _ = WEB_SUBTITLE.set("A secure local-first vcs".to_string());
        }
        if let Some(f) = read_key("web_footer") {
            let _ = WEB_FOOTER.set(f);
        } else {
            let _ = WEB_FOOTER.set(String::new());
        }
        if let Some(h) = read_key("web_homepage") {
            let _ = WEB_HOMEPAGE.set(h);
        } else {
            let _ = WEB_HOMEPAGE.set(String::new());
        }
        if let Some(d) = read_key("web_documentation") {
            let _ = WEB_DOCUMENTATION.set(d);
        } else {
            let _ = WEB_DOCUMENTATION.set(String::new());
        }
    }

    let (chat_tx, _) = broadcast::channel(100);
    let shared_state = Arc::new(AppState {
        conn: Mutex::new(conn),
        sessions: DashMap::new(),
        chat_tx,
    });

    let app = Router::new()
        .route("/", get(idx_commits))
        .route("/rss", get(serve_rss))
        .route("/ws", get(ws_handler))
        .route("/ws/chat", get(ws_chat_upgrade))
        .route("/chat", get(show_chat))
        .route("/commit/new", get(new_commit_form))
        .route("/commit/create", post(create_commit))
        .route("/commit/{id}", get(show_commit))
        .route("/commit/{id}/diff", get(show_commit_diff))
        .route("/commit/{id}/tree", get(show_commit_tree))
        .route("/commit/{id}/tree/{*path}", get(show_commit_tree))
        .route("/editor", get(editor_list))
        .route("/editor/new", post(editor_new))
        .route("/editor/{*path}", get(editor_edit).post(editor_save))
        .route("/todo", get(todo_list))
        .route("/todo/add", post(todo_add))
        .route("/todo/update/{id}/{status}", post(todo_update))
        .route("/file/{hash}", get(show_file))
        .route("/raw/{hash}", get(download_raw)) // <-- new: reliable way to view binary / huge files
        .route("/upload/{hash}", post(upload_atom))
        .route("/api/commits", get(api_commits))
        .with_state(shared_state);

    let addr = SocketAddr::from(([127, 0, 0, 1], port));

    ok(format!("Server running at https://{addr}").as_str());
    ok("Press Ctrl+C to stop.");

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

fn render_commits_list(conn: &Connection, page_num: usize) -> (String, String) {
    let per_page = 20;
    let offset = (page_num - 1) * per_page;

    let total_commits: i64 = {
        let mut stmt = conn.prepare("SELECT COUNT(*) FROM commits").unwrap();
        if let Ok(sqlite::State::Row) = stmt.next() {
            stmt.read(0).unwrap_or(0)
        } else {
            0
        }
    };
    let total_pages = (total_commits as f64 / per_page as f64).ceil() as i64;

    let query = "SELECT id, hash, author, message, timestamp FROM commits ORDER BY id DESC LIMIT ? OFFSET ?";
    let mut rows = String::new();

    let mut stmt = conn.prepare(query).unwrap();
    stmt.bind((1, per_page as i64)).unwrap();
    stmt.bind((2, offset as i64)).unwrap();

    while let Ok(sqlite::State::Row) = stmt.next() {
        let id: i64 = stmt.read("id").unwrap_or(0);
        let hash: String = stmt.read("hash").unwrap_or_default();
        let msg: String = stmt
            .read("message")
            .unwrap_or_else(|_| String::from("(no message)"));
        let date: String = stmt.read("timestamp").unwrap_or_else(|_| String::from(""));
        let author: String = stmt
            .read("author")
            .unwrap_or_else(|_| String::from("Unknown"));

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

    let mut nav_html = format!(
        "<div style='margin-top: 20px; font-size: 0.9em; padding: 10px; background: var(--nav-bg); border: 1px solid var(--border); border-radius: 4px;'>\
         <div style='margin-bottom: 10px;'>Page {} of {}</div>",
        page_num, total_pages
    );

    let mut links = Vec::new();
    if page_num > 1 {
        links.push(format!(
            "<a href='/?page={}' onclick='loadPage(event, {})'>&laquo; Newer</a>",
            page_num - 1,
            page_num - 1
        ));
    }
    if (page_num as i64) < total_pages {
        links.push(format!(
            "<a href='/?page={}' onclick='loadPage(event, {})'>Older &raquo;</a>",
            page_num + 1,
            page_num + 1
        ));
    }

    if !links.is_empty() {
        nav_html.push_str(&links.join(" | "));
    }
    nav_html.push_str("</div>");

    (rows, nav_html)
}

pub async fn api_commits(
    State(state): State<Arc<AppState>>,
    Query(pagination): Query<Pagination>,
) -> impl IntoResponse {
    let conn = match state.conn.lock() {
        Ok(g) => g,
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "DB lock poisoned").into_response(),
    };

    let page_num = pagination.page.unwrap_or(1).max(1);
    let (rows, nav) = render_commits_list(&conn, page_num);

    let html = format!(
        "<h3 id='latest'>Latest Commits</h3>\
         <table>{}</table>\
         {}",
        rows, nav
    );
    Html(html).into_response()
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

    // Spotify or YouTube Music URL
    let mut music_embed = String::new();
    {
        let mut stmt = conn
            .prepare("SELECT value FROM config WHERE key = 'spotify_url'")
            .unwrap();
        if let Ok(sqlite::State::Row) = stmt.next() {
            let url: String = stmt.read(0).unwrap();
            if let Some(embed_url) = get_spotify_embed_url(&url) {
                music_embed = format!(
                    "<div style='margin-bottom: 30px; border-radius: 12px; overflow: hidden; box-shadow: 0 4px 12px rgba(0,0,0,0.2);'>\
                       <iframe src='{}' width='100%' height='352' frameBorder='0' allowfullscreen='' allow='autoplay; clipboard-write; encrypted-media; fullscreen; picture-in-picture' loading='lazy'></iframe>\
                     </div>",
                    embed_url
                );
            } else if let Some(embed_url) = get_youtube_embed_url(&url) {
                music_embed = format!(
                    "<div style='margin-bottom: 30px; border-radius: 12px; overflow: hidden; box-shadow: 0 4px 12px rgba(0,0,0,0.2);'>\
                       <iframe width='100%' height='352' src='{}' title='YouTube music player' frameborder='0' allow='accelerometer; autoplay; clipboard-write; encrypted-media; gyroscope; picture-in-picture; web-share' allowfullscreen></iframe>\
                     </div>",
                    embed_url
                );
            }
        }
    }

    // YouTube Video Banner
    let mut video_banner = String::new();
    {
        let mut stmt = conn
            .prepare("SELECT value FROM config WHERE key = 'video_banner_url'")
            .unwrap();
        if let Ok(sqlite::State::Row) = stmt.next() {
            let url: String = stmt.read(0).unwrap();
            if let Some(embed_url) = get_youtube_embed_url(&url) {
                video_banner = format!(
                    "<div style='margin-bottom: 30px; border-radius: 12px; overflow: hidden; box-shadow: 0 4px 12px rgba(0,0,0,0.2); aspect-ratio: 16 / 9;'>\
                       <iframe width='100%' height='100%' src='{}' title='YouTube video player' frameborder='0' allow='accelerometer; autoplay; clipboard-write; encrypted-media; gyroscope; picture-in-picture; web-share' allowfullscreen></iframe>\
                     </div>",
                    embed_url
                );
            }
        }
    }

    // Image Banner
    let mut image_banner = String::new();
    {
        let mut stmt = conn
            .prepare("SELECT value FROM config WHERE key = 'banner_url'")
            .unwrap();
        if let Ok(sqlite::State::Row) = stmt.next() {
            let url: String = stmt.read(0).unwrap();
            image_banner = format!(
                "<div style='margin-bottom: 30px; border-radius: 12px; overflow: hidden; box-shadow: 0 4px 12px rgba(0,0,0,0.2);'>\
                   <img src='{}' style='width: 100%; height: auto; display: block;' alt='Project Banner'>\
                 </div>",
                html_escape(&url)
            );
        }
    }

    // Stats
    let total_commits: i64 = {
        let mut stmt = conn.prepare("SELECT COUNT(*) FROM commits").unwrap();
        if let Ok(sqlite::State::Row) = stmt.next() {
            stmt.read(0).unwrap_or(0)
        } else {
            0
        }
    };

    let contributors = crate::db::get_unique_contributors(&conn).unwrap_or_default();
    let contributor_names: Vec<String> = contributors.iter().map(|(n, _)| n.clone()).collect();

    let mut stats_tab =
        String::from("<div style='display: grid; grid-template-columns: 1fr 1fr; gap: 20px;'>");

    // Left: Author stats table
    stats_tab.push_str("<div><h3>Commits by Author</h3><table><thead><tr><th>Author</th><th>Commits</th></tr></thead><tbody>");
    for (name, count) in &contributors {
        stats_tab.push_str(&format!(
            "<tr><td>{}</td><td>{}</td></tr>",
            html_escape(name),
            count
        ));
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

    let (rows, nav_html) = render_commits_list(&conn, page_num);

    let mut body = String::new();
    body.push_str("<div class='tabs'>");
    body.push_str("<div class='tab active' onclick=\"openTab(event, 'tab-log')\">Log</div>");
    body.push_str(
        "<div class='tab' onclick=\"openTab(event, 'tab-contributors')\">Contributors</div>",
    );
    body.push_str("<div class='tab' onclick=\"openTab(event, 'tab-stats')\">Stats</div>");
    body.push_str("<div class='tab' onclick=\"openTab(event, 'tab-music')\">Music</div>");
    body.push_str("</div>");

    body.push_str("<div id='tab-log' class='tab-content active'>");
    if !image_banner.is_empty() {
        body.push_str(&image_banner);
    }
    if !video_banner.is_empty() {
        body.push_str(&video_banner);
    }
    body.push_str("<h3 id='latest'>Latest Commits</h3>");
    body.push_str("<table>");
    body.push_str(&rows);
    body.push_str("</table>");
    body.push_str(&nav_html);
    body.push_str("</div>");

    body.push_str("<div id='tab-music' class='tab-content'>");
    body.push_str("<h3>Music</h3>");
    body.push_str(&music_embed);
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

    page("Lys Repository", "", &body).into_response()
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
        let mut stmt_c = match conn.prepare(
            "SELECT message, hash, tree_hash, author, timestamp, id FROM commits WHERE id = ?",
        ) {
            Ok(s) => s,
            Err(_) => {
                return http_error(StatusCode::INTERNAL_SERVER_ERROR, "Failed to query commit");
            }
        };
        if stmt_c.bind((1, commit_id)).is_ok() {
            if let Ok(sqlite::State::Row) = stmt_c.next() {
                title = stmt_c.read("message").unwrap_or_else(|_| String::from(""));
                hash = stmt_c.read("hash").unwrap_or_else(|_| String::from(""));
                tree_hash = stmt_c
                    .read("tree_hash")
                    .unwrap_or_else(|_| String::from(""));
                author = stmt_c.read("author").unwrap_or_else(|_| String::from(""));
                date = stmt_c
                    .read("timestamp")
                    .unwrap_or_else(|_| String::from(""));
            }
        }
    }

    if tree_hash.is_empty() {
        return http_error(StatusCode::NOT_FOUND, "Commit not found");
    }

    // Search tags for this commit
    let mut tags = Vec::new();
    {
        let mut stmt_t =
            match conn.prepare("SELECT key FROM config WHERE key LIKE 'tag_%' AND value = ?") {
                Ok(s) => s,
                Err(_) => {
                    return http_error(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "Failed to prepare tags query",
                    );
                }
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
            Err(_) => {
                return http_error(StatusCode::INTERNAL_SERVER_ERROR, "Failed to query commit");
            }
        };
        if stmt_c.bind((1, commit_id)).is_ok() {
            if let Ok(sqlite::State::Row) = stmt_c.next() {
                commit_hash = stmt_c.read("hash").unwrap_or_else(|_| String::from(""));
                root_tree_hash = stmt_c
                    .read("tree_hash")
                    .unwrap_or_else(|_| String::from(""));
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
            Err(_) => {
                return http_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Failed to query tree nodes",
                );
            }
        };
        if let Err(_) = stmt.bind((1, current_tree_hash.as_str())) {
            return http_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to bind parent hash",
            );
        }
        if let Err(_) = stmt.bind((2, *comp)) {
            return http_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to bind component name",
            );
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
        breadcrumbs.push_str(&format!(
            " / <a href='/commit/{commit_id}/tree{acc_path}'>{}</a>",
            html_escape(comp)
        ));
    }

    let mut tree_html = String::new();
    tree_html.push_str("<thead><tr><th style='width: 20px;'></th><th>Name</th><th style='text-align: right;'>Hash</th><th style='text-align: left;'>Message</th><th style='text-align: right;'>Age</th><th style='text-align: right;'>Size</th></tr></thead>");
    let mut summary_html = String::new();
    if let Err(e) = render_tree_html_flat(
        &conn,
        commit_id,
        &path_str,
        &current_tree_hash,
        &mut tree_html,
        &mut summary_html,
    ) {
        return http_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            &format!("Failed to render tree: {}", e),
        );
    }

    let terminal_html = format!(
        "<div id='terminal-wrapper' class='terminal-wrapper'>
            <div class='terminal-tabs-bar' id='terminal-tabs-bar'>
                <div class='terminal-tab-item active' id='tab-btn-0' onclick='switchTerminalTab(0)'>
                    <span id='tab-label-0'>Terminal 1</span>
                    <span class='tab-close' onclick='closeTerminalTab(event, 0)'>&times;</span>
                </div>
                <button class='btn terminal-add-tab-btn' onclick='addTerminalTab()'>+</button>
            </div>
            <div id='terminal-tabs-container' class='terminal-tabs-container'>
                <div id='terminal-tab-content-0' class='terminal-tab-content active'>
                    <div id='terminal-panes-0' class='terminal-panes'>
                        <div id='pane-0' class='terminal-pane active'>
                            <div class='terminal-window' onclick='focusPane(0)'>
                                <div class='terminal-header'>
                                    <div class='terminal-dots'>
                                        <span class='dot red' onclick='closePane(0)'></span>
                                        <span class='dot yellow' onclick='minimizePane(0)'></span>
                                        <span class='dot green' onclick='maximizePane(0)'></span>
                                    </div>
                                    <div class='terminal-title'>Lys Interactive Shell - <span id='pane-title-0'>Pane 0</span></div>
                                    <div style='display: flex; gap: 10px; align-items: center;'>
                                        <input type='text' id='session-id-0' placeholder='Session Name' value='default' class='terminal-input' onclick='event.stopPropagation()'>
                                        <button class='btn terminal-btn' onclick='event.stopPropagation(); switchSession(0)'>Attach</button>
                                        <button class='btn terminal-btn' onclick='event.stopPropagation(); splitVertical(0)'>Split V</button>
                                        <button class='btn terminal-btn' onclick='event.stopPropagation(); splitHorizontal(0)'>Split H</button>
                                        <span id='current-session-label-0' class='terminal-session-label'>Session: default</span>
                                    </div>
                                </div>
                                <div id='terminal-0' class='terminal-container'></div>
                            </div>
                        </div>
                    </div>
                </div>
            </div>
        </div>
        <script src='https://cdn.jsdelivr.net/npm/xterm@5.3.0/lib/xterm.min.js'></script>
        <script src='https://cdn.jsdelivr.net/npm/xterm-addon-fit@0.8.0/lib/xterm-addon-fit.min.js'></script>
        <link rel='stylesheet' href='https://cdn.jsdelivr.net/npm/xterm@5.3.0/css/xterm.min.css' />
        <script>
            let panes = {{}};
            let paneCounter = 1;
            let terminalTabs = [0];
            let activeTerminalTab = 0;
            let terminalTabCounter = 1;

            function createPane(id, sessionId = 'default') {{
                const term = new Terminal({{
                    cursorBlink: true,
                    fontSize: 14,
                    lineHeight: 1.2,
                    fontFamily: 'SFMono-Regular, Consolas, \"Liberation Mono\", Menlo, monospace',
                    theme: {{
                        background: '#1a1a1a',
                        foreground: '#e0e0e0',
                        cursor: '#ffffff',
                        selection: 'rgba(255, 255, 255, 0.3)',
                        black: '#000000',
                        red: '#e06c75',
                        green: '#98c379',
                        yellow: '#d19a66',
                        blue: '#61afef',
                        magenta: '#c678dd',
                        cyan: '#56b6c2',
                        white: '#abb2bf',
                        brightBlack: '#5c6370',
                        brightRed: '#e06c75',
                        brightGreen: '#98c379',
                        brightYellow: '#d19a66',
                        brightBlue: '#61afef',
                        brightMagenta: '#c678dd',
                        brightCyan: '#56b6c2',
                        brightWhite: '#ffffff'
                    }}
                }});
                const fitAddon = new FitAddon.FitAddon();
                term.loadAddon(fitAddon);
                
                panes[id] = {{
                    term: term,
                    fitAddon: fitAddon,
                    socket: null,
                    command: '',
                    history: [],
                    historyIndex: -1,
                    sessionId: sessionId
                }};

                setTimeout(() => {{
                    const el = document.getElementById('terminal-' + id);
                    if (el) {{
                        term.open(el);
                        fitAddon.fit();
                        connect(id);
                        
                        // Focus on click
                        el.addEventListener('click', () => {{
                            focusPane(id);
                        }});
                    }}
                }}, 100);

                term.onData(data => {{
                    let p = panes[id];
                    if (data === '\\r') {{
                        if (p.command.trim().length > 0) {{
                            p.history.push(p.command);
                            p.historyIndex = -1;
                        }}
                        if (p.socket && p.socket.readyState === WebSocket.OPEN) {{
                            p.socket.send(p.command);
                        }}
                        p.command = '';
                        term.write('\\r\\n');
                    }} else if (data === '\\x7f') {{ // Backspace
                        if (p.command.length > 0) {{
                            p.command = p.command.slice(0, -1);
                            term.write('\\b \\b');
                        }}
                    }} else if (data === '\\x1b[A') {{ // Up arrow
                        if (p.history.length > 0) {{
                            if (p.historyIndex === -1) p.historyIndex = p.history.length - 1;
                            else if (p.historyIndex > 0) p.historyIndex--;
                            for (let i = 0; i < p.command.length; i++) term.write('\\b \\b');
                            p.command = p.history[p.historyIndex];
                            term.write(p.command);
                        }}
                    }} else if (data === '\\x1b[B') {{ // Down arrow
                        if (p.historyIndex !== -1) {{
                            if (p.historyIndex < p.history.length - 1) {{
                                p.historyIndex++;
                                for (let i = 0; i < p.command.length; i++) term.write('\\b \\b');
                                p.command = p.history[p.historyIndex];
                                term.write(p.command);
                            }} else {{
                                p.historyIndex = -1;
                                for (let i = 0; i < p.command.length; i++) term.write('\\b \\b');
                                p.command = '';
                            }}
                        }}
                    }} else if (data === '\\t') {{ // Tab
                        if (p.socket && p.socket.readyState === WebSocket.OPEN) {{
                            p.socket.send('complete:' + p.command);
                        }}
                    }} else if (data.length === 1 && data.charCodeAt(0) >= 32) {{
                        p.command += data;
                        term.write(data);
                    }}
                }});
            }}

            function connect(id) {{
                let p = panes[id];
                if (p.socket) p.socket.close();
                p.term.clear();
                
                // Show reconnecting status
                p.term.write('\\r\\n\\x1b[33mConnecting to Lys server...\\x1b[0m\\r\\n');
                
                let protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
                p.socket = new WebSocket(protocol + '//' + window.location.host + '/ws');
                
                p.socket.onmessage = (event) => {{
                    if (event.data === '\\x1b[2J\\x1b[H') {{
                        p.term.clear();
                        return;
                    }}
                    if (event.data.startsWith('suggestions:')) {{
                        const suggestions = event.data.substring(12).split(',');
                        if (suggestions.length === 1) {{
                            const lastSpace = p.command.lastIndexOf(' ');
                            const currentWord = lastSpace === -1 ? p.command : p.command.substring(lastSpace + 1);
                            const completion = suggestions[0].substring(currentWord.length);
                            p.command += completion;
                            p.term.write(completion);
                        }} else if (suggestions.length > 1) {{
                            p.term.write('\\r\\n' + suggestions.join('  ') + '\\r\\nlys> ' + p.command);
                        }}
                        return;
                    }}
                    p.term.write(event.data);
                }};
                
                p.socket.onopen = () => {{
                    p.term.clear();
                    const sid_el = document.getElementById('session-id-' + id);
                    const sid = sid_el ? sid_el.value : 'default';
                    p.sessionId = sid;
                    if (sid !== 'default') {{
                        p.socket.send('session ' + sid);
                    }}
                    const label = document.getElementById('current-session-label-' + id);
                    if (label) label.innerText = 'Session: ' + sid;
                }};

                p.socket.onclose = () => {{
                    p.term.write('\\r\\n\\x1b[31mConnection lost. Retrying in 3 seconds...\\x1b[0m\\r\\n');
                    setTimeout(() => connect(id), 3000);
                }};

                p.socket.onerror = (err) => {{
                    console.error('WebSocket Error on pane ' + id + ':', err);
                }};
            }}

            function switchSession(id) {{
                connect(id);
            }}

            function splitVertical(id) {{
                const oldPane = document.getElementById('pane-' + id);
                const parent = oldPane.parentNode;
                const newId = paneCounter++;
                
                const wrapper = document.createElement('div');
                wrapper.className = 'pane-split-v';
                parent.replaceChild(wrapper, oldPane);
                
                const pane1 = oldPane;
                const pane2 = createPaneElement(newId);
                
                wrapper.appendChild(pane1);
                wrapper.appendChild(pane2);
                
                createPane(newId, panes[id].sessionId);
                setTimeout(() => {{ 
                    panes[id].fitAddon.fit();
                    panes[newId].fitAddon.fit();
                }}, 200);
            }}

            function splitHorizontal(id) {{
                const oldPane = document.getElementById('pane-' + id);
                const parent = oldPane.parentNode;
                const newId = paneCounter++;
                
                const wrapper = document.createElement('div');
                wrapper.className = 'pane-split-h';
                parent.replaceChild(wrapper, oldPane);
                
                const pane1 = oldPane;
                const pane2 = createPaneElement(newId);
                
                wrapper.appendChild(pane1);
                wrapper.appendChild(pane2);
                
                createPane(newId, panes[id].sessionId);
                setTimeout(() => {{ 
                    panes[id].fitAddon.fit();
                    panes[newId].fitAddon.fit();
                }}, 200);
            }}

            function focusPane(id) {{
                // Remove active class from all terminal windows
                document.querySelectorAll('.terminal-window').forEach(el => el.classList.remove('active'));
                
                const pane = document.getElementById('pane-' + id);
                if (pane) {{
                    const window = pane.querySelector('.terminal-window');
                    if (window) window.classList.add('active');
                }}
                if (panes[id] && panes[id].term) {{
                    panes[id].term.focus();
                }}
            }}

            function createPaneElement(id) {{
                const div = document.createElement('div');
                div.id = 'pane-' + id;
                div.className = 'terminal-pane';
                div.innerHTML = `
                    <div class='terminal-window' onclick='focusPane(${{id}})'>
                        <div class='terminal-header'>
                            <div class='terminal-dots'>
                                <span class='dot red' onclick='closePane(${{id}})'></span>
                                <span class='dot yellow' onclick='minimizePane(${{id}})'></span>
                                <span class='dot green' onclick='maximizePane(${{id}})'></span>
                            </div>
                            <div class='terminal-title'>Lys Interactive Shell - <span id='pane-title-${{id}}'>Pane ${{id}}</span></div>
                            <div style='display: flex; gap: 10px; align-items: center;'>
                                <input type='text' id='session-id-${{id}}' placeholder='Session Name' value='default' class='terminal-input' onclick='event.stopPropagation()'>
                                <button class='btn terminal-btn' onclick='event.stopPropagation(); switchSession(${{id}})'>Attach</button>
                                <button class='btn terminal-btn' onclick='event.stopPropagation(); splitVertical(${{id}})'>Split V</button>
                                <button class='btn terminal-btn' onclick='event.stopPropagation(); splitHorizontal(${{id}})'>Split H</button>
                                <button class='btn terminal-btn' onclick='event.stopPropagation(); closePane(${{id}})' style='background:#d9534f !important'>&times;</button>
                                <span id='current-session-label-${{id}}' class='terminal-session-label'>Session: default</span>
                            </div>
                        </div>
                        <div id='terminal-${{id}}' class='terminal-container'></div>
                    </div>
                `;
                return div;
            }}

            function closePane(id) {{
                const pane = document.getElementById('pane-' + id);
                const parent = pane.parentNode;
                if (panes[id].socket) panes[id].socket.close();
                delete panes[id];
                
                if (parent.classList.contains('pane-split-v') || parent.classList.contains('pane-split-h')) {{
                    const otherPane = parent.children[0] === pane ? parent.children[1] : parent.children[0];
                    const grandParent = parent.parentNode;
                    grandParent.replaceChild(otherPane, parent);
                    
                    // Refit all remaining panes
                    Object.keys(panes).forEach(pid => {{
                        setTimeout(() => {{ if (panes[pid]) panes[pid].fitAddon.fit(); }}, 250);
                    }});
                }} else {{
                    pane.remove();
                }}
            }}

            function minimizePane(id) {{
                const pane = document.getElementById('pane-' + id);
                const container = pane.querySelector('.terminal-container');
                if (container.style.display === 'none') {{
                    container.style.display = 'block';
                    pane.style.flex = '1';
                    pane.style.minHeight = '200px';
                }} else {{
                    container.style.display = 'none';
                    pane.style.flex = '0 0 auto';
                    pane.style.minHeight = 'auto';
                }}
                setTimeout(() => {{ if (panes[id]) panes[id].fitAddon.fit(); }}, 100);
            }}

            function maximizePane(id) {{
                const pane = document.getElementById('pane-' + id);
                if (pane.classList.contains('maximized-pane')) {{
                    pane.classList.remove('maximized-pane');
                }} else {{
                    // Remove maximized from any other pane
                    document.querySelectorAll('.maximized-pane').forEach(el => el.classList.remove('maximized-pane'));
                    pane.classList.add('maximized-pane');
                }}
                setTimeout(() => {{ if (panes[id]) panes[id].fitAddon.fit(); }}, 200);
            }}

            function addTerminalTab() {{
                const tabId = terminalTabCounter++;
                const paneId = paneCounter++;
                
                // Add tab button
                const bar = document.getElementById('terminal-tabs-bar');
                const addBtn = bar.querySelector('.terminal-add-tab-btn');
                const newTabBtn = document.createElement('div');
                newTabBtn.className = 'terminal-tab-item';
                newTabBtn.id = 'tab-btn-' + tabId;
                newTabBtn.onclick = () => switchTerminalTab(tabId);
                newTabBtn.innerHTML = `
                    <span id='tab-label-${{tabId}}'>Terminal ${{tabId + 1}}</span>
                    <span class='tab-close' onclick='closeTerminalTab(event, ${{tabId}})'>&times;</span>
                `;
                bar.insertBefore(newTabBtn, addBtn);
                
                // Add tab content
                const container = document.getElementById('terminal-tabs-container');
                const newTabContent = document.createElement('div');
                newTabContent.id = 'terminal-tab-content-' + tabId;
                newTabContent.className = 'terminal-tab-content';
                newTabContent.innerHTML = `
                    <div id='terminal-panes-${{tabId}}' class='terminal-panes'>
                        <div id='pane-${{paneId}}' class='terminal-pane active'>
                            <div class='terminal-window' onclick='focusPane(${{paneId}})'>
                                <div class='terminal-header'>
                                    <div class='terminal-dots'>
                                        <span class='dot red' onclick='closePane(${{paneId}})'></span>
                                        <span class='dot yellow' onclick='minimizePane(${{paneId}})'></span>
                                        <span class='dot green' onclick='maximizePane(${{paneId}})'></span>
                                    </div>
                                    <div class='terminal-title'>Lys Interactive Shell - <span id='pane-title-${{paneId}}'>Pane ${{paneId}}</span></div>
                                    <div style='display: flex; gap: 10px; align-items: center;'>
                                        <input type='text' id='session-id-${{paneId}}' placeholder='Session Name' value='default' class='terminal-input' onclick='event.stopPropagation()'>
                                        <button class='btn terminal-btn' onclick='event.stopPropagation(); switchSession(${{paneId}})'>Attach</button>
                                        <button class='btn terminal-btn' onclick='event.stopPropagation(); splitVertical(${{paneId}})'>Split V</button>
                                        <button class='btn terminal-btn' onclick='event.stopPropagation(); splitHorizontal(${{paneId}})'>Split H</button>
                                        <span id='current-session-label-${{paneId}}' class='terminal-session-label'>Session: default</span>
                                    </div>
                                </div>
                                <div id='terminal-${{paneId}}' class='terminal-container'></div>
                            </div>
                        </div>
                    </div>
                `;
                container.appendChild(newTabContent);
                
                terminalTabs.push(tabId);
                createPane(paneId);
                switchTerminalTab(tabId);
            }}

            function switchTerminalTab(tabId) {{
                terminalTabs.forEach(id => {{
                    document.getElementById('tab-btn-' + id).classList.remove('active');
                    document.getElementById('terminal-tab-content-' + id).classList.remove('active');
                }});
                document.getElementById('tab-btn-' + tabId).classList.add('active');
                document.getElementById('terminal-tab-content-' + tabId).classList.add('active');
                activeTerminalTab = tabId;
                
                // Trigger refit for all panes in this tab
                const panesInTab = document.getElementById('terminal-tab-content-' + tabId).querySelectorAll('.terminal-container');
                panesInTab.forEach(container => {{
                    const pid = container.id.replace('terminal-', '');
                    if (panes[pid]) {{
                        setTimeout(() => panes[pid].fitAddon.fit(), 50);
                    }}
                }});
            }}

            function closeTerminalTab(event, tabId) {{
                event.stopPropagation();
                if (terminalTabs.length <= 1) return;
                
                const index = terminalTabs.indexOf(tabId);
                terminalTabs.splice(index, 1);
                
                // Cleanup panes in this tab
                const panesInTab = document.getElementById('terminal-tab-content-' + tabId).querySelectorAll('.terminal-container');
                panesInTab.forEach(container => {{
                    const pid = container.id.replace('terminal-', '');
                    if (panes[pid]) {{
                        if (panes[pid].socket) panes[pid].socket.close();
                        delete panes[pid];
                    }}
                }});
                
                document.getElementById('tab-btn-' + tabId).remove();
                document.getElementById('terminal-tab-content-' + tabId).remove();
                
                if (activeTerminalTab === tabId) {{
                    switchTerminalTab(terminalTabs[Math.max(0, index - 1)]);
                }}
            }}

            // Initial pane
            createPane(0);

            window.addEventListener('resize', () => {{
                Object.values(panes).forEach(p => {{ if (p) p.fitAddon.fit(); }});
            }});
            
            const observer = new MutationObserver((mutations) => {{
                mutations.forEach((mutation) => {{
                    if (mutation.attributeName === 'class') {{
                        const target = mutation.target;
                        if (target.id === 'terminal-tab' && target.classList.contains('active')) {{
                            setTimeout(() => {{
                                Object.values(panes).forEach(p => {{ if (p) p.fitAddon.fit(); }});
                            }}, 50);
                        }}
                    }}
                }});
            }});
            observer.observe(document.getElementById('terminal-tab'), {{ attributes: true }});
        </script>"
    );

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
            if stmt.bind((1, current_tree_hash.as_str())).is_err() {
                continue;
            }
            if stmt.bind((2, sf)).is_err() {
                continue;
            }
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
                            format!(
                                "<pre style='white-space: pre-wrap; font-family: sans-serif;'>{}</pre>",
                                html_escape(&text)
                            )
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
    tabs_headers.push_str(&format!(
        "<div class='tab {}' onclick='openTab(event, \"files-tab\")'>Files</div>",
        if active_tab == "FILES" { "active" } else { "" }
    ));
    tabs_bodies.push_str(&format!(
        "<div id='files-tab' class='tab-content {}'>{}<table class='tree-table'>{}</table></div>",
        if active_tab == "FILES" { "active" } else { "" },
        summary_html,
        tree_html
    ));

    // README tab
    if let Some(content) = special_content.get("README") {
        tabs_headers.push_str(&format!(
            "<div class='tab {}' onclick='openTab(event, \"readme-tab\")'>README</div>",
            if active_tab == "README" { "active" } else { "" }
        ));
        tabs_bodies.push_str(&format!(
            "<div id='readme-tab' class='tab-content {}'>{}</div>",
            if active_tab == "README" { "active" } else { "" },
            content
        ));
    }

    // Other special tabs
    for (label, content) in &special_content {
        if label == "README" {
            continue;
        }
        let tab_id = format!("{}-tab", label.to_lowercase().replace("_", "-"));
        tabs_headers.push_str(&format!(
            "<div class='tab' onclick='openTab(event, \"{}\")'>{}</div>",
            tab_id,
            label.replace("_", " ")
        ));
        tabs_bodies.push_str(&format!(
            "<div id='{}' class='tab-content'>{}</div>",
            tab_id, content
        ));
    }

    // Terminal tab
    tabs_headers
        .push_str("<div class='tab' onclick='openTab(event, \"terminal-tab\")'>Terminal</div>");
    tabs_bodies.push_str(&format!(
        "<div id='terminal-tab' class='tab-content'>{}</div>",
        terminal_html
    ));

    let tabs_html = format!("<div class='tabs'>{}</div>{}", tabs_headers, tabs_bodies);

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
         .breadcrumbs { margin-bottom: 20px; font-family: monospace; background: var(--nav-bg); padding: 12px; border: 1px solid var(--border); border-radius: 4px; }
         .terminal-window {
            border-radius: 8px;
            overflow: hidden;
            border: 1px solid #444;
            box-shadow: 0 10px 30px rgba(0,0,0,0.5);
            background: #1a1a1a;
            height: 100%;
            display: flex;
            flex-direction: column;
         }
         .terminal-panes {
            display: flex;
            flex-direction: column;
            height: 100%;
            gap: 10px;
         }
         .terminal-pane {
            flex: 1;
            min-height: 200px;
            height: 100%;
         }
         .pane-split-v {
            display: flex;
            flex-direction: row;
            flex: 1;
            gap: 10px;
            height: 100%;
         }
         .pane-split-h {
            display: flex;
            flex-direction: column;
            flex: 1;
            gap: 10px;
            height: 100%;
         }
         .terminal-header {
            background: #333;
            padding: 8px 15px;
            display: flex;
            justify-content: space-between;
            align-items: center;
            border-bottom: 1px solid #444;
            position: relative;
         }
         .terminal-window.active {
            border-color: var(--link);
            box-shadow: 0 10px 30px rgba(0,0,0,0.7);
         }
         .terminal-window.active .terminal-header {
            background: #444;
         }
         .terminal-dots { display: flex; gap: 6px; }
         .maximized-pane {
            position: fixed !important;
            top: 0 !important;
            left: 0 !important;
            width: 100vw !important;
            height: 100vh !important;
            z-index: 9999 !important;
            margin: 0 !important;
            background: #1a1a1a !important;
         }
         .dot { width: 12px; height: 12px; border-radius: 50%; display: inline-block; cursor: pointer; }
         .dot.red { background: #ff5f56; }
         .dot.yellow { background: #ffbd2e; }
         .dot.green { background: #27c93f; }
         .terminal-title {
            color: #abb2bf;
            font-size: 0.8em;
            font-family: sans-serif;
         }
         .terminal-input {
            padding: 3px 8px;
            border-radius: 4px;
            border: 1px solid #555;
            background: #222;
            color: #eee;
            font-size: 0.8em;
            width: 80px;
         }
         .terminal-btn {
            padding: 2px 8px !important;
            font-size: 0.75em !important;
            background: #444 !important;
            border-color: #555 !important;
            color: #eee !important;
            margin-right: 2px !important;
         }
         .terminal-session-label {
            font-size: 0.75em;
            color: #888;
         }
         .terminal-container {
            flex: 1;
            padding: 10px;
            overflow: hidden;
         }
         .terminal-wrapper {
            height: 75vh;
            min-height: 500px;
            margin-bottom: 20px;
            display: flex;
            flex-direction: column;
         }
         .terminal-tabs-bar {
            display: flex;
            background: #252525;
            padding: 5px 10px 0 10px;
            gap: 2px;
            border-bottom: 1px solid #444;
         }
         .terminal-tab-item {
            padding: 6px 15px;
            background: #333;
            color: #888;
            border-radius: 6px 6px 0 0;
            font-size: 0.8em;
            cursor: pointer;
            display: flex;
            align-items: center;
            gap: 8px;
            border: 1px solid #444;
            border-bottom: none;
            transition: all 0.2s;
         }
         .terminal-tab-item:hover {
            background: #444;
            color: #eee;
         }
         .terminal-tab-item.active {
            background: #1a1a1a;
            color: #eee;
            border-color: #555;
            padding-bottom: 7px;
            margin-bottom: -1px;
         }
         .tab-close {
            font-size: 1.2em;
            line-height: 1;
            color: #666;
         }
         .tab-close:hover {
            color: #ff5f56;
         }
         .terminal-add-tab-btn {
            background: transparent !important;
            border: none !important;
            color: #888 !important;
            font-size: 1.2em !important;
            padding: 0 10px !important;
            cursor: pointer;
         }
         .terminal-add-tab-btn:hover {
            color: #eee !important;
         }
         .terminal-tabs-container {
            flex: 1;
            position: relative;
            background: #1a1a1a;
         }
         .terminal-tab-content {
            display: none;
            height: 100%;
         }
         .terminal-tab-content.active {
            display: block;
         }
         #terminal ::-webkit-scrollbar { width: 8px; }
         #terminal ::-webkit-scrollbar-track { background: #1a1a1a; }
         #terminal ::-webkit-scrollbar-thumb { background: #444; border-radius: 4px; }
         #terminal ::-webkit-scrollbar-thumb:hover { background: #555; }
         #terminal { scrollbar-width: thin; scrollbar-color: #444 #1a1a1a; }",
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

    // Helper local pour récupérer le dernier commit touchant un chemin
    fn last_commit_for_path(
        conn: &Connection,
        full_path: &str,
        is_dir: bool,
        until_commit_id: i64,
    ) -> Option<(i64, String, String, String)> {
        // Attention: la table manifest peut ne pas exister sur d'anciens dépôts
        let base_sql = if is_dir {
            "SELECT c.id, c.hash, c.timestamp, c.message \
             FROM manifest m \
             JOIN commits c ON c.id = m.commit_id \
             WHERE (m.file_path = ?1 OR m.file_path LIKE (?1 || '/%')) AND c.id <= ?2 \
             ORDER BY c.timestamp DESC LIMIT 1"
        } else {
            "SELECT c.id, c.hash, c.timestamp, c.message \
             FROM manifest m \
             JOIN commits c ON c.id = m.commit_id \
             WHERE m.file_path = ?1 AND c.id <= ?2 \
ss             ORDER BY c.timestamp DESC LIMIT 1"
        };
        let mut stmt = match conn.prepare(base_sql) {
            Ok(s) => s,
            Err(_) => return None,
        };
        if stmt.bind((1, full_path)).is_err() {
            return None;
        }
        if stmt.bind((2, until_commit_id)).is_err() {
            return None;
        }
        if let Ok(sqlite::State::Row) = stmt.next() {
            let id = stmt.read::<i64, _>(0).ok()?;
            let hash = stmt.read::<String, _>(1).ok()?;
            let ts = stmt.read::<String, _>(2).ok()?;
            let msg = stmt.read::<String, _>(3).ok()?;
            // Ne garder que la première ligne du message
            let first_line = msg.lines().next().unwrap_or("").to_string();
            Some((id, hash, ts, first_line))
        } else {
            None
        }
    }

    for (name, hash, mode, size) in entries {
        let is_dir = mode == 16384 || mode == 0o040000 || mode == 0o755;
        let icon = if is_dir { "📁" } else { "📄" };

        let (full_path, link) = if is_dir {
            let full_path = if current_path.is_empty() {
                name.clone()
            } else {
                format!("{}/{}", current_path, name)
            };
            let link = format!(
                "<a href='/commit/{commit_id}/tree/{}' class='dir'>{}</a>",
                full_path,
                html_escape(&name)
            );
            (full_path, link)
        } else {
            let full_path = if current_path.is_empty() {
                name.clone()
            } else {
                format!("{}/{}", current_path, name)
            };
            let link = format!(
                "<a href='/file/{}' class='file'>{}</a>",
                html_escape(&hash),
                html_escape(&name)
            );
            (full_path, link)
        };

        let size_str = if is_dir {
            "-".to_string()
        } else {
            format!("{} B", size)
        };

        let (commit_hash_html, commit_msg_html, age_html) = if let Some((cid, chash, ts, msg)) =
            last_commit_for_path(conn, &full_path, is_dir, commit_id)
        {
            let age = time_ago(&ts);
            (
                format!(
                    "<a href='/commit/{}'><code class='hash'>{}</code></a>",
                    cid,
                    html_escape(short_hash(&chash))
                ),
                format!(
                    "<span class='meta' title='{}'> — {}</span>",
                    html_escape(&msg),
                    html_escape(&msg)
                ),
                html_escape(&age),
            )
        } else {
            (String::new(), String::new(), String::new())
        };

        out.push_str(&format!(
            "<tr>\
                <td style='width: 20px;'><span class='icon'>{}</span></td>\
                <td>{}</td>\
                <td style='text-align: right; white-space: nowrap;'>{}</td>\
                <td style='text-align: left;'>{}</td>\
                <td style='text-align: right; color: var(--meta); font-size: 0.8em; white-space: nowrap;'>{}</td>\
                <td style='text-align: right; color: var(--meta); font-size: 0.8em; white-space: nowrap;'>{}</td>\
             </tr>",
            icon, link, commit_hash_html, commit_msg_html, age_html, size_str
        ));
    }
    Ok(())
}

// -----------------------------
// TEAM CHAT (WebSocket + UI)
// -----------------------------
async fn show_chat() -> impl IntoResponse {
    let username = nix::unistd::User::from_uid(nix::unistd::getuid())
        .ok()
        .flatten()
        .map(|u| u.name)
        .unwrap_or_else(|| "web".into());
    let body = format!("
      <h3>Team Chat</h3>
      <div id='chat-box' style='height: 420px; overflow-y: auto; border:1px solid var(--border); border-radius:4px; padding:10px; background: var(--code-bg); font-family: monospace; font-size: 0.9em; margin-bottom:10px;'></div>
      <div style='display:flex; gap:8px; align-items: flex-end;'>
        <textarea id='message' placeholder='Type a message and press Enter' rows='1' style='flex:1; padding:8px; border:1px solid var(--border); background:var(--bg); color:var(--fg); resize: none; overflow-y: hidden; line-height: 1.4;'></textarea>
      </div>
      <script>
        (function(){{
          const log = document.getElementById('chat-box');
          const input = document.getElementById('message');
          const currentUser = {username:?};

          function autoResize() {{
            input.style.height = 'auto';
            input.style.height = input.scrollHeight + 'px';
          }}
          input.addEventListener('input', autoResize);

          function formatTime(ts) {{
            if(!ts) return '';
            // SQLite CURRENT_TIMESTAMP is YYYY-MM-DD HH:MM:SS
            // We convert it to ISO 8601 YYYY-MM-DDTHH:MM:SSZ
            const isoStr = ts.replace(' ', 'T') + 'Z';
            const date = new Date(isoStr);
            if (isNaN(date.getTime())) return '';
            const now = new Date();
            const diff = Math.floor((now - date) / 1000);
            if (diff < 0) return 'just now';
            if (diff < 60) return diff + 's ago';
            if (diff < 3600) return Math.floor(diff / 60) + 'm ago';
            if (diff < 86400) return Math.floor(diff / 3600) + 'h ago';
            return Math.floor(diff / 86400) + 'd ago';
          }}

          function append(obj){{
            const el = document.createElement('div');
            el.style.marginBottom = '10px';
            const meta = document.createElement('div');
            meta.style.fontSize = '0.8em';
            meta.style.color = 'var(--meta)';
            meta.className = 'message-meta';
            meta.dataset.timestamp = obj.created_at;
            const timeStr = formatTime(obj.created_at);
            meta.textContent = obj.sender + ' • ' + timeStr;
            
            const content = document.createElement('div');
            content.style.whiteSpace = 'pre-wrap';
            content.style.wordBreak = 'break-word';
            content.textContent = obj.content;
            
            el.appendChild(meta);
            el.appendChild(content);
            log.appendChild(el);
            log.scrollTop = log.scrollHeight;
          }}

          function refreshTimes() {{
            document.querySelectorAll('.message-meta').forEach(m => {{
              const ts = m.dataset.timestamp;
              const sender = m.textContent.split(' • ')[0];
              m.textContent = sender + ' • ' + formatTime(ts);
            }});
          }}
          setInterval(refreshTimes, 30000);
          const proto = (location.protocol === 'https:') ? 'wss' : 'ws';
          const ws = new WebSocket(proto + '://' + location.host + '/ws/chat');
          ws.onmessage = (ev)=>{{
            try {{
              const obj = JSON.parse(ev.data);
              append(obj);
            }} catch(_) {{
              console.error('Failed to parse message', ev.data);
            }}
          }};
          function send(){{
            const m = input.value.trim();
            if(!m) return;
            ws.send(JSON.stringify({{sender: currentUser, content: m}}));
            input.value='';
            input.style.height = 'auto';
          }}
          input.addEventListener('keydown', (e)=>{{ 
            if(e.key==='Enter' && !e.shiftKey){{ 
              e.preventDefault();
              send(); 
            }}
          }});
        }})();
      </script>
    ", username = username);
    page("Team Chat", "", &body).into_response()
}

async fn ws_chat_upgrade(
    State(state): State<Arc<AppState>>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| async move { handle_chat_socket(state, socket).await })
}

async fn handle_chat_socket(state: Arc<AppState>, mut socket: WebSocket) {
    use axum::extract::ws::Message;

    // 1) Récupérer l'historique récent (hors await), puis l'envoyer
    let hist: Vec<(String, String, String)> = {
        if let Ok(conn) = state.conn.lock() {
            if let Ok(mut stmt) = conn.prepare("SELECT sender, content, created_at FROM ephemeral_messages ORDER BY id DESC LIMIT 50") {
                let mut v = Vec::new();
                while let Ok(sqlite::State::Row) = stmt.next() {
                    let s: String = stmt.read(0).unwrap_or_else(|_| "?".into());
                    let c: String = stmt.read(1).unwrap_or_default();
                    let t: String = stmt.read(2).unwrap_or_default();
                    v.push((s, c, t));
                }
                v
            } else { Vec::new() }
        } else { Vec::new() }
    };
    // On renvoie dans l'ordre chronologique
    let mut hist = hist;
    hist.reverse();
    for (s, c, t) in hist {
        let json = format!("{{\"sender\":{0:?},\"content\":{1:?},\"created_at\":{2:?}}}", s, c, t);
        let _ = socket.send(Message::Text(json.into())).await;
    }

    // 2) Abonnement broadcast pour les nouveaux messages
    let mut rx = state.chat_tx.subscribe();

    // 3) Boucle principale: écoute à la fois le broadcast et ce client
    loop {
        tokio::select! {
            // Messages broadcast vers ce client
            Ok(line) = rx.recv() => {
                if socket.send(Message::Text(line.into())).await.is_err() {
                    break;
                }
            }
            // Messages entrants de ce client
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Text(txt))) => {
                        let s_txt = txt.as_str();
                        // On s'attend à un JSON {sender, content}
                        let (sender_name, content) = match serde_json::from_str::<serde_json::Value>(s_txt) {
                            Ok(v) => {
                                let s = v.get("sender").and_then(|x| x.as_str()).unwrap_or("web");
                                let c = v.get("content").and_then(|x| x.as_str()).unwrap_or("");
                                (s.to_string(), c.to_string())
                            }
                            Err(_) => ("web".to_string(), s_txt.to_string()),
                        };
                        if content.trim().is_empty() { continue; }
                        if let Ok(conn) = state.conn.lock() {
                            let _ = crate::chat::send_message(&conn, &sender_name, &content);
                        }
                        let now = Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
                        let payload = format!("{{\"sender\":{0:?},\"content\":{1:?},\"created_at\":{2:?}}}", sender_name, content, now);
                        let _ = state.chat_tx.send(payload);
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    _ => {}
                }
            }
        }
    }
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
            Err(_) => {
                return http_error(StatusCode::INTERNAL_SERVER_ERROR, "Failed to query commit");
            }
        };
        if let Err(_) = stmt.bind((1, commit_id)) {
            return http_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to bind commit ID",
            );
        }
        if let Ok(sqlite::State::Row) = stmt.next() {
            commit_hash = stmt.read("hash").unwrap_or_default();
            tree_hash = stmt.read("tree_hash").unwrap_or_default();
            let current_db_id: i64 = stmt.read("id").unwrap_or(0);

            if let Ok(mut stmt_p) =
                conn.prepare("SELECT tree_hash FROM commits WHERE id < ? ORDER BY id DESC LIMIT 1")
            {
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
                    diff_id,
                    path.display()
                ));
                diff_html.push_str(&format!(
                    "<div id='{}'>{}</div>",
                    diff_id,
                    render_diff(&old_bytes, &new_bytes, &mode)
                ));
            }
            (None, Some((new_hash, _))) => {
                // Added
                let new_bytes = get_raw_blob(&conn, new_hash);
                diff_html.push_str(&format!(
                    "<div class='diff-file-header'>\
                       <button class='btn copy-btn' onclick='copyToClipboard(\"{}\")'>Copy</button>\
                       <strong>Added: {}</strong>\
                     </div>",
                    diff_id,
                    path.display()
                ));
                diff_html.push_str(&format!(
                    "<div id='{}'>{}</div>",
                    diff_id,
                    render_diff(&[], &new_bytes, &mode)
                ));
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
        ),
    ).into_response()
}

fn get_raw_blob(conn: &Connection, hash: &str) -> Vec<u8> {
    if let Ok(mut stmt) = conn.prepare("SELECT content FROM store.blobs WHERE hash = ?") {
        if stmt.bind((1, hash)).is_ok() {
            if let Ok(sqlite::State::Row) = stmt.next() {
                if let Ok(content) = stmt.read::<Vec<u8>, _>(0) {
                    return decompress(&content);
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
        return format!(
            "<div class='diff-container'><pre style='margin:0;'><code>{}</code></pre></div>",
            html_escape(&new_s)
        );
    }

    let diff = similar::TextDiff::from_lines(&old_s, &new_s);

    if mode == "side-by-side" {
        let mut out = String::from("<table class='diff-ss-table'>");
        for opcode in diff.grouped_ops(3) {
            for op in opcode {
                match op {
                    similar::DiffOp::Equal {
                        old_index,
                        new_index,
                        len,
                    } => {
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
                    similar::DiffOp::Delete {
                        old_index, old_len, ..
                    } => {
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
                    similar::DiffOp::Insert {
                        new_index, new_len, ..
                    } => {
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
                    similar::DiffOp::Replace {
                        old_index,
                        old_len,
                        new_index,
                        new_len,
                    } => {
                        let common = old_len.min(new_len);
                        for i in 0..common {
                            let old_line = diff.old_slices()[old_index + i];
                            let new_line = diff.new_slices()[new_index + i];
                            out.push_str(&format!(
                                "<tr>\
                                    <td class='diff-line-num diff-deleted'>{}</td><td class='diff-ss-left diff-deleted'>{}</td>\
                                    <td class='diff-line-num diff-added'>{}</td><td class='diff-ss-right diff-added'>{}</td>\
                                 </tr>",
                                old_index + i + 1,
                                html_escape(&old_line.to_string()),
                                new_index + i + 1,
                                html_escape(&new_line.to_string())
                            ));
                        }
                        if old_len > common {
                            for i in common..old_len {
                                let old_line = diff.old_slices()[old_index + i];
                                out.push_str(&format!(
                                    "<tr>\
                                        <td class='diff-line-num diff-deleted'>{}</td><td class='diff-ss-left diff-deleted'>{}</td>\
                                        <td class='diff-line-num'></td><td class='diff-ss-right diff-ghost'></td>\
                                     </tr>",
                                    old_index + i + 1,
                                    html_escape(&old_line.to_string())
                                ));
                            }
                        } else if new_len > common {
                            for i in common..new_len {
                                let new_line = diff.new_slices()[new_index + i];
                                out.push_str(&format!(
                                    "<tr>\
                                        <td class='diff-line-num'></td><td class='diff-ss-left diff-ghost'></td>\
                                        <td class='diff-line-num diff-added'>{}</td><td class='diff-ss-right diff-added'>{}</td>\
                                     </tr>",
                                    new_index + i + 1,
                                    html_escape(&new_line.to_string())
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
        out.push_str(&format!(
            "<span class='{}'>{}{}</span>",
            class,
            sign,
            html_escape(&change.to_string())
        ));
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

                body.push_str(&format!(
                    "<pre class='line-numbers'><code class='{}'>{}</code></pre>",
                    lang_class,
                    html_escape(&text)
                ));
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
            header::CONTENT_TYPE,
            axum::http::HeaderValue::from_static("application/octet-stream"),
        );
        // Avoid header injection: keep filename simple.
        let filename = format!("lys-{}.bin", short_hash(&hash));
        let cd = format!("attachment; filename=\"{}\"", filename);
        if let Ok(v) = axum::http::HeaderValue::from_str(&cd) {
            headers.insert(header::CONTENT_DISPOSITION, v);
        }

        (StatusCode::OK, headers, bytes).into_response()
    } else {
        http_error(StatusCode::NOT_FOUND, "File not found")
    }
}

async fn serve_rss(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let conn = match state.conn.lock() {
        Ok(g) => g,
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "DB lock poisoned").into_response(),
    };

    // On récupère le nom du dossier actuel pour identifier le flux RSS
    let repo_name = std::env::current_dir()
        .ok()
        .and_then(|p| p.file_name().map(|s| s.to_string_lossy().into_owned()))
        .unwrap_or_else(|| "Lys".to_string());

    let query =
        "SELECT id, hash, author, message, timestamp FROM commits ORDER BY id DESC LIMIT 50";
    let mut stmt = match conn.prepare(query) {
        Ok(s) => s,
        Err(_) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, "Failed to query commits").into_response();
        }
    };

    let mut items = String::new();
    loop {
        match stmt.next() {
            Ok(sqlite::State::Row) => {
                let id: i64 = stmt.read("id").unwrap_or(0);
                let hash: String = stmt.read("hash").unwrap_or_default();
                let msg: String = stmt
                    .read("message")
                    .unwrap_or_else(|_| String::from("(no message)"));
                let date_str: String = stmt.read("timestamp").unwrap_or_else(|_| String::from(""));
                let author: String = stmt
                    .read("author")
                    .unwrap_or_else(|_| String::from("Unknown"));

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
    headers.insert(
        header::CONTENT_TYPE,
        header::HeaderValue::from_static("application/rss+xml"),
    );

    (StatusCode::OK, headers, rss).into_response()
}

// -----------------------------
// EDITOR & COMMIT HANDLERS
// -----------------------------

async fn editor_list() -> impl IntoResponse {
    let mut files = Vec::new();
    let walk = ignore::WalkBuilder::new(".")
        .hidden(false)
        .add_custom_ignore_filename("syl")
        .standard_filters(true)
        .build();

    for result in walk {
        if let Ok(entry) = result {
            if entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
                let path = entry.path();
                if path.components().any(|c| c.as_os_str() == ".lys") {
                    continue;
                }
                let rel = path.strip_prefix(".").unwrap_or(path);
                files.push(rel.to_string_lossy().to_string());
            }
        }
    }
    files.sort();

    let mut body = String::from("
        <h3>Editor - Select a file</h3>
        <div style='margin-bottom: 20px; padding: 15px; border: 1px solid var(--border); border-radius: 4px; background: var(--header-bg); display: flex; flex-direction: column; gap: 15px;'>
            <form action='/editor/new' method='post' style='display: flex; gap: 8px; align-items: center;'>
                <label for='path' style='font-weight: bold; font-size: 0.9em; min-width: 80px;'>New File:</label>
                <input type='text' id='path' name='path' placeholder='folder/file.txt' required style='padding: 6px; border: 1px solid var(--border); border-radius: 4px; background: var(--bg); color: var(--fg); flex: 1;'>
                <button type='submit' class='btn btn-active' style='margin-right: 0;'>Create</button>
            </form>
            <div style='display: flex; gap: 8px; align-items: center; border-top: 1px solid var(--border); padding-top: 15px;'>
                <label for='search' style='font-weight: bold; font-size: 0.9em; min-width: 80px;'>Search:</label>
                <input type='text' id='search' placeholder='Filter files...' style='padding: 6px; border: 1px solid var(--border); border-radius: 4px; background: var(--bg); color: var(--fg); flex: 1;'>
            </div>
        </div>
        <ul id='file-list'>");
    for f in files {
        body.push_str(&format!(
            "<li class='file-item'><a href='/editor/{}'>{}</a></li>",
            html_escape(&f),
            html_escape(&f)
        ));
    }
    body.push_str("</ul>
        <script>
            const searchInput = document.getElementById('search');
            const fileList = document.getElementById('file-list');
            const fileItems = fileList.getElementsByClassName('file-item');

            searchInput.addEventListener('input', function() {
                const query = searchInput.value.toLowerCase();
                for (let item of fileItems) {
                    const text = item.textContent.toLowerCase();
                    if (text.includes(query)) {
                        item.style.display = '';
                    } else {
                        item.style.display = 'none';
                    }
                }
            });
        </script>");

    page("Editor", "", &body).into_response()
}

async fn editor_new(
    axum::extract::Form(form): axum::extract::Form<NewFileForm>,
) -> impl IntoResponse {
    let path = form.path.trim().trim_start_matches('/');
    if path.is_empty() {
        return http_error(StatusCode::BAD_REQUEST, "Path cannot be empty");
    }

    let full_path = Path::new(".").join(path);
    if full_path.exists() {
        // Redirect to editor if file already exists
        return Response::builder()
            .status(StatusCode::SEE_OTHER)
            .header("Location", format!("/editor/{}", path))
            .body(axum::body::Body::empty())
            .unwrap();
    }

    // Create parent directories if they don't exist
    if let Some(parent) = full_path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            return http_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                &format!("Failed to create directories: {}", e),
            );
        }
    }

    // Create an empty file
    if let Err(e) = std::fs::write(&full_path, "") {
        return http_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            &format!("Failed to create file: {}", e),
        );
    }

    Response::builder()
        .status(StatusCode::SEE_OTHER)
        .header("Location", format!("/editor/{}", path))
        .body(axum::body::Body::empty())
        .unwrap()
}

async fn editor_edit(UrlPath(path): UrlPath<String>) -> impl IntoResponse {
    let full_path = Path::new(".").join(&path);
    if !full_path.exists() {
        return http_error(StatusCode::NOT_FOUND, "File not found");
    }

    match std::fs::read_to_string(&full_path) {
        Ok(content) => {
            let body = format!(
                "<h3>Editing: {}</h3>\
                 <div id='editor' style='width: 100%; height: 600px; border: 1px solid var(--border); border-radius: 4px;'>{}</div>\
                 <form id='editor-form' action='/editor/{}' method='post'>\
                   <input type='hidden' name='content' id='content-hidden'>\
                   <div style='margin-top: 20px;'>\
                     <button type='submit' class='btn btn-active'>Save Changes</button>\
                     <a href='/editor' class='btn'>Cancel</a>\
                   </div>\
                 </form>\
                 <script src='https://cdnjs.cloudflare.com/ajax/libs/ace/1.32.7/ace.js'></script>\
                 <script src='https://cdnjs.cloudflare.com/ajax/libs/ace/1.32.7/ext-language_tools.min.js'></script>\
                 <script src='https://cdnjs.cloudflare.com/ajax/libs/ace/1.32.7/ext-modelist.min.js'></script>\
                 <script>\
                   var editor = ace.edit('editor');\
                   editor.setTheme('ace/theme/tomorrow_night');\
                   if (window.matchMedia && window.matchMedia('(prefers-color-scheme: light)').matches) {{\
                     editor.setTheme('ace/theme/chrome');\
                   }}\
                   var modelist = ace.require('ace/ext/modelist');\
                   var mode = modelist.getModeForPath('{}').mode;\
                   editor.session.setMode(mode);\
                   editor.setOptions({{\
                     enableBasicAutocompletion: true,\
                     enableLiveAutocompletion: true,\
                     enableSnippets: true,\
                     fontSize: '14px',\
                     showPrintMargin: false,\
                     useSoftTabs: true,\
                     tabSize: 4\
                   }});\
                   document.getElementById('editor-form').onsubmit = function() {{\
                     document.getElementById('content-hidden').value = editor.getValue();\
                   }};\
                 </script>",
                html_escape(&path),
                html_escape(&content),
                html_escape(&path),
                html_escape(&path)
            );
            page(&format!("Editing {}", path), "", &body).into_response()
        }
        Err(_) => http_error(StatusCode::INTERNAL_SERVER_ERROR, "Failed to read file"),
    }
}

async fn editor_save(
    UrlPath(path): UrlPath<String>,
    axum::extract::Form(form): axum::extract::Form<EditorForm>,
) -> impl IntoResponse {
    let full_path = Path::new(".").join(&path);
    if let Err(e) = std::fs::write(&full_path, form.content) {
        return http_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            &format!("Failed to save file: {}", e),
        );
    }

    Response::builder()
        .status(StatusCode::SEE_OTHER)
        .header("Location", format!("/editor/{}", path))
        .body(axum::body::Body::empty())
        .unwrap()
}

async fn new_commit_form(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let conn = match state.conn.lock() {
        Ok(g) => g,
        Err(_) => return http_error(StatusCode::INTERNAL_SERVER_ERROR, "DB lock poisoned"),
    };

    // Obtenir le status pour montrer ce qui va être commité
    let branch = crate::db::get_current_branch(&conn).unwrap_or_else(|_| "main".to_string());
    let status = match crate::vcs::status(&conn, ".", &branch) {
        Ok(s) => s,
        Err(_) => Vec::new(),
    };

    let mut status_html = String::from("<div style='margin-bottom: 20px; padding: 10px; background: var(--menu-bg); border: 1px solid var(--border); font-family: monospace; font-size: 0.85em;'>");
    if status.is_empty() {
        status_html.push_str("No changes to commit.");
    } else {
        for s in &status {
            let (prefix, color, path) = match s {
                crate::vcs::FileStatus::New(p) => ("+", "#28a745", p),
                crate::vcs::FileStatus::Modified(p, _) => ("~", "#d4a017", p),
                crate::vcs::FileStatus::Deleted(p, _) => ("-", "#dc3545", p),
                _ => continue,
            };
            status_html.push_str(&format!(
                "<div style='color: {}'>{} {}</div>",
                color,
                prefix,
                path.display()
            ));
        }
    }
    status_html.push_str("</div>");

    let body = format!(
        "<h3>New Commit</h3>\
         {}\
         <form action='/commit/create' method='post' style='display: flex; flex-direction: column; gap: 15px;'>\
           <div>\
             <label style='display: block; font-weight: bold; margin-bottom: 5px;'>Summary:</label>\
             <input type='text' name='summary' required style='width: 100%; padding: 8px; border: 1px solid var(--border); background: var(--bg); color: var(--fg);'>\
           </div>\
           <div>\
             <label style='display: block; font-weight: bold; margin-bottom: 5px;'>Why (Reason for change):</label>\
             <textarea name='why' required style='width: 100%; height: 80px; padding: 8px; border: 1px solid var(--border); background: var(--bg); color: var(--fg);'></textarea>\
           </div>\
           <div>\
             <label style='display: block; font-weight: bold; margin-bottom: 5px;'>How (Technical details):</label>\
             <textarea name='how' required style='width: 100%; height: 80px; padding: 8px; border: 1px solid var(--border); background: var(--bg); color: var(--fg);'></textarea>\
           </div>\
           <div>\
             <label style='display: block; font-weight: bold; margin-bottom: 5px;'>Outcome (Result of changes):</label>\
             <textarea name='outcome' required style='width: 100%; height: 80px; padding: 8px; border: 1px solid var(--border); background: var(--bg); color: var(--fg);'></textarea>\
           </div>\
           <div style='margin-top: 10px;'>\
             <button type='submit' class='btn btn-active' {}>Commit Changes</button>\
             <a href='/' class='btn'>Cancel</a>\
           </div>\
         </form>",
        status_html,
        if status.is_empty() { "disabled" } else { "" }
    );

    page("New Commit", "", &body).into_response()
}

async fn create_commit(
    State(state): State<Arc<AppState>>,
    axum::extract::Form(form): axum::extract::Form<CommitForm>,
) -> impl IntoResponse {
    let conn = match state.conn.lock() {
        Ok(g) => g,
        Err(_) => return http_error(StatusCode::INTERNAL_SERVER_ERROR, "DB lock poisoned"),
    };

    // On vérifie qu'il y a bien des changements
    let branch = crate::db::get_current_branch(&conn).unwrap_or_else(|_| "main".to_string());
    let status = match crate::vcs::status(&conn, ".", &branch) {
        Ok(s) => s,
        Err(_) => Vec::new(),
    };

    if status.is_empty() {
        return http_error(StatusCode::BAD_REQUEST, "No changes to commit");
    }

    // Construction du message formaté (on émule Commit::Display)
    let message = format!(
        "{}\n\n{}\n\n{}\n\n{}",
        form.summary, form.why, form.how, form.outcome
    );

    let author = crate::commit::author();

    if let Err(e) = crate::vcs::commit(&conn, &message, &author) {
        return http_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            &format!("Commit failed: {}", e),
        );
    }

    Response::builder()
        .status(StatusCode::SEE_OTHER)
        .header("Location", "/")
        .body(axum::body::Body::empty())
        .unwrap()
}

// -----------------------------
// TODO HANDLERS
// -----------------------------

async fn todo_list(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let conn = match state.conn.lock() {
        Ok(g) => g,
        Err(_) => return http_error(StatusCode::INTERNAL_SERVER_ERROR, "DB lock poisoned"),
    };

    let _ = crate::todo::check_and_reset_todos(&conn);

    let query = "SELECT id, title, status, IFNULL(assigned_to, 'Me'), IFNULL(due_date, 'No limit') FROM todos ORDER BY CASE WHEN status = 'DONE' THEN 1 ELSE 0 END, created_at DESC";
    let mut stmt = match conn.prepare(query) {
        Ok(s) => s,
        Err(_) => return http_error(StatusCode::INTERNAL_SERVER_ERROR, "Failed to prepare query"),
    };

    let mut rows_html = String::new();
    while let Ok(sqlite::State::Row) = stmt.next() {
        let id: i64 = stmt.read(0).unwrap();
        let title: String = stmt.read(1).unwrap();
        let status: String = stmt.read(2).unwrap();
        let assigned: String = stmt.read(3).unwrap();
        let due_date: String = stmt.read(4).unwrap();

        let status_class = match status.as_str() {
            "TODO" => "todo-pending",
            "IN_PROGRESS" => "todo-progress",
            "DONE" => "todo-done",
            _ => "",
        };

        let mut actions = String::new();
        if status != "DONE" {
            if status == "TODO" {
                actions.push_str(&format!(
                    "<form action='/todo/update/{}/IN_PROGRESS' method='post' style='display:inline;'><button type='submit' class='btn'>Start</button></form>",
                    id
                ));
            }
            actions.push_str(&format!(
                "<form action='/todo/update/{}/DONE' method='post' style='display:inline;'><button type='submit' class='btn btn-active'>Complete</button></form>",
                id
            ));
        }

        rows_html.push_str(&format!(
            "<tr>\
                <td>{}</td>\
                <td><span class='todo-status {}'>{}</span></td>\
                <td><strong>{}</strong></td>\
                <td>{}</td>\
                <td>{}</td>\
                <td style='text-align: right;'>{}</td>\
            </tr>",
            id, status_class, status, html_escape(&title), html_escape(&assigned), html_escape(&due_date), actions
        ));
    }

    let body = format!(
        "<h3>Todo List</h3>\
         <div style='margin-bottom: 30px; background: var(--menu-bg); padding: 20px; border: 1px solid var(--border); border-radius: 8px;'>\
           <h4>Add New Task</h4>\
           <form action='/todo/add' method='post' style='display: flex; gap: 20px; align-items: flex-end; flex-wrap: wrap;'>\
             <div style='flex: 1; min-width: 200px;'>\
               <label style='display:block; font-size:0.8em; margin-bottom:5px;'>Title</label>\
               <input type='text' name='title' required style='width:100%; padding:8px; border:1px solid var(--border); border-radius:4px; background: var(--bg); color: var(--fg);'>\
             </div>\
             <div>\
               <label style='display:block; font-size:0.8em; margin-bottom:5px;'>Assigned to</label>\
               <input type='text' name='assigned_to' placeholder='Me' style='padding:8px; border:1px solid var(--border); border-radius:4px; background: var(--bg); color: var(--fg);'>\
             </div>\
             <div>\
               <label style='display:block; font-size:0.8em; margin-bottom:5px;'>Due Date</label>\
               <input type='date' name='due_date' style='padding:8px; border:1px solid var(--border); border-radius:4px; background: var(--bg); color: var(--fg);'>\
             </div>\
             <button type='submit' class='btn btn-active' style='height:38px;'>Add Todo</button>\
           </form>\
         </div>\
         <table>\
           <thead>\
             <tr>\
               <th style='width: 40px;'>ID</th>\
               <th style='width: 100px;'>Status</th>\
               <th>Task</th>\
               <th style='width: 150px;'>Assigned</th>\
               <th style='width: 150px;'>Due Date</th>\
               <th style='text-align: right; width: 200px;'>Actions</th>\
             </tr>\
           </thead>\
           <tbody>{}</tbody>\
         </table>",
        rows_html
    );

    page(
        "Todo List",
        ".todo-status { padding: 2px 8px; border-radius: 4px; font-size: 0.8em; font-weight: bold; }\
         .todo-pending { background: #6c757d; color: white; }\
         .todo-progress { background: #ffc107; color: black; }\
         .todo-done { background: #28a745; color: white; text-decoration: line-through; opacity: 0.7; }\
         h4 { margin-top: 0; margin-bottom: 15px; }",
        &body
    ).into_response()
}

async fn todo_add(
    State(state): State<Arc<AppState>>,
    axum::extract::Form(form): axum::extract::Form<TodoForm>,
) -> impl IntoResponse {
    let conn = match state.conn.lock() {
        Ok(g) => g,
        Err(_) => return http_error(StatusCode::INTERNAL_SERVER_ERROR, "DB lock poisoned"),
    };

    if let Err(e) = crate::todo::add_todo(
        &conn,
        &form.title,
        form.assigned_to.as_deref(),
        form.due_date.as_deref(),
    ) {
        return http_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            &format!("Failed to add todo: {}", e),
        );
    }

    Response::builder()
        .status(StatusCode::SEE_OTHER)
        .header("Location", "/todo")
        .body(axum::body::Body::empty())
        .unwrap()
}

async fn todo_update(
    State(state): State<Arc<AppState>>,
    UrlPath(params): UrlPath<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    let id: i64 = match params.get("id").and_then(|i| i.parse().ok()) {
        Some(i) => i,
        None => return http_error(StatusCode::BAD_REQUEST, "Invalid ID"),
    };
    let status = params.get("status").map(|s| s.as_str()).unwrap_or("");

    let conn = match state.conn.lock() {
        Ok(g) => g,
        Err(_) => return http_error(StatusCode::INTERNAL_SERVER_ERROR, "DB lock poisoned"),
    };

    let res = match status {
        "IN_PROGRESS" => crate::todo::start_todo(&conn, id),
        "DONE" => crate::todo::complete_todo(&conn, id),
        _ => return http_error(StatusCode::BAD_REQUEST, "Invalid status"),
    };

    if let Err(e) = res {
        return http_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            &format!("Failed to update todo: {}", e),
        );
    }

    Response::builder()
        .status(StatusCode::SEE_OTHER)
        .header("Location", "/todo")
        .body(axum::body::Body::empty())
        .unwrap()
}

async fn upload_atom(
    State(state): State<Arc<AppState>>,
    UrlPath(hash): UrlPath<String>,
    headers: HeaderMap,
    body: Bytes,
) -> StatusCode {
    // 1. On récupère la signature envoyée par le client dans l'entête
    let signature = match headers
        .get("X-Silex-Signature")
        .and_then(|s| s.to_str().ok())
    {
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

            let query = "INSERT OR IGNORE INTO store.blobs (hash, content, size) VALUES (?, ?, ?)";
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
