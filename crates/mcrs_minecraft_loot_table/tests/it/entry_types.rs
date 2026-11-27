use mcrs_minecraft_game_registries::registry_load::{load_registries, test_additions, test_packs};
use mcrs_minecraft_item_stack::{
    BundleContents, ChargedProjectiles, Container, Count, ItemStack, MaxStackSize,
};
use mcrs_minecraft_loot::context_key_set_keys::ContextKeySet;
use mcrs_minecraft_loot_condition::{ContextParam, LootContainer, LootContext, ParamNotAllowed};
use mcrs_minecraft_loot_table::{
    IssueOwner, LootProblem, LootScratch, LootTableFile, LootTables, SlotSourceFile,
};
use mcrs_minecraft_predicate::BlockHost;
use mcrs_minecraft_random::Random;
use mcrs_minecraft_random::xoroshiro::XoroshiroRandom;
use mcrs_minecraft_resource::{ResourceLocation, ResourceLocationBuf};
use mcrs_test_alloc::allocations;

use crate::fixture::{Around, context, item, names, state, stream, tool};
use crate::loot_tables::{ADDED, CHAIN, added_table, roll_with_stream};

fn stick(count: u8) -> (String, u8) {
    ("minecraft:stick".to_owned(), count)
}

fn snowball(count: u8) -> (String, u8) {
    ("minecraft:snowball".to_owned(), count)
}

fn fresh(name: &str) -> XoroshiroRandom {
    stream(&ADDED.1, added_table(name), 0)
}

fn roll_in(name: &str, context: &LootContext<'_, ()>) -> (Vec<Option<ItemStack>>, XoroshiroRandom) {
    let mut random = fresh(name);
    let mut stacks = Vec::new();
    ADDED
        .1
        .roll(
            added_table(name),
            context,
            &BlockHost::none(),
            &mut random,
            &mut LootScratch::new(),
            &mut stacks,
        )
        .expect("a block context");
    (stacks, random)
}

fn with_luck(luck: f32) -> LootContext<'static, ()> {
    let mut context = context(state("minecraft:stone", &[]), None);
    context.luck = luck;
    context
}

#[test]
fn loot_evl_010_a_nested_table_rolls_in_the_same_evaluation() {
    let empty = (String::new(), 0);
    for (name, stacks, float_draws) in [
        ("t:tag_all", vec![snowball(1), stick(1)], 0),
        (
            "t:nested_in_order",
            vec![stick(5), stick(5), snowball(1)],
            10,
        ),
        (
            "t:nested_undivided",
            vec![empty.clone(), empty, snowball(10)],
            0,
        ),
    ] {
        let mut expected = fresh(name);
        for _ in 0..float_draws {
            expected.next_f32();
        }
        assert_eq!(roll_with_stream(name), (stacks, expected), "{name}");
    }
}

#[test]
fn loot_evl_009_an_expanded_entry_gives_one_candidate_per_member() {
    for (name, members) in [
        ("t:tag_each", [stick(1), snowball(1)]),
        ("t:nested_each", [stick(1), snowball(1)]),
    ] {
        let mut expected = fresh(name);
        let pick = expected.next_u32_bound(2);
        let selected = members
            .into_iter()
            .nth(usize::try_from(pick).expect("a small index"))
            .expect("a member");
        assert_eq!(roll_with_stream(name), (vec![selected], expected), "{name}");
    }
}

#[test]
fn loot_evl_007_a_dynamic_entry_makes_the_stacks_of_the_context() {
    let enchanted = tool("minecraft:diamond_pickaxe", &[("minecraft:fortune", 3)]);
    let sticks = ItemStack::new(item("minecraft:stick"), Count::new(3).expect("a count"));
    let given = [Some(enchanted), None, Some(sticks)];
    let large = ItemStack::new(item("minecraft:stick"), Count::new(99).expect("a count"));
    let large = [Some(large.set(MaxStackSize(99)))];
    for (table, contents, sherds, expected) in [
        ("t:dynamic", &given[..], &[][..], given.to_vec()),
        ("t:dynamic", &[][..], &given[..], vec![]),
        ("t:dynamic_unknown", &given[..], &given[..], vec![]),
        ("t:dynamic", &large[..], &[][..], large.to_vec()),
    ] {
        let mut context = context(state("minecraft:stone", &[]), None);
        context.contents = contents;
        context.sherds = sherds;
        assert_eq!(
            roll_in(table, &context),
            (expected, fresh(table)),
            "{table}"
        );
    }
}

#[test]
fn loot_evl_003_quality_and_luck_change_the_weights() {
    for (luck, total) in [(0.0, Some(4)), (-1.0, Some(5)), (2.5, None), (3.0, None)] {
        let mut expected = fresh("t:quality");
        let stack = match total {
            Some(total) => match expected.next_u32_bound(total) {
                0 => stick(1),
                _ => snowball(1),
            },
            None => stick(1),
        };
        let (stacks, random) = roll_in("t:quality", &with_luck(luck));
        assert_eq!((names(stacks), random), (vec![stack], expected), "{luck}");
    }
}

#[test]
fn loot_evl_002_the_luck_adds_bonus_rolls() {
    for (luck, rolls) in [(0.0, 1), (2.0, 4), (0.5, 1), (-1.0, 0)] {
        let (stacks, random) = roll_in("t:luck_rolls", &with_luck(luck));
        assert_eq!(
            (names(stacks), random),
            (vec![stick(1); rolls], fresh("t:luck_rolls")),
            "{luck}"
        );
    }
}

#[test]
fn loot_ctx_016_entry_faults_are_issues_with_their_path() {
    let mut issues: Vec<_> = ADDED
        .2
        .iter()
        .filter(|issue| issue.name.namespace() == "f")
        .map(|issue| {
            (
                issue.name.as_str(),
                issue.path.as_str(),
                issue.problem.clone(),
            )
        })
        .collect();
    issues.sort_by(|left, right| (left.0, left.1).cmp(&(right.0, right.1)));
    let cycle_reports = [
        (
            vec!["f:a", "f:b"],
            vec![
                ("f:a", "pools[0].entries[0].value"),
                ("f:b", "pools[0].entries[0].value.pools[0].entries[0].value"),
            ],
        ),
        (
            vec!["f:self"],
            vec![("f:self", "pools[0].entries[0].children[1].value")],
        ),
    ];
    let mut cycles: Vec<Vec<&str>> = Vec::new();
    for (table, path, problem) in &issues {
        if let LootProblem::Cycle(members) = problem {
            let mut members: Vec<&str> = members.iter().map(ResourceLocationBuf::as_str).collect();
            members.sort_unstable();
            let reports = cycle_reports
                .iter()
                .find(|(expected, _)| *expected == members);
            assert!(
                reports.is_some_and(|(_, reports)| reports.contains(&(*table, *path))),
                "{table} at {path}: {members:?}"
            );
            cycles.push(members);
        }
    }
    cycles.sort_unstable();
    assert_eq!(cycles, [vec!["f:a", "f:b"], vec!["f:self"]]);
    issues.retain(|(_, _, problem)| !matches!(problem, LootProblem::Cycle(_)));
    let tool = LootProblem::ParamNotAllowed(ParamNotAllowed {
        param: ContextParam::Tool,
        key_set: ContextKeySet::Chest,
    });
    assert_eq!(
        issues,
        [
            (
                "f:alternatives",
                "pools[0].entries[0].children[0]",
                LootProblem::AlternativeWithoutCondition
            ),
            ("f:chest_nests", "pools[0].entries[0].value", tool),
            (
                "f:no_children",
                "pools[0].entries[0]",
                LootProblem::NoChildren
            ),
            ("f:set_item_air", "modifier", LootProblem::SetItemAir),
            (
                "f:unknown_table",
                "pools[0].entries[0].value",
                LootProblem::UnknownTable("f:missing".to_owned())
            ),
            (
                "f:unknown_tag",
                "pools[0].entries[0].items",
                LootProblem::UnknownTag("f:missing".to_owned())
            ),
        ]
    );
    assert_eq!(roll_with_stream("f:a"), (vec![], fresh("f:a")));
}

#[test]
fn loot_ctx_017_a_chain_of_nested_tables_past_the_limit_is_an_issue_at_each_place() {
    let issues: Vec<_> = ADDED
        .2
        .iter()
        .filter(|issue| issue.name.namespace() == "d")
        .map(|issue| (issue.name.as_str(), issue.path.as_str(), &issue.problem))
        .collect();
    let entry = "pools[0].entries[0].value";
    assert_eq!(
        issues,
        [
            ("d:chain/63", entry, &LootProblem::TooDeep),
            ("d:chain/64", entry, &LootProblem::TooDeep),
        ]
    );
    for (link, stacks) in [(0, vec![]), (CHAIN - 64, vec![stick(1)])] {
        let name = format!("d:chain/{link}");
        assert_eq!(roll_with_stream(&name), (stacks, fresh(&name)), "{name}");
    }
}

#[test]
fn reg_fld_790_and_801_decode_rejections_fail_strictly() {
    let entry_ok = |entry| {
        serde_json::from_str::<LootTableFile>(&format!(
            r#"{{"pools":[{{"rolls":1,"entries":[{entry}]}}]}}"#
        ))
        .is_ok()
    };
    assert!(!entry_ok(r#"{"type":"minecraft:nothing"}"#), "bad entry");
    assert!(
        entry_ok(r#"{"type":"minecraft:slots","slot_source":["t:a",{"type":"minecraft:empty"}]}"#),
        "valid slots entry"
    );
    let slot_ok = |json| serde_json::from_str::<SlotSourceFile>(json).is_ok();
    assert!(slot_ok(r##""#t:pair""##), "valid tag slot source");
    for bad in [
        r#"{"type":"minecraft:limit_slots","limit":0,"slot_source":"t:a"}"#,
        r#"{"type":"minecraft:limit_slots","limit":-1,"slot_source":"t:a"}"#,
        r#"{"type":"minecraft:slot_range","slots":"bogus.0"}"#,
        r#"{"type":"minecraft:contents","component":"minecraft:custom_data","slot_source":"t:a"}"#,
        r#"{"type":"minecraft:group","terms":[["t:a"]]}"#,
    ] {
        assert!(!slot_ok(bad), "invalid {bad}");
    }
}

struct SlotPack {
    registries: mcrs_minecraft_registries::Registries,
    loot: LootTables,
    issues: Vec<mcrs_minecraft_loot_table::LootIssue>,
}

fn slot_pack() -> SlotPack {
    let mut additions = test_additions();
    let files = &mut additions.files;
    let mut put = |path, json| files.insert("s", format!("data/s/{path}.json"), json);
    put(
        "slot_source/a",
        r#"{"type":"minecraft:slot_range","source":"container","slots":"container.*"}"#,
    );
    put(
        "slot_source/b",
        r#"{"type":"minecraft:slot_range","source":"this","slots":"hotbar.0"}"#,
    );
    put("tags/slot_source/pair", r#"{"values":["s:a","s:b"]}"#);
    put("slot_source/loop_a", r#""s:loop_b""#);
    put("slot_source/loop_b", r#""s:loop_a""#);
    put(
        "loot_table/all",
        r##"{"type":"minecraft:command_slot_source","pools":[{"rolls":1,"entries":[{"type":"minecraft:slots","slot_source":[
            {"type":"minecraft:limit_slots","limit":2,"slot_source":"#s:pair"},
            {"type":"minecraft:contents","component":"charged_projectiles","slot_source":{"type":"minecraft:slot_range","source":"container","slots":"container.2"}},
            {"type":"minecraft:empty"}
        ]}]}]}"##,
    );
    put(
        "loot_table/block",
        r#"{"type":"minecraft:block","pools":[{"rolls":1,"entries":[{"type":"minecraft:slots","slot_source":[
            {"type":"minecraft:filtered","item_filter":{"items":"minecraft:stick"},"slot_source":{"type":"minecraft:contents","component":"container","slot_source":{"type":"minecraft:slot_range","source":"block_entity","slots":"contents"}}},
            {"type":"minecraft:contents","component":"bundle_contents","slot_source":{"type":"minecraft:slot_range","source":"tool","slots":"contents"}}
        ]}]}]}"#,
    );
    put(
        "loot_table/bad",
        r#"{"type":"minecraft:chest","pools":[{"rolls":1,"entries":[{"type":"minecraft:item","name":"minecraft:air"},{"type":"minecraft:slots","slot_source":["s:missing","s:a"]}]}]}"#,
    );
    additions.embedded_packs.push("s".to_owned());
    let (registries, _) = load_registries(&test_packs(), additions).expect("slot pack loads");
    let items = mcrs_minecraft_engine_registries::test_item_table();
    let (loot, issues) = LootTables::bake(crate::fixture::blocks(), &registries, &items);
    SlotPack {
        registries,
        loot,
        issues,
    }
}

#[test]
fn loot_evl_013_slots_entry_validates_faults_and_evaluates_without_allocations() {
    let SlotPack {
        registries,
        loot,
        issues,
    } = slot_pack();
    let deny = ParamNotAllowed {
        param: ContextParam::Container,
        key_set: ContextKeySet::Chest,
    };
    for expected in [
        LootProblem::ItemAir,
        LootProblem::UnknownSlotSource("s:missing".to_owned()),
        LootProblem::ParamNotAllowed(deny),
    ] {
        assert!(
            issues
                .iter()
                .any(|issue| issue.name.as_str() == "s:bad" && issue.problem == expected),
            "missing {expected:?} in {issues:?}"
        );
    }
    assert!(
        issues
            .iter()
            .any(|issue| issue.owner == IssueOwner::SlotSource
                && matches!(issue.problem, LootProblem::Cycle(_))),
        "expected slot_source cycle"
    );
    let tables = registries.registry::<LootTableFile>().expect("loot tables");
    let lookup = |name| {
        tables
            .by_name(ResourceLocation::read(name).expect("name"))
            .expect(name)
    };
    let (all_table, block_table) = (lookup("s:all"), lookup("s:block"));
    let plain = |name| tool(name, &[]);
    let chest = plain("minecraft:chest").set(
        Container::new(vec![
            None,
            Some(plain("minecraft:stick")),
            Some(plain("minecraft:snowball")),
        ])
        .expect("container"),
    );
    let crossbow =
        plain("minecraft:crossbow").set(ChargedProjectiles(vec![plain("minecraft:arrow")]));
    let bundle = plain("minecraft:bundle").set(BundleContents(vec![plain("minecraft:snowball")]));
    let container_slots = [None, Some(plain("minecraft:stick")), Some(crossbow)];
    let (entity_slots, block_entity_slots) = ([Some(plain("minecraft:snowball"))], [Some(chest)]);
    let mut slot_context = LootContext::try_new(ContextKeySet::CommandSlotSource).expect("key set");
    slot_context.origin = Some(glam::DVec3::ZERO);
    slot_context.container = Some(LootContainer::Slots(&container_slots));
    slot_context.this_entity = Some(());
    let block_entity = mcrs_minecraft_nbt::compound::NbtCompound::default();
    let mut block_context = context(state("minecraft:stone", &[]), Some(&bundle));
    block_context.block_entity = Some(&block_entity);
    let world = Around {
        entity_slots: &entity_slots,
        block_entity_slots: &block_entity_slots,
        ..Around::default()
    };
    let mut scratch = LootScratch::new();
    let mut random = stream(&loot, all_table, 0);
    let mut stacks = Vec::new();
    let mut roll_both = |stacks: &mut Vec<Option<ItemStack>>| {
        for (table, context) in [(all_table, &slot_context), (block_table, &block_context)] {
            loot.roll(table, context, &world, &mut random, &mut scratch, stacks)
                .expect("roll");
        }
    };
    roll_both(&mut stacks);
    let arrow = ("minecraft:arrow".to_owned(), 1);
    assert_eq!(
        names(stacks.drain(..)),
        [stick(1), arrow, stick(1), snowball(1)]
    );
    let before = allocations();
    roll_both(&mut stacks);
    assert_eq!(allocations() - before, 0, "second pass should not allocate");
}
