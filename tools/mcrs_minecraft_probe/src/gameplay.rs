use crate::swing::EntityNames;
use crate::{Observe, PacketKind};
use anyhow::Context;
use mcrs_minecraft_item_key::item_keys::Item;
use mcrs_minecraft_protocol::{Decode, Packet};
use mcrs_minecraft_protocol_game::Direction;
use mcrs_minecraft_protocol_game::clientbound::{
    ClientboundDamageEvent, ClientboundEntityEvent, ClientboundGameEvent, ClientboundSetEntityData,
    ClientboundSetEquipment, ClientboundSetHealth,
};
use mcrs_minecraft_protocol_game::entity::{EquipmentSlot, MetaDataValue};
use mcrs_minecraft_protocol_game::game_event::{DemoMessage, EnterCredits, GameEventKind};
use mcrs_minecraft_protocol_game::item::data_component_type_keys::DataComponentType;
use mcrs_minecraft_protocol_game::item::{ItemComponentValue, ProtoStack};
use mcrs_minecraft_protocol_game::serverbound::PlayerCommandAction;
use mcrs_minecraft_registries::Wire;
use serde::{Deserialize, Serialize};
use std::fmt::Debug;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProbePlayerCommand {
    StopSleeping,
    StartSprinting,
    StopSprinting,
    StartRidingJump,
    StopRidingJump,
    OpenInventory,
    StartFallFlying,
}

impl From<ProbePlayerCommand> for PlayerCommandAction {
    fn from(command: ProbePlayerCommand) -> Self {
        match command {
            ProbePlayerCommand::StopSleeping => Self::StopSleeping,
            ProbePlayerCommand::StartSprinting => Self::StartSprinting,
            ProbePlayerCommand::StopSprinting => Self::StopSprinting,
            ProbePlayerCommand::StartRidingJump => Self::StartRidingJump,
            ProbePlayerCommand::StopRidingJump => Self::StopRidingJump,
            ProbePlayerCommand::OpenInventory => Self::OpenInventory,
            ProbePlayerCommand::StartFallFlying => Self::StartFallFlying,
        }
    }
}

pub(crate) enum GameplaySignal {
    Equipment(EquipmentObserved),
    EntityData(EntityDataObserved),
    Health(HealthObserved),
    DamageEvent(DamageEventObserved),
    EntityEvent(EntityEventObserved),
    GameEvent(GameEventObserved),
}

impl GameplaySignal {
    pub(crate) fn kind(&self) -> PacketKind {
        match self {
            Self::Equipment(_) => PacketKind::SetEquipment,
            Self::EntityData(_) => PacketKind::SetEntityData,
            Self::Health(_) => PacketKind::SetHealth,
            Self::DamageEvent(_) => PacketKind::DamageEvent,
            Self::EntityEvent(_) => PacketKind::EntityEvent,
            Self::GameEvent(_) => PacketKind::GameEvent,
        }
    }

    pub(crate) fn packet_kind(id: i32) -> Option<PacketKind> {
        match id {
            ClientboundSetEquipment::ID => Some(PacketKind::SetEquipment),
            ClientboundSetEntityData::ID => Some(PacketKind::SetEntityData),
            ClientboundSetHealth::ID => Some(PacketKind::SetHealth),
            ClientboundDamageEvent::ID => Some(PacketKind::DamageEvent),
            ClientboundEntityEvent::ID => Some(PacketKind::EntityEvent),
            ClientboundGameEvent::ID => Some(PacketKind::GameEvent),
            _ => None,
        }
    }

    pub(crate) fn read(
        observe: &Observe,
        client: &str,
        names: &EntityNames,
        id: i32,
        body: &[u8],
    ) -> anyhow::Result<Option<Self>> {
        Ok(Some(match id {
            ClientboundSetEquipment::ID if observe.equipment => {
                Self::Equipment(EquipmentObserved::read(client, names, body)?)
            }
            ClientboundSetEntityData::ID if observe.entity_data => {
                Self::EntityData(EntityDataObserved::read(client, names, body)?)
            }
            ClientboundSetHealth::ID if observe.health => {
                Self::Health(HealthObserved::read(client, body)?)
            }
            ClientboundDamageEvent::ID if observe.damage_event => {
                Self::DamageEvent(DamageEventObserved::read(client, names, body)?)
            }
            ClientboundEntityEvent::ID if observe.entity_event => {
                Self::EntityEvent(EntityEventObserved::read(client, names, body)?)
            }
            ClientboundGameEvent::ID if observe.game_event => {
                Self::GameEvent(GameEventObserved::read(client, body)?)
            }
            _ => return Ok(None),
        }))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EquipmentObserved {
    pub client: String,
    pub subject: String,
    pub slots: Vec<EquipmentSlotEntry>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EquipmentSlotEntry {
    pub slot: EquipmentSlot,
    pub stack: Option<EquipmentStack>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EquipmentStack {
    pub item: Item,
    pub count: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub damage: Option<i32>,
}

impl EquipmentStack {
    fn from_wire(stack: &ProtoStack<Wire>) -> anyhow::Result<Option<Self>> {
        if stack.is_empty() {
            return Ok(None);
        }
        let number = stack.id.number();
        let item = Item::from_protocol_id(number)
            .with_context(|| format!("the item {number} is not in the item registry"))?;
        let damage = match stack.components.get_value(DataComponentType::Damage) {
            Some(ItemComponentValue::Damage(damage)) => Some(damage.0.0),
            _ => None,
        };
        Ok(Some(Self {
            item,
            count: stack.count,
            damage,
        }))
    }
}

fn decode_exact<'a, P: Decode<'a>>(mut body: &'a [u8], name: &str) -> anyhow::Result<P> {
    let packet = P::decode(&mut body)?;
    anyhow::ensure!(body.is_empty(), "the {name} packet has trailing bytes");
    Ok(packet)
}

impl EquipmentObserved {
    pub(crate) fn read(client: &str, names: &EntityNames, body: &[u8]) -> anyhow::Result<Self> {
        let packet: ClientboundSetEquipment = decode_exact(body, "set_equipment")?;
        let mut slots = Vec::with_capacity(packet.slots.len());
        for (slot, stack) in &packet.slots {
            slots.push(EquipmentSlotEntry {
                slot: *slot,
                stack: EquipmentStack::from_wire(stack)?,
            });
        }
        Ok(Self {
            client: client.to_owned(),
            subject: names.resolve_subject(packet.entity_id.0),
            slots,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EntityDataObserved {
    pub client: String,
    pub subject: String,
    pub entries: Vec<EntityDataEntry>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EntityDataEntry {
    pub index: u8,
    #[serde(flatten)]
    pub value: EntityDataValue,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum EntityDataValue {
    Byte(i8),
    VarInt(i32),
    VarLong(i64),
    Float(f32),
    String(String),
    Text(mcrs_minecraft_protocol_game::text::Text),
    OptionalText(Option<mcrs_minecraft_protocol_game::text::Text>),
    Slot(Option<EquipmentStack>),
    Boolean(bool),
    Rotations([f32; 3]),
    BlockPos([i32; 3]),
    Direction(Direction),
    BlockState(i32),
    Pose(String),
    Other(String),
}

impl EntityDataValue {
    fn from_wire(value: MetaDataValue<'_>) -> anyhow::Result<Self> {
        Ok(match value {
            MetaDataValue::Byte(byte_val) => Self::Byte(byte_val),
            MetaDataValue::VarInt(var_int) => Self::VarInt(var_int.0),
            MetaDataValue::VarLong(var_long) => Self::VarLong(var_long.0),
            MetaDataValue::Float(float_val) => Self::Float(float_val),
            MetaDataValue::String(text) => Self::String(text.to_owned()),
            MetaDataValue::Text(text) => Self::Text(text),
            MetaDataValue::OptionalText(text) => Self::OptionalText(text),
            MetaDataValue::Slot(stack) => Self::Slot(EquipmentStack::from_wire(&stack)?),
            MetaDataValue::Boolean(flag) => Self::Boolean(flag),
            MetaDataValue::Rotations(rotations) => {
                Self::Rotations([rotations.x, rotations.y, rotations.z])
            }
            MetaDataValue::BlockPos(pos) => Self::BlockPos([pos.x, pos.y, pos.z]),
            MetaDataValue::Direction(direction) => Self::Direction(direction),
            MetaDataValue::BlockState(state) => Self::BlockState(i32::from(state.0)),
            MetaDataValue::Pose(pose) => Self::Pose(snake_debug(&pose)),
            other => Self::Other(format!("{other:?}")),
        })
    }
}

fn snake_debug(value: &impl Debug) -> String {
    let mut out = String::new();
    for (index, ch) in format!("{value:?}").chars().enumerate() {
        if ch.is_uppercase() && index > 0 {
            out.push('_');
        }
        out.extend(ch.to_lowercase());
    }
    out
}

impl EntityDataObserved {
    pub(crate) fn read(client: &str, names: &EntityNames, body: &[u8]) -> anyhow::Result<Self> {
        let packet: ClientboundSetEntityData<'_> = decode_exact(body, "set_entity_data")?;
        let mut entries = Vec::with_capacity(packet.metadata.0.len());
        for entry in packet.metadata.0 {
            entries.push(EntityDataEntry {
                index: entry.index,
                value: EntityDataValue::from_wire(entry.value)?,
            });
        }
        Ok(Self {
            client: client.to_owned(),
            subject: names.resolve_subject(packet.entity_id.0),
            entries,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HealthObserved {
    pub client: String,
    pub health: f32,
    pub food: i32,
    pub saturation: f32,
}

impl HealthObserved {
    pub(crate) fn read(client: &str, body: &[u8]) -> anyhow::Result<Self> {
        let packet: ClientboundSetHealth = decode_exact(body, "set_health")?;
        Ok(Self {
            client: client.to_owned(),
            health: packet.health,
            food: packet.food.0,
            saturation: packet.saturation,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DamageEventObserved {
    pub client: String,
    pub subject: String,
    pub damage_type: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cause: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub direct: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_position: Option<[f64; 3]>,
}

impl DamageEventObserved {
    pub(crate) fn read(client: &str, names: &EntityNames, body: &[u8]) -> anyhow::Result<Self> {
        let packet: ClientboundDamageEvent = decode_exact(body, "damage_event")?;
        let resolve = |shifted: i32| (shifted > 0).then(|| names.resolve_subject(shifted - 1));
        Ok(Self {
            client: client.to_owned(),
            subject: names.resolve_subject(packet.entity_id.0),
            damage_type: packet.damage_type.0,
            cause: resolve(packet.cause_entity_id.0),
            direct: resolve(packet.direct_entity_id.0),
            source_position: packet.source_position.map(|pos| [pos.x, pos.y, pos.z]),
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntityEventObserved {
    pub client: String,
    pub subject: String,
    pub status: i8,
}

impl EntityEventObserved {
    pub(crate) fn read(client: &str, names: &EntityNames, body: &[u8]) -> anyhow::Result<Self> {
        let packet: ClientboundEntityEvent = decode_exact(body, "entity_event")?;
        Ok(Self {
            client: client.to_owned(),
            subject: names.resolve_subject(packet.entity_id),
            status: packet.entity_status,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GameEventObserved {
    pub client: String,
    pub event: String,
    pub param: f32,
}

impl GameEventObserved {
    pub(crate) fn read(client: &str, body: &[u8]) -> anyhow::Result<Self> {
        let packet: ClientboundGameEvent = decode_exact(body, "game_event")?;
        let (event, param) = match packet.game_event {
            GameEventKind::NoRespawnBlockAvailable => ("no_respawn_block_available", 0.0),
            GameEventKind::EndRaining => ("end_raining", 0.0),
            GameEventKind::BeginRaining => ("begin_raining", 0.0),
            GameEventKind::ChangeGameMode(mode) => ("change_game_mode", f32::from(mode as u8)),
            GameEventKind::WinGame(EnterCredits::FirstTime) => ("win_game", 1.0),
            GameEventKind::WinGame(EnterCredits::SeenBefore) => ("win_game", 0.0),
            GameEventKind::DemoEvent(message) => (
                "demo_event",
                match message {
                    DemoMessage::Welcome => 0.0,
                    DemoMessage::MovementControls => 101.0,
                    DemoMessage::JumpControl => 102.0,
                    DemoMessage::InventoryControl => 103.0,
                    DemoMessage::ScreenshotControl => 104.0,
                },
            ),
            GameEventKind::PlayArrowHitSound => ("play_arrow_hit_sound", 0.0),
            GameEventKind::RainLevelChange(strength) => ("rain_level_change", strength),
            GameEventKind::ThunderLevelChange(strength) => ("thunder_level_change", strength),
            GameEventKind::PufferfishSting => ("pufferfish_sting", 0.0),
            GameEventKind::GuardianElderEffect => ("guardian_elder_effect", 0.0),
            GameEventKind::ImmediateRespawn(enabled) => ("immediate_respawn", f32::from(enabled)),
            GameEventKind::LimitedCrafting(enabled) => ("limited_crafting", f32::from(enabled)),
            GameEventKind::LevelChunksLoadStart => ("level_chunks_load_start", 0.0),
        };
        Ok(Self {
            client: client.to_owned(),
            event: event.to_owned(),
            param,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mcrs_minecraft_core::codec::Bounded;
    use mcrs_minecraft_protocol::{Encode, VarInt, VarLong};
    use mcrs_minecraft_protocol_game::Position;
    use mcrs_minecraft_protocol_game::entity::{Metadata, MetadataEntry};
    use mcrs_minecraft_protocol_game::entity_data_index as idx;
    use mcrs_minecraft_protocol_game::item::{ComponentPatch, Damage, ItemComponentValue};
    use mcrs_minecraft_registries::WireId;

    fn encode_packet<P: Encode>(packet: &P) -> Vec<u8> {
        let mut bytes = Vec::new();
        packet.encode(&mut bytes).unwrap();
        bytes
    }

    #[test]
    fn wire_decode_equipment_and_entity_data() {
        let mut names = EntityNames::default();
        names.record_login("Alice", 5);
        names.record_login("Bob", 9);

        let mut patch = ComponentPatch::<Wire>::EMPTY;
        patch.set_value(ItemComponentValue::Damage(Damage(Bounded(12))));
        let sword = ProtoStack {
            id: WireId::new(Item::IronSword.id().number()),
            count: 1,
            components: patch,
        };
        let eq_bytes = encode_packet(&ClientboundSetEquipment {
            entity_id: VarInt(5),
            slots: vec![
                (EquipmentSlot::MainHand, sword),
                (EquipmentSlot::OffHand, ProtoStack::default()),
            ],
        });
        let equip = EquipmentObserved::read("Bob", &names, &eq_bytes).unwrap();
        let main_hand = equip.slots[0].stack.as_ref().unwrap();
        let sword_ok = equip.subject == "Alice" && main_hand.damage == Some(12);
        assert!(
            sword_ok && equip.slots[1].stack.is_none(),
            "equipment slots"
        );

        let entry = |index, value| MetadataEntry { index, value };
        let entries = vec![
            entry(idx::SHARED_FLAGS, MetaDataValue::Byte(8)),
            entry(1, MetaDataValue::VarInt(VarInt(300))),
            entry(idx::TICKS_FROZEN, MetaDataValue::VarInt(VarInt(140))),
            entry(idx::LIVING_FLAGS, MetaDataValue::Byte(1)),
            entry(idx::HEALTH, MetaDataValue::Float(18.5)),
            entry(12, MetaDataValue::VarLong(VarLong(42))),
            entry(13, MetaDataValue::Boolean(true)),
        ];
        let mut raw = encode_packet(&ClientboundSetEntityData {
            entity_id: VarInt(5),
            metadata: Metadata(entries),
        });
        raw.pop();
        raw.extend_from_slice(&[idx::POSE, 20, 5, 14, 9]);
        raw.extend_from_slice(&[1.0f32, 2.0, 3.0].map(f32::to_be_bytes).concat());
        raw.push(0xFF);
        let meta = EntityDataObserved::read("Alice", &names, &raw).unwrap();
        let pose_ok = meta.entries[7].value == EntityDataValue::Pose("crouching".into());
        let rot_ok = meta.entries[8].value == EntityDataValue::Rotations([1.0, 2.0, 3.0]);
        let entries_ok = meta.subject == "Alice" && meta.entries.len() == 9 && pose_ok && rot_ok;
        assert!(entries_ok, "entity data entries");
        let json = serde_json::to_string(&meta).unwrap();
        let round_trip: EntityDataObserved = serde_json::from_str(&json).unwrap();
        let byte_ok = json.contains(r#""index":0,"kind":"byte","value":8"#);
        let health_ok = json.contains(r#""index":9,"kind":"float","value":18.5"#);
        let json_ok = round_trip == meta && byte_ok && health_ok;
        assert!(json_ok, "metadata json: {json}");
    }

    #[test]
    fn wire_decode_health_damage_entity_and_game_events() {
        let mut names = EntityNames::default();
        names.record_login("Alice", 5);
        names.record_login("Bob", 9);

        let health_wire = ClientboundSetHealth {
            health: 15.0,
            food: VarInt(18),
            saturation: 4.5,
        };
        let health = HealthObserved::read("Alice", &encode_packet(&health_wire)).unwrap();
        let health_ok = health.health == 15.0 && health.food == 18 && health.saturation == 4.5;
        assert!(health_ok, "health fields");

        let damage_wire = ClientboundDamageEvent {
            entity_id: VarInt(5),
            damage_type: VarInt(3),
            cause_entity_id: VarInt(10),
            direct_entity_id: VarInt(0),
            source_position: Some(Position::new(1.5, 64.0, -2.5).into()),
        };
        let damage =
            DamageEventObserved::read("Alice", &names, &encode_packet(&damage_wire)).unwrap();
        let damage_ok = damage.cause.as_deref() == Some("Bob")
            && damage.direct.is_none()
            && damage.source_position == Some([1.5, 64.0, -2.5]);
        assert!(damage_ok, "damage event fields");

        let entity_event_wire = ClientboundEntityEvent {
            entity_id: 5,
            entity_status: 3,
        };
        let entity_event =
            EntityEventObserved::read("Bob", &names, &encode_packet(&entity_event_wire)).unwrap();
        let entity_ok = entity_event.subject == "Alice" && entity_event.status == 3;
        assert!(entity_ok, "entity event fields");

        let game_event_wire = ClientboundGameEvent {
            game_event: GameEventKind::RainLevelChange(0.75),
        };
        let game_event =
            GameEventObserved::read("Alice", &encode_packet(&game_event_wire)).unwrap();
        let game_ok = game_event.event == "rain_level_change" && game_event.param == 0.75;
        assert!(game_ok, "game event fields");
    }

    #[test]
    fn wire_decode_entity_data_particles_followed_by_entry() {
        let mut names = EntityNames::default();
        names.record_login("Alice", 5);

        let mut raw = vec![5];
        raw.extend_from_slice(&[
            8, 16, 0x15, 0xFF, 0xFF, 0x00, 0x00, 0x3F, 0x80, 0x00, 0x00, 9, 17, 2, 0x00, 0x1C,
            0x80, 0xFF, 0x80, 0x00, 0, 0, 0x2A, 0xFF,
        ]);

        let meta = EntityDataObserved::read("Alice", &names, &raw).unwrap();
        assert_eq!(meta.entries.len(), 3);
        assert_eq!(meta.entries[0].index, 8);
        assert!(
            matches!(meta.entries[0].value, EntityDataValue::Other(_)),
            "particle entry should decode as EntityDataValue::Other"
        );
        assert_eq!(meta.entries[1].index, 9);
        assert!(
            matches!(meta.entries[1].value, EntityDataValue::Other(_)),
            "particles entry should decode as EntityDataValue::Other"
        );
        assert_eq!(meta.entries[2].index, 0);
        assert_eq!(meta.entries[2].value, EntityDataValue::Byte(0x2A));
    }
}
