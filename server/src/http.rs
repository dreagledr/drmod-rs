use std::net::SocketAddr;
use std::sync::Arc;
use axum::{Router, extract::State, response::Html, routing::get};

use crate::system::System;

struct AppState {
    system: Arc<System>,
    public_addr: String,
}

pub(crate) async fn serve_http(system: Arc<System>, public_addr: String) {
    let http_port: u16 = std::env::var("HTTP_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(8080);

    let state = Arc::new(AppState { system, public_addr: public_addr.clone() });

    let app = Router::new()
        .route("/", get(root_handler))
        .with_state(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], http_port));
    println!("[http] listening on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

async fn root_handler(State(state): State<Arc<AppState>>) -> Html<String> {
    let rooms = state.system.rooms.read().await;
    let html = render_page(&state.public_addr, &rooms);
    Html(html)
}

fn render_page(public_addr: &str, rooms: &std::collections::HashMap<String, Arc<tokio::sync::Mutex<crate::system::Room>>>) -> String {
    let mut rows = String::new();
    let mut room_count = 0usize;
    let mut player_count = 0usize;

    for (name, room_arc) in rooms.iter() {
        // Блокируем асинхронно внутри blocking-контекста — используем try_lock для мгновенного снапшота
        let Ok(room) = room_arc.try_lock() else {
            continue;
        };

        if room.clients.is_empty() {
            continue;
        }

        room_count += 1;
        rows.push_str("<tr><td class=\"room-name\" colspan=\"3\">");
        rows.push_str(&html_escape(name));
        rows.push_str("</td></tr>\n");

        for (id, client) in &room.clients {
            player_count += 1;
            rows.push_str(&format!(
                "<tr><td class=\"player-id\">{}</td><td class=\"player-name\">{}</td><td class=\"player-mission\">{}</td></tr>\n",
                id,
                html_escape(&client.name),
                client.mission_id,
            ));
        }
    }

    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>drmod-server</title>
<style>
*{{margin:0;padding:0;box-sizing:border-box}}
body{{background:#111;color:#ddd;font:14px/1.5 Consolas,monospace;padding:24px 32px}}
h1{{color:#fff;font-size:20px;margin-bottom:4px}}
.server-addr{{color:#888;font-size:13px;margin-bottom:24px}}
h2{{color:#ccc;font-size:16px;margin-top:24px;margin-bottom:8px}}
table{{border-collapse:collapse;width:100%;max-width:700px}}
td{{padding:4px 10px;border-bottom:1px solid #222}}
.room-name{{color:#ffa;font-weight:bold;padding-top:14px;font-size:15px}}
.player-id{{color:#888;width:90px}}
.player-name{{color:#fff}}
.player-mission{{color:#6af;width:100px;text-align:right}}
.empty{{color:#555;margin-top:8px}}
</style>
</head>
<body>
<h1>drmod-server</h1>
<p class="server-addr">Address: {addr}</p>
<h2>Rooms ({room_count}) &mdash; {player_count} player(s)</h2>
<table>
{rows}
</table>
</body>
</html>"#,
        addr = html_escape(public_addr),
        room_count = room_count,
        player_count = player_count,
        rows = rows,
    )
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
