use std::sync::Arc;
use tokio::net::UdpSocket;

use crate::mock::{update_mock_position, update_mock_skeleton};
use crate::system::System;
use crate::MAX_UDP;
use drmod_protocol::{PKT_POSITION, PKT_SKELETON, PositionPacket};

pub(crate) async fn handle_udp(udp: Arc<UdpSocket>, system: Arc<System>) {
    let mut buf = vec![0u8; MAX_UDP];
    loop {
        let (n, addr) = match udp.recv_from(&mut buf).await {
            Ok(v) => v,
            Err(e) => {
                println!("[udp] recv error: {}", e);
                continue;
            }
        };

        if n == 0 {
            continue;
        }

        let data = &buf[..n];
        let packet_type = data[0];

        match packet_type {
            PKT_POSITION => {
                let packet = match PositionPacket::from_wire(data) {
                    Some(p) => p,
                    None => continue,
                };
                let packet_id = packet.id;

                let rooms = system.rooms.read().await;
                for room_arc in rooms.values() {
                    let mut room_lock = room_arc.lock().await;
                    if room_lock.client_ids().contains(&packet_id) {
                        // Сохраняем last_position как wire (29 байт)
                        let wire = packet.to_wire();
                        room_lock.set_last_position(packet_id, &wire);

                        let mock_position = if system.enable_mock {
                            update_mock_position(&mut room_lock, &packet)
                        } else {
                            None
                        };

                        room_lock
                            .send_last_positions(&udp, addr, packet_id, mock_position.as_ref())
                            .await;
                        break;
                    }
                }
            }
            PKT_SKELETON => {
                // Читаем id из wire (байты 1-4)
                if data.len() < 5 {
                    continue;
                }
                let packet_id = u32::from_le_bytes([data[1], data[2], data[3], data[4]]);

                let rooms = system.rooms.read().await;
                for room_arc in rooms.values() {
                    let mut room_lock = room_arc.lock().await;
                    if room_lock.client_ids().contains(&packet_id) {
                        // Сохраняем сырой wire
                        room_lock.set_last_skeleton(packet_id, data.to_vec());

                        let mock_skeleton = if system.enable_mock {
                            update_mock_skeleton(&mut room_lock, data)
                        } else {
                            None
                        };

                        room_lock
                            .send_last_skeletons(&udp, addr, packet_id, mock_skeleton.as_ref())
                            .await;
                        break;
                    }
                }
            }
            _ => {} // неизвестный тип — игнорируем
        }
    }
}
