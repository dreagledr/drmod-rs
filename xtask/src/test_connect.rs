//! `cargo xtask test-connect` — the multiplayer server smoke test (was `test_connect.ps1`).
//!
//! Requires the server running (`docker compose up` in `drmod-server/`, or a locally built one).
//! Steps, one for one from the script: read the dashboard, connect a test player over TCP, check
//! that it appears, disconnect, check that it is gone.
//!
//! # What changed in the port
//!
//! The `connect`/`disconnect` frames are built from [`drmod_protocol::TcpMessage`] and framed with
//! [`drmod_protocol::make_msg`], and the reply is parsed back into the same enum. The PowerShell
//! script wrote `'{"type":"connect",...}' + "`0"` as a string and printed whatever came back — a
//! message shape that changed in the protocol would have left it sending something the server no
//! longer understood, with no compile error anywhere.
//!
//! ⚠️ **The reads have timeouts.** The dashboard is fetched with `ureq`'s global timeout, and the TCP
//! socket gets both a connect and a read timeout: a server that accepts the connection and never
//! answers is a failure here rather than a hang.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::time::Duration;

use drmod_protocol::{TcpMessage, make_msg};

use crate::{step, util, Error, Result};

const USAGE: &str = "\
Usage: cargo xtask test-connect [options]

Smoke-tests the multiplayer server: the HTTP dashboard and a TCP connect/disconnect.

Options:
  --server <host>      Server host (default: localhost)
  --tcp-port <port>    TCP port for the game protocol (default: 5222)
  --http-port <port>   Port the dashboard is served on (default: 8080)
  -h, --help           Show this message

Exits 0 when every check passed, 1 otherwise.";

/// How long the whole HTTP dashboard request may take.
const HTTP_TIMEOUT: Duration = Duration::from_secs(3);
/// How long to wait for the TCP connect, and for the server's answer.
const SOCKET_TIMEOUT: Duration = Duration::from_secs(5);

/// The delay the PowerShell script used after sending `connect`, so the server has processed it
/// before the dashboard is read. The server relays per frame, and the dashboard is a live view.
const SETTLE: Duration = Duration::from_millis(500);

pub fn run(root: PathBuf, args: &[String]) -> Result<()> {
    crate::announce(&root);

    let mut server = "localhost".to_owned();
    let mut tcp_port: u16 = 5222;
    let mut http_port: u16 = 8080;

    let mut args = util::Args::new(args);
    while let Some(arg) = args.next() {
        match arg {
            "--server" => server = args.value("--server")?,
            "--tcp-port" => tcp_port = parse_port(&args.value("--tcp-port")?, "--tcp-port")?,
            "--http-port" => http_port = parse_port(&args.value("--http-port")?, "--http-port")?,
            "-h" | "--help" => {
                println!("{USAGE}");
                return Ok(());
            }
            other => return Err(Error::new(format!("unknown option `{other}`\n\n{USAGE}"))),
        }
    }

    let http_url = format!("http://{server}:{http_port}");

    println!();
    step("Step 1: dashboard (no players yet)");
    dashboard(&http_url)?;

    println!();
    step("Step 2: connecting a test player");
    let mut connection = connect(&server, tcp_port)?;

    // ── Step 3 ──
    println!();
    step("Step 3: dashboard (should show the test player)");
    std::thread::sleep(SETTLE);
    dashboard(&http_url)?;

    // ── Step 4 ──
    println!();
    step("Step 4: disconnecting the test player");
    let disconnect = make_msg(&TcpMessage::Disconnect);
    connection
        .write_all(&disconnect)
        .map_err(|error| Error::new(format!("cannot send `disconnect`: {error}")))?;
    connection
        .flush()
        .map_err(|error| Error::new(format!("cannot flush the disconnect: {error}")))?;
    drop(connection);

    // ── Step 5 ──
    println!();
    step("Step 5: dashboard (should be empty again)");
    std::thread::sleep(SETTLE);
    dashboard(&http_url)?;

    println!();
    println!("All tests passed!");
    Ok(())
}

/// Opens the TCP connection and performs the `connect` handshake, returning the live stream.
///
/// The reply is read up to the `\0` delimiter and parsed back into [`TcpMessage`]: the id the server
/// assigns is what proves the handshake completed, rather than the mere fact that bytes arrived.
fn connect(server: &str, port: u16) -> Result<TcpStream> {
    let address = format!("{server}:{port}");
    let mut stream = TcpStream::connect(&address)
        .map_err(|error| Error::new(format!("TCP connect to {address} failed: {error}")))?;

    stream
        .set_read_timeout(Some(SOCKET_TIMEOUT))
        .map_err(|error| Error::new(format!("cannot set the read timeout: {error}")))?;
    stream
        .set_write_timeout(Some(SOCKET_TIMEOUT))
        .map_err(|error| Error::new(format!("cannot set the write timeout: {error}")))?;

    println!("TCP connected to {address}");

    let message = TcpMessage::Connect {
        room: "test-room".to_owned(),
        name: "TestBot".to_owned(),
        mission_id: 42,
    };

    stream
        .write_all(&make_msg(&message))
        .map_err(|error| Error::new(format!("cannot send `connect`: {error}")))?;
    stream
        .flush()
        .map_err(|error| Error::new(format!("cannot flush the connect: {error}")))?;

    println!("Sent: connect TestBot -> room test-room");

    let reply = read_line(&mut stream)?;
    match serde_json::from_str::<TcpMessage>(&reply) {
        Ok(TcpMessage::IdAssigned { id }) => println!("Server response: id {id} assigned"),
        Ok(other) => println!("Server response: {} (unexpected, but the connection is live)", type_name(&other)),
        Err(error) => {
            return Err(Error::new(format!(
                "the server's reply is not a protocol message: {error}\n       got: {reply}"
            )));
        }
    }

    Ok(stream)
}

/// Reads one `\0`-terminated message off the socket.
fn read_line(stream: &mut TcpStream) -> Result<String> {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 512];

    loop {
        let read = stream
            .read(&mut chunk)
            .map_err(|error| Error::new(format!("cannot read the server's reply: {error}")))?;

        if read == 0 {
            break;
        }

        // The protocol delimiter is the first `\0`; anything after it belongs to the next message.
        if let Some(end) = chunk[..read].iter().position(|byte| *byte == 0) {
            buffer.extend_from_slice(&chunk[..end]);
            break;
        }

        buffer.extend_from_slice(&chunk[..read]);
    }

    String::from_utf8(buffer)
        .map_err(|error| Error::new(format!("the server's reply is not UTF-8: {error}")))
}

/// Fetches and prints the dashboard, failing when it cannot be reached.
fn dashboard(url: &str) -> Result<()> {
    let config = ureq::Agent::config_builder()
        .timeout_global(Some(HTTP_TIMEOUT))
        .build();
    let agent: ureq::Agent = config.into();

    let body = agent
        .get(url)
        .call()
        .map_err(|error| Error::new(format!("dashboard unreachable at {url}: {error}")))?
        .into_body()
        .read_to_string()
        .map_err(|error| Error::new(format!("cannot read the dashboard at {url}: {error}")))?;

    println!("{body}");
    Ok(())
}

fn parse_port(value: &str, flag: &str) -> Result<u16> {
    value
        .parse()
        .map_err(|_| Error::new(format!("`{flag}` wants a port number, got `{value}`")))
}

/// The variant's name, for the one case where an unexpected message is worth printing.
fn type_name(message: &TcpMessage) -> &'static str {
    match message {
        TcpMessage::Connect { .. } => "connect",
        TcpMessage::Disconnect => "disconnect",
        TcpMessage::Ping => "ping",
        TcpMessage::IdAssigned { .. } => "id",
        TcpMessage::PlayerConnected { .. } => "player_connected",
        TcpMessage::PlayerDisconnected { .. } => "player_disconnected",
        TcpMessage::Pong => "pong",
    }
}
