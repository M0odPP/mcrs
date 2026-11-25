use mcrs_minecraft_probe::{
    Observed, PacketKind, PacketObserved, ProbeHand, Scenario, StepTrace, SwingAnimationKind,
    SwingObserved,
};
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

#[test]
fn every_scenario_loads_with_unique_step_ids() {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("scenarios");
    for entry in fs::read_dir(&directory).unwrap() {
        let path = entry.unwrap().path();
        let scenario: Scenario = serde_json::from_str(&fs::read_to_string(&path).unwrap())
            .unwrap_or_else(|error| panic!("{path:?} is not a scenario: {error}"));
        let mut ids = BTreeSet::new();
        for step in &scenario.steps {
            assert!(
                ids.insert(&step.id),
                "{path:?} repeats the step id {}",
                step.id
            );
        }
    }
}

#[test]
fn sequence_validates_nested_inventory_and_sequence_rules() {
    let nested_close = r#"{
        "steps": [
            {
                "id": "seq",
                "sequence": {
                    "steps": [
                        { "close": { "client": "Alice" } }
                    ]
                }
            }
        ]
    }"#;
    let nested_close_res: Result<Scenario, _> = serde_json::from_str(nested_close);
    let err = nested_close_res.unwrap_err().to_string();
    assert!(err.contains("observe.inventory"), "{err}");

    let nested_close_with_inventory = r#"{
        "observe": { "inventory": true },
        "steps": [
            {
                "id": "seq",
                "sequence": {
                    "steps": [
                        { "close": { "client": "Alice" } }
                    ]
                }
            }
        ]
    }"#;
    let allowed: Result<Scenario, _> = serde_json::from_str(nested_close_with_inventory);
    assert!(allowed.is_ok());

    let nested_sequence = r#"{
        "steps": [
            {
                "id": "seq",
                "sequence": {
                    "steps": [
                        { "punch": { "client": "Alice" } },
                        { "sequence": { "steps": [{ "wait": { "ms": 10 } }] } }
                    ]
                }
            }
        ]
    }"#;
    let nested_seq_res: Result<Scenario, _> = serde_json::from_str(nested_sequence);
    assert!(nested_seq_res.is_err());
}

#[test]
fn operator_levels_validate_range_and_membership() {
    let valid = r#"{
        "target": {
            "operators": ["Alice", "Carl"],
            "operator_levels": { "Carl": 1 }
        },
        "steps": []
    }"#;
    let scenario: Scenario = serde_json::from_str(valid).unwrap();
    assert_eq!(scenario.target.operator_level("Alice"), 4);
    assert_eq!(scenario.target.operator_level("Carl"), 1);

    let out_of_range = r#"{
        "target": {
            "operators": ["Carl"],
            "operator_levels": { "Carl": 5 }
        },
        "steps": []
    }"#;
    let out_of_range_res: Result<Scenario, _> = serde_json::from_str(out_of_range);
    assert!(out_of_range_res.is_err());

    let unlisted = r#"{
        "target": {
            "operators": ["Alice"],
            "operator_levels": { "Carl": 2 }
        },
        "steps": []
    }"#;
    let unlisted_res: Result<Scenario, _> = serde_json::from_str(unlisted);
    assert!(unlisted_res.is_err());
}

fn stab_swing(client: &str) -> SwingObserved {
    SwingObserved {
        client: client.to_owned(),
        subject: "Alice".to_owned(),
        hand: ProbeHand::Main,
        animation: SwingAnimationKind::Stab,
        duration: 6,
    }
}

#[test]
fn normalize_per_client_removes_cross_client_interleaving_while_preserving_order() {
    let make_step = |packets: Vec<PacketObserved>, swings: Vec<SwingObserved>| {
        let mut step = StepTrace {
            id: "step".to_owned(),
            observed: Observed::Sent {
                disconnects: Vec::new(),
                console_output: Vec::new(),
            },
            player_bans: None,
            inventory: None,
            sounds: None,
            command_trees: None,
            swings: Some(swings),
            swing_bytes: None,
            animates: None,
            equipment: None,
            entity_data: None,
            health: None,
            damage_events: None,
            entity_events: None,
            game_events: None,
            packets: Some(packets),
            checkpoints: Vec::new(),
            player_file: None,
        };
        step.normalize_per_client();
        step
    };

    let run_a = make_step(
        vec![
            PacketObserved {
                client: "Charlie".to_owned(),
                packet: PacketKind::SwingAnimation,
            },
            PacketObserved {
                client: "Bob".to_owned(),
                packet: PacketKind::Sound,
            },
            PacketObserved {
                client: "Bob".to_owned(),
                packet: PacketKind::SwingAnimation,
            },
            PacketObserved {
                client: "Charlie".to_owned(),
                packet: PacketKind::Sound,
            },
        ],
        vec![stab_swing("Charlie"), stab_swing("Bob")],
    );
    let run_b = make_step(
        vec![
            PacketObserved {
                client: "Bob".to_owned(),
                packet: PacketKind::Sound,
            },
            PacketObserved {
                client: "Bob".to_owned(),
                packet: PacketKind::SwingAnimation,
            },
            PacketObserved {
                client: "Charlie".to_owned(),
                packet: PacketKind::SwingAnimation,
            },
            PacketObserved {
                client: "Charlie".to_owned(),
                packet: PacketKind::Sound,
            },
        ],
        vec![stab_swing("Bob"), stab_swing("Charlie")],
    );

    assert_eq!(run_a, run_b);
    assert_eq!(
        run_a.packets.unwrap(),
        vec![
            PacketObserved {
                client: "Bob".to_owned(),
                packet: PacketKind::Sound,
            },
            PacketObserved {
                client: "Bob".to_owned(),
                packet: PacketKind::SwingAnimation,
            },
            PacketObserved {
                client: "Charlie".to_owned(),
                packet: PacketKind::SwingAnimation,
            },
            PacketObserved {
                client: "Charlie".to_owned(),
                packet: PacketKind::Sound,
            },
        ]
    );
}

#[test]
fn sync_position_requires_world_acks() {
    let missing_acks = r#"{
        "steps": [
            { "id": "join", "join": "Alice" },
            { "id": "sync", "sync_position": "Alice" }
        ]
    }"#;
    let err = serde_json::from_str::<Scenario>(missing_acks)
        .unwrap_err()
        .to_string();
    assert!(err.contains("sync_position"), "{err}");

    let nested_missing = r#"{
        "steps": [
            {
                "id": "seq",
                "sequence": { "steps": [{ "sync_position": "Alice" }] }
            }
        ]
    }"#;
    let nested_err = serde_json::from_str::<Scenario>(nested_missing)
        .unwrap_err()
        .to_string();
    assert!(nested_err.contains("sync_position"), "{nested_err}");

    let nested_with_swing = r#"{
        "observe": { "swing": true },
        "steps": [
            {
                "id": "seq",
                "sequence": { "steps": [{ "sync_position": "Alice" }] }
            }
        ]
    }"#;
    assert!(serde_json::from_str::<Scenario>(nested_with_swing).is_ok());

    let with_swing = r#"{
        "observe": { "swing": true },
        "steps": [
            { "id": "join", "join": "Alice" },
            { "id": "sync", "sync_position": "Alice" }
        ]
    }"#;
    assert!(serde_json::from_str::<Scenario>(with_swing).is_ok());
}

#[test]
fn wave1_scenario_and_trace_normalize_round_trip() {
    let scenario: Scenario = serde_json::from_str(
        r#"{"observe":{"equipment":true,"entity_data":true,"health":true,"damage_event":true,"entity_event":true,"game_event":true,"packet_order":true},"steps":[{"id":"move","move_player":{"client":"Alice","position":[1.0,64.0,2.0],"on_ground":true}},{"id":"command","player_command":{"client":"Alice","action":"start_sprinting"}},{"id":"input","player_input":{"client":"Alice","forward":true,"sprint":true}}]}"#,
    )
    .unwrap();
    let observe_ok = scenario.observe.health && scenario.observe.game_event;
    assert!(observe_ok && scenario.steps.len() == 3, "wave1 scenario");
    let mut step: StepTrace = serde_json::from_str(
        r#"{"id":"step","observed":"joined","health":[{"client":"Bob","health":10.0,"food":20,"saturation":5.0},{"client":"Alice","health":20.0,"food":20,"saturation":5.0}]}"#,
    )
    .unwrap();
    step.normalize_per_client();
    let decoded: StepTrace = serde_json::from_str(&serde_json::to_string(&step).unwrap()).unwrap();
    let clients: Vec<_> = decoded
        .health
        .iter()
        .flatten()
        .map(|entry| entry.client.as_str())
        .collect();
    assert!(
        decoded == step && clients == ["Alice", "Bob"],
        "round-trip and normalized order"
    );
}
