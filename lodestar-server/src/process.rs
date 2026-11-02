//! Running a child process with captured output, optional stdin and a timeout; shared by the
//! updater's Nix runner and the remote builder.

use std::process::{Output, Stdio};
use std::time::Duration;

use tokio::io::AsyncWriteExt;
use tokio::process::Command;

const LOG_TAIL_LINES: usize = 40;

/// Why a child produced no exit status.
#[derive(Debug)]
pub enum RunError {
    Start(std::io::Error),
    Wait(std::io::Error),
    Timeout,
}

/// The last 40 lines of `text`.
pub fn log_tail(text: &str) -> String {
    let lines: Vec<&str> = text.trim_end().lines().collect();
    lines[lines.len().saturating_sub(LOG_TAIL_LINES)..].join("\n")
}

/// Runs `command` with stdout and stderr captured and `stdin` (or nothing) as its input. The
/// child is killed when the timeout passes or the future is dropped.
pub async fn run(
    command: &mut Command,
    stdin: Option<&[u8]>,
    timeout: Duration,
) -> Result<Output, RunError> {
    let mut child = command
        .stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(RunError::Start)?;
    let input = child.stdin.take();
    let feed = async move {
        if let (Some(mut pipe), Some(bytes)) = (input, stdin) {
            // A child that exits without reading its input is not an error here; its exit
            // status says what happened.
            let _ = pipe.write_all(bytes).await;
        }
    };
    let finished = async {
        let (_, output) = tokio::join!(feed, child.wait_with_output());
        output
    };
    match tokio::time::timeout(timeout, finished).await {
        Ok(Ok(output)) => Ok(output),
        Ok(Err(error)) => Err(RunError::Wait(error)),
        Err(_) => Err(RunError::Timeout),
    }
}
