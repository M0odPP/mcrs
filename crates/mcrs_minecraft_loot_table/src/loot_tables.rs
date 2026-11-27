use mcrs_minecraft_block::definition::{BlockDefinitions, LootId};
use mcrs_minecraft_item_stack::{Count, ItemStack};
use mcrs_minecraft_item_table::ItemTable;
use mcrs_minecraft_loot::loot::LootTable;
use mcrs_minecraft_loot_condition::{
    ContextParam, ContextParams, LootContext, LootWorld, NamedEntries, between,
};
use mcrs_minecraft_random::xoroshiro::XoroshiroRandom;
use mcrs_minecraft_random::{Random, shuffle};
use mcrs_minecraft_registries::Registries;
use mcrs_minecraft_resource::Id;

use crate::baker;
use crate::issue_owner::IssueOwner;
use crate::item_rules::ItemRules;
use crate::keyed_seed::{self, KeyedSeed};
use crate::loot_issue::{LootIssue, LootProblem};
use crate::loot_scratch::LootScratch;
use crate::loot_table_file::LootTableFile;
use crate::roll_error::RollError;
use crate::table::{Evaluation, Table};

#[derive(Debug, Clone)]
pub struct LootTables {
    tables: Box<[Table]>,
    named: usize,
    entries: NamedEntries,
    by_block: Box<[Option<Id<LootTable>>]>,
    rules: ItemRules,
}

impl LootTables {
    #[must_use]
    pub fn bake(
        definitions: &BlockDefinitions,
        registries: &Registries,
        items: &ItemTable,
    ) -> (Self, Vec<LootIssue>) {
        let mut issues = Vec::new();
        let files = registries.registry::<LootTableFile>();
        let rules = ItemRules::bake(registries, items);
        let (entries, entry_issues) = NamedEntries::bake(definitions, registries, *items);
        issues.extend(entry_issues.into_iter().map(|issue| LootIssue {
            name: issue.entry,
            owner: IssueOwner::Named(issue.kind),
            path: String::new(),
            problem: LootProblem::Condition(issue.problem),
        }));
        let tables = baker::bake(definitions, registries, &entries, &mut issues).into_boxed_slice();
        let named = files.map_or(0, |files| files.len());
        let by_block = (0..definitions.loot_table_count())
            .map(|number| {
                let name = u16::try_from(number)
                    .ok()
                    .and_then(|number| definitions.loot_table(LootId(number)))?;
                let id = files.and_then(|files| files.by_name(name.as_location()));
                if id.is_none() {
                    issues.push(LootIssue {
                        name: name.clone(),
                        owner: IssueOwner::Table,
                        path: String::new(),
                        problem: LootProblem::MissingTable,
                    });
                }
                id
            })
            .collect();
        let loot = Self {
            tables,
            named,
            entries,
            by_block,
            rules,
        };
        (loot, issues)
    }

    #[must_use]
    pub fn block_table(&self, loot: LootId) -> Option<Id<LootTable>> {
        self.by_block.get(usize::from(loot.0)).copied().flatten()
    }

    pub fn keyed_stream(&self, seed: KeyedSeed) -> Result<XoroshiroRandom, RollError> {
        let baked = self.table(seed.table)?;
        Ok(keyed_seed::stream(seed, baked.name_key))
    }

    pub fn roll<E: Copy, W: LootWorld<E>, R: Random>(
        &self,
        table: Id<LootTable>,
        context: &LootContext<'_, E>,
        world: &W,
        random: &mut R,
        scratch: &mut LootScratch,
        stacks: &mut Vec<Option<ItemStack>>,
    ) -> Result<(), RollError> {
        let _ = self.table(table)?;
        check(context)?;
        let evaluation = Evaluation {
            tables: &self.tables,
            named: &self.entries,
            context,
            world,
            rules: &self.rules,
        };
        evaluation.root(table.index(), random, scratch, stacks);
        Ok(())
    }

    pub fn fill<E: Copy, W: LootWorld<E>, R: Random>(
        &self,
        table: Id<LootTable>,
        context: &LootContext<'_, E>,
        world: &W,
        random: &mut R,
        scratch: &mut LootScratch,
        slots: &mut [Option<ItemStack>],
    ) -> Result<(), RollError> {
        let _ = self.table(table)?;
        check(context)?;
        let mut rolled = core::mem::take(&mut scratch.rolled);
        rolled.clear();
        scratch.kept.clear();
        scratch.split.clear();
        scratch.free.clear();
        Evaluation {
            tables: &self.tables,
            named: &self.entries,
            context,
            world,
            rules: &self.rules,
        }
        .root(table.index(), random, scratch, &mut rolled);
        let (kept, split, free) = (&mut scratch.kept, &mut scratch.split, &mut scratch.free);
        free.extend(
            (0..slots.len()).filter(|&index| slots.get(index).is_some_and(Option::is_none)),
        );
        shuffle(free, random);
        for stack in rolled.drain(..).flatten() {
            (if stack.count.get() > 1 {
                &mut *split
            } else {
                &mut *kept
            })
            .push(stack);
        }
        let to_count = |value: i32| u8::try_from(value).ok().and_then(Count::new);
        while free.len() > kept.len().saturating_add(split.len()) && !split.is_empty() {
            let last = i32::try_from(split.len() - 1).unwrap_or(i32::MAX);
            let Some(index) = between(random, 0, last)
                .and_then(|v| usize::try_from(v).ok())
                .filter(|&i| i < split.len())
            else {
                break;
            };
            let mut stack = split.remove(index);
            let take = between(random, 1, i32::from(stack.count.get()) / 2).unwrap_or(1);
            let rest = i32::from(stack.count.get()) - take;
            if let (Some(rest), Some(taken)) = (to_count(rest), to_count(take)) {
                let mut part = stack.clone();
                (stack.count, part.count) = (rest, taken);
                for piece in [stack, part] {
                    (if piece.count.get() > 1 && random.next_bool() {
                        &mut *split
                    } else {
                        &mut *kept
                    })
                    .push(piece);
                }
            } else {
                kept.push(stack);
            }
        }
        kept.append(split);
        shuffle(kept, random);
        for stack in kept.drain(..) {
            if let Some(target) = free.pop().and_then(|slot| slots.get_mut(slot)) {
                *target = Some(stack);
            }
        }
        free.clear();
        scratch.rolled = rolled;
        Ok(())
    }

    fn table(&self, table: Id<LootTable>) -> Result<&Table, RollError> {
        self.tables
            .get(table.index())
            .filter(|_| table.index() < self.named)
            .ok_or(RollError::UnknownTable(table))
    }
}

fn check<E>(context: &LootContext<'_, E>) -> Result<(), RollError> {
    let held = context.held();
    let required = ContextParams::required(context.key_set());
    let allowed = ContextParams::allowed(context.key_set());
    for param in ContextParam::ALL {
        if required.contains(param) && !held.contains(param) {
            return Err(RollError::MissingParam(param));
        }
        if held.contains(param) && !allowed.contains(param) {
            return Err(RollError::UnexpectedParam(param));
        }
    }
    Ok(())
}
