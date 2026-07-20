use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::{TcpListener, TcpStream, UdpSocket};
use tokio::sync::{mpsc, Mutex, RwLock};
use tokio::time::{interval, Duration};
use uuid::Uuid;

const PORT: u16 = 5222;
const PACKET_SIZE: usize = 28;
const MOCK_ID: u32 = 0xFFFFFFFE;
const MOCK_NAME: &str = "Mock Player";
const MOCK_OFFSET: (f32, f32, f32) = (5.0, 0.0, 0.0);

// ── Packet (дубликат src/protocol.rs — сервер независим от 32-bit крейта) ──

#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
struct PositionPacket {
    id: u32,
    pos_x: f32,
    pos_y: f32,
    pos_z: f32,
    yaw: f32,
    hp: i32,
    mission_id: i32,
}

impl PositionPacket {
    fn from_bytes(bytes: &[u8; PACKET_SIZE]) -> Self {
        unsafe { std::mem::transmute(*bytes) }
    }

    fn to_bytes(&self) -> [u8; PACKET_SIZE] {
        unsafe { std::mem::transmute(*self) }
    }

    fn with_offset(&self, id: u32, name_offset: (f32, f32, f32)) -> Self {
        Self {
            id,
            pos_x: self.pos_x + name_offset.0,
            pos_y: self.pos_y + name_offset.1,
            pos_z: self.pos_z + name_offset.2,
            ..*self
        }
    }
}

// ── TCP messages ──

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
enum TcpIn {
    #[serde(rename = "connect")]
    Connect {
        room: String,
        name: String,
        mission_id: i32,
    },
    #[serde(rename = "disconnect")]
    Disconnect,
    #[serde(rename = "ping")]
    Ping,
}

fn make_msg<T: Serialize>(value: &T) -> Vec<u8> {
    let mut json = serde_json::to_vec(value).unwrap_or_default();
    json.push(0); // \0 delimiter
    json
}

// ── Client ──

struct Client {
    id: u32,
    name: String,
    mission_id: i32,
    last_packet: [u8; PACKET_SIZE],
    tcp_tx: mpsc::UnboundedSender<Vec<u8>>,
}

// ── Room ──

struct Room {
    clients: HashMap<u32, Client>,
}

impl Room {
    fn new() -> Self {
        Self {
            clients: HashMap::new(),
        }
    }

    fn add_client(&mut self, client: Client) {
        self.clients.insert(client.id, client);
    }

    fn remove_client(&mut self, id: u32) {
        self.clients.remove(&id);
    }

    fn client_ids(&self) -> Vec<u32> {
        self.clients.keys().copied().collect()
    }

    fn send_tcp(&self, exclude_id: u32, data: &[u8]) {
        for (id, c) in &self.clients {
            if *id != exclude_id {
                let _ = c.tcp_tx.send(data.to_vec());
            }
        }
    }

    fn send_tcp_all(&self, data: &[u8]) {
        for c in self.clients.values() {
            let _ = c.tcp_tx.send(data.to_vec());
        }
    }

    fn set_last_packet(&mut self, id: u32, packet: &[u8; PACKET_SIZE]) {
        if let Some(c) = self.clients.get_mut(&id) {
            c.last_packet = *packet;
        }
    }

    async fn send_last_packets(
        &self,
        udp: &UdpSocket,
        addr: SocketAddr,
        exclude_id: u32,
        mock_packet: Option<&[u8; PACKET_SIZE]>,
    ) {
        // Шлём last_packet всех реальных клиентов
        for (id, c) in &self.clients {
            if *id != exclude_id && *id != MOCK_ID {
                let _ = udp.send_to(&c.last_packet, addr).await;
            }
        }
        // Шлём mock-пакет если есть
        if let Some(pkt) = mock_packet {
            let _ = udp.send_to(pkt, addr).await;
        }
    }
}

// ── System ──

struct System {
    rooms: RwLock<HashMap<String, Arc<Mutex<Room>>>>,
    enable_mock: bool,
}

impl System {
    fn new(enable_mock: bool) -> Self {
        Self {
            rooms: RwLock::new(HashMap::new()),
            enable_mock,
        }
    }

    async fn get_or_create_room(&self, name: &str) -> Arc<Mutex<Room>> {
        let rooms = self.rooms.read().await;
        if let Some(room) = rooms.get(name) {
            return room.clone();
        }
        drop(rooms);

        let mut rooms = self.rooms.write().await;
        rooms
            .entry(name.to_string())
            .or_insert_with(|| Arc::new(Mutex::new(Room::new())))
            .clone()
    }

    async fn remove_room_if_empty(&self, name: &str) {
        let rooms = self.rooms.read().await;
        let room = match rooms.get(name) {
            Some(r) => r.clone(),
            None => return,
        };
        drop(rooms);

        let is_empty = room.lock().await.client_ids().is_empty();
        if is_empty {
            self.rooms.write().await.remove(name);
            println!("[room] deleted empty room \"{}\"", name);
        }
    }
}

// ── Mock helpers ──

fn spawn_mock(room: &mut Room, real_id: u32, real_packet: &PositionPacket) {
    let mock_packet = real_packet.with_offset(MOCK_ID, MOCK_OFFSET);

    let (tx, _rx) = mpsc::unbounded_channel::<Vec<u8>>();

    let mock = Client {
        id: MOCK_ID,
        name: MOCK_NAME.to_string(),
        mission_id: real_packet.mission_id,
        last_packet: mock_packet.to_bytes(),
        tcp_tx: tx,
    };
    room.add_client(mock);
    println!(
        "[mock] spawned for real_id={} in room",
        real_id
    );
}

fn remove_mock(room: &mut Room) {
    if room.client_ids().contains(&MOCK_ID) {
        room.remove_client(MOCK_ID);
        println!("[mock] removed");
    }
}

fn update_mock_packet(room: &mut Room, real_packet: &PositionPacket) -> Option<[u8; PACKET_SIZE]> {
    if !room.client_ids().contains(&MOCK_ID) {
        return None;
    }
    let mock_packet = real_packet.with_offset(MOCK_ID, MOCK_OFFSET);
    let bytes = mock_packet.to_bytes();
    // Обновляем last_packet у mock-клиента
    if let Some(mc) = room.clients.get_mut(&MOCK_ID) {
        mc.last_packet = bytes;
    }
    Some(bytes)
}

// ── TCP handler ──

async fn handle_tcp(
    stream: TcpStream,
    addr: SocketAddr,
    system: Arc<System>,
    _udp: Arc<UdpSocket>,
) {
    let id = Uuid::new_v4().as_fields().0; // u32
    let (tcp_tx, mut tcp_rx) = mpsc::unbounded_channel::<Vec<u8>>();

    // Разделяем stream на read/write половины
    let (read_half, write_half) = stream.into_split();
    let writer = Arc::new(tokio::sync::Mutex::new(write_half));

    // Фоновая задача: пишем в TCP из канала
    let writer_clone = writer.clone();
    tokio::spawn(async move {
        use tokio::io::AsyncWriteExt;
        while let Some(data) = tcp_rx.recv().await {
            let _ = writer_clone.lock().await.write_all(&data).await;
        }
    });

    // Читаем JSON-сообщения (разделитель \0)
    use tokio::io::{AsyncBufReadExt, BufReader};
    let mut reader = BufReader::new(read_half);
    let mut buf = Vec::new();

    let mut room_name: Option<String> = None;
    let mut client_name = String::new();

    loop {
        buf.clear();
        match reader.read_until(0, &mut buf).await {
            Ok(0) => break, // EOF
            Ok(_) => {
                if buf.last() == Some(&0) {
                    buf.pop(); // убрать \0
                }
                let msg: TcpIn = match serde_json::from_slice(&buf) {
                    Ok(m) => m,
                    Err(e) => {
                        println!("[tcp] bad json from {}: {}", addr, e);
                        continue;
                    }
                };

                match msg {
                    TcpIn::Connect {
                        room,
                        name,
                        mission_id: mid,
                    } => {
                        client_name = name;
                        let current_mission_id = mid;
                        room_name = Some(room.clone());

                        let room_arc = system.get_or_create_room(&room).await;
                        let mut room_lock = room_arc.lock().await;

                        let client = Client {
                            id,
                            name: client_name.clone(),
                            mission_id: current_mission_id,
                            last_packet: [0u8; PACKET_SIZE],
                            tcp_tx: tcp_tx.clone(),
                        };

                        // Сообщаем новому клиенту его ID
                        let id_msg = serde_json::json!({
                            "type": "id",
                            "id": id
                        });
                        let _ = tcp_tx.send(make_msg(&id_msg));

                        // Оповещаем остальных о новом игроке
                        let connect_msg = serde_json::json!({
                            "type": "player_connected",
                            "id": id,
                            "name": client_name,
                            "mission_id": current_mission_id
                        });
                        let connect_data = make_msg(&connect_msg);
                        room_lock.send_tcp(id, &connect_data);

                        // Сообщаем новому о существующих игроках
                        for (existing_id, existing) in room_lock.clients.iter() {
                            if *existing_id != id {
                                let existing_msg = serde_json::json!({
                                    "type": "player_connected",
                                    "id": existing_id,
                                    "name": existing.name,
                                    "mission_id": existing.mission_id
                                });
                                let _ = tcp_tx.send(make_msg(&existing_msg));
                            }
                        }

                        // Добавляем mock если это первый реальный игрок
                        if system.enable_mock {
                            let real_count = room_lock
                                .client_ids()
                                .iter()
                                .filter(|&&cid| cid != MOCK_ID)
                                .count();
                            if real_count == 0 {
                                let dummy_packet = PositionPacket {
                                    id,
                                    pos_x: 0.0,
                                    pos_y: 0.0,
                                    pos_z: 0.0,
                                    yaw: 0.0,
                                    hp: 100,
                                    mission_id: current_mission_id,
                                };
                                spawn_mock(&mut room_lock, id, &dummy_packet);

                                let mock_msg = serde_json::json!({
                                    "type": "player_connected",
                                    "id": MOCK_ID,
                                    "name": MOCK_NAME,
                                    "mission_id": current_mission_id
                                });
                                let _ = tcp_tx.send(make_msg(&mock_msg));
                            }
                        }

                        room_lock.add_client(client);

                        println!(
                            "[tcp] {} (id={}) joined room \"{}\"",
                            client_name, id, room
                        );
                    }

                    TcpIn::Disconnect => break,

                    TcpIn::Ping => {
                        let pong = serde_json::json!({"type": "pong"});
                        let _ = tcp_tx.send(make_msg(&pong));
                    }
                }
            }
            Err(e) => {
                println!("[tcp] read error from {}: {}", addr, e);
                break;
            }
        }
    }

    // ── Disconnect cleanup ──
    if let Some(ref room) = room_name {
        let room_arc = system.get_or_create_room(room).await;
        let mut room_lock = room_arc.lock().await;

        room_lock.remove_client(id);

        // Удаляем mock если это был последний реальный игрок
        if system.enable_mock {
            let real_count = room_lock
                .client_ids()
                .iter()
                .filter(|&&cid| cid != MOCK_ID)
                .count();
            if real_count == 0 {
                remove_mock(&mut room_lock);
            }
        }

        // Оповещаем остальных
        let disconnect_msg = serde_json::json!({
            "type": "player_disconnected",
            "id": id
        });
        let data = make_msg(&disconnect_msg);
        room_lock.send_tcp_all(&data);

        println!("[tcp] {} (id={}) disconnected", client_name, id);
    }

    // Удаляем комнату если пуста
    if let Some(ref room) = room_name {
        system.remove_room_if_empty(room).await;
    }
}

// ── UDP listener ──

async fn handle_udp(udp: Arc<UdpSocket>, system: Arc<System>) {
    let mut buf = [0u8; PACKET_SIZE];
    loop {
        let (n, addr) = match udp.recv_from(&mut buf).await {
            Ok(v) => v,
            Err(e) => {
                println!("[udp] recv error: {}", e);
                continue;
            }
        };

        if n != PACKET_SIZE {
            continue;
        }

        let packet = PositionPacket::from_bytes(&buf);
        let packet_id = { packet.id };

        // Ищем клиента по ID во всех комнатах
        let rooms = system.rooms.read().await;
        for room_arc in rooms.values() {
            let mut room_lock = room_arc.lock().await;
            if room_lock.client_ids().contains(&packet_id) {
                // Сохраняем last_packet
                room_lock.set_last_packet(packet_id, &buf);

                // Обновляем mock если есть
                let mock_packet = if system.enable_mock {
                    update_mock_packet(&mut room_lock, &packet)
                } else {
                    None
                };

                // Ретранслируем всем остальным (включая mock)
                room_lock
                    .send_last_packets(&udp, addr, packet_id, mock_packet.as_ref())
                    .await;
                break;
            }
        }
    }
}

// ── Ping loop ──

async fn ping_loop(system: Arc<System>) {
    let mut tick = interval(Duration::from_secs(2));
    loop {
        tick.tick().await;
        let ping = make_msg(&serde_json::json!({"type": "ping"}));
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
    let enable_mock = std::env::args().any(|a| a == "--mock");
    if enable_mock {
        println!("[server] mock player enabled");
    }

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
    let udp_tcp = udp_socket.clone();
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
            tokio::spawn(handle_tcp(
                stream,
                addr,
                system_tcp.clone(),
                udp_tcp.clone(),
            ));
        }
    });

    let system_udp = system.clone();
    let udp_udp = udp_socket.clone();
    tokio::spawn(handle_udp(udp_udp, system_udp));

    ping_loop(system).await;
}
