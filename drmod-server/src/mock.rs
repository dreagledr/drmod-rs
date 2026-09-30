use tokio::sync::mpsc;

use crate::{
    MOCK_ID, MOCK_NAME, MOCK_OFFSET,
    system::{Client, Room},
};
use drmod_protocol::{PositionPacket, SkeletonPacket};

pub fn spawn_mock(room: &mut Room, real_id: u32, real_packet: &PositionPacket) {
    let mock_position = real_packet.with_offset(MOCK_ID, MOCK_OFFSET).to_wire();

    let (tx, _rx) = mpsc::unbounded_channel::<Vec<u8>>();

    let mock = Client {
        id: MOCK_ID,
        name: MOCK_NAME.to_string(),
        mission_id: real_packet.mission_id,
        last_position: mock_position,
        last_skeleton: None,
        tcp_tx: tx,
    };
    room.add_client(mock);
    println!("[mock] spawned for real_id={} in room", real_id);
}

pub fn remove_mock(room: &mut Room) {
    if room.client_ids().contains(&MOCK_ID) {
        room.remove_client(MOCK_ID);
        println!("[mock] removed");
    }
}

pub fn update_mock_position(room: &mut Room, real_packet: &PositionPacket) -> Option<[u8; 29]> {
    if !room.client_ids().contains(&MOCK_ID) {
        return None;
    }
    let mock_wire = real_packet.with_offset(MOCK_ID, MOCK_OFFSET).to_wire();
    if let Some(mc) = room.clients.get_mut(&MOCK_ID) {
        mc.last_position = mock_wire;
    }
    Some(mock_wire)
}

pub fn update_mock_skeleton(room: &mut Room, real_skel_data: &[u8]) -> Option<Vec<u8>> {
    if !room.client_ids().contains(&MOCK_ID) {
        return None;
    }
    let skel = SkeletonPacket::from_wire(real_skel_data)?;
    let mut bones = skel.bones;
    for bone in &mut bones {
        bone.offset(MOCK_OFFSET.0, MOCK_OFFSET.1, MOCK_OFFSET.2);
    }
    let mock_wire = SkeletonPacket {
        id: MOCK_ID,
        bones,
    }
    .to_wire();
    if let Some(mc) = room.clients.get_mut(&MOCK_ID) {
        mc.last_skeleton = Some(mock_wire.clone());
    }
    Some(mock_wire)
}
