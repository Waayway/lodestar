use std::fs::File;
use std::io::Write;
use std::process::{Command, ExitStatus, Stdio};

use super::outcome::Outcome;

/// The apply's log file, `applies/<id>.log`, that every step appends to.
pub(super) struct ApplyLog(pub(super) File);

impl ApplyLog {
    pub(super) fn line(&mut self, text: &str) {
        let _ = writeln!(self.0, "==> {text}");
    }

    pub(super) fn stdio(&self) -> anyhow::Result<(Stdio, Stdio)> {
        Ok((
            Stdio::from(self.0.try_clone()?),
            Stdio::from(self.0.try_clone()?),
        ))
    }

    /// Runs `command` with stdout and stderr appended to the log.
    pub(super) fn run(&mut self, what: &str, command: &mut Command) -> Result<(), Outcome> {
        self.line(what);
        let fail = |message: String| Outcome::Failed(None, message);
        let (stdout, stderr) = self.stdio().map_err(|error| fail(error.to_string()))?;
        let status = command
            .stdin(Stdio::null())
            .stdout(stdout)
            .stderr(stderr)
            .status()
            .map_err(|error| fail(format!("{what}: {error}")))?;
        self.finish(what, status)
    }

    pub(super) fn finish(&mut self, what: &str, status: ExitStatus) -> Result<(), Outcome> {
        if status.success() {
            return Ok(());
        }
        let message = format!("{what} failed ({status})");
        self.line(&message);
        Err(Outcome::Failed(status.code(), message))
    }
}
