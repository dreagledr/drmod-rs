use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::UdpSocket;
use tokio::sync::RwLock;
use tokio::sync::{Mutex, mpsc};

use super::MOCK_ID;

pub(crate) struct Client {
    pub(crate) id: u32,
    pub(crate) name: String,
    pub(crate) mission_id: i32,
    pub(crate) last_position: [u8; 29],
    pub(crate) last_skeleton: Option<Vec<u8>>,
    pub(crate) tcp_tx: mpsc::UnboundedSender<Vec<u8>>,
}

pub(crate) struct Room {
    pub(crate) clients: HashMap<u32, Client>,
}

impl Room {
    pub(crate) fn new() -> Self {
        Self {
            clients: HashMap::new(),
        }
    }

    pub(crate) fn add_client(&mut self, client: Client) {
        self.clients.insert(client.id, client);
    }

    pub(crate) fn remove_client(&mut self, id: u32) {
        self.clients.remove(&id);
    }

    pub(crate) fn client_ids(&self) -> Vec<u32> {
        self.clients.keys().copied().collect()
    }

    pub(crate) fn send_tcp(&self, exclude_id: u32, data: &[u8]) {
        for (id, c) in &self.clients {
            if *id != exclude_id {
                let _ = c.tcp_tx.send(data.to_vec());
            }
        }
    }

    pub(crate) fn send_tcp_all(&self, data: &[u8]) {
        for c in self.clients.values() {
            let _ = c.tcp_tx.send(data.to_vec());
        }
    }

    pub(crate) fn set_last_position(&mut self, id: u32, wire: &[u8; 29]) {
        if let Some(c) = self.clients.get_mut(&id) {
            c.last_position = *wire;
        }
    }

    pub(crate) fn set_last_skeleton(&mut self, id: u32, wire: Vec<u8>) {
        if let Some(c) = self.clients.get_mut(&id) {
            c.last_skeleton = Some(wire);
        }
    }

    pub(crate) async fn send_last_positions(
        &self,
        udp: &UdpSocket,
        addr: SocketAddr,
        exclude_id: u32,
        mock_position: Option<&[u8; 29]>,
    ) {
        for (id, c) in &self.clients {
            if *id != exclude_id && *id != MOCK_ID {
                let _ = udp.send_to(&c.last_position, addr).await;
            }
        }
        if let Some(pkt) = mock_position {
            let _ = udp.send_to(pkt, addr).await;
        }
    }

    pub(crate) async fn send_last_skeletons(
        &self,
        udp: &UdpSocket,
        addr: SocketAddr,
        exclude_id: u32,
        mock_skeleton: Option<&Vec<u8>>,
    ) {
        for (id, c) in &self.clients {
            if *id != exclude_id && *id != MOCK_ID {
                if let Some(ref skel) = c.last_skeleton {
                    let _ = udp.send_to(skel, addr).await;
                }
            }
        }
        if let Some(pkt) = mock_skeleton {
            let _ = udp.send_to(pkt, addr).await;
        }
    }
}

pub(crate) struct System {
    pub(crate) rooms: RwLock<HashMap<String, Arc<Mutex<Room>>>>,
    pub(crate) enable_mock: bool,
}

impl System {
    pub(crate) fn new(enable_mock: bool) -> Self {
        Self {
            rooms: RwLock::new(HashMap::new()),
            enable_mock,
        }
    }

    pub(crate) async fn get_or_create_room(&self, name: &str) -> Arc<Mutex<Room>> {
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

    pub(crate) async fn remove_room_if_empty(&self, name: &str) {
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
