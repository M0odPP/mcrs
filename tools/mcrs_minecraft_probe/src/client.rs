use crate::gameplay::GameplaySignal;
use crate::menu_view::MenuView;
use crate::pose::Pose;
use crate::swing::{CapturedSwing, EntityNames};
use crate::{
    AnimateObserved, Click, CommandTree, ConnectionState, MenuPacket, Observe, PacketKind,
    PredictedStack, SoundPlayed, SwingObserved,
};
use anyhow::Context;
use mcrs_minecraft_container_click::Clicked;
use mcrs_minecraft_core::{Look, VERSION};
use mcrs_minecraft_network::SwitchMode;
use mcrs_minecraft_network::join::Configuration;
use mcrs_minecraft_network::packet_io::PacketIo;
use mcrs_minecraft_protocol::handshake::Intent;
use mcrs_minecraft_protocol::packets::common::serverbound::{ClientInformation, KeepAlive};
use mcrs_minecraft_protocol::packets::configuration::clientbound::ClientboundDisconnect as ConfigurationDisconnect;
use mcrs_minecraft_protocol::packets::configuration::serverbound::{
    ServerboundClientInformation as ConfigClientInformation, ServerboundFinishConfiguration,
};
use mcrs_minecraft_protocol::packets::game::clientbound::{
    ClientboundDisconnect, ClientboundKeepAlive,
};
use mcrs_minecraft_protocol::packets::game::serverbound::ServerboundKeepAlive;
use mcrs_minecraft_protocol::packets::intent::serverbound::ServerboundHandshake;
use mcrs_minecraft_protocol::packets::login::clientbound::{
    ClientboundLoginDisconnect, ClientboundLoginFinished, ClientboundLoginFinishedWithOptions,
    LoginCompression, OptionFlags,
};
use mcrs_minecraft_protocol::packets::login::serverbound::{
    ServerboundHello, ServerboundLoginAcknowledged,
};
use mcrs_minecraft_protocol::setting::{ChatMode, DisplayedSkinParts, MainArm, ParticleStatus};
use mcrs_minecraft_protocol::uuid::Uuid;
use mcrs_minecraft_protocol::{
    Bounded, CompressionThreshold, Decode, Encode, Packet, VarInt, WritePacket,
};
use mcrs_minecraft_protocol_game::clientbound::{
    ClientboundAddEntity, ClientboundBlockChangedAck, ClientboundBlockUpdate,
    ClientboundChunkBatchFinished, ClientboundCommandSuggestions, ClientboundCommands,
    ClientboundEntityPositionSync, ClientboundLogin, ClientboundMoveEntityPos,
    ClientboundMoveEntityPosRot, ClientboundPlayerPosition, ClientboundRemoveEntities,
    ClientboundRespawn, ClientboundSectionBlocksUpdate, ClientboundSystemChatPacket,
    ClientboundTeleportEntity,
};
use mcrs_minecraft_protocol_game::player::PlayerAction;
use mcrs_minecraft_protocol_game::serverbound::{
    PlayerCommandAction, PlayerInputFlags, ServerboundAttack, ServerboundChatCommand,
    ServerboundChunkBatchReceived, ServerboundClientInformation as GameClientInformation,
    ServerboundClientTickEnd, ServerboundCommandSuggestion, ServerboundContainerClick,
    ServerboundContainerClose, ServerboundInteract, ServerboundMovePlayerPos,
    ServerboundMovePlayerPosRot, ServerboundPlayerAction, ServerboundPlayerCommand,
    ServerboundPlayerInput, ServerboundPlayerLoaded, ServerboundPunch, ServerboundSetCarriedItem,
    ServerboundSetCreativeModeSlot, ServerboundUseItem, ServerboundUseItemOn,
};
use mcrs_minecraft_protocol_game::{Direction, Hand, LpVec3, MoveFlags, Position};
use mcrs_minecraft_text::Text;
use mcrs_voxel::BlockPos;
use std::collections::VecDeque;
use std::io::{self, Write};
use std::net::SocketAddr;
use std::ops::ControlFlow;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio::sync::oneshot;
use tokio::time;

const CLOSE_LIMIT: Duration = Duration::from_secs(5);
pub(crate) const WRITE_LIMIT: Duration = Duration::from_secs(5);
const CHUNKS_PER_TICK: f32 = 64.0;

pub(crate) struct Joined {
    pub(crate) io: Box<PacketIo<TcpStream>>,
    pub(crate) player_id: i32,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum LoadSignal {
    Send,
    Defer,
}

pub(crate) enum Login {
    Joined(Joined),
    Refused {
        state: ConnectionState,
        reason: Option<Text>,
    },
}

pub(crate) struct QueuedOrder {
    pub(crate) order: Order,
    pub(crate) deadline: time::Instant,
    pub(crate) written: Option<oneshot::Sender<Result<(), String>>>,
}

pub(crate) enum Order {
    Command(String),
    Suggest {
        id: i32,
        line: String,
    },
    ClientMainArm(MainArm),
    PunchExtraByte,
    Leave,
    SendLoaded,
    SyncPosition,
    Click(Click),
    Close(Option<i32>),
    SelectSlot(i16),
    PlayerAction {
        action: PlayerAction,
        position: [i32; 3],
        face: Direction,
    },
    CreativeSlot {
        slot: i16,
        stack: Option<PredictedStack>,
    },
    UseBlock([i32; 3]),
    Punch,
    PunchBurst(u32),
    UseItem {
        hand: Hand,
        sequence: i32,
    },
    UseItemOn {
        hand: Hand,
        position: [i32; 3],
        face: Direction,
        sequence: i32,
    },
    Attack(String),
    Interact {
        target: String,
        hand: Hand,
        secondary: bool,
    },
    MovePlayer([f64; 3], bool, Option<[f32; 2]>),
    PlayerCommand(PlayerCommandAction, i32),
    PlayerInput(PlayerInputFlags),
}

pub(crate) struct Event {
    pub(crate) client: String,
    pub(crate) signal: Signal,
    pub(crate) at: time::Instant,
}

impl Event {
    pub(crate) fn now(client: String, signal: Signal) -> Self {
        Self {
            client,
            signal,
            at: time::Instant::now(),
        }
    }
}

pub(crate) enum Signal {
    Chat(Text),
    Menu(MenuPacket),
    Sound(SoundPlayed),
    Commands(CommandTree),
    Suggestions {
        id: i32,
        start: i32,
        length: i32,
        suggestions: Vec<String>,
    },
    Swing(CapturedSwing),
    BlockAck(i32),
    Added(String),
    Removed(Vec<String>),
    PositionAccepted(Position),
    Animate(AnimateObserved),
    Gameplay(GameplaySignal),
    Packet(PacketKind),
    Disconnected(Text),
    Closed,
    Failed(String),
}

impl Signal {
    fn is_terminal(&self) -> bool {
        matches!(self, Self::Disconnected(_) | Self::Closed | Self::Failed(_))
    }
}

pub(crate) async fn log_in(address: SocketAddr, name: &str) -> anyhow::Result<Login> {
    let mut io = PacketIo::connect(address).await?;
    let switch_mode = SwitchMode::of_own_version();
    io.send_packet(&ServerboundHandshake {
        protocol_version: VarInt(VERSION.protocol_version),
        server_address: Bounded(address.ip().to_string().as_str()),
        server_port: address.port(),
        intent: Intent::Login,
    })
    .await?;
    io.send_packet(&ServerboundHello {
        username: Bounded(name),
        profile_id: Uuid::nil(),
    })
    .await?;

    let mut state = ConnectionState::Login;
    let mut stored_threshold = CompressionThreshold(0);
    let mut configuration = Configuration::default();
    loop {
        let (id, body) = match io.recv_frame().await {
            Ok(frame) => frame,
            Err(error) if is_close(&error) => {
                return Ok(Login::Refused {
                    state,
                    reason: None,
                });
            }
            Err(error) => return Err(error),
        };
        let reason = match state {
            ConnectionState::Login if id == ClientboundLoginDisconnect::ID => {
                let packet: ClientboundLoginDisconnect = decode(&body)?;
                Some(serde_json::from_str(packet.reason.0)?)
            }
            ConnectionState::Login if id == LoginCompression::ID => {
                let packet: LoginCompression = decode(&body)?;
                let threshold = CompressionThreshold(packet.threshold.0);
                match switch_mode {
                    SwitchMode::Immediate => io.set_compression(threshold),
                    SwitchMode::Acknowledged => stored_threshold = threshold,
                }
                None
            }
            ConnectionState::Login if id == ClientboundLoginFinished::ID => {
                acknowledge_login(&mut io, switch_mode, stored_threshold, &body).await?;
                state = ConnectionState::Configuration;
                None
            }
            ConnectionState::Login => anyhow::bail!("unexpected login packet {id:#04x}"),
            ConnectionState::Configuration if id == ConfigurationDisconnect::ID => {
                let packet: ConfigurationDisconnect = decode(&body)?;
                Some(packet.reason)
            }
            ConnectionState::Configuration => {
                if configuration.read(id, &body, true, &mut io)?.is_some() {
                    io.write_packet_fallible(&ServerboundFinishConfiguration)?;
                    state = ConnectionState::Play;
                }
                io.flush().await?;
                None
            }
            ConnectionState::Play if id == ClientboundDisconnect::ID => {
                let packet: ClientboundDisconnect = decode(&body)?;
                Some(packet.reason)
            }
            ConnectionState::Play if id == ClientboundLogin::ID => {
                let packet: ClientboundLogin = decode(&body)?;
                return Ok(Login::Joined(Joined {
                    io: Box::new(io),
                    player_id: packet.player_id,
                }));
            }
            ConnectionState::Play => None,
        };
        if reason.is_some() {
            return Ok(Login::Refused { state, reason });
        }
    }
}

async fn acknowledge_login(
    io: &mut PacketIo<TcpStream>,
    switch_mode: SwitchMode,
    stored_threshold: CompressionThreshold,
    body: &[u8],
) -> anyhow::Result<()> {
    let options = match switch_mode {
        SwitchMode::Immediate => OptionFlags::new(),
        SwitchMode::Acknowledged => {
            let packet: ClientboundLoginFinishedWithOptions = decode(body)?;
            packet.options
        }
    };
    anyhow::ensure!(!options.cipher(), "the server asked for encryption");
    let compression = options.compression().then_some(stored_threshold);
    if let Some(threshold) = compression {
        io.set_inbound_compression(threshold);
    }
    io.send_packet(&ServerboundLoginAcknowledged).await?;
    if let Some(threshold) = compression {
        io.set_outbound_compression(threshold);
    }
    io.send_packet(&ConfigClientInformation(client_information(MainArm::Right)))
        .await?;
    Ok(())
}

pub(crate) fn needs_world_acks(observe: &Observe) -> bool {
    observe.inventory
        || observe.swing
        || observe.animate
        || observe.equipment
        || observe.entity_data
        || observe.health
        || observe.damage_event
        || observe.entity_event
        || observe.game_event
        || observe.packet_order
}

fn client_information(main_arm: MainArm) -> ClientInformation<'static> {
    ClientInformation {
        locale: "en_us",
        view_distance: 2,
        chat_mode: ChatMode::Enabled,
        chat_colors: true,
        displayed_skin_parts: DisplayedSkinParts::from_bits(0x7f),
        main_arm,
        enable_text_filtering: false,
        allow_server_listings: true,
        particle_status: ParticleStatus::All,
    }
}

pub(crate) struct SessionConfig {
    pub(crate) observe: Observe,
    pub(crate) tracks_pose: bool,
    pub(crate) known_players: Vec<String>,
    pub(crate) load_signal: LoadSignal,
}

pub(crate) async fn session(
    joined: Joined,
    client: String,
    config: SessionConfig,
    mut orders: UnboundedReceiver<QueuedOrder>,
    events: UnboundedSender<Event>,
) {
    let mut io = joined.io;
    if config.load_signal == LoadSignal::Send
        && needs_world_acks(&config.observe)
        && let Err(error) = automatic_reply(&mut io, &ServerboundPlayerLoaded).await
    {
        let signal = failure(&client, &error);
        let _ = events.send(Event::now(client, signal));
        return;
    }
    let mut state = SessionState {
        client: &client,
        player_id: joined.player_id,
        observe: &config.observe,
        tracks_pose: config.tracks_pose,
        view: MenuView::default(),
        pose: Pose::default(),
        names: EntityNames::with_players(&config.known_players),
        awaiting_respawn_pos: false,
    };
    state.names.record_login(&client, joined.player_id);
    let mut pending_orders: VecDeque<QueuedOrder> = VecDeque::new();
    let signal = loop {
        tokio::select! {
            frame = io.recv_frame() => {
                let (id, body) = match frame {
                    Ok(frame) => frame,
                    Err(error) if is_close(&error) => break Signal::Closed,
                    Err(error) => break failure(&client, &error),
                };
                match read_game_packet(&mut io, &mut state, id, &body).await {
                    Ok(Some(signal)) if !signal.is_terminal() => {
                        if events.send(Event::now(client.clone(), signal)).is_err() {
                            return;
                        }
                    }
                    Ok(Some(signal)) => break signal,
                    Ok(None) => {}
                    Err(error) => break failure(&client, &error),
                }
                if !state.awaiting_respawn_pos
                    && let ControlFlow::Break(signal) =
                        drain_pending(&mut io, &mut state, &client, &mut pending_orders).await
                {
                    break signal;
                }
            }
            queued = orders.recv() => {
                let Some(queued) = queued else {
                    let _ = time::timeout(CLOSE_LIMIT, io.close()).await;
                    break Signal::Closed;
                };
                if matches!(queued.order, Order::Leave) {
                    let _ = time::timeout(CLOSE_LIMIT, io.close()).await;
                    if let Some(done) = queued.written {
                        let _ = done.send(Ok(()));
                    }
                    break Signal::Closed;
                }
                if state.awaiting_respawn_pos
                    && (!pending_orders.is_empty()
                        || matches!(
                            queued.order,
                            Order::Attack(_)
                                | Order::Interact { .. }
                                | Order::MovePlayer(..)
                                | Order::SyncPosition
                                | Order::UseItem { .. }
                        ))
                {
                    pending_orders.push_back(queued);
                    continue;
                }
                if let ControlFlow::Break(signal) =
                    dispatch_queued(&mut io, &mut state, &client, queued).await
                {
                    break signal;
                }
            }
        }
    };
    let _ = events.send(Event::now(client, signal));
}

async fn drain_pending(
    io: &mut PacketIo<TcpStream>,
    state: &mut SessionState<'_>,
    client: &str,
    pending: &mut VecDeque<QueuedOrder>,
) -> ControlFlow<Signal> {
    while let Some(queued) = pending.pop_front() {
        dispatch_queued(io, state, client, queued).await?;
    }
    ControlFlow::Continue(())
}

async fn dispatch_queued(
    io: &mut PacketIo<TcpStream>,
    state: &mut SessionState<'_>,
    client: &str,
    queued: QueuedOrder,
) -> ControlFlow<Signal> {
    if queued
        .written
        .as_ref()
        .is_some_and(oneshot::Sender::is_closed)
    {
        return ControlFlow::Continue(());
    }
    let Some(budget) = queued
        .deadline
        .checked_duration_since(time::Instant::now())
        .filter(|remaining| !remaining.is_zero())
    else {
        if let Some(done) = queued.written {
            let _ = done.send(Err(format!("{client} order deadline expired before write")));
        }
        return ControlFlow::Continue(());
    };
    let result = match time::timeout(budget, send_order(io, state, queued.order)).await {
        Ok(res) => res,
        Err(_) => Err(anyhow::anyhow!(
            "{client} packet write timed out after {budget:?}"
        )),
    };
    match result {
        Ok(()) => {
            if let Some(done) = queued.written {
                let _ = done.send(Ok(()));
            }
            ControlFlow::Continue(())
        }
        Err(error) => {
            let message = format!("{error:#}");
            if let Some(done) = queued.written {
                let _ = done.send(Err(message));
            }
            ControlFlow::Break(failure(client, &error))
        }
    }
}

fn failure(client: &str, error: &anyhow::Error) -> Signal {
    eprintln!("{client}: {error:#}");
    Signal::Failed(format!("{error:#}"))
}

struct SessionState<'a> {
    client: &'a str,
    player_id: i32,
    observe: &'a Observe,
    tracks_pose: bool,
    view: MenuView,
    pose: Pose,
    names: EntityNames,
    awaiting_respawn_pos: bool,
}

#[derive(Debug)]
struct PunchWithExtraByte;

impl Packet for PunchWithExtraByte {
    const ID: i32 = ServerboundPunch::ID;
    const NAME: &'static str = ServerboundPunch::NAME;
    const SIDE: mcrs_minecraft_protocol::PacketSide = ServerboundPunch::SIDE;
    const STATE: mcrs_minecraft_protocol::ConnectionState = ServerboundPunch::STATE;
}

impl Encode for PunchWithExtraByte {
    fn encode(&self, mut out: impl Write) -> mcrs_minecraft_protocol::Result<()> {
        out.write_all(&[0x01])?;
        Ok(())
    }
}

async fn send_order(
    io: &mut PacketIo<TcpStream>,
    state: &mut SessionState<'_>,
    order: Order,
) -> anyhow::Result<()> {
    match order {
        Order::Command(line) => {
            io.send_packet(&ServerboundChatCommand {
                command: Bounded(line.as_str()),
            })
            .await
        }
        Order::Suggest { id, line } => {
            io.send_packet(&ServerboundCommandSuggestion {
                id: VarInt(id),
                text: Bounded(line.as_str()),
            })
            .await
        }
        Order::ClientMainArm(arm) => {
            io.send_packet(&GameClientInformation(client_information(arm)))
                .await
        }
        Order::PunchExtraByte => io.send_packet(&PunchWithExtraByte).await,
        Order::Leave => Ok(()),
        Order::SendLoaded => io.send_packet(&ServerboundPlayerLoaded).await,
        Order::SyncPosition => send_sync_position(io, state).await,
        Order::Click(click) => send_click(io, &state.view, click).await,
        Order::Close(container) => {
            let container_id = VarInt(container.unwrap_or(state.view.container()));
            state.view.close();
            io.send_packet(&ServerboundContainerClose { container_id })
                .await
        }
        Order::SelectSlot(slot) => {
            let slot = slot.cast_unsigned();
            io.send_packet(&ServerboundSetCarriedItem { slot }).await
        }
        Order::PlayerAction {
            action,
            position: [x, y, z],
            face,
        } => {
            io.send_packet(&ServerboundPlayerAction {
                action,
                pos: BlockPos::new(x, y, z),
                direction: face,
                sequence: VarInt(0),
            })
            .await
        }
        Order::CreativeSlot { slot, stack } => {
            io.send_packet(&ServerboundSetCreativeModeSlot {
                slot,
                item: PredictedStack::with_length(stack)?,
            })
            .await
        }
        Order::UseBlock(position) => {
            send_use_item_on(io, Hand::Main, position, Direction::Up, 0).await
        }
        Order::Punch => io.send_packet(&ServerboundPunch).await,
        Order::PunchBurst(count) => {
            for _ in 0..count {
                io.send_packet(&ServerboundPunch).await?;
            }
            Ok(())
        }
        Order::UseItem { hand, sequence } => {
            io.send_packet(&ServerboundUseItem {
                hand,
                sequence: VarInt(sequence),
                look: state.pose.look(),
            })
            .await
        }
        Order::UseItemOn {
            hand,
            position,
            face,
            sequence,
        } => send_use_item_on(io, hand, position, face, sequence).await,
        Order::Attack(target) => {
            let entity_id = resolve_target_id(state, &target)?;
            io.send_packet(&ServerboundAttack { entity_id }).await
        }
        Order::Interact {
            target,
            hand,
            secondary,
        } => {
            let entity_id = resolve_target_id(state, &target)?;
            io.send_packet(&ServerboundInteract {
                entity_id,
                hand,
                location: LpVec3::default(),
                using_secondary_action: secondary,
            })
            .await
        }
        other => send_gameplay_order(io, state, other).await,
    }
}

fn prepare_move_player(
    pose: &mut Pose,
    client: &str,
    position: [f64; 3],
    on_ground: bool,
    look: Option<[f32; 2]>,
) -> anyhow::Result<MoveFlags> {
    if !pose.is_accepted() {
        anyhow::bail!("{client} cannot move_player before accepting a player_position packet");
    }
    pose.move_to(position, on_ground, look);
    Ok(MoveFlags::new().with_on_ground(on_ground))
}

async fn send_move_and_tick_end<P: Packet + Encode>(
    io: &mut PacketIo<TcpStream>,
    packet: &P,
) -> anyhow::Result<()> {
    io.send_packet(packet).await?;
    io.send_packet(&ServerboundClientTickEnd).await
}

async fn send_gameplay_order(
    io: &mut PacketIo<TcpStream>,
    state: &mut SessionState<'_>,
    order: Order,
) -> anyhow::Result<()> {
    match order {
        Order::MovePlayer(pos, on_ground, look) => {
            let flags = prepare_move_player(&mut state.pose, state.client, pos, on_ground, look)?;
            let position = Position::new(pos[0], pos[1], pos[2]);
            match look {
                Some([yaw, pitch]) => {
                    let look = Look { yaw, pitch };
                    send_move_and_tick_end(
                        io,
                        &ServerboundMovePlayerPosRot {
                            position,
                            look,
                            flags,
                        },
                    )
                    .await
                }
                None => {
                    send_move_and_tick_end(io, &ServerboundMovePlayerPos { position, flags }).await
                }
            }
        }
        Order::PlayerCommand(action, data) => {
            let entity_id = VarInt(state.player_id);
            io.send_packet(&ServerboundPlayerCommand {
                entity_id,
                action,
                data: VarInt(data),
            })
            .await
        }
        Order::PlayerInput(input) => io.send_packet(&ServerboundPlayerInput { input }).await,
        _ => Ok(()),
    }
}

fn resolve_target_id(state: &SessionState<'_>, target: &str) -> anyhow::Result<VarInt> {
    state
        .names
        .target_id(target, state.pose.accepted_position())
}

async fn send_sync_position(
    io: &mut PacketIo<TcpStream>,
    state: &SessionState<'_>,
) -> anyhow::Result<()> {
    match state.pose.move_pos_rot() {
        Some(packet) => send_move_and_tick_end(io, &packet).await,
        None => anyhow::bail!(
            "{} cannot sync_position before accepting a player_position packet",
            state.client
        ),
    }
}

async fn send_click(
    io: &mut PacketIo<TcpStream>,
    view: &MenuView,
    click: Click,
) -> anyhow::Result<()> {
    let changed_slots = click
        .changed_slots
        .iter()
        .map(|changed| (changed.slot, changed.stack.map(PredictedStack::hashed)))
        .collect();
    let container = click.container.unwrap_or(view.container());
    let state_id = view
        .state_id(container)
        .checked_add(click.state_id_offset)
        .context("the state id offset overflows")?;
    let clicked = Clicked::from(click.slot);
    anyhow::ensure!(
        i16::try_from(clicked).ok() == Some(click.slot),
        "the slot {} has no click on the wire",
        click.slot
    );
    io.send_packet(&ServerboundContainerClick {
        container_id: VarInt(container),
        state_seqno: VarInt(state_id),
        click: mcrs_minecraft_container_click::Click {
            kind: click.mode.into(),
            button: click.button,
            clicked,
        },
        changed_slots: Bounded(changed_slots),
        carried_item: click.carried.map(PredictedStack::hashed),
    })
    .await
}

struct FaceHitPoint {
    x: f32,
    y: f32,
    z: f32,
}

fn face_hit_point(face: Direction) -> FaceHitPoint {
    match face {
        Direction::Down => FaceHitPoint {
            x: 0.5,
            y: 0.0,
            z: 0.5,
        },
        Direction::Up => FaceHitPoint {
            x: 0.5,
            y: 1.0,
            z: 0.5,
        },
        Direction::North => FaceHitPoint {
            x: 0.5,
            y: 0.5,
            z: 0.0,
        },
        Direction::South => FaceHitPoint {
            x: 0.5,
            y: 0.5,
            z: 1.0,
        },
        Direction::West => FaceHitPoint {
            x: 0.0,
            y: 0.5,
            z: 0.5,
        },
        Direction::East => FaceHitPoint {
            x: 1.0,
            y: 0.5,
            z: 0.5,
        },
    }
}

async fn send_use_item_on(
    io: &mut PacketIo<TcpStream>,
    hand: Hand,
    [x, y, z]: [i32; 3],
    face: Direction,
    sequence: i32,
) -> anyhow::Result<()> {
    let hit = face_hit_point(face);
    io.send_packet(&ServerboundUseItemOn {
        hand,
        block_pos: BlockPos::new(x, y, z),
        face,
        cursor_x: hit.x,
        cursor_y: hit.y,
        cursor_z: hit.z,
        inside_block: false,
        world_border_hit: false,
        sequence: VarInt(sequence),
    })
    .await
}

async fn read_game_packet(
    io: &mut PacketIo<TcpStream>,
    state: &mut SessionState<'_>,
    id: i32,
    body: &[u8],
) -> anyhow::Result<Option<Signal>> {
    if id == ClientboundKeepAlive::ID {
        let packet: ClientboundKeepAlive = decode(body)?;
        let payload = packet.0.payload;
        automatic_reply(io, &ServerboundKeepAlive(KeepAlive { payload })).await?;
    } else if id == ClientboundSystemChatPacket::ID {
        let packet: ClientboundSystemChatPacket = decode(body)?;
        if !packet.overlay {
            return Ok(Some(Signal::Chat(packet.content.to_text()?)));
        }
    } else if id == ClientboundDisconnect::ID {
        let packet: ClientboundDisconnect = decode(body)?;
        return Ok(Some(Signal::Disconnected(packet.reason)));
    } else if id == ClientboundCommandSuggestions::ID {
        let packet: ClientboundCommandSuggestions<'_> = decode(body)?;
        return Ok(Some(Signal::Suggestions {
            id: packet.id.0,
            start: packet.start.0,
            length: packet.length.0,
            suggestions: packet
                .suggestions
                .into_iter()
                .map(|entry| entry.text.into_owned())
                .collect(),
        }));
    } else if state.observe.sound && SoundPlayed::is_packet(id) {
        return Ok(Some(Signal::Sound(SoundPlayed::read(state.client, body)?)));
    } else if !state.observe.commands.is_empty() && id == ClientboundCommands::ID {
        let tree = CommandTree::read(state.client, body, &state.observe.commands)?;
        return Ok(Some(Signal::Commands(tree)));
    } else if state.observe.swing && SwingObserved::is_packet(id) {
        return Ok(Some(Signal::Swing(SwingObserved::read(
            state.client,
            &state.names,
            body,
        )?)));
    } else if state.observe.animate && AnimateObserved::is_packet(id) {
        return Ok(Some(Signal::Animate(AnimateObserved::read(
            state.client,
            &state.names,
            body,
        )?)));
    } else if let Some(signal) = read_observed(state, id, body)? {
        return Ok(Some(signal));
    } else if id == ClientboundAddEntity::ID {
        let packet: ClientboundAddEntity = decode(body)?;
        state.names.record_add_entity(&packet);
        return Ok(Some(Signal::Added(
            state.names.resolve_subject(packet.id.0),
        )));
    } else if id == ClientboundRemoveEntities::ID {
        let packet: ClientboundRemoveEntities = decode(body)?;
        return Ok(Some(Signal::Removed(state.names.remove_entities(&packet))));
    } else if id == ClientboundBlockChangedAck::ID {
        let packet: ClientboundBlockChangedAck = decode(body)?;
        return Ok(Some(Signal::BlockAck(packet.sequence.0)));
    } else if id == ClientboundTeleportEntity::ID {
        let packet: ClientboundTeleportEntity = decode(body)?;
        state.names.record_teleport(&packet);
    } else if id == ClientboundEntityPositionSync::ID {
        let packet: ClientboundEntityPositionSync = decode(body)?;
        state.names.record_position_sync(&packet);
    } else if id == ClientboundMoveEntityPos::ID {
        let packet: ClientboundMoveEntityPos = decode(body)?;
        state
            .names
            .record_move_delta(packet.entity_id.0, &packet.delta);
    } else if id == ClientboundMoveEntityPosRot::ID {
        let packet: ClientboundMoveEntityPosRot = decode(body)?;
        state
            .names
            .record_move_delta(packet.entity_id.0, &packet.delta);
    } else if id == ClientboundRespawn::ID {
        let _: ClientboundRespawn = decode(body)?;
        state.names.clear_world_entities();
        state.pose.clear_accepted();
        state.awaiting_respawn_pos = state.tracks_pose;
    } else if state.tracks_pose && id == ClientboundPlayerPosition::ID {
        return accept_position(io, state, body).await.map(Some);
    } else if needs_world_acks(state.observe) && id == ClientboundChunkBatchFinished::ID {
        automatic_reply(
            io,
            &ServerboundChunkBatchReceived {
                desired_chunks_per_tick: CHUNKS_PER_TICK,
            },
        )
        .await?;
    } else if !state.observe.inventory {
        return Ok(None);
    } else if let Some(packet) = MenuPacket::read(id, body)? {
        state.view.observe(&packet);
        return Ok(Some(Signal::Menu(packet)));
    }
    Ok(None)
}

async fn accept_position(
    io: &mut PacketIo<TcpStream>,
    state: &mut SessionState<'_>,
    body: &[u8],
) -> anyhow::Result<Signal> {
    let packet: ClientboundPlayerPosition = decode(body)?;
    let acceptance = state.pose.accept(&packet);
    automatic_reply(io, &acceptance).await?;
    state.awaiting_respawn_pos = false;
    Ok(Signal::PositionAccepted(acceptance.position))
}

async fn automatic_reply<P>(io: &mut PacketIo<TcpStream>, packet: &P) -> anyhow::Result<()>
where
    P: Packet + Encode,
{
    time::timeout(WRITE_LIMIT, io.send_packet(packet))
        .await
        .context("automatic packet write timed out")?
}

fn read_observed(state: &SessionState<'_>, id: i32, body: &[u8]) -> anyhow::Result<Option<Signal>> {
    if let Some(signal) = GameplaySignal::read(state.observe, state.client, &state.names, id, body)?
    {
        return Ok(Some(Signal::Gameplay(signal)));
    }
    Ok(state
        .observe
        .packet_order
        .then(|| packet_order_kind(id))
        .flatten()
        .map(Signal::Packet))
}

fn packet_order_kind(id: i32) -> Option<PacketKind> {
    if SwingObserved::is_packet(id) {
        Some(PacketKind::SwingAnimation)
    } else if SoundPlayed::is_packet(id) {
        Some(PacketKind::Sound)
    } else if id == ClientboundBlockUpdate::ID {
        Some(PacketKind::BlockUpdate)
    } else if id == ClientboundSectionBlocksUpdate::ID {
        Some(PacketKind::SectionBlocksUpdate)
    } else {
        GameplaySignal::packet_kind(id)
    }
}

fn decode<'a, P: Decode<'a>>(mut body: &'a [u8]) -> anyhow::Result<P> {
    Ok(P::decode(&mut body)?)
}

fn is_close(error: &anyhow::Error) -> bool {
    error.downcast_ref::<io::Error>().is_some_and(|error| {
        matches!(
            error.kind(),
            io::ErrorKind::UnexpectedEof
                | io::ErrorKind::ConnectionReset
                | io::ErrorKind::ConnectionAborted
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packet_order_and_observed_signals_cover_gameplay_packets() {
        use PacketKind::*;
        use mcrs_minecraft_protocol_game::clientbound as wire;
        for (packet_id, kind) in [
            (wire::ClientboundSetEquipment::ID, SetEquipment),
            (wire::ClientboundSetEntityData::ID, SetEntityData),
            (wire::ClientboundSetHealth::ID, SetHealth),
            (wire::ClientboundDamageEvent::ID, DamageEvent),
            (wire::ClientboundEntityEvent::ID, EntityEvent),
            (wire::ClientboundGameEvent::ID, GameEvent),
        ] {
            let actual = packet_order_kind(packet_id);
            assert!(actual == Some(kind), "packet {packet_id} mismatch");
        }
    }

    #[test]
    fn move_player_requires_accepted_pose_and_preserves_on_ground() {
        let mut pose = Pose::default();
        assert!(!pose.is_accepted(), "fresh pose unaccepted");
        let err = prepare_move_player(&mut pose, "Alice", [1.0, 64.0, 2.0], false, None)
            .unwrap_err()
            .to_string();
        assert!(err.contains("player_position"), "{err}");
        let unaccepted = !pose.is_accepted() && pose.move_pos_rot().is_none();
        assert!(unaccepted, "pose stayed unaccepted");

        pose.accept(&ClientboundPlayerPosition {
            teleport_id: VarInt(1),
            position: Position::new(0.0, 64.0, 0.0).into(),
            velocity: Default::default(),
            look: Look {
                yaw: 10.0,
                pitch: 5.0,
            },
            flags: Default::default(),
        });
        let accepted = pose.is_accepted() && pose.move_pos_rot().unwrap().flags.on_ground();
        assert!(accepted, "accept sets accepted and on_ground");

        let target = [1.5, 65.0, -2.5];
        let look = Some([90.0, 15.0]);
        let flags = prepare_move_player(&mut pose, "Alice", target, false, look).unwrap();
        let sync = pose.move_pos_rot().expect("accepted pose builds sync");
        let pos_ok = sync.position == Position::new(1.5, 65.0, -2.5);
        let look_ok = sync.look
            == Look {
                yaw: 90.0,
                pitch: 15.0,
            };
        let ground_ok = !flags.on_ground() && !sync.flags.on_ground();
        assert!(pos_ok && look_ok && ground_ok, "sync matches move_player");
    }

    #[test]
    fn move_player_and_sync_position_end_the_client_tick() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            let (client_stream, server_stream) =
                tokio::join!(TcpStream::connect(addr), listener.accept());
            let mut client_io = PacketIo::new(client_stream.unwrap());
            let mut server_io = PacketIo::new(server_stream.unwrap().0);

            let mut pose = Pose::default();
            pose.accept(&ClientboundPlayerPosition {
                teleport_id: VarInt(1),
                position: Position::new(0.0, 64.0, 0.0).into(),
                velocity: Default::default(),
                look: Look {
                    yaw: 10.0,
                    pitch: 5.0,
                },
                flags: Default::default(),
            });
            let observe = Observe::default();
            let mut state = SessionState {
                client: "Alice",
                player_id: 7,
                observe: &observe,
                tracks_pose: true,
                view: MenuView::default(),
                pose,
                names: EntityNames::default(),
                awaiting_respawn_pos: false,
            };

            send_order(&mut client_io, &mut state, Order::SyncPosition)
                .await
                .unwrap();
            send_order(
                &mut client_io,
                &mut state,
                Order::MovePlayer([1.0, 64.0, 2.0], true, None),
            )
            .await
            .unwrap();
            send_order(
                &mut client_io,
                &mut state,
                Order::MovePlayer([2.0, 65.0, 3.0], false, Some([90.0, 15.0])),
            )
            .await
            .unwrap();

            let expected = [
                ServerboundMovePlayerPosRot::ID,
                ServerboundClientTickEnd::ID,
                ServerboundMovePlayerPos::ID,
                ServerboundClientTickEnd::ID,
                ServerboundMovePlayerPosRot::ID,
                ServerboundClientTickEnd::ID,
            ];
            for expected_id in expected {
                let (id, body) = server_io.recv_frame().await.unwrap();
                assert_eq!(id, expected_id);
                if id == ServerboundClientTickEnd::ID {
                    let mut r = &body[..];
                    ServerboundClientTickEnd::decode(&mut r).unwrap();
                    assert!(
                        r.is_empty(),
                        "ServerboundClientTickEnd must leave no trailing bytes"
                    );
                }
            }
        });
    }
}
