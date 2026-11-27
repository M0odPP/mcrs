use mcrs_minecraft_item_stack::{BundleContents, ChargedProjectiles, Container, ItemStack};
use mcrs_minecraft_loot_condition::{ContextParam, LootWorld};
use mcrs_minecraft_predicate::{BakedItemPredicate, ItemSubject, PredicateScratch, SlotRange};
use mcrs_minecraft_random::Random;

use crate::loot_scratch::LootScratch;
use crate::modifier_file::ContentsComponent;
use crate::table::{Evaluation, Place, Span};
use crate::work_stack::WorkStack;

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum SlotSourceNode {
    Empty,
    Named(u32),
    SlotRange {
        slots: SlotRange,
        source: ContextParam,
    },
    Group(Span),
    Filtered {
        child: u32,
        filter: Box<BakedItemPredicate>,
    },
    LimitSlots {
        child: u32,
        limit: u32,
    },
    Contents {
        child: u32,
        component: ContentsComponent,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SlotWrapper {
    Filter { table: usize, node: u32 },
    Limit { remaining: u32 },
    Contents(ContentsComponent),
}

struct SlotEvaluation<'a, 'e, 'c, E, W, R> {
    evaluation: &'a Evaluation<'e, 'c, E, W>,
    place: &'a Place<'a>,
    random: &'a mut R,
    predicates: &'a mut PredicateScratch,
    stacks: &'a mut Vec<Option<ItemStack>>,
}

pub(crate) fn emit<'e, 'c, E: Copy, W: LootWorld<E>, R: Random>(
    evaluation: &Evaluation<'e, 'c, E, W>,
    place: &Place<'_>,
    node: u32,
    random: &mut R,
    scratch: &mut LootScratch,
    stacks: &mut Vec<Option<ItemStack>>,
) {
    scratch.slot_wrappers.clear();
    let mut wrappers = core::mem::take(&mut scratch.slot_wrappers);
    let mut runner = SlotEvaluation {
        evaluation,
        place,
        random,
        predicates: &mut scratch.predicates,
        stacks,
    };
    let _ = runner.evaluate_node(place.table_index, node, &mut wrappers);
    wrappers.clear();
    scratch.slot_wrappers = wrappers;
}

impl<'e, E: Copy, W: LootWorld<E>, R: Random> SlotEvaluation<'_, 'e, '_, E, W, R> {
    fn evaluate_node(
        &mut self,
        table_index: usize,
        node_index: u32,
        wrappers: &mut Vec<SlotWrapper>,
    ) -> Option<usize> {
        let evaluation = self.evaluation;
        let node = evaluation
            .tables
            .get(table_index)?
            .slot_source(node_index)?;
        match node {
            SlotSourceNode::Empty => None,
            SlotSourceNode::Named(slot) => {
                let target =
                    usize::try_from(evaluation.tables.get(table_index)?.nested(*slot)?).ok()?;
                self.evaluate_node(target, 0, wrappers)
            }
            SlotSourceNode::SlotRange { slots, source } => slots.slots().find_map(|slot| {
                let subject = evaluation.context.slot(*source, evaluation.world, slot)?;
                self.forward(wrappers, subject)
            }),
            SlotSourceNode::Group(children) => children
                .indices()
                .find_map(|child| self.evaluate_node(table_index, child, wrappers)),
            SlotSourceNode::Filtered { child, .. } => {
                wrappers.push(SlotWrapper::Filter {
                    table: table_index,
                    node: node_index,
                });
                let stop = self.evaluate_node(table_index, *child, wrappers);
                wrappers.pop();
                stop
            }
            SlotSourceNode::LimitSlots { child, limit } if *limit > 0 => {
                let my_index = wrappers.len();
                wrappers.push(SlotWrapper::Limit { remaining: *limit });
                let stop = self.evaluate_node(table_index, *child, wrappers);
                wrappers.pop();
                stop.filter(|&index| index != my_index)
            }
            SlotSourceNode::LimitSlots { .. } => None,
            SlotSourceNode::Contents { child, component } => {
                wrappers.push(SlotWrapper::Contents(*component));
                let stop = self.evaluate_node(table_index, *child, wrappers);
                wrappers.pop();
                stop
            }
        }
    }

    fn forward(&mut self, wrappers: &mut [SlotWrapper], subject: ItemSubject<'e>) -> Option<usize> {
        let evaluation = self.evaluation;
        let Some((wrapper, outer)) = wrappers.split_last_mut() else {
            if let ItemSubject::Stack(stack) = subject
                && Some(stack.kind) != evaluation.rules.air
            {
                let work = WorkStack::copy(Some(stack));
                evaluation.emit_stack(self.place, work, self.random, self.predicates, self.stacks);
            }
            return None;
        };
        let my_index = outer.len();
        match wrapper {
            SlotWrapper::Filter { table, node } => {
                let Some(SlotSourceNode::Filtered { filter, .. }) = evaluation
                    .tables
                    .get(*table)
                    .and_then(|table| table.slot_source(*node))
                else {
                    return None;
                };
                filter
                    .matches(subject, &evaluation.rules.items)
                    .then(|| self.forward(outer, subject))
                    .flatten()
            }
            SlotWrapper::Limit { remaining } => {
                if *remaining == 0 {
                    return Some(my_index);
                }
                *remaining -= 1;
                self.forward(outer, subject)
                    .or_else(|| (*remaining == 0).then_some(my_index))
            }
            SlotWrapper::Contents(component) => match subject {
                ItemSubject::Stack(stack) => self.forward_contents(outer, stack, *component),
                ItemSubject::Empty => None,
            },
        }
    }

    fn forward_contents(
        &mut self,
        outer: &mut [SlotWrapper],
        stack: &'e ItemStack,
        component: ContentsComponent,
    ) -> Option<usize> {
        let items = &self.evaluation.rules.items;
        let slice = match component {
            ContentsComponent::Container => {
                return items
                    .effective::<Container>(stack)?
                    .slots()
                    .iter()
                    .find_map(|slot| {
                        let subject = slot.as_ref().map_or(ItemSubject::Empty, ItemSubject::Stack);
                        self.forward(outer, subject)
                    });
            }
            ContentsComponent::BundleContents => &items.effective::<BundleContents>(stack)?.0,
            ContentsComponent::ChargedProjectiles => {
                &items.effective::<ChargedProjectiles>(stack)?.0
            }
        };
        slice
            .iter()
            .find_map(|item| self.forward(outer, ItemSubject::Stack(item)))
    }
}
