use bevy_ecs::prelude::*;
use mcrs_command::{ArgumentHandle, BuildError, NodeId};
use mcrs_minecraft_command::{CommandLevel, PermissionCheck};
use mcrs_minecraft_command_argument::TimeArgument;
use mcrs_minecraft_command_engine::{
    Call, CommandOutput, CommandRegistrar, CommandResult, EffectQueue, Feedback,
};
use mcrs_minecraft_command_vanilla::{VanillaCommands, VanillaContext};
use mcrs_minecraft_engine::server_edit::ServerEdit;
use mcrs_minecraft_text::Text;
use mcrs_minecraft_weather::WeatherKind::{self, Clear, Rain, Thunder};
use mcrs_minecraft_weather::{
    CLEAR_TICKS_MAX, CLEAR_TICKS_MIN, RAIN_ON_MAX, RAIN_ON_MIN, THUNDER_ON_MAX, THUNDER_ON_MIN,
};

use crate::server_change::ServerChange;

#[derive(Clone, Copy)]
pub(crate) struct WeatherCommand {
    clear_duration: ArgumentHandle<TimeArgument>,
    rain_duration: ArgumentHandle<TimeArgument>,
    thunder_duration: ArgumentHandle<TimeArgument>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct WeatherChange {
    edit: ServerEdit,
    key: &'static str,
    feedback: Feedback,
    result: i32,
}

fn edit_for(kind: WeatherKind, ticks: u32) -> ServerEdit {
    match kind {
        Clear => ServerEdit::ClearWeather { ticks },
        Rain => ServerEdit::RainWeather { ticks },
        Thunder => ServerEdit::ThunderWeather { ticks },
    }
}

fn resolve_change(kind: WeatherKind, duration: Option<i32>) -> WeatherChange {
    let (lo, hi, key) = match kind {
        Clear => (
            CLEAR_TICKS_MIN,
            CLEAR_TICKS_MAX,
            "commands.weather.set.clear",
        ),
        Rain => (RAIN_ON_MIN, RAIN_ON_MAX, "commands.weather.set.rain"),
        Thunder => (
            THUNDER_ON_MIN,
            THUNDER_ON_MAX,
            "commands.weather.set.thunder",
        ),
    };
    let ticks = duration.map_or_else(
        || rand::random_range(lo..=hi),
        |v| u32::try_from(v).unwrap_or(0),
    );
    WeatherChange {
        edit: edit_for(kind, ticks),
        key,
        feedback: Feedback::SourceAndOperators,
        result: duration.unwrap_or(-1),
    }
}

fn add_branch(
    reg: &mut CommandRegistrar<'_, VanillaCommands, WeatherKind>,
    parent: NodeId,
    name: &str,
    kind: WeatherKind,
) -> Result<ArgumentHandle<TimeArgument>, BuildError> {
    let n = reg.literal(parent, name).runs(kind).in_any_world().id()?;
    reg.arg(n, "duration", TimeArgument { min: 1 })
        .runs(kind)
        .in_any_world()
        .id()
}

pub(crate) fn build(
    reg: &mut CommandRegistrar<'_, VanillaCommands, WeatherKind>,
) -> Result<WeatherCommand, BuildError> {
    let req = PermissionCheck::Require(CommandLevel::Gamemasters);
    let root = reg.literal(NodeId::ROOT, "weather").requires(req).id()?;
    Ok(WeatherCommand {
        clear_duration: add_branch(reg, root, "clear", Clear)?,
        rain_duration: add_branch(reg, root, "rain", Rain)?,
        thunder_duration: add_branch(reg, root, "thunder", Thunder)?,
    })
}

pub(crate) fn run(
    In(call): In<Call<WeatherKind, WeatherCommand>>,
    context: VanillaContext,
    mut output: CommandOutput,
    mut changes: ResMut<EffectQueue<ServerChange>>,
) -> CommandResult {
    let handle = match call.branch {
        Clear => call.command.clear_duration,
        Rain => call.command.rain_duration,
        Thunder => call.command.thunder_duration,
    };
    let change = resolve_change(call.branch, context.opt(&call.invocation, handle)?);
    output.effect(&mut changes, ServerChange(change.edit));
    let msg = Text::translate(change.key, vec![]);
    output.success(&call.invocation, msg, change.feedback);
    Ok(change.result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy_app::App;
    use mcrs_command::ParseScratch;
    use mcrs_minecraft_command::{NoLookup, Source};
    use mcrs_minecraft_command_engine::{AppCommandsExt, CommandPlugin, CommandSet, CommandTree};
    use mcrs_minecraft_engine_world::world::sub_app::DimensionPlugins;

    #[test]
    fn command_graph_and_changes_match_spec() {
        let mut app = App::new();
        app.init_resource::<DimensionPlugins>();
        app.add_plugins(CommandPlugin::<VanillaCommands>::new());
        app.add_command_effect(crate::server_change::apply);
        app.add_command(build, run).expect("ok");
        app.finish();
        let g = app
            .world()
            .resource::<CommandTree<VanillaCommands>>()
            .graph();
        let mut s = ParseScratch::<<VanillaCommands as CommandSet>::Error>::default();
        let mut ok = |l, c| {
            g.parse(c, 0, &Source::new(l, &NoLookup), &mut s)
                .check(g)
                .is_ok()
        };
        for l in [CommandLevel::All, CommandLevel::Moderators] {
            for cmd in ["weather clear", "weather rain", "weather thunder"] {
                assert!(!ok(l, cmd), "gamemaster required: {cmd}");
            }
        }
        let gm = CommandLevel::Gamemasters;
        let cmds = "weather clear|weather clear 100|weather clear 5s|weather rain|weather rain 1d|weather thunder|weather thunder 1t";
        for c in cmds.split('|') {
            assert!(ok(gm, c), "{c}");
        }
        for bad in "weather clear 0|weather rain 0s|weather thunder 0d|weather clear -5".split('|')
        {
            assert!(!ok(gm, bad), "{bad}");
        }
        let chk = |k, sub, lo, hi| {
            let (key, fb) = (
                format!("commands.weather.set.{sub}"),
                Feedback::SourceAndOperators,
            );
            let (a, r) = (resolve_change(k, None), resolve_change(k, Some(2400)));
            assert_eq!((a.key, a.feedback, a.result), (&*key, fb, -1));
            assert!((lo..=hi).any(|v| edit_for(k, v) == a.edit), "{k:?}");
            let exp = (edit_for(k, 2400), &*key, fb, 2400);
            assert_eq!((r.edit, r.key, r.feedback, r.result), exp);
        };
        chk(Clear, "clear", CLEAR_TICKS_MIN, CLEAR_TICKS_MAX);
        chk(Rain, "rain", RAIN_ON_MIN, RAIN_ON_MAX);
        chk(Thunder, "thunder", THUNDER_ON_MIN, THUNDER_ON_MAX);
    }
}
