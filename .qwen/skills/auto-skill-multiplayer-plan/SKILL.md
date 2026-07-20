---
name: multiplayer-plan
description: Full multiplayer implementation plan for drmod-rs — Rust server (tokio, 64-bit) with TCP JSON + UDP binary relay, client-side non-blocking UDP in frame, CylinderRenderer ghost for other players, filtered by active segment mission_id
source: auto-skill
extracted_at: '2026-07-20T11:31:28.098Z'
updated_at: '2026-07-20T14:00:00.000Z'
---

# Multiplayer Implementation Plan — drmod-rs

Reference architecture from `mmultiplayer/` (Mirror's Edge multiplayer mod, C++/Go). Adapting the server-relay pattern to Rust with existing drmod-rs components.

## Architecture

```
Server (Rust, tokio, 64-bit, standalone binary) on :5222
 ├─ TCP: JSON messages (connect, disconnect, ping/pong), delimiter \0
 ├─ UDP: binary PositionPacket (28 bytes) — relay to room members
 └─ Rooms → Clients → last_packet (no mission filtering on server)
      ↑↓
drmod_rs_lib.dll (32-bit, in MGR:R process)
 ├─ src/net.rs: NetClient
 │    ├─ TcpStream — blocking, in dedicated std::thread → mpsc::channel → main thread
 │    └─ UdpSocket — non-blocking, polled every frame in render()
 ├─ render_3d: CylinderRenderer for each RemotePlayer on same mission
 └─ render: draw_world_pos (name + distance) + UI "Multiplayer" window
```

**Why:** mmultiplayer uses `std::thread playerHandlerThread(PlayerHandler)` for blocking `recvfrom`. Rust equivalent: `std::thread::spawn` for TCP, `set_nonblocking(true)` + `while let Ok(...)` in-frame for UDP. Sending via `send_to` on UDP is non-blocking — safe in frame.

## Key Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Server language | Rust (tokio) | Single codebase, no Go toolchain |
| Server arch | 64-bit, separate crate `server/` | Docker-friendly, not constrained by game's 32-bit |
| Room ↔ mission | No binding on server | Players change missions mid-session; filter on client by `active_segment.mission_id` |
| 3D marker | CylinderRenderer (blue) + draw_world_pos | Reuses existing `d3d_render.rs` and `world_to_screen` |
| Position type | `segment::Vec3` | Already exists, re-export or move to protocol.rs |
| UDP in frame | Non-blocking `recv_from` loop | `WouldBlock` returns instantly; no frame drops |
| TCP in frame | Separate `std::thread` + `mpsc::channel` | JSON parsing is blocking; offload to thread |

## Protocol

### UDP — PositionPacket (28 bytes, little-endian, `#[repr(C, packed)]`)

```
Offset  Size  Type   Field
0       4     u32    id
4       4     f32    pos_x
8       4     f32    pos_y
12      4     f32    pos_z
16      4     f32    yaw        (reserved, always 0)
20      4     i32    hp
24      4     i32    mission_id
```

Serialization: `unsafe { std::mem::transmute::<PositionPacket, [u8; 28]> }` — zero-cost, no alloc.

### TCP — JSON (serde, tag-based enum)

**Client → Server:**
```json
{"type":"connect","room":"default","name":"Raiden","mission_id":0}
{"type":"disconnect"}
{"type":"ping"}
```

**Server → Client:**
```json
{"type":"id","id":12345}
{"type":"connect","id":67890,"name":"Player2","mission_id":264}
{"type":"disconnect","id":67890}
{"type":"pong"}
```

```rust
#[derive(Serialize, Deserialize)]
#[serde(tag = "type")]
enum TcpMessage {
    #[serde(rename = "connect")]
    Connect { room: String, name: String, mission_id: i32 },
    #[serde(rename = "disconnect")]
    Disconnect,
    #[serde(rename = "ping")]
    Ping,
    #[serde(rename = "id")]
    IdAssigned { id: u32 },
    #[serde(rename = "connect")]
    PlayerConnected { id: u32, name: String, mission_id: i32 },
    #[serde(rename = "disconnect")]
    PlayerDisconnected { id: u32 },
    #[serde(rename = "pong")]
    Pong,
}
```

## Files

### New

| File | Purpose | ~Lines |
|------|---------|--------|
| `src/protocol.rs` | `PositionPacket`, `TcpMessage` enum, serde derives | 50 |
| `src/net.rs` | `NetClient` struct, `RemotePlayer`, TCP thread, UDP in-frame | 200 |
| `server/Cargo.toml` | Standalone crate, deps: tokio (rt+net), serde, serde_json, uuid | 15 |
| `server/.cargo/config.toml` | No `target` override (native 64-bit) | 2 |
| `server/src/main.rs` | TCP accept, JSON dispatch, UDP relay, ping loop | 300 |

### Modified

| File | Change |
|------|--------|
| `Cargo.toml` | `[workspace] members = ["server"]` |
| `src/lib.rs` | +6 fields in `HelloHud`, +TCP/UDP handling in `render`, +remote player rendering in `render_3d`, +UI window |

## Implementation Order

### Step 0: Separate server crate

Create `server/` with own `Cargo.toml` and `.cargo/config.toml` (no `target = "i686-..."`). Add to root `[workspace] members`.

**Why:** Root `.cargo/config.toml` forces `i686-pc-windows-msvc` for the DLL. Server must compile as 64-bit for Docker.

### Step 1: `src/protocol.rs`

Shared types between client and server. `PositionPacket` with `#[repr(C, packed)]`, `TcpMessage` enum with `#[serde(tag = "type")]`.

### Step 2: Root `Cargo.toml`

Add `[workspace]` with `members = ["server"]`. Server deps only in `server/Cargo.toml` — DLL does NOT link tokio.

### Step 3: `server/src/main.rs`

Port `mmultiplayer/Server/` architecture:

```
main()
 ├─ tokio::spawn(tcp_listener)
 ├─ tokio::spawn(udp_listener)
 └─ tokio::spawn(ping_loop)  // every 2s → {"type":"ping"}
```

**TCP listener:** accept → `Client { id: uuid, tcp }` → spawn `client_handler()`.
- Reads JSON until `\0` delimiter
- `"connect"` → join/create room, store name/mission_id, reply `{"type":"id"}`, notify others
- `"disconnect"` → remove from room, notify others
- `"ping"` → reply `{"type":"pong"}`

**UDP listener:** `recv_from` → parse `PositionPacket` → `client.set_last_packet(buf)` → `room.send_last_packets(udp, addr, exclude_id)`.
- `send_last_packets`: for each other client in room, `udp.send_to(&c.last_packet, addr)`

**Structures:**
```rust
struct System { rooms: HashMap<String, Room> }
struct Room { name: String, clients: HashMap<u32, Client> }
struct Client {
    id: u32, name: String, mission_id: i32,
    last_packet: Vec<u8>,  // 28 bytes
    tcp_tx: UnboundedSender<String>,  // for room→client JSON
}
```

### Step 4: `src/net.rs`

```rust
pub struct NetClient {
    tcp: TcpStream,
    udp: UdpSocket,
    pub my_id: u32,
    pub remote_players: Vec<RemotePlayer>,
    tcp_rx: mpsc::Receiver<TcpEvent>,
}

pub struct RemotePlayer {
    pub id: u32,
    pub name: String,
    pub pos: segment::Vec3,
    pub hp: i32,
    pub mission_id: i32,
    pub last_update: Instant,
}
```

**`NetClient::new(addr, name, room)`:**
1. `TcpStream::connect(addr)` — blocking, called once in `HelloHud::new()` or on button click
2. `UdpSocket::bind("0.0.0.0:0")` + `.set_nonblocking(true)`
3. Send `{"type":"connect","room":...,"name":...}`
4. Spawn `std::thread` for TCP reads:
   - Read byte-by-byte until `\0`
   - `serde_json::from_slice::<TcpMessage>()`
   - `tx.send(event)`

**In-frame calls (from `HelloHud::render()`):**
```rust
// TCP events
while let Ok(event) = nc.tcp_rx.try_recv() {
    match event {
        IdAssigned(id) => nc.my_id = id,
        PlayerConnected(p) => nc.remote_players.push(p),
        PlayerDisconnected(id) => nc.remote_players.retain(|p| p.id != id),
    }
}

// Send own position (if changed)
if current_pos != last_sent_pos || mission_id != last_sent_mission_id {
    let packet = PositionPacket { id: nc.my_id, pos_x, pos_y, pos_z, yaw: 0, hp, mission_id };
    nc.udp.send_to(&transmute::<_, [u8; 28]>(packet), server_addr);
}

// Receive others' positions (non-blocking)
let mut buf = [0u8; 28];
while let Ok((n, _)) = nc.udp.recv_from(&mut buf) {
    if n == 28 {
        let p: PositionPacket = transmute(buf);
        if p.id != nc.my_id {
            // update or insert RemotePlayer
        }
    }
}
```

### Step 5: `src/lib.rs` — fields + render

**New fields in `HelloHud`:**
```rust
net_client: Option<NetClient>,
player_name: String,        // "Raiden"
room_name: String,          // "default"
server_addr: String,        // "127.0.0.1:5222"
last_sent_pos: Option<segment::Vec3>,
last_sent_mission_id: i32,
```

**In `render_3d()`** — after ghost cylinder, add:
```rust
if let (Some(nc), Some(seg)) = (&self.net_client, &self.active_segment) {
    for rp in &nc.remote_players {
        if rp.mission_id != seg.mission_id { continue; }
        if rp.last_update.elapsed() > Duration::from_secs(5) { continue; }
        self.dummy.render(device, (rp.pos.x, rp.pos.y, rp.pos.z), 0.4, 2.0, 0x8000FFFF, &view_proj);
    }
}
```

**In `render()`** — 2D overlay:
```rust
for rp in &nc.remote_players {
    if rp.mission_id != active_mission { continue; }
    draw_world_pos(ui, (rp.pos.x, rp.pos.y, rp.pos.z), camera_ptr, 0xFF8080FF,
        &format!("{} ({}HP)", rp.name, rp.hp));
}
```

### Step 6: `src/lib.rs` — UI window

```rust
ui.window("Multiplayer")
    .size([300.0, 250.0], Condition::FirstUseEver)
    .build(|| {
        ui.input_text("Server", &mut self.server_addr);
        ui.input_text("Name", &mut self.player_name);
        ui.input_text("Room", &mut self.room_name);
        
        if self.net_client.is_none() {
            if ui.button("Connect") { /* NetClient::new(...) */ }
        } else {
            if ui.button("Disconnect") { self.net_client = None; }
            ui.text(format!("Status: Connected (ID: {})", nc.my_id));
        }
        
        // Player list
        ui.separator();
        ui.text("Players:");
        ui.text(format!("{} (you) - {}", self.player_name, my_mission));
        for rp in &nc.remote_players {
            let active = if same_mission { "active" } else { "" };
            ui.text(format!("{} - 0x{:04X} {}", rp.name, rp.mission_id, active));
        }
    });
```

## Implementation Notes & Lessons Learned

### CRITICAL: ImGui window collapse stops code execution

**Bug:** Network polling (`poll_tcp`, `send_position`, `recv_positions`) was placed inside `ui.window("##hello").build(|| { ... })`. When the window is collapsed (hidden), the closure body **does not execute** — positions stop sending, UDP packets stop receiving, ghost disappears.

**Fix:** Move all per-frame logic (network, state updates) **before** any `ui.window(...).build()` calls. Only ImGui drawing code belongs inside window closures:

```rust
fn render(&mut self, ui: &mut Ui) {
    // ── Per-frame logic (ALWAYS runs) ──
    if let Some(ref mut nc) = self.net_client {
        nc.poll_tcp();
        nc.send_position(...);
        nc.recv_positions();
    }

    // ── UI drawing (only when window is open) ──
    ui.window("Data").build(|| { /* display-only code */ });
}
```

**Why this matters:** This is a general hudhook/imgui-rs pattern — any state-mutating logic inside `.build()` closures is conditional on window visibility. For injected overlays where per-frame processing is critical (network, segment tracking, debug logging), always place it outside ImGui windows.

**How to apply:** Before adding any new feature to the overlay, ask: "Does this need to run every frame, or only when the window is visible?" If every frame — put it before `ui.window(...)`. If display-only — inside the closure.

### Server: tokio feature requirements

The server needs explicit tokio features beyond "net" and "rt":
```toml
tokio = { version = "1", features = [
    "net", "rt-multi-thread", "macros", "time", "sync", "io-util"
] }
```
`"sync"` is needed for `tokio::sync::Mutex`, `tokio::sync::RwLock`, and `tokio::sync::mpsc`. `"io-util"` is needed for `AsyncWriteExt`, `AsyncBufReadExt`, `BufReader`.

### tokio::sync locks for data crossing .await

`std::sync::MutexGuard` and `std::sync::RwLockReadGuard` are **not `Send`** — they cannot be held across `.await` points inside `tokio::spawn`. When data needs to be accessed across an async boundary (e.g., UDP `send_to` after reading room state), use `tokio::sync::Mutex` and `tokio::sync::RwLock` instead. Their guards (`MutexGuard` from tokio) **are** `Send`.

**Pattern:**
```rust
// BAD — std::sync::MutexGuard is not Send, fails tokio::spawn
let lock = room.lock().unwrap();
lock.send_last_packets(&udp, addr, id, mock).await; // ERROR

// GOOD — tokio::sync::MutexGuard is Send
let mut lock = room.lock().await;
lock.send_last_packets(&udp, addr, id, mock).await;
```

For `TcpStream::write_all()` across await, wrap `OwnedWriteHalf` in `Arc<tokio::sync::Mutex<...>>`:
```rust
let writer = Arc::new(tokio::sync::Mutex::new(write_half));
tokio::spawn(async move {
    while let Some(data) = tcp_rx.recv().await {
        let _ = writer.lock().await.write_all(&data).await;
    }
});
```

### Packed struct field access (E0793)

`#[repr(C, packed)]` structs have 1-byte alignment. Taking a reference to a field (`&packet.id`) produces an unaligned reference — undefined behavior. Always **copy the field to a local variable** before using:

```rust
// WRONG
if room_lock.client_ids().contains(&packet.id) { ... }

// RIGHT
let packet_id = { packet.id };
if room_lock.client_ids().contains(&packet_id) { ... }
```

Same in net.rs for `format!("Player {}", packet.id)`.

### serde tag uniqueness

`#[serde(tag = "type")]` requires **unique tag values** across all variants. Having both `Connect` (client→server) and `PlayerConnected` (server→client) with `#[serde(rename = "connect")]` causes unreachable pattern warnings and incorrect deserialization.

**Solution:** use distinct tags:
```rust
// Client → Server
#[serde(rename = "connect")] Connect { ... }
#[serde(rename = "disconnect")] Disconnect
#[serde(rename = "ping")] Ping

// Server → Client  
#[serde(rename = "id")] IdAssigned { id: u32 }
#[serde(rename = "player_connected")] PlayerConnected { ... }
#[serde(rename = "player_disconnected")] PlayerDisconnected { id: u32 }
#[serde(rename = "pong")] Pong
```

Server code sends `"type": "player_connected"` and `"type": "player_disconnected"` (the original `"connect"` and `"disconnect"` were renamed in migration).

### Edition 2024: no explicit `ref` in patterns

Rust 2024 automatically borrows when matching on references. Explicit `ref` in patterns is now an error:
```rust
// ERROR in edition 2024
if let Some(ref nc) = &self.net_client { ... }

// CORRECT
if let Some(nc) = &self.net_client { ... }
```

### Variable scope for `hp`

`hp` is read inside the `else` branch of `if player_obj_ptr.is_null()` but must also be available in the network section below (outside the else). Declare `let mut hp: i32 = 0;` **before** the if/else, then assign inside the else branch:
```rust
let mut hp: i32 = 0;
if player_obj_ptr.is_null() { ... }
else {
    hp = unsafe { *(player_obj_ptr.add(0x870) as *const i32) };
    ...
}
// hp is now available for network send_position
```

### Server target architecture

Root `.cargo/config.toml` forces `i686-pc-windows-msvc`. Server is a separate workspace member with its own `.cargo/config.toml`:
```toml
# server/.cargo/config.toml
[build]
target = "x86_64-pc-windows-msvc"
```
This overrides the root setting for builds inside `server/`. For Docker: `cargo build --target x86_64-unknown-linux-gnu`.

### Mock player implementation

For testing without a second game instance:
- Created on `connect` of first real player in room
- `id = 0xFFFFFFFE`, `name = "Mock Player"`
- Position: real player's position + `(5.0, 0.0, 0.0)` — offset applied in `update_mock_packet()`
- Removed on `disconnect` of last real player
- Announced to clients as `player_connected` / `player_disconnected`
- No actual TCP connection — `tcp_tx` is a dummy channel that's never read from

Client-side filtering by `is_mock` flag (for UI display only — rendering still depends on mission_id match).

### Server mock position update timing

The mock packet is generated each time the real player sends a UDP `PositionPacket`. The `update_mock_packet()` function:
1. Checks if MOCK_ID exists in the room
2. Applies `MOCK_OFFSET` to the incoming real packet
3. Stores the mocked bytes as the mock client's `last_packet`
4. Returns the mocked bytes for relay

This means the mock player moves in real-time, always offset from the real player.

### Workspace structure

```
[workspace]
members = ["server"]

[dependencies]  # root crate (DLL + injector)
hudhook = ...
serde = ...
serde_json = ...
# NO tokio here — server only
```

Server dependencies are isolated in `server/Cargo.toml`.

## Verification

| Step | Command | Check |
|------|---------|-------|
| Build | `cargo build --release` | DLL (32-bit) + server.exe (64-bit) compile |
| Lint | `cargo clippy --workspace` | No warnings |
| Server | `cargo run --bin server` | Starts on :5222, logs connections |
| Client | Inject + Connect | Status shows "Connected (ID: ...)" |
| Multiplayer | Two game instances, same room, same segment | Blue cylinder + name visible for other player |

## Reused Components

| Component | Source | Usage |
|-----------|--------|-------|
| `CylinderRenderer::render()` | `src/d3d_render.rs` | 3D marker for remote players (blue, `0x8000FFFF`) |
| `draw_world_pos()` | `src/overlay.rs` | 2D name/distance overlay for remote players |
| `world_to_screen()` | `src/overlay.rs` | Used by `draw_world_pos` |
| `segment::Vec3` | `src/segment.rs` | Position type for RemotePlayer, packet fields |
| `segment::ActiveSegment` | `src/segment.rs` | `mission_id` for client-side filter |
| `GameMenuStatus` | `src/game.rs` | Not directly used, but available for menu state checks |

## Post-Implementation Refactoring

After the multiplayer feature was implemented, the codebase was cleaned up by extracting reusable UI and overlay functions into separate modules. The approach and lessons learned:

### Overlay extraction → `src/overlay.rs`

**What was moved:** `world_to_screen`, `format_duration_ms`, `draw_world_pos` — three pure functions used for 2D screen-space marker rendering.

**Procedure:**
1. Create `src/overlay.rs` with the functions marked `pub`
2. Add `mod overlay;` to `lib.rs`
3. Delete the function bodies from `lib.rs`
4. Replace all calls: `format_duration_ms(` → `overlay::format_duration_ms(` (same for `draw_world_pos`, `world_to_screen`)

**Why:** These functions have no dependency on `HelloHud` state — they take all data as parameters. Moving them out reduces `lib.rs` size and makes them testable independently.

### UI extraction → `src/ui.rs`

**What was moved:** `render_multiplayer_window` (the ImGui "Multiplayer" window). The `render_main_window` function was also prepared but left unused — the `##hello` window is too large for safe extraction via the edit tool.

**Procedure:**
1. Create `src/ui.rs` with `use crate::HelloHud;` and functions accepting `&Ui` + `&mut HelloHud`
2. Mark all fields accessed by `ui.rs` as `pub(crate)` in the `HelloHud` struct
3. Add `mod ui;` to `lib.rs`
4. Replace the window closure in `render()` with `ui::render_multiplayer_window(ui, self);`

**Why use `pub(crate)` instead of getters:** The ui functions need mutable access to many fields (`net_client`, `player_name`, `server_addr`, `active_segment`, etc.). Getters/setters for 20+ fields would be boilerplate-heavy. `pub(crate)` limits visibility to the current crate — no external API impact.

### Git workflow for safe refactoring

When dealing with files that are too large for a single `edit` operation (e.g., the 400-line `##hello` window):

1. **Commit after each successful step.** This creates a restore point. If the next edit corrupts the file, `git checkout` restores to the last good state.
2. **Restore and retry small.** If an edit leaves the file in a broken state (mismatched braces, leftover code), `git checkout` immediately and try a smaller edit.
3. **Prefer `replace_all` for mechanical renames** (e.g., `format_duration_ms(` → `overlay::format_duration_ms(`). These are atomic and cannot partially fail.
4. **For large block replacements**, replace the opening marker first (e.g., `.build(|| {` → function call), then remove the orphaned body in subsequent small edits. The edit tool handles ~20-line old_string blocks reliably; larger blocks risk whitespace mismatches.
5. **Avoid PowerShell for UTF-8 files** — PowerShell's `Set-Content` can corrupt non-ASCII characters (Cyrillic comments). Use the edit tool or `git checkout` for all text mutations.

**Applied example:**
```
git checkout src/lib.rs                    # start clean
# Edit 1: mod overlay; + function deletion (edit tool, ~130 line old_string)
cargo check --lib && git commit -m "..."
# Edit 2: Multiplayer window replacement (edit tool, ~70 line old_string)
cargo check --lib && git commit -m "..."
# Stop — ##hello left in lib.rs (too large for safe edit)
```
