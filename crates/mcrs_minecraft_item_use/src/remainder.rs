use core::mem::{align_of, size_of};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ConsumeMode {
    #[default]
    Survival,
    Creative,
}

const _: () = assert!(
    size_of::<ConsumeMode>() == 1 && align_of::<ConsumeMode>() == 1,
    "ConsumeMode fits in one byte with 1-byte alignment"
);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RemainderOutcome<T> {
    Unchanged,
    EmptySlot,
    DecrementSource { remaining_count: u8 },
    ReplaceSlot { remainder: T },
    DecrementAndGive { remaining_count: u8, remainder: T },
}

const _: () = assert!(
    size_of::<RemainderOutcome<()>>() == 2 && align_of::<RemainderOutcome<()>>() == 1,
    "RemainderOutcome<()> packs tag and count into 2 bytes"
);

#[must_use]
pub fn compute_consume_remainder<T>(
    source_count: u8,
    mode: ConsumeMode,
    remainder: Option<T>,
) -> RemainderOutcome<T> {
    if matches!(mode, ConsumeMode::Creative) || source_count == 0 {
        return RemainderOutcome::Unchanged;
    }
    let remaining_count = source_count.saturating_sub(1);
    match (remaining_count, remainder) {
        (0, None) => RemainderOutcome::EmptySlot,
        (0, Some(remainder)) => RemainderOutcome::ReplaceSlot { remainder },
        (_, None) => RemainderOutcome::DecrementSource { remaining_count },
        (_, Some(remainder)) => RemainderOutcome::DecrementAndGive {
            remaining_count,
            remainder,
        },
    }
}
