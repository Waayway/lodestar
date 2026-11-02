//! The last recorded build of one channel's head, as the metrics report it.

/// What `lodestar_build_*` reports for a channel.
#[derive(Clone, Debug, PartialEq)]
pub struct BuildMetric {
    /// `ok` or `failed`.
    pub result: String,
    /// When the newest host result was recorded (unix seconds).
    pub finished_at: i64,
    /// How long the last build the server ran for the channel took; none before the first one
    /// since the server started.
    pub duration_seconds: Option<f64>,
}
