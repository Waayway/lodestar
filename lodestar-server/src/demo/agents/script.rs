/// How a simulated host's applies end.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Script {
    /// Every apply succeeds.
    Succeed,
    /// The checkout has tracked changes: every apply is skipped (`dirty`).
    Dirty,
    /// The first apply fails in `nix build`; later ones succeed.
    FailFirst,
}
