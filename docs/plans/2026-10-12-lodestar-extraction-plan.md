# Lodestar extraction plan (2026-10-12)

The fleet manager was built inside `Waayway/nixos-config` (paths `pkgs/fleet`,
`modules/fleet`, `tests/fleet-vm.nix`, `lib/fleetHosts.nix`). This repo was
extracted from it with `git filter-repo` (history kept) and is being made
generic: same architecture (Tailscale, agents pull from a public git repo,
channels), every user-specific value becomes a module option.

Decisions: name **lodestar**; CLI binary `lodestar`; daemons `lodestar-agent`,
`lodestar-server`; NixOS/darwin options `services.lodestar-agent` /
`services.lodestar-server`; license `MIT OR Apache-2.0`; public repo
`github.com/Waayway/lodestar`.

Layout after extraction: cargo workspace at the root (`fleet-proto`,
`fleet-agent`, `fleet-server` + `migration`, `fleetctl`, `web/`),
`default.nix` (crane + buildNpmPackage), `nix/modules/{agent,server,audit-hook}.nix`,
`nix/tests/vm.nix`, `nix/lib/hostsFrom.nix` (was `fleetHosts.nix`),
`docs/fleet.md`, `docs/plans/2026-10-12-fleet-manager-{design,plan}.md`
(historical: written for Waayway/nixos-config).

## 1. Renames

| Now | New |
|---|---|
| crates `fleet-proto`, `fleet-agent`, `fleet-server`, `fleet-migration`, `fleetctl` (dirs too) | `lodestar-proto`, `lodestar-agent`, `lodestar-server`, `lodestar-migration`, `lodestar-cli`; all `publish = false` |
| binaries `fleet-agent`, `fleet-server`, `fleetctl` | `lodestar-agent`, `lodestar-server`, `lodestar` |
| lib names `fleet_proto`, `fleet_agent`, `fleet_server`, `fleetctl` | `lodestar_proto`, `lodestar_agent`, `lodestar_server`, `lodestar_cli` |
| crane `pname = "fleet"` (`default.nix:16`) | `lodestar` |
| clap names / log prefixes `fleet-agent:`, `fleet-server:` | `lodestar-agent:`, `lodestar-server:` |
| systemd units `fleet-agent`, `fleet-server`, transient `fleet-apply-*` (`launcher.rs:60`); launchd `fleet-agent`; `/var/log/fleet-agent.log` | `lodestar-agent`, `lodestar-server`, `lodestar-apply-*`; launchd label `org.lodestar.agent`; `/var/log/lodestar-agent.log` |
| `/var/lib/fleet-agent`, `/var/lib/fleet-server`, `/etc/fleet-agent`, `/etc/fleet-server`, DB `fleet.db` | `/var/lib/lodestar-agent`, `/var/lib/lodestar-server`, `/etc/lodestar-agent`, `/etc/lodestar-server`, `lodestar.db` |
| user/group `fleet-server`, `--operator=fleet-server` | `lodestar-server` |
| header `X-Fleet` | `X-Lodestar` |
| env `FLEET_SERVER`, `FLEET_API`, `FLEET_WEB_DIST` | `LODESTAR_SERVER`, `LODESTAR_API`, `LODESTAR_WEB_DIST` |
| nix packages `fleet-agent`, `fleet-server`, `fleet-web`, `fleetctl`; app `fleet-demo`, `fleetctl`; checks `fleet-bindings`, `fleet-vm`, `fleet-firewall`; devShell `fleet`; activation script `fleetAudit` | `lodestar-agent`, `lodestar-server`, `lodestar-web`, `lodestar` (CLI, also `default`); apps `lodestar-demo`, `lodestar`; checks `bindings`, `vm`, `firewall`; devShell `default`; `lodestarAudit` |
| UI title "Fleet" | "Lodestar" |

## 2. Generic options

Agent (`services.lodestar-agent`):
- `enable` (plain enable option; default false)
- `user` (required): the account that owns the checkout; git and the build run as it
- `hostName` (default `config.networking.hostName`)
- `channel` (default `"main"`): the branch this host follows
- `managerHost` (required): Tailscale/MagicDNS name of the server
- `repoUrl` (required)
- `flakePath` (default `"${config.users.users.${user}.home}/.flake"` on NixOS; on darwin `/Users/<user>/.flake`)
- `flakeAttr` (default `nixosConfigurations.<hostName>.config.system.build.toplevel` on NixOS, `darwinConfigurations.<hostName>.system` on darwin): passed to the agent instead of hardcoded in `apply.rs` / demo
- `port` (7461), `listen`, `rebuildCommand`, `revisionCommand` (test-only)
- `package`, `cliPackage` (defaults: this flake's packages for `pkgs.stdenv.hostPlatform.system`), `installCli` (default true)
- No `specialArgs` from the consumer: replace `fleet`, `hostPlatform`, `user`, `currentSystemName`, `inputs.self` with options / `pkgs.stdenv.hostPlatform` / `config`.

Server (`services.lodestar-server`):
- `hosts` (`attrsOf { channel; os; system; user; }`), usually from `lodestar.lib.hostsFrom`
- `repoUrl`, `pushUrl` (required), `deployKeyFile` (null)
- `listenAddress` (required, Tailscale IP), `listenPort` (8080)
- `allowedUsers` (default `[]`), `allowedTags` (default `[]`)
- `agentPort` (default `config.services.lodestar-agent.port` if that exists, else 7461)
- `promote = { from = "unstable"; to = "stable"; }` (nullable = no promote). Hardcoded today in `mirror.rs:212-249`, `api.rs:304`, `fleetctl/src/lib.rs:419`, `PromoteDialog.tsx:38`; expose through the snapshot (`Snapshot.promote_from/to` or similar) so UI and CLI use it.
- `testing.*` stays test-only.

CLI: `--server` / `LODESTAR_SERVER` required when no config (no `nixos-manager` default); optionally a config file `~/.config/lodestar/config.toml` with `server = "..."`.

UI: "~/.flake" copy (`format.ts:48-52`) uses the host's actual `flake_path` from the checkout status.

`lodestar.lib.hostsFrom { nixosConfigurations ? {}, darwinConfigurations ? {} }`: every system with `services.lodestar-agent.enable`, mapped to `{ channel; os; system; user; }` read from that system's config.

## 3. Neutral demo and test data

- Demo hosts (`demo/mod.rs:74-89`): neutral names (e.g. `laptop-1`, `laptop-2`, `desktop`, `workstation`, `server-1`, `vpn`, `media`) with the same scripted behaviours; commit subjects (`demo/repo.rs:21-40`) generic; repo URL (`demo/agents.rs:201`) `https://example.com/infra.git`.
- Tests: `Waayway@github` / "Thijs van Waaij" (`tests/fixtures/whois-user.json`, `tests/access.rs`, `tests/api.rs:586`) → `alice@example.com` / "Alice"; `sudo_user "thijs"` (`tests/poller.rs:167`) → `alice`; `100.92.102.84` (`net.rs:87-95`) → `100.64.0.10`.

## 4. Flake

`flake.nix` (inputs: nixpkgs 26.05, crane) with outputs:
- `packages.<system>.{lodestar-agent,lodestar-server,lodestar-web,lodestar,default}`
- `apps.<system>.{lodestar,lodestar-demo,default}`
- `nixosModules.{agent,server,default}`, `darwinModules.{agent,default}`; modules get this flake's packages as defaults without `inputs.self` from the consumer
- `lib.hostsFrom`
- `checks.<system>.bindings`; `checks.x86_64-linux.{vm,firewall}` (firewall: eval a minimal `nixosSystem` with both modules enabled and assert 7461/8080 on `tailscale0`)
- `devShells.<system>.default`
- systems: aarch64-darwin, x86_64-darwin, x86_64-linux, aarch64-linux

The VM test drops the consumer `specialArgs` and uses the options.

## 5. Docs

- `README.md`: what it is; architecture; requirements (Tailscale, a public git repo with one flake output per host, channel branches); quick start (`nix run github:Waayway/lodestar#lodestar-demo`); flake usage with `hostsFrom`; module options; Tailscale grants (manager → agents tcp:7461, users → dashboard tcp:8080); security model (agent source check, whois access, `X-Lodestar` CSRF header, git as user / switch as root); CLI; promote; limitations.
- `AGENTS.md` for this repo (layout, dev shell, tests, conventions).
- `LICENSE-MIT`, `LICENSE-APACHE`; `license = "MIT OR Apache-2.0"` in Cargo and package.json.
- `docs/plans/2026-10-12-fleet-manager-*.md` get a header: historical, written for Waayway/nixos-config.
- `docs/fleet.md` becomes `docs/operations.md` (generic).

## 6. Issues for after publishing

- `lodestar rebuild --wait` against the host that runs the server loses its connection when that switch restarts the server; retry.
- Agent `read_record` race: a just-finished apply can be recorded as "interrupted"; re-read after taking `apply.lock`.
- The darwin agent (launchd) has not run on a real Mac yet.
- `advertiseTags` option for Tailscale tags on agents.

## 7. Consumer cutover (Waayway/nixos-config)

- Input `lodestar.url = "github:Waayway/lodestar"` (nixpkgs follows); import `lodestar.nixosModules.default` / `darwinModules.default`.
- Host attrs `fleet` / `channel` map to `services.lodestar-agent.{enable,channel}` plus Thijs's `managerHost`/`repoUrl`.
- nixos-manager sets `services.lodestar-server` (hosts via `hostsFrom`, URLs, `allowedUsers`/`allowedTags`, `listenAddress`).
- Remove `pkgs/fleet`, `modules/fleet`, `tests/fleet-vm.nix`, `lib/fleetHosts.nix`, the `fleetHosts` export and the fleet parts of `programs.nix`.
- State migration on nixos-manager (`/var/lib/fleet-*` → `/var/lib/lodestar-*`, incl. DB, mirror, known_hosts, activations log, apply records) in the same switch as the rename.
