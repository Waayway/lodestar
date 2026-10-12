//! Where the server is: `--server`, `LODESTAR_SERVER`, or the config file.

use anyhow::{Result, bail};

/// The config file the CLI reads when neither `--server` nor `LODESTAR_SERVER` is given.
pub fn config_path() -> Option<std::path::PathBuf> {
    let base = match std::env::var_os("XDG_CONFIG_HOME").filter(|value| !value.is_empty()) {
        Some(dir) => std::path::PathBuf::from(dir),
        None => std::path::PathBuf::from(std::env::var_os("HOME")?).join(".config"),
    };
    Some(base.join("lodestar").join("config.toml"))
}

/// Reads `server = "..."` from the CLI config file's text. Only top-level `key = "string"`
/// lines are understood; anything else (comments, other keys) is ignored.
pub fn server_from_config(text: &str) -> Option<String> {
    let mut in_table = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_table = true;
            continue;
        }
        if in_table || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if key.trim() != "server" {
            continue;
        }
        let value = value.trim();
        let value = value
            .strip_prefix('"')
            .and_then(|rest| rest.split_once('"'))
            .map(|(quoted, _)| quoted)
            .or_else(|| {
                value
                    .strip_prefix('\'')
                    .and_then(|rest| rest.split_once('\''))
                    .map(|(quoted, _)| quoted)
            })?;
        return Some(value.to_string()).filter(|server| !server.is_empty());
    }
    None
}

/// The server URL: `--server` / `LODESTAR_SERVER`, else the config file.
pub fn resolve_server(flag: Option<String>) -> Result<String> {
    if let Some(server) = flag.filter(|server| !server.is_empty()) {
        return Ok(server);
    }
    let path = config_path();
    if let Some(path) = &path
        && let Ok(text) = std::fs::read_to_string(path)
        && let Some(server) = server_from_config(&text)
    {
        return Ok(server);
    }
    bail!(
        "no server given: pass --server <url>, set LODESTAR_SERVER, or put `server = \"http://<host>:8080\"` in {}",
        path.map_or("~/.config/lodestar/config.toml".into(), |path| path
            .display()
            .to_string())
    )
}
