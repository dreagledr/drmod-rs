use serde::{Deserialize, Serialize};

// ── UDP packet type discriminators ────────────────────────────────

pub const PKT_POSITION: u8 = 0x00;
pub const PKT_SKELETON: u8 = 0x01;

// ── Position packet (28 байт тело) ───────────────────────────────

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
    /// Wire-формат: [PKT_POSITION] [28 байт тела] = 29 байт.
    pub fn to_wire(&self) -> [u8; 29] {
        let mut buf = [0u8; 29];
        buf[0] = PKT_POSITION;
        buf[1..29].copy_from_slice(&self.to_bytes());
        buf
    }

    /// Парсинг из wire-формата (29 байт, первый байт уже проверен).
    pub fn from_wire(data: &[u8]) -> Option<Self> {
        if data.len() < 29 || data[0] != PKT_POSITION {
            return None;
        }
        let mut arr = [0u8; 28];
        arr.copy_from_slice(&data[1..29]);
        Some(Self::from_bytes(&arr))
    }

    pub fn to_bytes(&self) -> [u8; 28] {
        unsafe { std::mem::transmute(*self) }
    }

    pub fn from_bytes(bytes: &[u8; 28]) -> Self {
        unsafe { std::mem::transmute(*bytes) }
    }

    /// Копирует пакет с заменой id и смещением позиции.
    pub fn with_offset(&self, id: u32, offset: (f32, f32, f32)) -> Self {
        Self {
            id,
            pos_x: self.pos_x + offset.0,
            pos_y: self.pos_y + offset.1,
            pos_z: self.pos_z + offset.2,
            ..*self
        }
    }
}

// ── Skeleton packet ──────────────────────────────────────────────

/// Одна кость в wire-формате (16 байт, little-endian).
///
///   0: bone_index (u16)
///   2: parent_index (i16) — -1 = нет родителя
///   4: x (f32)
///   8: y (f32)
///  12: z (f32)
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct SkeletonBone {
    pub index: u16,
    pub parent_index: i16,
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl SkeletonBone {
    pub fn offset(&mut self, dx: f32, dy: f32, dz: f32) {
        self.x += dx;
        self.y += dy;
        self.z += dz;
    }
}

/// Скелет-пакет для UDP (переменной длины).
///
/// Wire-формат:
///   0: PKT_SKELETON (u8)
///   1: id (u32, le)
///   5: bone_count (u16, le)
///   7: bones [bone_count × 16 байт]
pub struct SkeletonPacket {
    pub id: u32,
    pub bones: Vec<SkeletonBone>,
}

impl SkeletonPacket {
    pub fn to_wire(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(7 + self.bones.len() * 16);
        buf.push(PKT_SKELETON);
        buf.extend_from_slice(&self.id.to_le_bytes());
        buf.extend_from_slice(&(self.bones.len() as u16).to_le_bytes());
        for bone in &self.bones {
            buf.extend_from_slice(&bone.index.to_le_bytes());
            buf.extend_from_slice(&bone.parent_index.to_le_bytes());
            buf.extend_from_slice(&bone.x.to_le_bytes());
            buf.extend_from_slice(&bone.y.to_le_bytes());
            buf.extend_from_slice(&bone.z.to_le_bytes());
        }
        buf
    }

    pub fn from_wire(data: &[u8]) -> Option<Self> {
        if data.len() < 7 || data[0] != PKT_SKELETON {
            return None;
        }
        let id = u32::from_le_bytes([data[1], data[2], data[3], data[4]]);
        let count = u16::from_le_bytes([data[5], data[6]]) as usize;
        let expected_len = 7 + count * 16;
        if data.len() < expected_len {
            return None;
        }
        let mut bones = Vec::with_capacity(count);
        for i in 0..count {
            let off = 7 + i * 16;
            let index = u16::from_le_bytes([data[off], data[off + 1]]);
            let parent_index = i16::from_le_bytes([data[off + 2], data[off + 3]]);
            let x = f32::from_le_bytes([
                data[off + 4],
                data[off + 5],
                data[off + 6],
                data[off + 7],
            ]);
            let y = f32::from_le_bytes([
                data[off + 8],
                data[off + 9],
                data[off + 10],
                data[off + 11],
            ]);
            let z = f32::from_le_bytes([
                data[off + 12],
                data[off + 13],
                data[off + 14],
                data[off + 15],
            ]);
            bones.push(SkeletonBone {
                index,
                parent_index,
                x,
                y,
                z,
            });
        }
        Some(Self { id, bones })
    }
}

// ── Unified UDP parse ────────────────────────────────────────────

pub enum UdpPacket {
    Position(PositionPacket),
    Skeleton(SkeletonPacket),
}

/// Парсит UDP-датаграмму: читает первый байт-дискриминатор, возвращает вариант.
pub fn parse_udp(data: &[u8]) -> Option<UdpPacket> {
    match *data.first()? {
        PKT_POSITION => PositionPacket::from_wire(data).map(UdpPacket::Position),
        PKT_SKELETON => SkeletonPacket::from_wire(data).map(UdpPacket::Skeleton),
        _ => None,
    }
}

// ── TCP messages ─────────────────────────────────────────────────

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

/// Сериализует значение в JSON-вектор, добавляет `\0`-разделитель.
pub fn make_msg<T: Serialize>(value: &T) -> Vec<u8> {
    let mut json = serde_json::to_vec(value).unwrap_or_default();
    json.push(0);
    json
}
