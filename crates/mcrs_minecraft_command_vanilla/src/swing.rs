use crate::entity_view::EntityView;
use crate::select::{SelectionBuffers, SelfSource, targets_or_self};
use crate::set::{VanillaCommands, VanillaContext};
use bevy_ecs::change_detection::Mut;
use bevy_ecs::entity::Entity;
use bevy_ecs::prelude::{In, Local, ResMut};
use bevy_ecs::world::World;
use mcrs_command::{ArgumentHandle, BuildError, NodeId};
use mcrs_minecraft_command::{CommandLevel, PermissionCheck};
use mcrs_minecraft_command_argument::{SwingAnimationArgument, SwingAnimationName, TimeArgument};
use mcrs_minecraft_command_argument_entity::EntityArgument;
use mcrs_minecraft_command_engine::{
    Call, CommandFailure, CommandOutput, CommandRegistrar, CommandResult, EffectQueue, Feedback,
    Worlds,
};
use mcrs_minecraft_damage::Vitals;
use mcrs_minecraft_engine::player_order::SwingOrder;
use mcrs_minecraft_engine_entity_motion::MotionState;
use mcrs_minecraft_engine_physics::{self as physics, Mob, MobChanges, MobSwing, MobSwingSpec};
use mcrs_minecraft_engine_world::birth_clock::BirthClock;
use mcrs_minecraft_engine_world::session::PlayerSession;
use mcrs_minecraft_protocol_game::{Hand, SwingAnimationKind};
use mcrs_minecraft_text::Text;

const DEFAULT_DURATION: i32 = 6;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum SwingEntity {
    Player {
        player: PlayerSession,
        hand: Hand,
        animation: SwingAnimationKind,
        duration: i32,
    },
    Mob {
        target: Entity,
        hand: Hand,
        animation: SwingAnimationKind,
        duration: i32,
    },
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Branch {
    Default,
    Hand(Hand),
    Animation(Hand),
    Duration(Hand),
}

#[derive(Clone, Copy)]
pub(crate) struct SwingCommand {
    targets: ArgumentHandle<EntityArgument>,
    main: HandBranch,
    off: HandBranch,
}

#[derive(Clone, Copy)]
struct HandBranch {
    animation: ArgumentHandle<SwingAnimationArgument>,
    duration: ArgumentHandle<TimeArgument>,
}

fn animation_kind(name: SwingAnimationName) -> SwingAnimationKind {
    match name {
        SwingAnimationName::None => SwingAnimationKind::None,
        SwingAnimationName::Whack => SwingAnimationKind::Whack,
        SwingAnimationName::Stab => SwingAnimationKind::Stab,
    }
}

fn add_hand_branch(
    registrar: &mut CommandRegistrar<'_, VanillaCommands, Branch>,
    parent: ArgumentHandle<EntityArgument>,
    name: &'static str,
    hand: Hand,
) -> Result<HandBranch, BuildError> {
    let hand_node = registrar
        .literal(parent, name)
        .runs(Branch::Hand(hand))
        .id()?;
    let animation = registrar
        .arg(hand_node, "animation", SwingAnimationArgument)
        .runs(Branch::Animation(hand))
        .id()?;
    let duration = registrar
        .arg(animation, "duration", TimeArgument { min: 1 })
        .runs(Branch::Duration(hand))
        .id()?;
    Ok(HandBranch {
        animation,
        duration,
    })
}

pub(crate) fn build(
    registrar: &mut CommandRegistrar<'_, VanillaCommands, Branch>,
) -> Result<SwingCommand, BuildError> {
    let swing = registrar
        .literal(NodeId::ROOT, "swing")
        .requires(PermissionCheck::Require(CommandLevel::Gamemasters))
        .runs(Branch::Default)
        .id()?;
    let targets = registrar
        .arg(swing, "targets", EntityArgument::ENTITIES)
        .runs(Branch::Default)
        .id()?;
    let main = add_hand_branch(registrar, targets, "mainhand", Hand::Main)?;
    let off = add_hand_branch(registrar, targets, "offhand", Hand::Off)?;
    Ok(SwingCommand { targets, main, off })
}

struct SwingParams {
    hand: Hand,
    animation: SwingAnimationKind,
    duration: i32,
}

fn resolve_params(
    context: &VanillaContext,
    call: &Call<Branch, SwingCommand>,
) -> Result<SwingParams, CommandFailure> {
    let branch_of = |hand| match hand {
        Hand::Main => call.command.main,
        Hand::Off => call.command.off,
    };
    let (hand, animation, duration) = match call.branch {
        Branch::Default => (Hand::Main, SwingAnimationKind::Whack, DEFAULT_DURATION),
        Branch::Hand(hand) => (hand, SwingAnimationKind::Whack, DEFAULT_DURATION),
        Branch::Animation(hand) => (
            hand,
            animation_kind(context.arg(&call.invocation, branch_of(hand).animation)?),
            DEFAULT_DURATION,
        ),
        Branch::Duration(hand) => {
            let branch = branch_of(hand);
            (
                hand,
                animation_kind(context.arg(&call.invocation, branch.animation)?),
                context.arg(&call.invocation, branch.duration)?,
            )
        }
    };
    Ok(SwingParams {
        hand,
        animation,
        duration,
    })
}

pub(crate) fn run(
    In(call): In<Call<Branch, SwingCommand>>,
    context: VanillaContext,
    mut output: CommandOutput,
    mut swings: ResMut<EffectQueue<SwingEntity>>,
    mut found: Local<SelectionBuffers>,
) -> CommandResult {
    let SwingParams {
        hand,
        animation,
        duration,
    } = resolve_params(&context, &call)?;
    found.with(|targets| {
        targets_or_self(
            &context,
            &call.invocation,
            call.command.targets,
            "argument.entity.notfound.entity",
            SelfSource::Entity,
            targets,
        )?;
        let mut count: i32 = 0;
        let mut first_living = None;
        for target in targets.iter().filter(EntityView::is_living) {
            first_living.get_or_insert(target);
            count = count.saturating_add(1);
            match target {
                EntityView::Player(player) => output.effect(
                    &mut swings,
                    SwingEntity::Player {
                        player: player.session,
                        hand,
                        animation,
                        duration,
                    },
                ),
                EntityView::Mob { view, .. } => output.effect(
                    &mut swings,
                    SwingEntity::Mob {
                        target: view.entity,
                        hand,
                        animation,
                        duration,
                    },
                ),
                EntityView::Item { .. } => {}
            }
        }
        if count == 0 {
            return Err(CommandFailure::translate(
                "commands.swing.failed.notliving",
                vec![],
            ));
        }
        let message = match (first_living, count) {
            (Some(target), 1) => {
                Text::translate("commands.swing.success.single", vec![target.display_name()])
            }
            _ => Text::translate_args("commands.swing.success.multiple", vec![count.into()]),
        };
        output.success(&call.invocation, message, Feedback::SourceAndOperators);
        Ok(count)
    })
}

pub(crate) fn apply(worlds: &mut Worlds, effect: SwingEntity) {
    match effect {
        SwingEntity::Player {
            player,
            hand,
            animation,
            duration,
        } => {
            let order = SwingOrder::new(hand, animation, duration);
            if let Err(error) = worlds.swing(player, order) {
                tracing::debug!(?player, ?error, "swing command skipped offline player");
            }
        }
        SwingEntity::Mob {
            target,
            hand,
            animation,
            duration,
        } => {
            let world: &mut World = worlds.world_mut();
            let tick = world
                .get_resource::<BirthClock>()
                .map_or(0, |clock| clock.0);
            let spec = MobSwingSpec::new(hand, animation, duration);
            let swung = world
                .try_resource_scope(|world, mut changes: Mut<MobChanges>| {
                    let mut mob = world.get_entity_mut(target).ok()?;
                    if !mob.contains::<Mob>() {
                        return None;
                    }
                    let state = mob.get::<MotionState>()?.0;
                    let vitals = mob.get::<Vitals>().copied();
                    let mut swing = mob.get_mut::<MobSwing>()?;
                    Some(physics::swing_mob(
                        target,
                        &state,
                        vitals.as_ref(),
                        &mut swing,
                        &mut changes,
                        spec,
                        tick,
                    ))
                })
                .flatten();
            if swung.is_none() {
                tracing::debug!(?target, "a command swing finds no mob; it does nothing");
            }
        }
    }
}
