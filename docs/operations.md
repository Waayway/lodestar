# Operating Lodestar

Day-to-day use of a Lodestar setup: channels, enrolling hosts, applying,
promoting, and what to look at when something goes wrong. Setup and options
are in `README.md`.

## Channels

A channel is a branch of your flake repository. Every host follows exactly one
(`services.lodestar-agent.channel`). A common layout:

- `unstable`: new work lands here; workstations follow it.
- `stable`: servers follow it. Move it forward with **Promote** in the
  dashboard or `lodestar promote --yes` (fast-forward only).

Promote is configured with `services.lodestar-server.promote` (default
`unstable` → `stable`; `null` turns it off). With a single `main` channel you
don't need it.

The agent skips an apply (and says why in the dashboard) when the host's
checkout is not on its channel branch, has uncommitted changes to tracked
files, or has commits that are not on `origin/<channel>` (and when the commit
is not on the channel at all).
Nothing on a host changes without an apply.

## Enrolling a host

1. In the host's configuration: `services.lodestar-agent.enable = true;` plus
   `user`, `managerHost`, `repoUrl` and `channel` (see `README.md`).
2. Make `flakePath` (default `~/.flake` of `user`) a clean clone of the
   repository, on the channel branch, tracking `origin`.
3. Switch the host once by hand. If the server's `hosts` come from
   `lodestar.lib.hostsFrom`, switch the server too. The host shows up in the
   dashboard once the server can reach port 7461 over Tailscale.

## Applying

Dashboard: `http://<server>:8080` (only the allowed Tailscale users and tags
get in).

```sh
lodestar status                          # host, os, channel, online, running, behind, last apply
lodestar rebuild desktop --wait          # force rebuild: re-apply the channel head even if the host
                                         # already runs it; exit 0 only if it ended ok
lodestar rebuild --channel stable --wait [--timeout 900]
lodestar log desktop [APPLY_ID]          # default: the host's last apply
lodestar promote [--yes]                 # shows the candidate; --yes moves the target channel
lodestar audit --limit 20
```

`lodestar` needs the server URL: `--server http://manager:8080`,
`LODESTAR_SERVER` (the agent module sets it on every enrolled host from
`services.lodestar-agent.serverUrl`, default `http://<managerHost>:8080`), or
`server = "http://manager:8080"` in `~/.config/lodestar/config.toml`.

An offline host answers `unreachable`: the apply is recorded and the server
re-sends it when the host comes back. `rebuild --wait` keeps retrying while the
server itself is unreachable (for example when the apply switched the server's
own host and restarted it).

State-changing API calls need the header `X-Lodestar: 1` (CSRF guard), e.g.
`curl -X POST -H 'X-Lodestar: 1' http://manager:8080/api/hosts/desktop/apply`.

## Where things are

Agent (every enrolled host):

- config: `/etc/lodestar-agent/config.json`
- state: `/var/lib/lodestar-agent/` (`applies/<id>.{json,log}`, `activations.jsonl`,
  `apply-in-progress.json` while an apply runs)
- NixOS: `systemctl status lodestar-agent`, applies run as transient units
  `lodestar-apply-<id>` (`journalctl -u 'lodestar-apply-*'`)
- darwin: launchd label `org.lodestar.agent`, log `/var/log/lodestar-agent.log`

Server:

- config: `/etc/lodestar-server/config.json`, hosts: `/etc/lodestar-server/hosts.json`
- state: `/var/lib/lodestar-server/` (`lodestar.db`, `mirror.git`, ssh `known_hosts`)
- `systemctl status lodestar-server`, runs as user `lodestar-server`

Every activation of an enrolled host (applies, manual `nixos-rebuild` /
`darwin-rebuild`, boots) is appended to `activations.jsonl` and shows up in the
dashboard's audit log, with `SUDO_USER` for manual switches.

## Troubleshooting

- **Host "off"**: Tailscale says the device is offline, or its name does not
  match the agent's `hostName`.
- **"agent unreachable"**: the device is online but nothing answers on 7461:
  is `lodestar-agent` running, and does the Tailscale policy allow the server
  to reach `tcp:7461` on it?
- **403 from the agent**: the request did not come from `managerHost`'s
  Tailscale addresses (check that `managerHost` resolves on the agent host).
- **Dashboard says you don't have access**: your Tailscale login is not in
  `allowedUsers` and your device has none of `allowedTags`. A tagged device
  loses its user identity in Tailscale, so tag servers, not your laptop.
- **Promote fails with a push error**: the server needs `deployKeyFile` with
  write access to `pushUrl`, readable by the `lodestar-server` user.

## Developing

```sh
nix run .#lodestar-demo                          # simulated hosts, dashboard at http://127.0.0.1:8080
nix run .#lodestar-demo -- --listen 127.0.0.1:18080 --speed 4
nix develop                                      # cargo, rustc, clippy, node 22, sqlite, git
nix develop -c cargo test --workspace
cd web && nix develop .. -c npx vitest run
cd web && LODESTAR_API=http://127.0.0.1:18080 nix develop .. -c npm run dev
nix build .#checks.aarch64-darwin.bindings       # TS bindings up to date
```

Linux-only checks need an x86_64-linux builder (KVM for the VM test):
`checks.x86_64-linux.vm` (server, two agents and a git remote in VMs) and
`checks.x86_64-linux.firewall`.
