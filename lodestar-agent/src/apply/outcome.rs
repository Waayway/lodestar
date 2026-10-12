use lodestar_proto::SkipReason;

/// How an apply ended, before it is written into the record.
pub(super) enum Outcome {
    Ok,
    Skipped(SkipReason, String),
    Failed(Option<i32>, String),
}
