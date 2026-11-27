use std::sync::LazyLock;

use glam::DVec3;
use mcrs_minecraft_biome::Biome;
use mcrs_minecraft_engine_registries::{RegistryNames, test_item_table};
use mcrs_minecraft_environment::environment_attribute_keys::EnvironmentAttribute;
use mcrs_minecraft_environment::{AttributeValue, WorldClock};
use mcrs_minecraft_fixtures::fixture;
use mcrs_minecraft_game_registries::registry_load::{
    load_registries, test_additions, test_packs, test_registries,
};
use mcrs_minecraft_item_stack::{ItemStack, read_saved};
use mcrs_minecraft_item_table::ItemTable;
use mcrs_minecraft_loot::context_key_set_keys::ContextKeySet;
use mcrs_minecraft_loot::loot::LootTable;
use mcrs_minecraft_loot_condition::{
    BlockEntityView, ClockView, Condition, ConditionBaker, ConditionFile, EnvironmentView,
    FloatProvider, FloatProviderFile, IntProvider, IntProviderFile, LootContext, LootInputs,
    NamedEntries, NbtNumber, ScoreHolder, ScoreView, StorageView, Tool, WeatherView,
};
use mcrs_minecraft_loot_table::{LootIssue, LootScratch, LootTableFile, LootTables};
use mcrs_minecraft_nbt_path::NbtPath;
use mcrs_minecraft_predicate::{EntityView, LocationView, PredicateScratch};
use mcrs_minecraft_random::legacy::LegacyRandom;
use mcrs_minecraft_registries::Registries;
use mcrs_minecraft_resource::{Id, ResourceLocation};
use mcrs_minecraft_snbt::parse_compound;
use mcrs_voxel::{BlockPos, BlockStateId};

use crate::fixture::{blocks, state};

const MAGIC: &[u8; 8] = b"MCLOOTR0";

const REPLAYED: usize = 167;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CaseKind {
    Roll,
    Fill,
    Condition,
    IntProvider,
    FloatProvider,
}

enum Source {
    Table,
    Inline(String),
}

struct SavedStack {
    item: String,
    count: i32,
    patch: String,
}

enum ContextValue {
    Vec3(DVec3),
    Stack(SavedStack),
    BlockState(String),
    Float(f32),
}

struct ContextEntry {
    key: String,
    value: ContextValue,
}

enum Outcome {
    Stacks(Vec<SavedStack>),
    Fill {
        slots: u32,
        items: Vec<(u32, SavedStack)>,
    },
    Tests(Vec<bool>),
    Ints(Vec<i32>),
    Floats(Vec<f32>),
}

struct Case {
    kind: CaseKind,
    name: String,
    biome: String,
    source: Source,
    key_set: String,
    luck: f32,
    values: Vec<ContextEntry>,
    seed: i64,
    outcome: Outcome,
    after: [i64; 2],
}

struct Reader<'a>(&'a [u8]);

impl Reader<'_> {
    fn take<const N: usize>(&mut self) -> [u8; N] {
        let (head, rest) = self
            .0
            .split_first_chunk::<N>()
            .expect("the file holds the field");
        self.0 = rest;
        *head
    }

    fn byte(&mut self) -> u8 {
        u8::from_le_bytes(self.take())
    }

    fn u32(&mut self) -> u32 {
        u32::from_le_bytes(self.take())
    }

    fn length(&mut self) -> usize {
        usize::try_from(self.u32()).expect("a length fits usize")
    }

    fn i32(&mut self) -> i32 {
        i32::from_le_bytes(self.take())
    }

    fn i64(&mut self) -> i64 {
        i64::from_le_bytes(self.take())
    }

    fn f32(&mut self) -> f32 {
        f32::from_le_bytes(self.take())
    }

    fn f64(&mut self) -> f64 {
        f64::from_le_bytes(self.take())
    }

    fn text(&mut self) -> String {
        let len = self.length();
        let (text, rest) = self
            .0
            .split_at_checked(len)
            .expect("the file holds the text");
        self.0 = rest;
        String::from_utf8(text.to_vec()).expect("the text is UTF-8")
    }

    fn stack(&mut self) -> SavedStack {
        SavedStack {
            item: self.text(),
            count: self.i32(),
            patch: self.text(),
        }
    }

    fn samples<T>(&mut self, mut sample: impl FnMut(&mut Self) -> T) -> Vec<T> {
        let count = self.length();
        (0..count).map(|_| sample(self)).collect()
    }

    fn case(&mut self) -> Case {
        let kind = match self.byte() {
            0 => Some(CaseKind::Roll),
            1 => Some(CaseKind::Fill),
            2 => Some(CaseKind::Condition),
            3 => Some(CaseKind::IntProvider),
            4 => Some(CaseKind::FloatProvider),
            _ => None,
        }
        .expect("a case kind of the layout");
        let name = self.text();
        let biome = self.text();
        let source = match (self.byte(), self.text()) {
            (0, _) => Some(Source::Table),
            (1, text) => Some(Source::Inline(text)),
            _ => None,
        }
        .expect("a source of the layout");
        let key_set = self.text();
        let luck = self.f32();
        let values = self.samples(Self::context_entry);
        assert_eq!(self.byte(), 0, "the layout knows only the generator 0");
        let seed = self.i64();
        let outcome = match kind {
            CaseKind::Roll => Outcome::Stacks(self.samples(Self::stack)),
            CaseKind::Fill => Outcome::Fill {
                slots: self.u32(),
                items: self.samples(|reader| (reader.u32(), reader.stack())),
            },
            CaseKind::Condition => Outcome::Tests(self.samples(|reader| reader.byte() == 1)),
            CaseKind::IntProvider => Outcome::Ints(self.samples(Self::i32)),
            CaseKind::FloatProvider => Outcome::Floats(self.samples(Self::f32)),
        };
        let after = [self.i64(), self.i64()];
        Case {
            kind,
            name,
            biome,
            source,
            key_set,
            luck,
            values,
            seed,
            outcome,
            after,
        }
    }

    fn context_entry(&mut self) -> ContextEntry {
        let key = self.text();
        let value = match self.byte() {
            0 => Some(ContextValue::Vec3(DVec3::new(
                self.f64(),
                self.f64(),
                self.f64(),
            ))),
            1 => Some(ContextValue::Stack(self.stack())),
            2 => Some(ContextValue::BlockState(self.text())),
            3 => Some(ContextValue::Float(self.f32())),
            _ => None,
        }
        .expect("a value kind of the layout");
        ContextEntry { key, value }
    }
}

static CASES: LazyLock<Vec<Case>> = LazyLock::new(|| {
    let payload = fixture("loot").dump(MAGIC);
    let mut reader = Reader(&payload);
    let count = reader.length();
    let cases = (0..count).map(|_| reader.case()).collect();
    assert!(reader.0.is_empty(), "the file ends at the last case");
    cases
});

struct Replay {
    registries: Registries,
    loot: LootTables,
    issues: Vec<LootIssue>,
    items: ItemTable,
}

enum CaseTool {
    Absent,
    Empty,
    Stack(ItemStack),
}

impl CaseTool {
    fn as_tool(&self) -> Option<Tool<'_>> {
        match self {
            CaseTool::Absent => None,
            CaseTool::Empty => Some(Tool::Empty),
            CaseTool::Stack(stack) => Some(Tool::Stack(stack)),
        }
    }
}

static REPLAY: LazyLock<Replay> = LazyLock::new(|| {
    let mut additions = test_additions();
    for (index, case) in CASES.iter().enumerate() {
        if let (CaseKind::Roll, Source::Inline(table)) = (case.kind, &case.source) {
            additions.files.insert(
                "replay",
                format!("data/replay/loot_table/case_{index}.json"),
                table.as_str(),
            );
        }
    }
    additions.embedded_packs.push("replay".to_owned());
    let (registries, _) = load_registries(&test_packs(), additions).expect("the pack loads");
    let items = test_item_table();
    let (loot, issues) = LootTables::bake(blocks(), &registries, &items);
    Replay {
        registries,
        loot,
        issues,
        items,
    }
});

struct ReplayWorld {
    biome: Option<Id<Biome>>,
}

impl LocationView for ReplayWorld {
    fn block_state(&self, _pos: BlockPos) -> Option<BlockStateId> {
        Some(BlockStateId::AIR)
    }

    fn biome(&self, _pos: BlockPos) -> Option<Id<Biome>> {
        self.biome
    }
}

impl EntityView for ReplayWorld {
    type Entity = ();
}

impl BlockEntityView for ReplayWorld {}

impl<E> ScoreView<E> for ReplayWorld {
    fn score(&self, _: ScoreHolder<'_, E>, _: &str) -> Option<i32> {
        None
    }
}

impl StorageView for ReplayWorld {
    fn storage_number(&self, _: ResourceLocation<'_>, _: &NbtPath) -> Option<NbtNumber> {
        None
    }
}

impl EnvironmentView for ReplayWorld {
    fn environment_value(
        &self,
        _: EnvironmentAttribute,
        _: Option<DVec3>,
    ) -> Option<AttributeValue> {
        None
    }
}

impl WeatherView for ReplayWorld {
    fn rain_strength(&self) -> Option<f32> {
        None
    }
}

impl ClockView for ReplayWorld {
    fn total_ticks(&self, _: Id<WorldClock>) -> Option<i64> {
        None
    }
}

fn read_key(key: &str) -> bool {
    matches!(
        key,
        "minecraft:origin"
            | "minecraft:block_state"
            | "minecraft:tool"
            | "minecraft:explosion_radius"
    )
}

fn key_set(case: &Case) -> Option<ContextKeySet> {
    ContextKeySet::ALL
        .iter()
        .copied()
        .find(|set| set.as_static_str() == case.key_set)
        .filter(|&set| set != ContextKeySet::Generic)
}

fn decode(stack: &SavedStack, replay: &Replay) -> Option<ItemStack> {
    if stack.item == "minecraft:air" && stack.count == 0 {
        return None;
    }
    let saved = parse_compound(&format!(
        "{{id:\"{}\",count:{},components:{}}}",
        stack.item, stack.count, stack.patch
    ))
    .expect("the stack is SNBT");
    let names = RegistryNames::new(&replay.registries, &replay.items);
    Some(read_saved(&saved, &names).expect("a stack of the vanilla registries"))
}

fn block_state(text: &str) -> BlockStateId {
    let (block, properties) = match text.split_once('[') {
        Some((block, rest)) => (block, rest.trim_end_matches(']')),
        None => (text, ""),
    };
    let values: Vec<(&str, &str)> = properties
        .split(',')
        .filter_map(|pair| pair.split_once('='))
        .collect();
    state(block, &values)
}

fn context<'a>(case: &Case, tool: &'a CaseTool) -> LootContext<'a, ()> {
    let key_set = key_set(case).expect("a pending case has no context");
    let mut context = LootContext::try_new(key_set).expect("a source key set");
    context.tool = tool.as_tool();
    for entry in &case.values {
        match &entry.value {
            ContextValue::Vec3(origin) => context.origin = Some(*origin),
            ContextValue::BlockState(text) => context.block_state = Some(block_state(text)),
            ContextValue::Float(radius) => context.explosion_radius = Some(*radius),
            ContextValue::Stack(_) => {}
        }
    }
    context
}

fn tool_of(case: &Case, replay: &Replay) -> CaseTool {
    let stack = case.values.iter().find_map(|entry| match &entry.value {
        ContextValue::Stack(stack) if entry.key == "minecraft:tool" => Some(stack),
        _ => None,
    });
    let Some(stack) = stack else {
        return CaseTool::Absent;
    };
    match decode(stack, replay) {
        Some(stack) => CaseTool::Stack(stack),
        None => CaseTool::Empty,
    }
}

fn source_file<T: serde::de::DeserializeOwned>(case: &Case) -> T {
    let text = match &case.source {
        Source::Inline(text) => Some(text),
        Source::Table => None,
    }
    .expect("a replayed case has an inline source");
    serde_json::from_str(text).expect("the source has the file form")
}

fn without_problems<T>(baker: &mut ConditionBaker, baked: T) -> Result<T, String> {
    let problems: Vec<_> = baker.drain_problems().collect();
    match problems.is_empty() {
        true => Ok(baked),
        false => Err(format!("bake problems {problems:?}")),
    }
}

static ENTRIES: LazyLock<NamedEntries> =
    LazyLock::new(|| NamedEntries::bake(blocks(), test_registries(), test_item_table()).0);

fn condition(case: &Case) -> Result<Condition, String> {
    let file: ConditionFile = source_file(case);
    let mut baker = ConditionBaker::new(blocks(), test_registries(), &ENTRIES);
    let condition = baker.condition(&file);
    without_problems(&mut baker, condition)
}

fn int_provider(case: &Case) -> Result<IntProvider, String> {
    let file: IntProviderFile = source_file(case);
    let mut baker = ConditionBaker::new(blocks(), test_registries(), &ENTRIES);
    let provider = baker.int_provider(&file);
    without_problems(&mut baker, provider)
}

fn float_provider(case: &Case) -> Result<FloatProvider, String> {
    let file: FloatProviderFile = source_file(case);
    let mut baker = ConditionBaker::new(blocks(), test_registries(), &ENTRIES);
    let provider = baker.float_provider(&file);
    without_problems(&mut baker, provider)
}

fn replay(index: usize, case: &Case, replay: &Replay) -> Option<String> {
    let label = format!("{} (seed {})", case.name, case.seed);
    differences(index, case, replay)
        .err()
        .map(|difference| format!("{label}: {difference}"))
}

fn find_table(replay: &Replay, name: &str) -> Result<Id<LootTable>, String> {
    let issues: Vec<&LootIssue> = (replay.issues.iter())
        .filter(|issue| issue.name.as_str() == name)
        .collect();
    if !issues.is_empty() {
        return Err(format!("bake issues {issues:?}"));
    }
    Ok(replay
        .registries
        .registry::<LootTableFile>()
        .and_then(|tables| tables.by_name(ResourceLocation::read(name).expect("a table name")))
        .expect("a table of the data pack"))
}

fn differences(index: usize, case: &Case, replay: &Replay) -> Result<(), String> {
    assert_eq!(case.luck, 0.0, "unexpected luck on {}", case.name);
    for entry in &case.values {
        assert!(read_key(&entry.key), "unexpected key {}", entry.key);
    }
    let tool = tool_of(case, replay);
    let context = context(case, &tool);
    let mut random = LegacyRandom::new(case.seed.cast_unsigned());
    let biome = ResourceLocation::read(&case.biome)
        .ok()
        .and_then(|name| replay.registries.entries::<Biome>()?.by_name(name));
    let world = ReplayWorld { biome };
    let inputs = LootInputs {
        named: &ENTRIES,
        context: &context,
        world: &world,
    };
    let mut scratch = PredicateScratch::default();
    let differs = match &case.outcome {
        Outcome::Stacks(expected) => {
            let table = find_table(replay, &format!("replay:case_{index}"))?;
            let mut stacks = Vec::new();
            replay
                .loot
                .roll(
                    table,
                    &context,
                    &world,
                    &mut random,
                    &mut LootScratch::new(),
                    &mut stacks,
                )
                .expect("the context of the case");
            let expected: Vec<Option<ItemStack>> =
                expected.iter().map(|stack| decode(stack, replay)).collect();
            (stacks != expected).then(|| format!("stacks {stacks:?}, expected {expected:?}"))
        }
        Outcome::Fill { slots, items } => {
            let name = case
                .name
                .split_once(' ')
                .map_or(case.name.as_str(), |(t, _)| t);
            let table = find_table(replay, name)?;
            let count = usize::try_from(*slots).expect("slot count");
            let (mut actual, mut expected) = (vec![None; count], vec![None; count]);
            replay
                .loot
                .fill(
                    table,
                    &context,
                    &world,
                    &mut random,
                    &mut LootScratch::new(),
                    &mut actual,
                )
                .expect("context");
            for (slot, saved) in items {
                let slot = usize::try_from(*slot).expect("slot index");
                *expected.get_mut(slot).expect("slot in range") = decode(saved, replay);
            }
            (actual != expected).then(|| format!("slots {actual:?}, expected {expected:?}"))
        }
        Outcome::Tests(expected) => {
            let condition = condition(case)?;
            let tests: Vec<bool> = expected
                .iter()
                .map(|_| condition.test(inputs, &mut random, &mut scratch))
                .collect();
            (&tests != expected).then(|| format!("tests {tests:?}, expected {expected:?}"))
        }
        Outcome::Ints(expected) => {
            let provider = int_provider(case)?;
            let ints: Vec<i32> = expected
                .iter()
                .map(|_| provider.read(inputs, &mut random, &mut scratch))
                .collect();
            (&ints != expected).then(|| format!("ints {ints:?}, expected {expected:?}"))
        }
        Outcome::Floats(expected) => {
            let provider = float_provider(case)?;
            let floats: Vec<u32> = expected
                .iter()
                .map(|_| provider.read(inputs, &mut random, &mut scratch).to_bits())
                .collect();
            let expected: Vec<u32> = expected.iter().map(|value| value.to_bits()).collect();
            (floats != expected).then(|| format!("float bits {floats:?}, expected {expected:?}"))
        }
    };
    if let Some(difference) = differs {
        return Err(difference);
    }
    let after = [random.next_java_long(), random.next_java_long()];
    match after == case.after {
        true => Ok(()),
        false => Err(format!("generator {after:?}, expected {:?}", case.after)),
    }
}

#[test]
fn the_captured_loot_cases_replay_in_exact_mode() {
    let mut differences = Vec::new();
    for (index, case) in CASES.iter().enumerate() {
        differences.extend(replay(index, case, &REPLAY));
    }
    assert_eq!(CASES.len(), REPLAYED, "expected {REPLAYED} replay cases");
    assert_eq!(differences, Vec::<String>::new());
}
