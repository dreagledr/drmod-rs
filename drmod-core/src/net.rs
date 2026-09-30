use crate::segment::Vec3;
use drmod_protocol::{make_msg, parse_udp, PositionPacket, SkeletonBone, SkeletonPacket, TcpMessage, UdpPacket};
use std::io::{Read, Write};
use std::net::{Shutdown, TcpStream, ToSocketAddrs, UdpSocket};
use std::sync::mpsc;
use std::time::Instant;

/// Данные об удалённом игроке.
#[derive(Debug, Clone)]
pub struct RemotePlayer {
    pub id: u32,
    pub name: String,
    pub pos: Vec3,
    pub hp: i32,
    pub mission_id: i32,
    pub last_update: Instant,
    pub is_mock: bool, // отмечен ли как mock-игрок (id = 0xFFFFFFFE)
    pub skeleton: Vec<SkeletonBone>,
}

/// События из TCP-потока в главный поток.
pub enum TcpEvent {
    IdAssigned(u32),
    PlayerConnected {
        id: u32,
        name: String,
        mission_id: i32,
    },
    PlayerDisconnected(u32),
    Pong,
}

/// Сетевой клиент. Владеет:
/// - TCP-соединением (блокирующим, в отдельном потоке)
/// - UDP-сокетом (неблокирующим, опрос в кадре)
pub struct NetClient {
    tcp: Option<TcpStream>,
    tcp_handle: Option<std::thread::JoinHandle<()>>,
    tcp_rx: mpsc::Receiver<TcpEvent>,
    udp: UdpSocket,
    server_addr: String,
    pub my_id: u32,
    pub remote_players: Vec<RemotePlayer>,
}

impl NetClient {
    /// Подключается к серверу, регистрируется в комнате.
    /// `name` — имя игрока, `room` — имя комнаты.
    pub fn new(server_addr: &str, name: &str, room: &str) -> Result<Self, String> {
        let addr = server_addr
            .to_socket_addrs()
            .map_err(|e| format!("bad address: {}", e))?
            .next()
            .ok_or("no address resolved")?;

        // TCP connect (блокирующий)
        let mut tcp = TcpStream::connect(addr).map_err(|e| format!("tcp connect: {}", e))?;

        // Отправляем connect
        let payload = make_msg(&TcpMessage::Connect {
            room: room.to_string(),
            name: name.to_string(),
            mission_id: 0,
        });
        tcp.write_all(&payload)
            .map_err(|e| format!("tcp write: {}", e))?;

        // UDP bind (неблокирующий)
        let udp = UdpSocket::bind("0.0.0.0:0").map_err(|e| format!("udp bind: {}", e))?;
        udp.set_nonblocking(true)
            .map_err(|e| format!("udp nonblocking: {}", e))?;

        let (tx, rx) = mpsc::channel();

        // Клонируем TCP для потока, оригинал оставляем в NetClient для shutdown в Drop
        let mut tcp_clone = tcp
            .try_clone()
            .map_err(|e| format!("tcp clone: {}", e))?;

        // Запускаем TCP-поток чтения
        let tcp_handle = {
            std::thread::Builder::new()
                .name("drmod-tcp".into())
                .spawn(move || {
                    let mut byte = [0u8; 1];
                    let mut buf = Vec::new();
                    while let Ok(()) = tcp_clone.read_exact(&mut byte) {
                        if byte[0] == 0 {
                            if let Ok(msg) = serde_json::from_slice::<TcpMessage>(&buf) {
                                let event = match msg {
                                    TcpMessage::IdAssigned { id } => {
                                        Some(TcpEvent::IdAssigned(id))
                                    }
                                    TcpMessage::PlayerConnected {
                                        id,
                                        name,
                                        mission_id,
                                    } => Some(TcpEvent::PlayerConnected {
                                        id,
                                        name,
                                        mission_id,
                                    }),
                                    TcpMessage::PlayerDisconnected { id } => {
                                        Some(TcpEvent::PlayerDisconnected(id))
                                    }
                                    TcpMessage::Pong => Some(TcpEvent::Pong),
                                    _ => None,
                                };
                                if let Some(event) = event
                                    && tx.send(event).is_err() {
                                        break;
                                    }
                            }
                            buf.clear();
                        } else {
                            buf.push(byte[0]);
                        }
                    }
                })
                .map_err(|e| format!("thread spawn: {}", e))?
        };

        Ok(Self {
            tcp: Some(tcp),
            tcp_handle: Some(tcp_handle),
            tcp_rx: rx,
            udp,
            server_addr: server_addr.to_string(),
            my_id: 0,
            remote_players: Vec::new(),
        })
    }

    /// Обработать накопившиеся TCP-события. Вызывать в кадре.
    pub fn poll_tcp(&mut self) {
        while let Ok(event) = self.tcp_rx.try_recv() {
            match event {
                TcpEvent::IdAssigned(id) => {
                    self.my_id = id;
                }
                TcpEvent::PlayerConnected {
                    id,
                    name,
                    mission_id,
                } => {
                    // Не добавляем дубликат
                    if !self.remote_players.iter().any(|p| p.id == id) {
                        self.remote_players.push(RemotePlayer {
                            id,
                            name,
                            pos: Vec3 {
                                x: 0.0,
                                y: 0.0,
                                z: 0.0,
                            },
                            hp: 0,
                            mission_id,
                            last_update: Instant::now(),
                            is_mock: id == 0xFFFFFFFE,
                            skeleton: Vec::new(),
                        });
                    }
                }
                TcpEvent::PlayerDisconnected(id) => {
                    self.remote_players.retain(|p| p.id != id);
                }
                TcpEvent::Pong => {
                    // no-op, keep-alive подтверждение
                }
            }
        }
    }

    /// Отправить свою позицию через UDP (29 байт wire). Вызывать в кадре.
    pub fn send_position(&self, pos: Vec3, hp: i32, mission_id: i32) {
        if self.my_id == 0 {
            return;
        }
        let packet = PositionPacket {
            id: self.my_id,
            pos_x: pos.x,
            pos_y: pos.y,
            pos_z: pos.z,
            yaw: 0.0,
            hp,
            mission_id,
        };
        let bytes = packet.to_wire();
        let addr = match self.server_addr.to_socket_addrs() {
            Ok(mut a) => a.next(),
            Err(_) => None,
        };
        if let Some(addr) = addr {
            let _ = self.udp.send_to(&bytes, addr);
        }
    }

    /// Отправить скелет через UDP. Вызывать в кадре (раз в ~100 мс).
    pub fn send_skeleton(&self, bones: &[SkeletonBone]) {
        if self.my_id == 0 {
            return;
        }
        let packet = SkeletonPacket {
            id: self.my_id,
            bones: bones.to_vec(),
        };
        let bytes = packet.to_wire();
        let addr = match self.server_addr.to_socket_addrs() {
            Ok(mut a) => a.next(),
            Err(_) => None,
        };
        if let Some(addr) = addr {
            let _ = self.udp.send_to(&bytes, addr);
        }
    }

    /// Принять UDP-пакеты (позиции и скелеты). Вызывать в кадре.
    pub fn recv_udp(&mut self) {
        let mut buf = [0u8; 8192];
        loop {
            match self.udp.recv_from(&mut buf) {
                Ok((n, _)) => {
                    if n == 0 {
                        continue;
                    }
                    match parse_udp(&buf[..n]) {
                        Some(UdpPacket::Position(packet)) => {
                            if packet.id == self.my_id {
                                continue;
                            }
                            if let Some(rp) = self
                                .remote_players
                                .iter_mut()
                                .find(|p| p.id == packet.id)
                            {
                                rp.pos = Vec3 {
                                    x: packet.pos_x,
                                    y: packet.pos_y,
                                    z: packet.pos_z,
                                };
                                rp.hp = packet.hp;
                                rp.mission_id = packet.mission_id;
                                rp.last_update = Instant::now();
                            } else {
                                let pid = packet.id;
                                self.remote_players.push(RemotePlayer {
                                    id: pid,
                                    name: format!("Player {}", pid),
                                    pos: Vec3 {
                                        x: packet.pos_x,
                                        y: packet.pos_y,
                                        z: packet.pos_z,
                                    },
                                    hp: packet.hp,
                                    mission_id: packet.mission_id,
                                    last_update: Instant::now(),
                                    is_mock: packet.id == 0xFFFFFFFE,
                                    skeleton: Vec::new(),
                                });
                            }
                        }
                        Some(UdpPacket::Skeleton(skel)) => {
                            if skel.id == self.my_id {
                                continue;
                            }
                            if let Some(rp) = self
                                .remote_players
                                .iter_mut()
                                .find(|p| p.id == skel.id)
                            {
                                rp.skeleton = skel.bones;
                                rp.last_update = Instant::now();
                            }
                            // Не создаём игрока только по скелету — ждём позицию
                        }
                        None => {} // неизвестный/битый пакет
                    }
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(_) => break,
            }
        }
    }
}

impl Drop for NetClient {
    fn drop(&mut self) {
        // 1. Вырубаем TCP-сокет — это разбудит поток, заблокированный в read_exact()
        if let Some(ref tcp) = self.tcp {
            let _ = tcp.shutdown(Shutdown::Both);
        }
        // 2. Ждём завершения TCP-потока (shutdown гарантирует, что read_exact вернёт ошибку)
        if let Some(handle) = self.tcp_handle.take() {
            let _ = handle.join();
        }
    }
}
