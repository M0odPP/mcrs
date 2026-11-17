use bevy_ecs::hierarchy::ChildOf;
use bevy_ecs::message::{MessageReader, MessageWriter};
use bevy_ecs::system::{Query, Res};
use mcrs_minecraft_engine::swing::SwingPlayer;
use mcrs_minecraft_engine_item::RequestDrop;
use mcrs_minecraft_engine_world::session::Owner;
use mcrs_minecraft_swing::{SpectatorRule, SwingDelivery, SwingSpec};

use crate::VanillaInventoryRequests;
use crate::request_ids::HAND_DROP_REQUEST;

pub(crate) fn swing_on_hand_drop(
    mut drops: MessageReader<RequestDrop>,
    requests: Option<Res<VanillaInventoryRequests>>,
    holders: Query<(Option<&Owner>, Option<&ChildOf>)>,
    mut swings: MessageWriter<SwingPlayer>,
) {
    let Some(requests) = requests else {
        return;
    };
    for drop in drops.read() {
        if drop.id.source != requests.source || drop.id.serial != HAND_DROP_REQUEST {
            continue;
        }
        let Ok((owner, parent)) = holders.get(drop.holder) else {
            continue;
        };
        let player = if owner.is_some() {
            drop.holder
        } else if let Some(parent) = parent {
            parent.parent()
        } else {
            continue;
        };
        swings.write(SwingPlayer {
            player,
            spec: SwingSpec::DEFAULT_MAIN,
            delivery: SwingDelivery::ObserversOnly,
            spectator: SpectatorRule::Block,
        });
    }
}
