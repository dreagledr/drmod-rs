use std::sync::Arc;
use tokio::net::{TcpListener, UdpSocket};
use tokio::time::{Duration, interval};

use crate::system::System;
use drmod_protocol::{make_msg, TcpMessage};

mod http;
mod mock;
mod system;
mod tcp;
mod udp;

const PORT: u16 = 5222;
const MOCK_ID: u32 = 0xFFFFFFFE;
const MOCK_NAME: &str = "Mock Player";
const MOCK_OFFSET: (f32, f32, f32) = (5.0, 0.0, 0.0);
const MAX_UDP: usize = 8192;

// ── Ping loop ──

async fn ping_loop(system: Arc<System>) {
    let mut tick = interval(Duration::from_secs(2));
    loop {
        tick.tick().await;
        let ping = make_msg(&TcpMessage::Ping);
        let rooms = system.rooms.read().await;
        for room_arc in rooms.values() {
            let room = room_arc.lock().await;
            room.send_tcp_all(&ping);
        }
    }
}

// ── main ──

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    let enable_mock = args.iter().any(|a| a == "--mock");

    let public_addr = {
        let mut addr = std::env::var("PUBLIC_ADDR").unwrap_or_else(|_| String::from("localhost:5222"));
        let mut i = 1;
        while i < args.len() {
            if args[i] == "--public-addr" && i + 1 < args.len() {
                addr = args[i + 1].clone();
                break;
            }
            i += 1;
        }
        addr
    };

    if enable_mock {
        println!("[server] mock player enabled");
    }
    println!("[server] public address: {}", public_addr);

    let system = Arc::new(System::new(enable_mock));

    let tcp_listener = TcpListener::bind(format!("0.0.0.0:{}", PORT))
        .await
        .expect("failed to bind TCP");
    println!("[server] TCP listening on :{}", PORT);

    let udp_socket = Arc::new(
        UdpSocket::bind(format!("0.0.0.0:{}", PORT))
            .await
            .expect("failed to bind UDP"),
    );
    println!("[server] UDP listening on :{}", PORT);

    let system_tcp = system.clone();
    tokio::spawn(async move {
        loop {
            let (stream, addr) = match tcp_listener.accept().await {
                Ok(v) => v,
                Err(e) => {
                    println!("[tcp] accept error: {}", e);
                    continue;
                }
            };
            println!("[tcp] new connection from {}", addr);
            tokio::spawn(tcp::handle_tcp(stream, addr, system_tcp.clone()));
        }
    });

    let system_udp = system.clone();
    let udp_udp = udp_socket.clone();
    tokio::spawn(udp::handle_udp(udp_udp, system_udp));

    let system_http = system.clone();
    let http_addr = public_addr.clone();
    tokio::spawn(http::serve_http(system_http, http_addr));

    ping_loop(system).await;
}
