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
            hosts = lodestar.lib.hostsFrom { inherit (self) nixosConfigurations; };
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
| `nixosModules.agent`, `nixosModules.server`, `nixosModules.default` (both) | NixOS modules |
| `darwinModules.agent`, `darwinModules.default` | nix-darwin module (agent only) |
| `lib.hostsFrom { nixosConfigurations ? {}, darwinConfigurations ? {} }` | `services.lodestar-server.hosts` from every configuration that enables the agent, keyed by `hostName` |
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
| `channel` | `"main"` | branch this host follows |
| `managerHost` | (required) | Tailscale name of the server's host; the only accepted source |
| `repoUrl` | (required) | git URL of the flake repository (`origin`) |
| `flakePath` | `~user/.flake` (`/Users/<user>/.flake` on darwin) | the checkout |
| `flakeAttr` | `nixosConfigurations.<hostName>.config.system.build.toplevel` / `darwinConfigurations.<hostName>.system` | what is built and switched to |
| `port` | `7461` | opened on `tailscale0` (NixOS) |
| `listen` | `null` (the Tailscale addresses) | explicit `ip:port` |
| `package`, `cliPackage` | this flake's | |
| `installCli` | `true` | put `lodestar` on the PATH, with `LODESTAR_SERVER` set to `serverUrl` |
| `serverUrl` | `null` = `http://<managerHost>:8080` (the local `listenPort` on the server's host) | where the CLI finds the server; set it if the server uses another port |
| `rebuildCommand`, `revisionCommand` | `null` | tests only |

`services.lodestar-server` (NixOS):

| Option | Default | |
|---|---|---|
| `enable` | `false` | |
| `hosts` | `{}` | `attrsOf { channel; os; system; user; }`, usually `lodestar.lib.hostsFrom { ... }` |
| `repoUrl` | (required) | mirrored read-only |
| `pushUrl` | `null` | where promote pushes; required unless `promote = null` |
| `deployKeyFile` | `null` | SSH key with write access to `pushUrl`, readable by `lodestar-server` (outside the store, e.g. sops-nix) |
| `listenAddress` | (required) | this host's Tailscale IP; anything else is refused |
| `listenPort` | `8080` | opened on `tailscale0` |
| `allowedUsers` | `[]` | Tailscale logins allowed in |
| `allowedTags` | `[]` | Tailscale device tags allowed in |
| `agentPort` | the agent's `port`, else `7461` | one port for all agents: keep `services.lodestar-agent.port` the same everywhere |
| `promote` | `{ from = "unstable"; to = "stable"; }` | `null` disables promote |
| `package` | this flake's | |
| `testing.*` | | tests only |

The server needs `services.tailscale` on its host; the module makes the
`lodestar-server` user a Tailscale operator (`--operator`) so it can run
`tailscale status` and `tailscale whois`.

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
lodestar audit [--limit 20]
```

The server URL comes from `--server`, `LODESTAR_SERVER` (set system-wide by
the agent module, see `serverUrl`), or `~/.config/lodestar/config.toml`:

```toml
server = "http://manager:8080"
```

`rebuild --wait` keeps retrying while the server is restarting (e.g. when the
apply switched the server's own host).

## Promote

With two channels (`unstable` for workstations, `stable` for servers), promote
fast-forwards `stable` to a commit of `unstable` that already ran fine
somewhere. It is a non-force push from the server to `pushUrl`, refused unless
`stable` is an ancestor of the commit. Configure the pair with
`services.lodestar-server.promote`, or set it to `null` to hide it.

## Limitations

- The darwin agent (launchd) has not run on a real Mac yet.
- No option yet to advertise a Tailscale tag on agent hosts.
- One server; the dashboard has no history of past host states beyond the
  audit log.

More: `docs/operations.md` (day-to-day use, file locations, troubleshooting);
`docs/plans/` (the original design, written when Lodestar lived in a personal
NixOS configuration).

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT), at your option.
