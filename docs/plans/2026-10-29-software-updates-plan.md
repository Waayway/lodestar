# Software Updates Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Lodestar bumps flake inputs when tracked software (first: Tailscale) has a newer version, pushes the build-checked bump to `unstable`, and exports per-host running/latest/up-to-date metrics that a Grafana dashboard and alerts use.

**Architecture:** Hosts bake their tracked versions into `/etc/lodestar/versions.json` (agent module option); the agent returns them in `AgentStatus`; the server keeps them per host, checks GitHub releases for the latest version, runs a daily updater (worktree of the mirror → `nix flake update <input>` → compare versions → build → fast-forward push with the deploy key) and serves `/metrics` on a separate LAN listener that monitor's Prometheus scrapes.

**Tech Stack:** Rust (axum, tokio, sea-orm, serde, ts-rs, reqwest as already used by `agent_client.rs`), Preact + vitest, NixOS/nix-darwin modules, Grafana provisioning JSON, 1Password CLI, `gh`.

**Spec:** `docs/plans/2026-10-29-software-updates-design.md` (this repo). Tasks marked **[flake]** happen in `~/.flake`; the rest in `~/lodestar`.

## Global Constraints

- "Up to date" compares with the latest upstream GitHub release that is neither draft nor prerelease; tag `v1.104.2` → `1.104.2`.
- Version compare: strip leading `v`, cut at first `-` or `+`, compare dot parts numerically, missing part = 0; a non-numeric part → incomparable → "unknown", never "up to date".
- The updater only moves inputs; it never overrides package sources. It pushes to `updates.channel` (default `unstable`) only, fast-forward only, never to `stable`.
- No commit when no tracked version got newer on any host (lock churn alone never reaches the repo).
- Commit author `Lodestar <lodestar@vanwaaij.com>`, only `flake.lock`, message `chore(<input>): bump for <software> <old> -> <new>[, <software> <old> -> <new>…]`.
- Defaults: `intervalSecs = 86400`, `upstreamIntervalSecs = 21600`, metrics port `9466`, `software = { }` turns the updater off.
- Metric names and labels exactly as spec section 6.
- Every git/nix argument from config is validated before use (repo rule): input names `^[A-Za-z0-9_-]+$`, GitHub `^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$`, software names `^[a-z0-9_-]+$`, channel via the existing `valid_channel`.
- Repo conventions: `cargo fmt`, clippy `-D warnings`, errors with `anyhow`, tests in `<crate>/tests/`, conventional commits, `nix fmt` for Nix. `~/.flake` rules from its `AGENTS.md` (thin host files, `lib.mkMerge`, commit only own paths, commits as `thijs@vanwaaij.com`).
- Check commands: `nix develop -c cargo test --workspace`, `nix develop -c cargo clippy --workspace --all-targets -- -D warnings`, `nix develop -c cargo fmt --check`, `cd web && nix develop .. -c npx vitest run`.

## Review Focus

1. A `versions.json` that is missing, empty or not a JSON object of strings: the agent still answers `/status`, with `versions: {}` (test in Task 3).
2. A host stops tracking a software (key removed): its `running_info`/`up_to_date` series disappear instead of keeping the old value — the stored map is replaced, never merged (test in Task 3).
3. `unstable` moves while the updater builds: result `push-failed` ("channel moved"), nothing force-pushed, next run retries (test in Task 8).
4. GitHub answers 403 (rate limit), 500, or a list of only prereleases: the last known latest version stays, the error is recorded and `checked_at` does not advance (test in Task 6).
5. "Check now" while a run is in progress: `409` "already running", the running run is not disturbed (test in Task 9).

---

### Task 1 [flake]: Deploy key and branch protection

**Files:**
- Modify: `~/.flake/modules/server/lodestar/server.nix` (opnix secret + `deployKeyFile`, inside the existing `mkIf`)
- Modify: `~/.flake/docs/lodestar.md` (open items 1 and 2)

**Interfaces:**
- Produces: `services.lodestar-server.deployKeyFile = config.services.onepassword-secrets.secretPaths.lodestarDeployKey` on nixos-manager; 1Password item `lodestar-deploy-key` (Homelab, category SSH Key).

- [ ] **Step 1: Generate the key in 1Password** — `op --account KVKXJLIAFVBK5GYLWVS37CJCTE item create --category ssh --ssh-generate-key ed25519 --vault Homelab --title lodestar-deploy-key`. Expected: item created; nothing written to disk.
- [ ] **Step 2: Register it on GitHub** — `op read "op://Homelab/lodestar-deploy-key/public key"` piped into `GH_TOKEN=$(gh auth token -u Waayway) gh api repos/Waayway/nixos-config/keys -f title=lodestar-server -f key=@- -F read_only=false` (or `-f key="$(…)"`). Expected: JSON with `"read_only": false`.
- [ ] **Step 3: Rulesets** — `gh api repos/Waayway/nixos-config/rulesets` POST one ruleset `protect-channels`, target branch, include `refs/heads/stable` and `refs/heads/unstable`, rules `non_fast_forward` and `deletion`, enforcement `active`, no bypass actors. Expected: `gh api repos/Waayway/nixos-config/rulesets` lists it.
- [ ] **Step 4: Nix** — in the `mkIf`: `services.onepassword-secrets.secrets.lodestarDeployKey = { reference = "op://Homelab/lodestar-deploy-key/private key?ssh-format=openssh"; owner = "lodestar-server"; mode = "0400"; services = [ "lodestar-server" ]; };` and `services.lodestar-server.deployKeyFile = config.services.onepassword-secrets.secretPaths.lodestarDeployKey;`. Confirm the Lodestar server module pins GitHub's SSH host key for the push (`rg -n known_hosts ~/lodestar/nix/modules/server.nix ~/lodestar/lodestar-server/src/mirror`); if it does not, set `programs.ssh.knownHosts."github.com".publicKey` to GitHub's published ed25519 key in the same file.
- [ ] **Step 5: Verify eval** — `nix eval --raw .#nixosConfigurations.nixos-manager.config.services.lodestar-server.deployKeyFile`. Expected: `/var/lib/opnix/secrets/lodestarDeployKey` (or opnix's path).
- [ ] **Step 6: Docs** — mark open items 1 and 2 done in `docs/lodestar.md` (item 2: nixos-manager re-tagged `tag:nixos,tag:manager` 2026-10-27).
- [ ] **Step 7: Commit** — `git commit -m "feat(lodestar): deploy key for promote" -- modules/server/lodestar/server.nix docs/lodestar.md`.
- [ ] **Step 8: Roll out and prove** (ask before pushing) — push `unstable`, fast-forward `stable` by hand one last time, `lodestar rebuild nixos-manager --wait`, then make a no-op test: `lodestar promote` shows "up to date". The first real promote after this is the proof; record it in the task notes.

### Task 2: Version comparison

**Files:**
- Create: `lodestar-server/src/updates/mod.rs`, `lodestar-server/src/updates/version.rs`
- Modify: `lodestar-server/src/lib.rs` (`pub mod updates;`)
- Test: `lodestar-server/tests/version.rs`

**Interfaces:**
- Produces: `pub fn parse_version(s: &str) -> Option<Vec<u64>>`; `pub fn compare_versions(a: &str, b: &str) -> Option<std::cmp::Ordering>`; `pub fn tag_to_version(tag: &str) -> String` (strips one leading `v`).

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn equal_after_suffix_cut() { assert_eq!(compare_versions("1.104.2-t7f4efe814-g4939ea2e4", "1.104.2"), Some(Equal)); }
#[test] fn numeric_not_lexical() { assert_eq!(compare_versions("1.98.10", "1.104.2"), Some(Less)); }
#[test] fn missing_part_is_zero() { assert_eq!(compare_versions("1.104", "1.104.0"), Some(Equal)); }
#[test] fn leading_v() { assert_eq!(compare_versions("v1.104.2", "1.104.2"), Some(Equal)); }
#[test] fn plus_suffix() { assert_eq!(compare_versions("2.0.0+build5", "2.0.0"), Some(Equal)); }
#[test] fn non_numeric_is_incomparable() { assert_eq!(compare_versions("1.104.x", "1.104.2"), None); assert_eq!(compare_versions("", "1"), None); }
#[test] fn tag() { assert_eq!(tag_to_version("v1.104.2"), "1.104.2"); assert_eq!(tag_to_version("1.2"), "1.2"); }
```

- [ ] **Step 2: Run, expect FAIL** — `nix develop -c cargo test -p lodestar-server --test version` → unresolved import.
- [ ] **Step 3: Implement** in `updates/version.rs` per Global Constraints.
- [ ] **Step 4: Run, expect PASS** — same command.
- [ ] **Step 5: Commit** — `feat(server): version comparison for tracked software`.

### Task 3: Agent reports versions; server keeps them per host

**Files:**
- Modify: `lodestar-proto/src/agent/agent_status.rs` (`#[serde(default)] pub versions: BTreeMap<String, String>`)
- Modify: `lodestar-proto/src/dashboard/host_view.rs` (`pub versions: BTreeMap<String, String>`)
- Modify: `lodestar-agent/src/config/agent_config.rs` (`#[serde(default = "default_versions_file")] pub versions_file: PathBuf`, default `/etc/lodestar/versions.json`)
- Create: `lodestar-agent/src/versions.rs`; Modify: `lodestar-agent/src/status.rs` (`collect` fills `versions`), `lodestar-agent/src/lib.rs`
- Modify: `lodestar-server/src/state/app_state.rs` (`pub(crate) versions: Arc<Mutex<HashMap<String, BTreeMap<String, String>>>>`), `lodestar-server/src/poller/host_poll.rs` (`handle_status` replaces the host's entry), `lodestar-server/src/state/host_view.rs` (fills `HostView.versions`)
- Regenerate: `web/src/bindings/*` (ts-rs command from AGENTS.md); fix demo agents (`lodestar-server/src/demo/agents/*`) to send `versions: {"tailscale": "1.104.2"}` for one demo host and an older one for another.
- Test: `lodestar-agent/tests/versions.rs`, `lodestar-server/tests/poller.rs`, `lodestar-proto` serde test in its existing tests

**Interfaces:**
- Produces: `pub fn read_versions(path: &Path) -> BTreeMap<String, String>` (agent; never fails); `AppState.versions` (server, keyed by host name); `AgentStatus.versions`, `HostView.versions`.

- [ ] **Step 1: Failing tests**
  - agent `read_versions`: file `{"tailscale":"1.104.2"}` → one entry; missing file → empty; `[]`, `{"a":1}`, `not json`, empty file → empty (Review Focus 1).
  - proto: `AgentStatus` JSON without `versions` deserialises with an empty map (old agents).
  - poller (`tests/poller.rs`, existing fake agent pattern): status with `{"tailscale":"1.98.10"}` then with `{}` → `host_view(...).versions` is first one entry, then empty (Review Focus 2).
- [ ] **Step 2: Run, expect FAIL** — `nix develop -c cargo test --workspace` (compile errors on the new field).
- [ ] **Step 3: Implement** — agent reads the file on every `collect` call; server `handle_status` does `versions.insert(host, status.versions.clone())` (replace).
- [ ] **Step 4: Regenerate bindings, run all checks** — ts-rs export, `cargo test --workspace`, clippy, fmt, vitest. Expected: all pass.
- [ ] **Step 5: Commit** — `feat(agent): report tracked software versions`.

### Task 4: Nix modules — agent `versions`, server `updates` and `metrics`

**Files:**
- Modify: `nix/modules/agent/common.nix` (option `versions`, `environment.etc."lodestar/versions.json"`, `versions_file` in the agent JSON config)
- Modify: `nix/modules/server.nix` (options `updates.{channel,intervalSecs,upstreamIntervalSecs,software}`, `metrics.{address,port}`; into `config.json` as `updates` and `metrics`; firewall: `metrics.port` is **not** opened by the module — the caller opens it, the flake's monitoring agent does)
- Test: `nix/tests/agent-module.nix` (darwin eval), `nix flake check`

**Interfaces:**
- Produces (Nix):
  - `services.lodestar-agent.versions : attrsOf str`, default `{ }`.
  - `services.lodestar-server.updates.software.<name> = { input : str; upstream.github : str; }`; `updates.channel : str = "unstable"`; `updates.intervalSecs : int = 86400`; `updates.upstreamIntervalSecs : int = 21600`.
  - `services.lodestar-server.metrics = { address : nullOr str = null; port : port = 9466; }` (null address = no metrics listener).
- Produces (JSON, consumed by Task 5): `"updates": {"channel","interval_secs","upstream_interval_secs","software": {"<name>": {"input","upstream": {"github": "owner/repo"}}}}`, `"metrics": {"listen": "<addr>:<port>"}` or absent.

- [ ] **Step 1: Failing check** — extend `nix/tests/agent-module.nix` to set `versions.tailscale = "1.104.2"` and assert `environment.etc."lodestar/versions.json".text == builtins.toJSON { tailscale = "1.104.2"; }`; add the same assertion for NixOS in the VM test's node config evaluation (`nix eval .#checks.x86_64-linux.vm.nodes...` is too slow on darwin; an `assertions` entry in a small eval test is enough).
- [ ] **Step 2: Run, expect FAIL** — `nix build .#checks.aarch64-darwin.agent-module` (name as in `flake.nix`).
- [ ] **Step 3: Implement** options and JSON; validate names in Nix with `assertions` (software names `^[a-z0-9_-]+$`, inputs `^[A-Za-z0-9_-]+$`, github `owner/repo` regex from Global Constraints).
- [ ] **Step 4: Run, expect PASS**; `nix fmt`; `nix flake check --no-build`.
- [ ] **Step 5: Commit** — `feat(nix): versions, updates and metrics options`.

### Task 5: Server config for updates and metrics

**Files:**
- Create: `lodestar-server/src/config/updates_config.rs`
- Modify: `lodestar-server/src/config/server_config.rs` (`#[serde(default)] pub updates: UpdatesConfig`, `#[serde(default)] pub metrics: Option<MetricsConfig>`, `#[serde(default = "default_nix_bin")] pub nix_bin: PathBuf` = `"nix"`), `config/mod.rs`
- Test: `lodestar-server/tests/config.rs` (create if no config test file exists; otherwise the existing one)

**Interfaces:**
- Produces:
  ```rust
  pub struct UpdatesConfig { pub channel: String, pub interval_secs: u64, pub upstream_interval_secs: u64, pub software: BTreeMap<String, SoftwareConfig> }
  pub struct SoftwareConfig { pub input: String, pub upstream: Upstream }
  pub enum Upstream { Github(String) }   // serde: {"github": "owner/repo"}
  pub struct MetricsConfig { pub listen: SocketAddr }
  impl UpdatesConfig { pub fn check(&self) -> anyhow::Result<()>; pub fn inputs(&self) -> BTreeMap<String, Vec<String>> /* input -> software names */ }
  ```
  `Default` for `UpdatesConfig`: channel `unstable`, 86400, 21600, empty software. `ServerConfig::check_updates()` called in `server::start` next to `check_listen`.

- [ ] **Step 1: Failing tests** — JSON from Task 4's shape parses; absent `updates` → default with empty software; `check()` rejects input `"nixpkgs unstable"`, github `"tailscale"`, software `"Tail scale"`, channel `"-x"`; `inputs()` groups two software on one input.
- [ ] **Step 2: Run, expect FAIL.**
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run, expect PASS.**
- [ ] **Step 5: Commit** — `feat(server): updates and metrics config`.

### Task 6: Upstream release check

**Files:**
- Create: `lodestar-server/src/updates/upstream.rs`, `lodestar-server/src/updates/upstream_loop.rs`
- Modify: `lodestar-server/src/state/app_state.rs` (`pub(crate) latest: Arc<Mutex<HashMap<String, LatestVersion>>>`, `pub(crate) upstream: Arc<dyn UpstreamProvider>`), `lodestar-server/src/poller/mod.rs` (`spawn` also starts the upstream loop when `software` is non-empty)
- Test: `lodestar-server/tests/upstream.rs`

**Interfaces:**
- Produces:
  ```rust
  pub struct LatestVersion { pub version: Option<String>, pub checked_at: Option<i64>, pub error: Option<String> }
  pub fn latest_release(json: &str) -> anyhow::Result<Option<String>>   // first non-draft, non-prerelease tag, via tag_to_version
  #[async_trait] pub trait UpstreamProvider: Send + Sync { async fn releases(&self, repo: &str) -> anyhow::Result<String>; }
  pub struct GithubProvider;   // GET https://api.github.com/repos/{repo}/releases?per_page=20, User-Agent "lodestar", Accept application/vnd.github+json
  pub async fn check_once(state: &AppState)   // updates every software's LatestVersion
  ```
  On success: `version = Some(v)`, `checked_at = now`, `error = None`. On error or no stable release: keep `version` and `checked_at`, set `error`.

- [ ] **Step 1: Failing tests** — `latest_release` on a fixture list `[draft v2.0.0, prerelease v1.105.0-pre, v1.104.2, v1.104.1]` → `Some("1.104.2")`; only prereleases → `None`; invalid JSON → `Err`. `check_once` with a fake provider: success sets version/checked_at; then the provider returns `Err("403 rate limit")` → version and checked_at unchanged, error set (Review Focus 4); then only prereleases → same as error.
- [ ] **Step 2: Run, expect FAIL.**
- [ ] **Step 3: Implement**; the loop sleeps `upstream_interval_secs` between rounds, first round at start.
- [ ] **Step 4: Run, expect PASS.**
- [ ] **Step 5: Commit** — `feat(server): check upstream releases`.

### Task 7: `/metrics` listener

**Files:**
- Create: `lodestar-server/src/updates/metrics.rs`, `lodestar-server/src/api/metrics.rs`
- Modify: `lodestar-server/src/api/app.rs` (`pub fn metrics_app(state: AppState) -> Router` with only `GET /metrics`), `lodestar-server/src/server.rs` (bind `config.metrics.listen` when set, `spawn_serve(..., api::metrics_app(...))`, log `lodestar-server: metrics on {addr}`; `ServerHandle.metrics_addr: Option<SocketAddr>`)
- Test: `lodestar-server/tests/metrics.rs`

**Interfaces:**
- Consumes: `AppState.versions` (Task 3), `AppState.latest` (Task 6), `compare_versions` (Task 2).
- Produces: `pub fn render(input: &MetricsInput) -> String` where `MetricsInput { running: BTreeMap<String /*host*/, BTreeMap<String, String>>, latest: BTreeMap<String, LatestVersion> }` — pure, sorted output, label values escaped (`\`, `"`, newline). Task 9 adds `last_runs: BTreeMap<String /*input*/, UpdateRun>` and the `lodestar_update_*` metrics.

- [ ] **Step 1: Failing tests** — hosts `a: tailscale 1.104.2`, `b: tailscale 1.98.10`, `c: tailscale 1.x`, latest `1.104.2` → output contains `lodestar_software_running_info{host="a",software="tailscale",version="1.104.2"} 1`, `lodestar_software_up_to_date{host="a",software="tailscale"} 1`, `…{host="b",…} 0`, no `up_to_date` line for `c`, `lodestar_software_latest_info{software="tailscale",version="1.104.2"} 1`, `lodestar_software_latest_checked_timestamp_seconds{software="tailscale"} <t>`; a software reported by hosts but with `latest.version == None` → running lines only; `# TYPE … gauge` header per metric; a label value with `"` is escaped. Server test: start with `metrics.listen = 127.0.0.1:0`, `GET /metrics` → 200 `text/plain; version=0.0.4`; the dashboard listener answers `/metrics` with 404.
- [ ] **Step 2: Run, expect FAIL.**
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run, expect PASS.**
- [ ] **Step 5: Commit** — `feat(server): prometheus metrics for tracked software`.

### Task 8: Mirror worktree and fast-forward push

**Files:**
- Modify: `lodestar-server/src/mirror/git_mirror.rs`, create `lodestar-server/src/mirror/push_error.rs` (or extend `promote_error.rs` if the shape fits)
- Test: `lodestar-server/tests/mirror.rs`

**Interfaces:**
- Produces:
  ```rust
  impl Mirror {
      /// Fresh worktree of `channel`'s head at `dir` (removed first if present); returns the head commit.
      pub async fn worktree(&self, channel: &str, dir: &Path) -> anyhow::Result<String>;
      /// Commits `flake.lock` in `dir` as Lodestar <lodestar@vanwaaij.com> with `message`, pushes it to
      /// `channel` iff the remote head is still `base`; returns the new commit.
      pub async fn push_lock(&self, dir: &Path, channel: &str, base: &str, message: &str) -> Result<String, PushError>;
      pub async fn remove_worktree(&self, dir: &Path);
  }
  pub enum PushError { ChannelMoved { expected: String, actual: String }, Rejected(String), Git(anyhow::Error) }
  ```
  `push_lock` holds `write_lock`, fetches, compares the channel head with `base`, pushes `HEAD:refs/heads/<channel>` without `--force` (deploy key via the existing `GIT_SSH_COMMAND` path). Message validated: one line, no NUL.

- [ ] **Step 1: Failing tests** (existing bare-repo fixtures in `tests/mirror.rs`) — worktree returns the channel head and contains `flake.lock`; modify `flake.lock`, `push_lock` → remote `unstable` advanced by one commit authored `Lodestar <lodestar@vanwaaij.com>` touching only `flake.lock`; push another commit to the bare repo between `worktree` and `push_lock` → `PushError::ChannelMoved`, remote unchanged (Review Focus 3); invalid channel → error before git runs.
- [ ] **Step 2: Run, expect FAIL.**
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run, expect PASS.**
- [ ] **Step 5: Commit** — `feat(server): worktree and fast-forward lock push`.

### Task 9: The updater

**Files:**
- Create: `lodestar-server/src/updates/nix_runner.rs`, `lodestar-server/src/updates/updater.rs`, `lodestar-server/src/updates/updater_loop.rs`
- Modify: `lodestar-server/src/state/app_state.rs` (`pub(crate) updates: Arc<UpdatesState>`, `pub(crate) nix: Arc<dyn NixRunner>`), `lodestar-server/src/poller/mod.rs` (spawn the loop when `software` is non-empty)
- Test: `lodestar-server/tests/updater.rs`

**Interfaces:**
- Consumes: `Mirror::{worktree, push_lock, remove_worktree}` (Task 8), `compare_versions` (Task 2), `UpdatesConfig::inputs` (Task 5), `insert_audit` / `last_audit` (`db/audit.rs`), `ManagedHost { name, os, .. }`.
- Produces:
  ```rust
  #[async_trait] pub trait NixRunner: Send + Sync {
      async fn flake_update(&self, dir: &Path, input: &str) -> anyhow::Result<()>;
      async fn eval_versions(&self, dir: &Path, host: &ManagedHost) -> anyhow::Result<BTreeMap<String, String>>; // {nixos,darwin}Configurations.<h>.config.services.lodestar-agent.versions
      async fn build(&self, dir: &Path, host: &ManagedHost) -> Result<(), String /*log tail, 40 lines*/>;      // linux: toplevel build; darwin: eval system.drvPath
  }
  pub struct NixCommand { pub bin: PathBuf }   // `nix --extra-experimental-features 'nix-command flakes' ...`
  pub enum UpdateOutcome { Pushed { commit: String, changes: Vec<VersionChange> }, NoChange, EvalFailed { host: String, log: String }, BuildFailed { host: String, log: String }, PushFailed { reason: String } }
  pub struct VersionChange { pub software: String, pub from: String, pub to: String }
  pub struct UpdateRun { pub input: String, pub finished_at: i64, pub outcome: UpdateOutcome }
  pub struct UpdatesState { pub running: tokio::sync::Mutex<()>, pub last: std::sync::Mutex<BTreeMap<String, UpdateRun>> }
  pub async fn run_all(state: &AppState) -> Result<Vec<UpdateRun>, AlreadyRunning>;   // try_lock on `running`
  pub async fn run_input(state: &AppState, input: &str, software: &[String]) -> UpdateRun;
  pub fn commit_message(input: &str, changes: &[VersionChange]) -> String;
  pub async fn restore_last(state: &AppState);   // from the newest audit event of kind "update" per input
  ```
  `run_input` follows spec section 5 steps 1–7 in order. Only software in `software` counts as a change; per software, `from`/`to` are the lowest old and highest new version across hosts. Hosts whose versions did not change are not built. Audit: `kind = "update"`, `actor = "lodestar"`, `host = None`, `details = {"input", "result", "commit"?, "changes"?, "host"?, "log"?}`. Result names: `pushed`, `no-change`, `eval-failed`, `build-failed`, `push-failed`. Worktree dir `state_dir/updater/<input>`, removed after every run.

- [ ] **Step 1: Failing tests** with a `FakeNix` (scripted per host and phase) and the bare-repo fixture:
  - `commit_message("nixpkgs-unstable", [tailscale 1.98.10→1.104.2])` == `"chore(nixpkgs-unstable): bump for tailscale 1.98.10 -> 1.104.2"`; two changes joined with `", "` in software-name order.
  - lock unchanged by `flake_update` → `NoChange`, remote unchanged, audit `no-change`.
  - lock changed but tracked versions equal → `NoChange`, remote unchanged.
  - versions moved on host `a` only, build ok → `Pushed`, remote advanced, only `a` built, audit `pushed` with commit.
  - build of `a` fails → `BuildFailed{host:"a"}`, remote unchanged, log tail in audit.
  - eval fails on `b` → `EvalFailed{host:"b"}`.
  - channel moved (fixture pushes during `FakeNix::build`) → `PushFailed`, remote has only the other commit.
  - `run_all` called while another `run_all` holds the lock → `Err(AlreadyRunning)` (Review Focus 5).
  - `restore_last` after a `pushed` audit event → `last["nixpkgs-unstable"]` restored.
- [ ] **Step 2: Run, expect FAIL.**
- [ ] **Step 3: Implement**; the loop runs `run_all` every `interval_secs`, first run one interval after start (not at boot); `restore_last` at start. Feed `UpdatesState.last` into `MetricsInput.last_runs` (Task 7) and add a metrics test for `lodestar_update_last_result{input="nixpkgs-unstable",result="pushed"} 1` and the timestamp.
- [ ] **Step 4: Run, expect PASS**; full workspace checks.
- [ ] **Step 5: Commit** — `feat(server): updater bumps inputs when tracked software moves`.

### Task 10: API and CLI

**Files:**
- Create: `lodestar-proto/src/dashboard/updates_view.rs` (export via ts-rs), `lodestar-server/src/api/updates.rs`, `lodestar-cli/src/commands/updates.rs`
- Modify: `lodestar-server/src/api/app.rs` (`read_routes`: `GET /updates`; `act_routes`: `POST /updates/run`), `lodestar-cli/src/cli.rs` (`Command::Updates { #[command(subcommand)] action: Option<UpdatesAction> }`, `UpdatesAction::Run { #[arg(long)] wait: bool, #[arg(long, default_value_t = 1800)] timeout: u64 }`), `commands/dispatch.rs`, `commands/mod.rs`
- Test: `lodestar-server/tests/api.rs`, CLI tests next to existing ones

**Interfaces:**
- Produces: `UpdatesView { software: Vec<SoftwareView>, inputs: Vec<InputView>, running: bool }`, `SoftwareView { name, input, latest: Option<String>, checked_at: Option<i64>, error: Option<String> }`, `InputView { name, last: Option<UpdateRunView> }`, `UpdateRunView { finished_at: i64, result: String, commit: Option<String>, changes: Vec<VersionChangeView>, host: Option<String> }`. `POST /api/updates/run` → `202` and starts `run_all` in the background; `409 {"error":"already running"}` if running; audit `update-requested` with the actor. CLI `updates` prints a table (software, latest, input, last result, age); `updates run --wait` polls `GET /api/updates` until `running == false` and exits 1 unless every input's last result is `pushed` or `no-change`.
- [ ] **Step 1: Failing tests** — `GET /api/updates` shape with one software; `POST` as reader → 403 (existing access tests' pattern), as admin → 202 then 409 while the fake run blocks; CLI table rendering.
- [ ] **Step 2–4:** fail, implement, pass (workspace checks, ts-rs export).
- [ ] **Step 5: Commit** — `feat: updates API and CLI`.

### Task 11: Dashboard

**Files:**
- Create: `web/src/components/fleet/VersionsCell.tsx`, `web/src/components/fleet/UpdatesCard.tsx`
- Modify: `web/src/components/fleet/HostRow.tsx`, `HostCard.tsx`, `HostTable.tsx` (column "Software"), `web/src/pages/DashboardPage.tsx` (UpdatesCard next to PromoteCard, hidden when `software` is empty), connection/state code that fetches `/api/updates` on load and after the run button
- Test: `web/tests/updates.test.tsx`

**Interfaces:**
- Consumes: `HostView.versions`, `UpdatesView` bindings, `ActGate` for the button (same rules as PromoteCard).
- [ ] **Step 1: Failing tests** — VersionsCell shows `tailscale 1.98.10` with the "behind" tone when latest is `1.104.2`, plain when equal, nothing when the host reports no versions; UpdatesCard shows last result and relative time, "Check now" disabled for readers and off-tailnet admins (existing ActGate tests' pattern), clicking posts once and shows "Running…".
- [ ] **Step 2–4:** fail, implement following `docs/style-guide.md`, pass (`npx vitest run`, `npm run build` via the dev shell).
- [ ] **Step 5: Commit** — `feat(web): software versions and updates panel`.

### Task 12: VM test

**Files:**
- Modify: `nix/tests/vm.nix`
**Interfaces:** Consumes Tasks 3, 4, 7.
- [ ] **Step 1:** give one agent node `services.lodestar-agent.versions.tailscale = "1.0.0"`; give the server `metrics.address = "<server ip>"` and `updates.software.tailscale = { input = "nixpkgs"; upstream.github = "tailscale/tailscale"; }` (no network in the VM: the upstream check records an error, which the test also asserts via `/api/updates`).
- [ ] **Step 2:** testScript: wait for the poll, then `curl http://<server ip>:9466/metrics` contains `lodestar_software_running_info{host="<agent>",software="tailscale",version="1.0.0"} 1` and no `up_to_date` line for it.
- [ ] **Step 3: Run on nixos-manager** (KVM): bundle the branch as in `~/.flake/AGENTS.md` ("Linux builder"), `nix build -L .#checks.x86_64-linux.vm`. Expected: success.
- [ ] **Step 4: Commit** — `test(vm): versions and metrics`.

### Task 13 [flake]: Wire Lodestar into the flake

**Files:**
- Modify: `~/.flake/flake.lock` (`nix flake update lodestar` after the lodestar branch is pushed and merged)
- Modify: `~/.flake/modules/common/networking/tailscale.nix` (Linux: `services.lodestar-agent.versions.tailscale = config.services.tailscale.package.version;` — guard with `lib.mkIf (config.services ? lodestar-agent)` only if the option isn't declared on every host; check `modules/common/lodestar/`)
- Modify: `~/.flake/modules/server/lodestar/server.nix` (`updates.software.tailscale = { input = "nixpkgs-unstable"; upstream.github = "tailscale/tailscale"; };`, `metrics.address = config.homelab.lan.address;`, `homelab.monitoring.targets.lodestar.port = cfg.metrics.port;` — the monitoring agent then opens it to monitor only)
- Modify: `~/.flake/docs/lodestar.md` (section "Software updates": what is tracked, where, how to add one, `lodestar updates`)
**Interfaces:** Consumes Task 4's options.
- [ ] **Step 1:** edits above; `nixfmt` as in AGENTS.md.
- [ ] **Step 2: Verify** — all five eval commands from `~/.flake/AGENTS.md` plus router/identity/prod/monitor; `nix eval --json .#nixosConfigurations.prod.config.environment.etc."lodestar/versions.json".text` → `"{\"tailscale\":\"1.104.2\"}"`; `nix eval .#nixosConfigurations.nixos-manager.config.homelab.monitoring.targets.lodestar.port` → `9466`; `nix flake check --no-build --keep-going`.
- [ ] **Step 3: Commit** — `feat(lodestar): track tailscale, metrics for monitoring` (own paths only).

### Task 14 [flake]: Grafana dashboard and alerts

**Files:**
- Create: `~/.flake/modules/server/monitoring/dashboards/software-versions.json` (uid `software-versions`, title "Software versions")
- Modify: `~/.flake/modules/server/monitoring/alerts.nix` (three rules, existing rule shape and contact point)
**Interfaces:** Consumes the metric names from spec section 6.
- [ ] **Step 1: Dashboard** — table panel: query `lodestar_software_running_info` joined with `lodestar_software_up_to_date` (instant, format table, transform "join by labels" on host+software) and `lodestar_software_latest_info`; status column value mapping 1 → "up to date" (green), 0 → "behind" (red), empty → "unknown" (grey). Stat panels: `count(lodestar_software_up_to_date == 0) or vector(0)` "Hosts behind"; `lodestar_update_last_result == 1` (label `result` as text) and `time() - lodestar_update_last_run_timestamp_seconds` "Last updater run".
- [ ] **Step 2: Alerts** — "Software behind upstream": `lodestar_software_up_to_date == 0`, `for = "7d"`; "Updater failed": `lodestar_update_last_result{result=~"build-failed|push-failed|eval-failed"} == 1`, `for = "0s"`; "Upstream check stale": `time() - lodestar_software_latest_checked_timestamp_seconds > 86400`, `for = "1h"`. Contact: the existing VWP BACKUP Discord point.
- [ ] **Step 3: Verify** — `jq empty` on the JSON; `nix eval` monitor's toplevel drvPath; build monitor's Prometheus config and run `promtool check config` on it (on nixos-manager, bundle as in AGENTS.md).
- [ ] **Step 4: Commit** — `feat(monitoring): software versions dashboard and alerts`.

### Task 15: Rollout

- [ ] **Step 1:** lodestar: push the branch, merge per its workflow; flake: `nix flake update lodestar`, Tasks 13–14 committed. Ask before pushing.
- [ ] **Step 2:** push `unstable`; `lodestar promote --yes` (proves Task 1); apply servers through Lodestar; router and identity by hand (build on nixos-manager, `nix copy`, switch, per `~/.flake/AGENTS.md`).
- [ ] **Step 3:** `curl http://10.0.10.170:9466/metrics` from monitor shows running versions for every online host; Grafana "Software versions" shows the table.
- [ ] **Step 4:** `lodestar updates run --wait` → `no-change` (Tailscale is current) and `lodestar updates` shows it; Lodestar dashboard shows the panel.
- [ ] **Step 5:** update the memory note and `docs/lodestar.md` with what was rolled out.
