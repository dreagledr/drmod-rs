use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::mock::{remove_mock, spawn_mock};
use crate::system::{Client, System};
use crate::{MOCK_ID, MOCK_NAME};
use drmod_protocol::{make_msg, PositionPacket, TcpMessage};

pub(crate) async fn handle_tcp(stream: TcpStream, addr: SocketAddr, system: Arc<System>) {
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
                let msg: TcpMessage = match serde_json::from_slice(&buf) {
                    Ok(m) => m,
                    Err(e) => {
                        println!("[tcp] bad json from {}: {}", addr, e);
                        continue;
                    }
                };

                match msg {
                    TcpMessage::Connect {
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
                            last_position: [0u8; 29],
                            last_skeleton: None,
                            tcp_tx: tcp_tx.clone(),
                        };

                        // Сообщаем новому клиенту его ID
                        let _ = tcp_tx.send(make_msg(&TcpMessage::IdAssigned { id }));

                        // Оповещаем остальных о новом игроке
                        let connect_data = make_msg(&TcpMessage::PlayerConnected {
                            id,
                            name: client_name.clone(),
                            mission_id: current_mission_id,
                        });
                        room_lock.send_tcp(id, &connect_data);

                        // Сообщаем новому о существующих игроках
                        for (existing_id, existing) in room_lock.clients.iter() {
                            if *existing_id != id {
                                let existing_data = make_msg(&TcpMessage::PlayerConnected {
                                    id: *existing_id,
                                    name: existing.name.clone(),
                                    mission_id: existing.mission_id,
                                });
                                let _ = tcp_tx.send(existing_data);
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

                                let mock_data = make_msg(&TcpMessage::PlayerConnected {
                                    id: MOCK_ID,
                                    name: MOCK_NAME.to_string(),
                                    mission_id: current_mission_id,
                                });
                                let _ = tcp_tx.send(mock_data);
                            }
                        }

                        room_lock.add_client(client);

                        println!("[tcp] {} (id={}) joined room \"{}\"", client_name, id, room);
                    }

                    TcpMessage::Disconnect => break,

                    TcpMessage::Ping => {
                        let pong = make_msg(&TcpMessage::Pong);
                        let _ = tcp_tx.send(make_msg(&pong));
                    }
                    _ => {}
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
        let data = make_msg(&TcpMessage::PlayerDisconnected { id });
        room_lock.send_tcp_all(&data);

        println!("[tcp] {} (id={}) disconnected", client_name, id);
    }

    // Удаляем комнату если пуста
    if let Some(ref room) = room_name {
        system.remove_room_if_empty(room).await;
    }
}
