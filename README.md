# Lodestar

See which commit of your Nix flake every machine runs, and tell any of them to
move to the newest commit of its channel with one click. For NixOS and
nix-darwin hosts on one Tailscale tailnet, with the configuration in one git
repository.

Lodestar was partly developed with AI assistance, orchestrated by a human
(Waayway).

```sh
nix run github:Waayway/lodestar#lodestar-demo   # dashboard against simulated hosts, http://127.0.0.1:8080
```

## How it works

```
             your repo (GitHub, ...)          one branch per channel: unstable, stable, ...
              ^ git fetch      ^ git push (promote only)
              |                |
  agents -----+         lodestar-server  <---- you: dashboard / `lodestar` CLI (tcp:8080)
  (every host)            (one host)
     ^                         |
     +---- tcp:7461 -----------+   "apply commit X" / status / logs, over Tailscale
```

- **lodestar-agent** runs as root on every enrolled host. It listens on the
  host's Tailscale address (port 7461) and only accepts requests from the
  server's host. An apply fetches the repository into the user's checkout
  (default `~/.flake`), fast-forwards it to the requested commit, runs
  `nix build` **as that user**, then switches **as root** (systemd-run on
  NixOS, a detached process on darwin, so the switch can restart the agent).
- **lodestar-server** runs on one NixOS host: the dashboard and REST/WebSocket
  API, a read-only git mirror (channel heads, "N behind", commit subjects), a
  SQLite database of applies and audit events, and a poller that asks every
  agent for its status and re-sends applies to hosts that were offline.
- **lodestar** is the CLI for the same API.
- **Builds tab** (with the build gate on): the newest commits the builder built, every
  Linux host with its store path or the log of its failed build, and Retry build; see
  `docs/operations.md` "Build gate".
- **Services tab**: links to the web applications your hosts run, declared in Nix
  (`lodestar.links`), each with a live HTTP check and the state of its machine.
- **Public URL** (optional): a second listener behind your reverse proxy, with login
  through Kanidm (OpenID Connect); see `docs/operations.md` "Public URL".
- **Software updates** (optional): the server tracks versions of software you name
  (e.g. Tailscale) per host and upstream, bumps the flake input that provides it once a
  day when a newer version is out, and pushes the build-checked `flake.lock` to a channel
  (default `unstable`, never the promote target). A Prometheus `/metrics` listener shows
  what runs where and whether it is up to date; see `docs/operations.md` "Software
  updates".
- Nothing changes on a host until someone presses apply (or runs
  `lodestar rebuild`). Every activation, including manual `nixos-rebuild` /
  `darwin-rebuild` runs and boots, lands in the audit log.

## Requirements

- Every host on one Tailscale tailnet, with MagicDNS.
- A git repository with a flake that has one output per host
  (`nixosConfigurations.<host>` / `darwinConfigurations.<host>`, or any
  attribute via `flakeAttr`). It must be fetchable by each host's user without
  prompting (public over HTTPS, or credentials that user already has).
- One branch per channel (e.g. `unstable` and `stable`, or just `main`).
- On each host, a clone of the repository at `flakePath`, owned by `user`, on
  its channel branch.

## Flake usage

```nix
{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-26.05";
    lodestar.url = "github:Waayway/lodestar";
    lodestar.inputs.nixpkgs.follows = "nixpkgs";
  };

  outputs = { self, nixpkgs, lodestar, ... }: {
    nixosConfigurations.desktop = nixpkgs.lib.nixosSystem {
      system = "x86_64-linux";
      modules = [
        lodestar.nixosModules.agent
        {
          services.lodestar-agent = {
            enable = true;
            user = "alice";
            channel = "unstable";
            managerHost = "manager";               # Tailscale name of the server's host
            repoUrl = "https://github.com/alice/infra";
          };
        }
        ./hosts/desktop.nix
      ];
    };

    nixosConfigurations.manager = nixpkgs.lib.nixosSystem {
      system = "x86_64-linux";
      modules = [
        lodestar.nixosModules.default             # agent + server
        {
          services.tailscale.enable = true;
          services.lodestar-agent = {
            enable = true;
            user = "alice";
            channel = "stable";
            managerHost = "manager";
            repoUrl = "https://github.com/alice/infra";
          };
          services.lodestar-server = {
            enable = true;
            # NixOS only; with nix-darwin hosts too:
            #   hostsFrom { inherit (self) nixosConfigurations darwinConfigurations; }
            # `tailnet`: your MagicDNS domain; each host is bound to `<hostName>.<tailnet>`
            # (or its agent's `tailscale.nodeId` / `tailscale.dnsName`), never to the
            # HostName a device reports.
            hosts = lodestar.lib.hostsFrom {
              inherit (self) nixosConfigurations;
              tailnet = "example.ts.net";
            };
            repoUrl = "https://github.com/alice/infra";
            pushUrl = "git@github.com:alice/infra.git";
            listenAddress = "100.64.0.1";          # this host's Tailscale IP
            allowedUsers = [ "alice@example.com" ];
          };
        }
        ./hosts/manager.nix
      ];
    };

    # nix-darwin: lodestar.darwinModules.agent (same services.lodestar-agent options)
  };
}
```

Exports:

| Output | What |
|---|---|
| `nixosModules.agent`, `nixosModules.server`, `nixosModules.cache`, `nixosModules.default` (all) | NixOS modules (`cache` is the build cache host, off by default) |
| `darwinModules.agent`, `darwinModules.default` | nix-darwin module (agent only) |
| `lib.hostsFrom { nixosConfigurations ? {}, darwinConfigurations ? {}, tailnet ? null }` | `services.lodestar-server.hosts` from every configuration that enables the agent, keyed by `hostName`, with `nodeId` from the agent's `tailscale.nodeId` and `dnsName` from its `tailscale.dnsName`, else `<hostName>.<tailnet>` |
| `packages.<system>.{lodestar, lodestar-agent, lodestar-server, lodestar-web, default}` | `default` is the CLI |
| `apps.<system>.{lodestar, lodestar-demo, default}` | |

The modules default their packages to this flake's builds; the consumer passes
no `specialArgs`.

## Module options

`services.lodestar-agent` (NixOS and nix-darwin):

| Option | Default | |
|---|---|---|
| `enable` | `false` | |
| `user` | (required) | owns the checkout; git and `nix build` run as this user |
| `hostName` | `networking.hostName` | the name the server knows this host by |
| `tailscale.nodeId` | `null` | this host's stable Tailscale node ID, for `hostsFrom` (the server talks to the agent only on that node) |
| `tailscale.dnsName` | `null` = `<hostName>.<tailnet>` in `hostsFrom` | this host's full MagicDNS name, when it differs |
| `channel` | `"main"` | branch this host follows |
| `managerHost` | (required) | Tailscale name of the server's host; the only accepted source |
| `repoUrl` | (required) | git URL of the flake repository (`origin`) |
| `flakePath` | `~user/.flake` (`/Users/<user>/.flake` on darwin) | the checkout |
| `flakeAttr` | `nixosConfigurations.<hostName>.config.system.build.toplevel` / `darwinConfigurations.<hostName>.system` | what is built and switched to |
| `port` | `7461` | opened on `tailscale0` (NixOS) |
| `listen` | `null` (the Tailscale addresses) | explicit `ip:port` |
| `package`, `cliPackage` | this flake's | |
| `binaryPath` | `/var/lib/lodestar-agent/bin/lodestar-agent` | fixed path the agent is copied to on activation (when `package` changed); the daemon and the apply wrapper run from here |
| `installCli` | `true` | put `lodestar` on the PATH, with `LODESTAR_SERVER` set to `serverUrl` |
| `serverUrl` | `null` = `http://<managerHost>:8080` (the local `listenPort` on the server's host) | where the CLI finds the server; set it if the server uses another port |
| `versions` | `{}` | software name → version on this host (e.g. `{ tailscale = config.services.tailscale.package.version; }`), written to `/etc/lodestar/versions.json` and reported in the agent's status; the names are the server's `updates.software` keys |
| `rebuildCommand`, `revisionCommand` | `null` | tests only |

nix-darwin only:

| Option | Default | |
|---|---|---|
| `macos.stableSigning.enable` | `true` | sign `binaryPath` with a local certificate so macOS permissions survive updates (see below) |
| `macos.stableSigning.certificate` | `null` | PEM certificate shared by all Macs; its key is placed once per Mac (see below). Null: each Mac makes its own |
| `macos.stableSigning.directory` | `/var/lib/lodestar-agent/signing` | root-only key and certificate |
| `macos.firewall.enable` | `true` | when the macOS firewall is on, allow incoming connections for `binaryPath` |

`services.lodestar-server` (NixOS):

| Option | Default | |
|---|---|---|
| `enable` | `false` | |
| `hosts` | `{}` | `attrsOf { channel; os; system; user; nodeId; dnsName; }`, usually `lodestar.lib.hostsFrom { ... }`; every host needs `nodeId` (stable Tailscale node ID) or `dnsName` (full MagicDNS name), see "Host identity" in `docs/operations.md` |
| `repoUrl` | (required) | mirrored read-only |
| `pushUrl` | `null` | where promote pushes; required unless `promote = null` |
| `deployKeyFile` | `null` | SSH key with write access to `pushUrl`, readable by `lodestar-server` (outside the store, e.g. opnix or sops-nix) |
| `listenAddress` | (required) | this host's Tailscale IP; anything else is refused |
| `listenPort` | `8080` | opened on `tailscale0` |
| `allowedUsers` | `[]` | Tailscale logins allowed in |
| `allowedTags` | `[]` | Tailscale device tags allowed in |
| `agentPort` | the agent's `port`, else `7461` | one port for all agents: keep `services.lodestar-agent.port` the same everywhere |
| `promote` | `{ from = "unstable"; to = "stable"; }` | `null` disables promote |
| `updates.software.<name>.input` | (required) | flake input that provides `<name>` (`[A-Za-z0-9_-]+`); `<name>` is `[a-z0-9_-]+` and matches the agents' `versions` keys |
| `updates.software.<name>.upstream.github` | (required) | `owner/repo` whose latest release (not draft, not prerelease) is "latest" |
| `updates.software` | `{}` | empty turns the updater and the upstream checks off |
| `updates.channel` | `"unstable"` | channel the updater pushes `flake.lock` bumps to; must not be the promote target (`stable` when `promote = null`) |
| `updates.intervalSecs` | `86400` | seconds between updater runs, counted from the last finished run |
| `updates.upstreamIntervalSecs` | `21600` | seconds between upstream release checks |
| `metrics.address` | `null` | address of the Prometheus `/metrics` listener; null runs none. Not opened in the firewall: open it to your scraper yourself |
| `metrics.port` | `9466` | |
| `builds.enable` | `false` | the build gate: every Linux host's system is built once on the builder and published to its cache before any agent may apply the commit (`docs/operations.md` "Build gate"); off, applies never wait |
| `builds.store` | `""` | the builder's store, `ssh-ng://[user@]host[:port]` (or `ssh://`); required while enabled |
| `builds.sshKeyFile` | `null` | SSH key for the builder, readable by `lodestar-server` (outside the store, like `deployKeyFile`) |
| `builds.publishCommand` | `[ "cache-publish" ]` | run on the builder over ssh as `<command…> <commit> <store-path>…` with the channel index on stdin (`services.lodestar-cache` provides `cache-publish`) |
| `builds.timeoutSecs` | `7200` | seconds before a build of one commit (evaluation and build of all its hosts) or a publish is given up |
| `package` | this flake's | |
| `testing.*` | | tests only |

The server needs `services.tailscale` on its host; the module makes the
`lodestar-server` user a Tailscale operator (`--operator`) so it can run
`tailscale status` and `tailscale whois`. It also writes `nix_bin` (the host's
`nix.package`) into the server's config, which the updater uses for `nix flake update`,
`nix eval` and `nix build`; pushing a bump needs `pushUrl` and `deployKeyFile` like
promote.

`services.lodestar-cache` (NixOS; the build cache host, see `docs/operations.md`
"Build cache"):

| Option | Default | |
|---|---|---|
| `enable` | `false` | |
| `user` | `"lodestar"` | account the server logs in as over ssh (ssh-ng builds, `cache-publish`); a normal user with bash, no password, a trusted Nix user |
| `authorizedKeys` | `[]` | public half of the server's `builds.sshKeyFile` |
| `cacheDir` | `"/mnt/nix-cache"` | the file binary cache; not created by the module (mount it writable by `user`, readable by nginx) |
| `signingKeyFile` | (required) | `nix key generate-secret` key that signs published paths, readable by `user` (outside the store) |
| `listen.port` | `80` | nginx serves `cacheDir` at `http://<host>:<port>/nix` |
| `allowedClients` | `[]` | the only addresses the port is opened to (iptables or nftables rules); never opened globally |

## macOS permissions

During an apply, home-manager steps run under the agent (e.g. `osascript`
talking to System Events, or reading `~/Library/Group Containers`), so macOS
asks whether *lodestar-agent* may do that. A store build is ad-hoc signed and
macOS knows it only by its hash, so every new build would ask again.

On nix-darwin the module therefore copies the agent to `binaryPath` and signs
the copy with a self-signed certificate (key in
`/var/lib/lodestar-agent/signing`, root only, never in the System or login
keychain, never in git). Its designated requirement is
`identifier "org.lodestar.agent" and certificate leaf = H"<sha1>"`, the same
for every build, so:

- Click **Allow** once for each prompt on the first apply after enabling it
  (typically *Automation → System Events* and *access data from other apps*);
  the grants stay through later updates.
- With the macOS firewall on, the agent is added and unblocked
  (`socketfilterfw`) on every activation, so there is no incoming-connections
  prompt.
- If signing fails the switch still succeeds: the agent runs ad-hoc signed and
  the activation prints a warning.
- One key for all Macs (recommended): set `macos.stableSigning.certificate` to
  the PEM certificate (public, fine in git) and keep the private key in a
  password manager. On each Mac, once: `sudo lodestar-agent-signing-key <
  key.pem` (for example `op document get "Lodestar Agent Signing Key" | sudo
  lodestar-agent-signing-key`), then switch. The module never creates a key
  then; without the key it warns and the agent stays ad-hoc signed.
- Without `certificate`, each Mac creates its own on first activation. New
  certificate: `sudo rm -rf /var/lib/lodestar-agent/signing`, switch, then
  `sudo launchctl kickstart -k system/org.lodestar.agent`; macOS asks once more.
- Granting without any click needs an MDM (PPPC profile); that is out of scope.

## Tailscale policy

With an explicit grants policy (no allow-all), the server's host must reach
every agent on tcp:7461, and you must reach the dashboard on tcp:8080:

```jsonc
"tagOwners": { "tag:managed": ["alice@example.com"] },
"grants": [
  // the server's host -> every enrolled host
  { "src": ["tag:managed"], "dst": ["tag:managed", "alice@example.com"], "ip": ["tcp:7461"] },
  // you -> the dashboard
  { "src": ["alice@example.com"], "dst": ["tag:managed"], "ip": ["tcp:8080"] }
]
```

`tag:managed` is an example: Lodestar does not set Tailscale tags (there is no
`advertiseTags` option yet), so use a tag you already give your servers
(`tailscale up --advertise-tags=tag:managed`, or `services.tailscale.extraUpFlags`)
and list untagged devices by their owner's login, as above. Narrow `src` to the
server's host if you prefer. A tagged device loses its user identity in
Tailscale, so keep the laptops you use the dashboard from untagged and list your
login in `allowedUsers`.

## Security model

- **Agent source check**: an agent only answers requests from the addresses
  `managerHost` resolves to (plus its own Tailscale addresses when it is the
  server's host).
- **Dashboard access**: every request's peer is looked up with
  `tailscale whois`; it must be an `allowedUsers` login or carry an
  `allowedTags` tag. The server only listens on its Tailscale address, so whois
  can always vouch for the peer.
- **CSRF**: state-changing requests need the header `X-Lodestar: 1` (a
  cross-site page cannot set it without a CORS preflight, which the server
  never allows), and browsers must report `Sec-Fetch-Site: same-origin`. The
  dashboard and CLI send the header.
- **Least privilege**: git and `nix build` run as the checkout's user; only
  the switch runs as root. The server runs as its own system user. Commit
  hashes and channel names are validated before they reach git.
- The agent never applies a commit that is not on its channel, never
  discards local changes, and never moves a checkout that has commits the
  channel lacks: it skips and says why.

## CLI

```sh
lodestar status
lodestar rebuild <host> [--wait] [--timeout 900]
lodestar rebuild --channel <channel> [--wait]
lodestar log <host> [apply-id]
lodestar promote [--yes]
lodestar updates
lodestar updates run [--wait] [--timeout 7200]
lodestar build status
lodestar build retry <commit>
lodestar audit [--limit 20]
```

The server URL comes from `--server`, `LODESTAR_SERVER` (set system-wide by
the agent module, see `serverUrl`), or `~/.config/lodestar/config.toml`:

```toml
server = "http://manager:8080"
```

`rebuild --wait` keeps retrying while the server is restarting (e.g. when the
apply switched the server's own host). With the build gate on, `rebuild` waits (up to two
hours) for the server to build the channel's head first and says so
(`waiting for the build of <commit> on the builder…`). `build status` shows each channel's
build; `build retry <commit>` lets the failed hosts of a commit be built again. `updates` lists the tracked software, its latest
release and the last updater result per flake input; `updates run --wait` starts a run
and exits 0 only when every input ended `pushed` or `no-change`.

## Promote

With two channels (`unstable` for workstations, `stable` for servers), promote
fast-forwards `stable` to a commit of `unstable` that already ran fine
somewhere. It is a non-force push from the server to `pushUrl`, refused unless
`stable` is an ancestor of the commit. Configure the pair with
`services.lodestar-server.promote`, or set it to `null` to hide it.

## Limitations

- No option yet to advertise a Tailscale tag on agent hosts.
- One server; the dashboard has no history of past host states beyond the
  audit log.

More: `docs/operations.md` (day-to-day use, file locations, troubleshooting);
`docs/plans/` (the original design, written when Lodestar lived in a personal
NixOS configuration).

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT), at your option.
