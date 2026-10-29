# Software updates: input bumps and version tracking

Status: agreed in conversation 2026-10-29, written for review. Prompted by Tailscale
TS-2026-013: nixos-26.05 shipped 1.98.10 while 1.104.2 was out, and Tailscale cannot
update itself on NixOS ("individual package updates are not supported on NixOS"), so
updates have to come from the flake inputs. Spans this repo and `~/.flake`.

## Goal

Lodestar keeps chosen software current without anyone running `nix flake update` by
hand, and Grafana shows per host whether that software matches the latest upstream
release.

- **Generic:** any software is one config entry (flake input + upstream release
  source). Tailscale is the first entry.
- **"Up to date" means the latest upstream release**, not the newest nixpkgs version.
  When nixpkgs lags behind upstream, Grafana shows "behind"; Lodestar does not build
  from upstream sources itself, it only moves inputs forward.
- **Lodestar pushes to `unstable` only**, and only after the affected hosts build.
  Workstations pick the bump up through Lodestar as usual; servers wait for a promote.

Out of scope: building packages from upstream sources (overriding `src`/hashes),
automatic promotion to `stable`, upstream sources other than GitHub releases, software
not installed by Nix (pve, jenkins-vm, primary-server, the Pixel).

## 0. Deploy key (prerequisite)

Lodestar cannot push today: `deployKeyFile` is unset, so `lodestar promote --yes` fails
("Please make sure you have the correct access rights") and promotion was done by hand
on 2026-10-27. Both promote and the updater need it.

- Generate an ed25519 SSH key **in 1Password**: `op item create --category ssh
  --ssh-generate-key ed25519 --vault Homelab --title lodestar-deploy-key`. The private
  key exists only in 1Password and in opnix's secret file on nixos-manager.
- Register the public key on `Waayway/nixos-config` as a deploy key with write access
  (`gh api repos/Waayway/nixos-config/keys -f read_only=false`, Waayway token).
- `~/.flake/hosts/servers/nixos-manager.nix` (or a server module gated on
  `services.lodestar-server.enable`): opnix secret `lodestarDeployKey`, reference
  `op://Homelab/lodestar-deploy-key/private key?ssh-format=openssh`, owner
  `lodestar-server`, mode 0400, `services = [ "lodestar-server" ]`; set
  `services.lodestar-server.deployKeyFile` to its path. `pushUrl` is already
  `git@github.com:Waayway/nixos-config`; GitHub's host key is pinned in the server's
  known-hosts (check the module does this; add it if not).
- GitHub rulesets on `stable` and `unstable`: no force pushes, no deletion. A deploy key
  can write every branch; the rulesets limit what a leaked key can do.
- Done when `lodestar promote --yes` pushes, and `~/.flake/docs/lodestar.md` open item 1
  is closed (item 2's re-tag of nixos-manager was done 2026-10-27).

## 1. Configuration

### Server: what to update

New option on the server module (`nix/modules/server.nix`), written to the server's
JSON config:

```nix
services.lodestar-server.updates = {
  channel = "unstable";        # branch the bumps are pushed to
  intervalSecs = 86400;        # updater run, plus "check now"
  upstreamIntervalSecs = 21600;
  software.tailscale = {
    input = "nixpkgs-unstable";               # flake input that supplies it
    upstream.github = "tailscale/tailscale";  # latest = newest non-draft, non-prerelease release
  };
};
```

`software = { }` (the default) turns the updater off. Several entries may name the same
input; the input is bumped once and all of them move together.

### Agent: what each host runs

New agent option `services.lodestar-agent.versions` (attrs of strings, default `{ }`),
written to `/etc/lodestar/versions.json` (NixOS `environment.etc`, nix-darwin
`environment.etc`). The host's config says which package it really uses, so the value
is what that host runs, not a guess:

```nix
services.lodestar-agent.versions.tailscale = config.services.tailscale.package.version;
```

In `~/.flake` this lives next to the software (Tailscale:
`modules/common/networking/tailscale.nix` on Linux; on the Macs the Homebrew cask is
not managed by Nix, so darwin reports nothing for Tailscale until it is). Software names
are the same keys as `updates.software`.

## 2. Version reporting

- `AgentStatus` (`lodestar-proto`) gains `versions: BTreeMap<String, String>`,
  `#[serde(default)]`. The agent reads `/etc/lodestar/versions.json` on each status
  request (missing file: empty map). Old agents keep working and report nothing.
- The poller stores the map in the host's in-memory view; `HostView` carries it to the
  dashboard and the metrics.

## 3. Upstream check

- Every `upstreamIntervalSecs`, per software, `GET
  https://api.github.com/repos/<owner>/<repo>/releases?per_page=20`, unauthenticated
  (60 requests/hour is plenty), first release that is neither draft nor prerelease.
  Tag `v1.104.2` becomes `1.104.2`.
- On failure the last known version stays, marked stale (`checked_at` old); the error is
  logged and counted in metrics. Nothing else depends on it.

## 4. Version comparison

One function, unit-tested: strip a leading `v`, cut at the first `-` or `+`
(`1.104.1-t7f4efe814-g...` → `1.104.1`), compare dot-separated parts numerically, a
missing part is 0. A part that is not a number makes the pair incomparable: the host
shows "unknown", never "up to date".

## 5. The updater

A new `updater/` module in lodestar-server, one run at a time (a mutex; "check now"
while running returns "already running").

Per input that some software uses:

1. Fetch the mirror; make a scratch worktree of `<channel>`'s head under
   `state_dir/updater/`.
2. Read each configured host's `versions` at the current lock: `nix eval --json
   .#{nixos,darwin}Configurations.<host>.config.services.lodestar-agent.versions`
   (hosts from the existing hosts file; flake attribute from its `os`).
3. `nix flake update <input>` in the worktree. Lock unchanged: done, result
   `no-change`.
4. Read the versions again. No tracked software got newer on any host: discard, result
   `no-change` (lock churn alone never reaches the repo).
5. Build the toplevel of every Linux host whose versions changed (`nix build --no-link
   .#nixosConfigurations.<host>.config.system.build.toplevel`); darwin hosts are only
   evaluated (`.#darwinConfigurations.<host>.system.drvPath`), they can't be built here.
   Any failure: discard, result `build-failed`, with the failing host and the log tail.
6. Commit `flake.lock` only, as `Lodestar <lodestar@vanwaaij.com>`, message
   `chore(<input>): bump for tailscale 1.98.10 -> 1.104.2` (all changed software,
   highest old → new). Push to `<channel>` fast-forward only (the head must still be the
   one from step 1; otherwise result `push-failed: channel moved`, retried next run).
   Pushing takes the mirror's write lock, like promote.
7. Record the run as an audit event (`update`, input, result, commit), which also
   restores the last result per input after a restart.

Resources: builds run as the `lodestar-server` user, which must be a trusted Nix user or
build through the daemon (it does today for evaluation; check). On nixos-manager (4
cores) a run that builds takes minutes; that is fine for a daily job.

## 6. Metrics

New listener for Prometheus, separate from the tailnet and router entrances, so scraping
needs no identity: `services.lodestar-server.metrics = { address; port = 9466; }`,
`GET /metrics`, text format. In `~/.flake` it listens on the LAN address and its port
is declared as a monitoring target (`homelab.monitoring.targets.lodestar.port = 9466`
on nixos-manager), so monitor's Prometheus picks it up like every other exporter, and
the firewall opens it only to monitor.

| Metric | Labels | Value |
|---|---|---|
| `lodestar_software_running_info` | `host`, `software`, `version` | 1 |
| `lodestar_software_latest_info` | `software`, `version` | 1 |
| `lodestar_software_latest_checked_timestamp_seconds` | `software` | unix time |
| `lodestar_software_latest_check_errors_total` | `software` | counter: failed upstream checks since start ("no stable release" included) |
| `lodestar_software_up_to_date` | `host`, `software` | 1 up to date, 0 behind; absent when unknown |
| `lodestar_update_last_run_timestamp_seconds` | `input` | unix time |
| `lodestar_update_last_result` | `input`, `result` | 1 for the last result (`pushed`, `no-change`, `build-failed`, `push-failed`, `eval-failed`) |

## 7. Grafana (`~/.flake`)

- Dashboard file `modules/server/monitoring/dashboards/software-versions.json`:
  - Table: host × software, running version, latest version, status (up to date /
    behind / unknown), from the metrics above.
  - Stats: hosts behind, last updater result per input and its age.
- Alert rules in `modules/server/monitoring/alerts.nix`, to the existing VWP BACKUP
  Discord contact:
  - a host behind upstream for more than 7 days (`lodestar_software_up_to_date == 0`
    for 7d);
  - an updater run that ended `build-failed` or `push-failed`;
  - the upstream check older than 24h.

## 8. Lodestar dashboard and CLI

- Fleet table: a versions cell per host listing tracked software, behind ones marked.
- An "Updates" panel: per input, last run, result, commit; a "Check now" button
  (admin action, tailnet-only, like apply and promote).
- API: `GET /api/updates` (state), `POST /api/updates/run` (CSRF and access rules as for
  the other actions).
- CLI: `lodestar updates` (table) and `lodestar updates run [--wait]`.

## 9. Testing

- Rust unit tests: version comparison (including Tailscale's client-version format),
  release-list parsing (drafts, prereleases, `v` prefix), metrics rendering.
- Updater integration tests in `lodestar-server/tests/` against a scratch git repo and a
  fake `nix` command (as the demo does for git): no change, change but no tracked
  version moved, build failed, channel moved before push, pushed.
- NixOS VM test (`nix/tests/vm.nix`): an agent with `versions` set; the server's
  `/metrics` shows `lodestar_software_running_info` for it.
- `~/.flake`: all hosts evaluate, the dashboard JSON parses, Prometheus config passes
  `promtool check config`.

## Rollout

1. Deploy key (section 0); promote works.
2. Lodestar release with versions, upstream check, metrics, UI (no updater yet);
   `nix flake update lodestar` in `~/.flake`, agents report Tailscale, Grafana shows
   the table.
3. Updater on, `updates.software.tailscale` set; first run by hand with `lodestar
   updates run --wait`.
4. Promote to `stable`; servers apply through Lodestar (router and identity by hand
   while they can't evaluate the flake).

## Open questions

None blocking. Later: more upstream sources (PyPI, a URL with a regex), version
tracking for non-Nix hosts.
