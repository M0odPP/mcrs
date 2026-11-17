use crate::entity_view::EntityView;
use crate::select::{SelectionBuffers, SelfSource, targets_or_self};
use crate::set::{VanillaCommands, VanillaContext};
use bevy_ecs::prelude::*;
use mcrs_command::{ArgumentHandle, BuildError, NodeId};
use mcrs_minecraft_command::{CommandLevel, PermissionCheck};
use mcrs_minecraft_command_argument::{SwingAnimationArgument, TimeArgument};
use mcrs_minecraft_command_argument_entity::EntityArgument;
use mcrs_minecraft_command_engine::{
    Call, CommandFailure, CommandOutput, CommandRegistrar, CommandResult, EffectQueue, Feedback,
    Worlds,
};
use mcrs_minecraft_engine_brain::EntityRows;
use mcrs_minecraft_engine_physics::{self as physics, MobChanges, MobSwingStates, Mobs};
use mcrs_minecraft_engine_world::birth_clock::BirthClock;
use mcrs_minecraft_engine_world::session::PlayerSession;
use mcrs_minecraft_entity_rows::Placed;
use mcrs_minecraft_swing::{SwingDelivery, SwingHand, SwingSpec};
use mcrs_minecraft_text::Text;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SwingEntity {
    Player {
        player: PlayerSession,
        spec: SwingSpec,
    },
    Mob {
        at: Placed,
        spec: SwingSpec,
    },
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Branch {
    Default,
    Targets,
    Mainhand,
    Offhand,
    MainAnimation,
    OffAnimation,
    MainDuration,
    OffDuration,
}

#[derive(Clone, Copy)]
pub(crate) struct SwingCommand {
    targets: ArgumentHandle<EntityArgument>,
    main_animation: ArgumentHandle<SwingAnimationArgument>,
    main_duration: ArgumentHandle<TimeArgument>,
    off_animation: ArgumentHandle<SwingAnimationArgument>,
    off_duration: ArgumentHandle<TimeArgument>,
}

#[derive(Clone, Copy)]
struct HandBranch {
    animation: ArgumentHandle<SwingAnimationArgument>,
    duration: ArgumentHandle<TimeArgument>,
}

fn add_hand_branch(
    registrar: &mut CommandRegistrar<'_, VanillaCommands, Branch>,
    parent: ArgumentHandle<EntityArgument>,
    name: &'static str,
    hand_branch: Branch,
    anim_branch: Branch,
    duration_branch: Branch,
) -> Result<HandBranch, BuildError> {
    let hand = registrar.literal(parent, name).runs(hand_branch).id()?;
    let animation = registrar
        .arg(hand, "animation", SwingAnimationArgument)
        .runs(anim_branch)
        .id()?;
    let duration = registrar
        .arg(animation, "duration", TimeArgument { min: 1 })
        .runs(duration_branch)
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
        .runs(Branch::Targets)
        .id()?;
    let main = add_hand_branch(
        registrar,
        targets,
        "mainhand",
        Branch::Mainhand,
        Branch::MainAnimation,
        Branch::MainDuration,
    )?;
    let off = add_hand_branch(
        registrar,
        targets,
        "offhand",
        Branch::Offhand,
        Branch::OffAnimation,
        Branch::OffDuration,
    )?;
    Ok(SwingCommand {
        targets,
        main_animation: main.animation,
        main_duration: main.duration,
        off_animation: off.animation,
        off_duration: off.duration,
    })
}

pub(crate) fn run(
    In(call): In<Call<Branch, SwingCommand>>,
    context: VanillaContext,
    mut output: CommandOutput,
    mut swings: ResMut<EffectQueue<SwingEntity>>,
    mut found: Local<SelectionBuffers>,
) -> CommandResult {
    let command = call.command;
    let spec = match call.branch {
        Branch::Default | Branch::Targets | Branch::Mainhand => SwingSpec::DEFAULT_MAIN,
        Branch::Offhand => SwingSpec {
            hand: SwingHand::Off,
            ..SwingSpec::DEFAULT_MAIN
        },
        Branch::MainAnimation => SwingSpec::new(
            SwingHand::Main,
            context.arg(&call.invocation, command.main_animation)?,
            SwingSpec::DEFAULT_MAIN.duration,
        ),
        Branch::OffAnimation => SwingSpec::new(
            SwingHand::Off,
            context.arg(&call.invocation, command.off_animation)?,
            SwingSpec::DEFAULT_MAIN.duration,
        ),
        Branch::MainDuration => SwingSpec::new(
            SwingHand::Main,
            context.arg(&call.invocation, command.main_animation)?,
            context.arg(&call.invocation, command.main_duration)?,
        ),
        Branch::OffDuration => SwingSpec::new(
            SwingHand::Off,
            context.arg(&call.invocation, command.off_animation)?,
            context.arg(&call.invocation, command.off_duration)?,
        ),
    };
    found.with(|targets| {
        targets_or_self(
            &context,
            &call.invocation,
            command.targets,
            "argument.entity.notfound.entity",
            SelfSource::Entity,
            targets,
        )?;
        let outcome = emit_swings(targets.iter(), spec, |effect| {
            output.effect(&mut swings, effect);
        })?;
        output.success(
            &call.invocation,
            outcome.message,
            Feedback::SourceAndOperators,
        );
        Ok(outcome.count)
    })
}

#[derive(Debug, PartialEq)]
struct SwingOutcome {
    count: i32,
    message: Text,
}

fn emit_swings<'c>(
    targets: impl IntoIterator<Item = EntityView<'c>>,
    spec: SwingSpec,
    mut push: impl FnMut(SwingEntity),
) -> Result<SwingOutcome, CommandFailure> {
    let mut count: i32 = 0;
    let mut first_living = None;
    for target in targets.into_iter().filter(EntityView::is_living) {
        if first_living.is_none() {
            first_living = Some(target);
        }
        count = count.saturating_add(1);
        push(match target {
            EntityView::Player(player) => SwingEntity::Player {
                player: player.session,
                spec,
            },
            EntityView::Mob { view, .. } => SwingEntity::Mob { at: view.at, spec },
        });
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
    Ok(SwingOutcome { count, message })
}

pub(crate) fn apply(worlds: &mut Worlds, effect: SwingEntity) {
    match effect {
        SwingEntity::Player { player, spec } => {
            if let Err(error) = worlds.swing_player(player, spec, SwingDelivery::SelfAndObservers) {
                tracing::debug!(?player, ?error, "swing command skipped offline player");
            }
        }
        SwingEntity::Mob { at, spec } => {
            let world: &mut World = worlds.world_mut();
            let tick = world
                .get_resource::<BirthClock>()
                .map_or(0, |clock| clock.0);
            let swung = world
                .try_resource_scope(|world, rows: Mut<EntityRows>| {
                    world.try_resource_scope(|world, mobs: Mut<Mobs>| {
                        world.try_resource_scope(|world, mut states: Mut<MobSwingStates>| {
                            let mut changes = world.get_resource_mut::<MobChanges>()?;
                            Some(physics::swing_mob(
                                &rows,
                                &mobs,
                                &mut states,
                                &mut changes,
                                at,
                                spec,
                                tick,
                            ))
                        })
                    })
                })
                .flatten()
                .flatten()
                .flatten();
            if swung.is_none() {
                tracing::debug!(?at, "a command swing finds no mob row; it does nothing");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mob_view::MobView;
    use bevy_math::DVec3;
    use mcrs_minecraft_engine_physics::{Mob, mob_body, mob_row};
    use mcrs_minecraft_entity::EntityTypeData;
    use mcrs_minecraft_entity_key::{EntityId, EntityKey, TileKey};
    use mcrs_minecraft_protocol_game::entity::MobLooks;

    fn mob_view(kind: &'static EntityTypeData, id: u64) -> MobView<'static> {
        let mob = Box::leak(Box::new(Mob {
            kind,
            uuid: u128::from(id),
            looks: MobLooks::default(),
            custom_name: None,
            tags: Vec::new(),
        }));
        let row = mob_row(kind);
        let state = Box::leak(Box::new(mob_body(
            kind,
            &row,
            EntityKey(u128::from(id)),
            DVec3::ZERO,
        )));
        MobView {
            at: Placed {
                tile: TileKey::new(0, 0, 0),
                id: EntityId(id),
            },
            state,
            mob,
        }
    }

    #[test]
    fn non_living_targets_return_notliving_failure_and_mixed_skips_non_living() {
        let item = EntityView::Mob {
            view: mob_view(&EntityTypeData::ITEM, 1),
            dimension: 0,
        };
        assert_eq!(
            emit_swings([item], SwingSpec::DEFAULT_MAIN, |_| {}),
            Err(CommandFailure::translate(
                "commands.swing.failed.notliving",
                vec![]
            ))
        );

        let pig_view = mob_view(&EntityTypeData::PIG, 2);
        let pig = EntityView::Mob {
            view: pig_view,
            dimension: 0,
        };
        let mut effects = Vec::new();
        let outcome =
            emit_swings([item, pig], SwingSpec::DEFAULT_MAIN, |e| effects.push(e)).unwrap();
        assert_eq!(outcome.count, 1);
        assert_eq!(effects.len(), 1);
        assert_eq!(
            outcome.message,
            Text::translate(
                "commands.swing.success.single",
                vec![pig_view.display_name()]
            )
        );
    }
}
