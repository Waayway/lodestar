//! The `flake.lock` of a channel's flake, and the Nix options every evaluation of that flake
//! runs with. A writer of a channel branch controls the flake the server evaluates (prebuilds,
//! the build gate, the updater) before any apply, so the server only evaluates it as committed
//! and only lets it reach remote sources: never a file the server itself can read.

use std::collections::{BTreeSet, HashMap};
use std::path::{Component, Path};

use anyhow::{Context, bail};
use serde_json::{Map, Value};

/// Sources a channel's flake may fetch, as Nix `allowed-uris` (whole schemes).
const ALLOWED_URIS: &str =
    "github: gitlab: sourcehut: https: git+https: tarball+https: file+https:";

/// Nix options for every command on a channel's flake: flake settings from `nixConfig` are
/// ignored, and evaluation is restricted, so flake inputs and fetchers (`builtins.fetchTree`,
/// `builtins.fetchurl`, …) can only reach `ALLOWED_URIS`, never a local path. Nothing is built
/// while evaluating: an imported derivation would be built by the Nix daemon, outside these
/// limits (a fixed-output `builtin:fetchurl` reaches any address the host reaches).
pub const RESTRICTED_EVALUATION: &[&str] = &[
    "--option",
    "accept-flake-config",
    "false",
    "--option",
    "restrict-eval",
    "true",
    "--option",
    "allowed-uris",
    ALLOWED_URIS,
    "--option",
    "allow-import-from-derivation",
    "false",
];

/// The lock as committed: Nix fails rather than lock an input itself. (Not with
/// `--no-write-lock-file`: with it Nix ignores this flag, locks missing inputs in memory and
/// evaluates them.)
pub const COMMITTED_LOCK: &str = "--no-update-lock-file";

/// How deep `same_input` follows inputs of inputs.
const MAX_DEPTH: usize = 256;

/// A parsed `flake.lock`.
pub struct FlakeLock {
    nodes: Map<String, Value>,
    root: String,
}

impl FlakeLock {
    pub fn parse(text: &str) -> anyhow::Result<Self> {
        let lock: Value = serde_json::from_str(text).context("flake.lock is not JSON")?;
        let root = lock["root"].as_str().unwrap_or("root").to_string();
        let Some(nodes) = lock["nodes"].as_object() else {
            bail!("flake.lock has no nodes");
        };
        if !nodes.get(&root).is_some_and(Value::is_object) {
            bail!("flake.lock has no root node");
        }
        if let Some(root_node) = nodes.get(&root)
            && (root_node.get("locked").is_some() || root_node.get("original").is_some())
        {
            bail!("flake.lock: the root node must not have a source");
        }
        Ok(Self {
            nodes: nodes.clone(),
            root,
        })
    }

    /// The `flake.lock` in `dir`; `None` when there is none (Nix then fails on any input,
    /// since it may not lock one). Only a regular file is read, never what a symlink names.
    pub fn read(dir: &Path) -> anyhow::Result<Option<Self>> {
        let path = dir.join("flake.lock");
        let metadata = match std::fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(error).with_context(|| format!("reading {}", path.display()));
            }
        };
        if !metadata.is_file() {
            bail!("flake.lock is not a regular file");
        }
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        Self::parse(&text).map(Some)
    }

    /// Fails unless the `flake.lock` in `dir` (if any) locks remote sources only: what must
    /// hold before Nix evaluates the flake.
    pub fn check_dir(dir: &Path) -> anyhow::Result<()> {
        match Self::read(dir)? {
            Some(lock) => lock.check_sources(),
            None => Ok(()),
        }
    }

    /// Whether the root node has a direct input `input`.
    pub fn has_input(&self, input: &str) -> bool {
        self.root_inputs().contains_key(input)
    }

    /// Fails when a node is not locked or has a source outside the remote kinds: an absolute
    /// `path`, or a `git`/`tarball`/`file` URL that is not `https://`.
    /// Relative paths stay inside the flake's own source: no `..` or absolute component.
    pub fn check_sources(&self) -> anyhow::Result<()> {
        for (name, node) in &self.nodes {
            if *name == self.root {
                continue;
            }
            if let Some(inputs) = node.get("inputs").and_then(Value::as_object)
                && inputs.values().any(|reference| {
                    reference
                        .as_str()
                        .is_some_and(|reference| reference == self.root)
                })
            {
                bail!(
                    "flake.lock: input {name:?} refers to the root node {:?}",
                    self.root
                );
            }
            let Some(locked) = node.get("locked") else {
                bail!("flake.lock: input {name:?} is not locked");
            };
            check_source(name, locked, false)?;
            if let Some(original) = node.get("original") {
                check_source(name, original, true)?;
            }
        }
        Ok(())
    }

    /// Fails unless `updated` (this lock after `nix flake update <input>`) differs from this
    /// lock in `input` only: the same root inputs, every other one locked as before.
    pub fn check_update(&self, updated: &FlakeLock, input: &str) -> anyhow::Result<()> {
        let before = self.root_inputs();
        let after = updated.root_inputs();
        let names: BTreeSet<&String> = before.keys().collect();
        if names != after.keys().collect::<BTreeSet<_>>() {
            bail!("the update of {input:?} changed which inputs flake.lock has");
        }
        let mut memo = HashMap::new();
        for (name, reference) in &before {
            if name != input && !same_input(self, updated, reference, &after[name], &mut memo, 0)? {
                bail!("the update of {input:?} also changed input {name:?}");
            }
        }
        Ok(())
    }

    fn root_inputs(&self) -> Map<String, Value> {
        self.nodes[&self.root]
            .get("inputs")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default()
    }

    fn node(&self, name: &str) -> anyhow::Result<&Map<String, Value>> {
        self.nodes
            .get(name)
            .and_then(Value::as_object)
            .with_context(|| format!("flake.lock: no node {name:?}"))
    }
}

/// Whether input reference `a` of `old` and `b` of `new` (a node name, or a `follows` path)
/// lock the same thing, inputs of inputs included. Node names may differ; `memo` holds the
/// pairs of node names already compared (or being compared).
fn same_input(
    old: &FlakeLock,
    new: &FlakeLock,
    a: &Value,
    b: &Value,
    memo: &mut HashMap<(String, String), bool>,
    depth: usize,
) -> anyhow::Result<bool> {
    if depth > MAX_DEPTH {
        bail!("flake.lock: inputs nest deeper than {MAX_DEPTH}");
    }
    let (Some(a), Some(b)) = (a.as_str(), b.as_str()) else {
        // `follows` paths (or anything else) compare as written.
        return Ok(a == b);
    };
    let key = (a.to_string(), b.to_string());
    if let Some(same) = memo.get(&key) {
        return Ok(*same);
    }
    memo.insert(key.clone(), true);
    let (node_a, node_b) = (old.node(a)?, new.node(b)?);
    let without_inputs = |node: &Map<String, Value>| {
        let mut node = node.clone();
        node.remove("inputs");
        node
    };
    let empty = Map::new();
    let inputs = |node: &'_ Map<String, Value>| -> Map<String, Value> {
        node.get("inputs")
            .and_then(Value::as_object)
            .unwrap_or(&empty)
            .clone()
    };
    let (inputs_a, inputs_b) = (inputs(node_a), inputs(node_b));
    let mut same = without_inputs(node_a) == without_inputs(node_b)
        && inputs_a.keys().collect::<BTreeSet<_>>() == inputs_b.keys().collect::<BTreeSet<_>>();
    if same {
        for (name, reference) in &inputs_a {
            if !same_input(old, new, reference, &inputs_b[name], memo, depth + 1)? {
                same = false;
                break;
            }
        }
    }
    memo.insert(key, same);
    Ok(same)
}

/// Checks one `locked` (or `original`) source of node `name`.
fn check_source(name: &str, source: &Value, original: bool) -> anyhow::Result<()> {
    let kind = source["type"].as_str().unwrap_or_default();
    let remote_url = |schemes: &[&str]| {
        source["url"]
            .as_str()
            .is_some_and(|url| schemes.iter().any(|scheme| url.starts_with(scheme)))
    };
    let allowed = match kind {
        "github" | "gitlab" | "sourcehut" => true,
        "git" => remote_url(&["https://"]),
        "tarball" | "file" => remote_url(&["https://"]),
        "path" => source["path"].as_str().is_some_and(|path| {
            !path.starts_with('/')
                && !Path::new(path)
                    .components()
                    .any(|component| matches!(component, Component::ParentDir | Component::RootDir))
        }),
        // The registry name a lock was made from; what is fetched is `locked`.
        "indirect" => original,
        _ => false,
    };
    if allowed {
        Ok(())
    } else {
        bail!("flake.lock: input {name:?} has a {kind:?} source that is not remote")
    }
}
