use crate::segment::Vec3;
use serde::{Deserialize, Serialize};

/// Бинарный UDP-пакет с позицией игрока (28 байт, little-endian).
///
/// Раскладка offset/размер:
///   0: id (u32)
///   4: pos_x (f32)
///   8: pos_y (f32)
///  12: pos_z (f32)
///  16: yaw (f32) — резерв, пока 0
///  20: hp (i32)
///  24: mission_id (i32)
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct PositionPacket {
    pub id: u32,
    pub pos_x: f32,
    pub pos_y: f32,
    pub pos_z: f32,
    pub yaw: f32,
    pub hp: i32,
    pub mission_id: i32,
}

impl PositionPacket {
    pub fn to_vec3(&self) -> Vec3 {
        Vec3 {
            x: self.pos_x,
            y: self.pos_y,
            z: self.pos_z,
        }
    }

    pub fn from_vec3(id: u32, pos: Vec3, hp: i32, mission_id: i32) -> Self {
        Self {
            id,
            pos_x: pos.x,
            pos_y: pos.y,
            pos_z: pos.z,
            yaw: 0.0,
            hp,
            mission_id,
        }
    }

    pub fn to_bytes(&self) -> [u8; 28] {
        unsafe { std::mem::transmute(*self) }
    }

    pub fn from_bytes(bytes: &[u8; 28]) -> Self {
        unsafe { std::mem::transmute(*bytes) }
    }
}

/// TCP-сообщения (JSON, разделитель \0).
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum TcpMessage {
    // Client → Server
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

    // Server → Client
    #[serde(rename = "id")]
    IdAssigned { id: u32 },
    #[serde(rename = "player_connected")]
    PlayerConnected {
        id: u32,
        name: String,
        mission_id: i32,
    },
    #[serde(rename = "player_disconnected")]
    PlayerDisconnected { id: u32 },
    #[serde(rename = "pong")]
    Pong,
}
