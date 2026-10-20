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

## Service links

The **Services** tab lists the web applications the fleet hosts, with a link to each and a
live state. Links are declared in Nix, next to the service that runs, and follow git like
everything else.

```nix
# in any host's configuration (NixOS or nix-darwin)
lodestar.links.jellyfin = {
  title = "Jellyfin";                       # default: the attribute name
  url = "https://jellyfin.example.com";
  group = "Media";                          # default: "Services"
  description = "Films and series";
  icon = "jellyfin";                        # Simple Icons slug or a Lodestar/Lucide icon name
  check.url = "http://localhost:8096/";     # probe this instead of `url` (hairpin NAT)
  check.insecureTls = true;                 # accept a self-signed certificate (PVE, TrueNAS)
  check.expectStatus = [ 200 404 ];         # statuses that count as up (here: 404 too)
};
```

`host` defaults to the declaring configuration's `services.lodestar-agent.hostName`; set it
for services that run elsewhere. `check.enable = false` shows the link without probing it.
Known brand icons are `jellyfin`, `nextcloud`, `proxmox`, `truenas`, `traefikproxy` and
`immich`; any other name falls back to a Lodestar icon of that name, then to the title's
first letter.

On the server host, collect them from the flake like the host list (a duplicate id fails
evaluation); links for machines Nix does not manage can be added by hand with the same
options:

```nix
services.lodestar-server.links =
  lodestar.lib.linksFrom { inherit (self) nixosConfigurations darwinConfigurations; };
```

This writes `/etc/lodestar-server/links.json`; a change restarts the server on switch, and
reaches it through the normal apply of the server host.

The server probes every link with `check.enable` every 60 s (at most 8 at a time) with a
`GET` and a 5 s timeout. Redirects are not followed and the body is not read. A link is
**up** when the status is in `check.expectStatus` (default 200 204 301 302 303 307 308 401
403, so a login page counts as up); otherwise it is **down**, with the reason: HTTP status,
timed out, connection refused, unreachable, TLS error. Before the first probe a link is
"checking"; with `check.enable = false` it is "not checked". If the host is off or its
agent does not answer, a down link says so as the cause. If the probe clients cannot be
built, checks are disabled with a log line.

Results live in server memory only (no history) and are sent to the dashboard when a state
changes. The sub line counts from the last state change ("up since 3h", "down since 2h").
After a server restart all links show "checking" until the first round, "down since" counts
from the restart, and the latency shown is from the last state change, not the last probe.

## Public URL

The server can also serve the dashboard behind a reverse proxy (the "router entrance"),
for a public URL such as `https://lodestar.example.com`. The tailnet listener and the CLI
are unchanged. On the router entrance:

- **anonymous** visitors see a reduced Services feed (`/api/public/services`: title,
  group, description, icon, `up`/`down`/`unknown` and since when) and a **Log in** button;
- **readers** (logged in, in `readerGroup`) see the whole dashboard read-only;
- **admins** (in `adminGroup`, which wins over `readerGroup`) can also apply and promote,
  but only on requests the router marks as coming over Tailscale; elsewhere the buttons
  are disabled with "Only over Tailscale".

Routes: the dashboard itself, `/api/me` (who you are and whether you can act),
`/api/public/services`, the read routes (`/api/snapshot`, apply logs, `/api/audit`, the
WebSocket `/api/ws`, which also needs the public URL as its `Origin`), the act routes,
and `/auth/login`, `/auth/callback`, `/auth/logout`. Refusals are JSON `{"error": …}`:
401 `log in first` (anonymous on a read or act route), 403 `only admins can do this`, 403
`only over Tailscale`.

### Router contract

- Forward the public host to `proxyListen` (`http://<address>:<port>`, default port
  8081), from an address in `trustedProxies`. The firewall opens that port for those
  sources only; the server answers every other peer with a plain 403 `forbidden`.
  Forward to the literal `proxyListen` address, not a host name, and list the router's
  source address in `trustedProxies` in the same address family: a router that connects
  over IPv6 while `trustedProxies` holds its IPv4 address (or the reverse) is refused.
- On the tailnet entrance, set the header `X-Homelab-Via: tailnet`. On the public
  entrance, remove any `X-Homelab-Via` the client sent. The server trusts the header from
  trusted peers only, and only as a single header with the exact value `tailnet`; no
  header, or several, counts as unmarked.
- The router terminates TLS; the server sets `__Host-` cookies (`Secure`), so the public
  URL must be `https://` (the server refuses to start with an `http://` one).

### Options

```nix
services.lodestar-server = {
  proxyListen.address = "10.0.10.170";      # LAN address; null (default) turns it off
  proxyListen.port = 8081;
  trustedProxies = [ "10.0.10.110" ];       # the router(s)
  publicUrl = "https://lodestar.example.com";
  oidc = {
    issuer = "https://id.example.com/oauth2/openid/lodestar";
    clientId = "lodestar";                  # default
    clientSecretFile = config.services.onepassword-secrets.secretPaths.lodestar-oidc;
    readerGroup = "lodestar-users@id.example.com";
    adminGroup = "admins@id.example.com";
  };
};
```

The secret file must be readable by the `lodestar-server` user; it is read at each login
and surrounding whitespace (a trailing newline) is ignored.

### The Kanidm client

- Issuer: `https://<kanidm>/oauth2/openid/<client id>` (the server reads its discovery
  document, cached 1 h, as are Kanidm's signing keys: after a key rotation, logins can
  fail for up to an hour, or until the server restarts).
- Redirect URL (`originUrl`): `<publicUrl>/auth/callback`, exactly; landing:
  `<publicUrl>`.
- `preferShortUsername = true`, so the user is `alice`, not `alice@id.example.com`
  (the server takes `preferred_username`).
- Scope maps: `openid profile groups` for every group that may log in. A user whose
  groups have no scope map is refused by Kanidm itself.
- Group names in the options are the values of the ID token's `groups` claim, compared
  exactly: Kanidm sends each group as `<name>@<kanidm domain>` (and as its UUID), e.g.
  `lodestar-users@id.example.com`.
- ID tokens signed ES256 (Kanidm's default) or RS256 are accepted; PKCE is always used.

### Sessions

A login creates a session of 12 hours (cookie `__Host-lodestar_session`, stored hashed in
`lodestar.db`). The role is taken from the groups at login: a group change takes effect at
the next login. A new login in a browser ends the session that browser had before.
**Log out** deletes the session; an open dashboard's WebSocket closes within a minute
after its session is gone. Pending logins (between `/auth/login` and the callback) live
10 minutes in memory: a server restart in between shows "Login failed" with **Try
again**. At most 256 are kept (the oldest dropped), so anonymous clients flooding
`/auth/login` can make a real login fail the same way. Starting a second login in another
tab replaces the first one's login cookie: the first tab's callback then shows "Login
failed".

### Troubleshooting

- Every refused login logs `lodestar-server: login refused: <reason>`
  (`journalctl -u lodestar-server`), for example `alice is in no Lodestar group`
  (check the group names against the `@domain` form), `unknown or expired login state`
  (a restart, a replayed or bookmarked callback), `issuer unreachable: …`, `rejected
  (ID token)`. A successful one logs `lodestar-server: login: alice as reader`.
- "Can't reach the login service": the server cannot fetch the issuer (DNS, TLS trust,
  Kanidm down).
- "No access": the user is in neither group (or Kanidm sent the login back with an
  error). A user without a scope map is stopped by Kanidm before that, on Kanidm's own
  page.
- 403 `forbidden` on everything: the request did not come from a `trustedProxies`
  address (the router forwards from another address, or the request skipped it).
- Admin buttons disabled with "Only over Tailscale" while on the tailnet: the router
  did not set `X-Homelab-Via: tailnet` on that entrance.

## Where things are

Agent (every enrolled host):

- config: `/etc/lodestar-agent/config.json`
- state: `/var/lib/lodestar-agent/` (`applies/<id>.{json,log}`, `activations.jsonl`,
  `apply-in-progress.json` while an apply runs)
- binary: `/var/lib/lodestar-agent/bin/lodestar-agent` (`binaryPath`), copied from
  the store on activation when the package changed (`bin/.lodestar-agent.source`
  holds the store path it came from); the daemon and the apply wrapper run it
- darwin signing: `/var/lib/lodestar-agent/signing/` (`key.pem`, `cert.pem`,
  `cert.sha1`; root, 0700; with `macos.stableSigning.certificate` the key is
  placed with `sudo lodestar-agent-signing-key < key.pem`); check with
  `codesign -dr - /var/lib/lodestar-agent/bin/lodestar-agent`
- NixOS: `systemctl status lodestar-agent`, applies run as transient units
  `lodestar-apply-<id>` (`journalctl -u 'lodestar-apply-*'`)
- darwin: launchd label `org.lodestar.agent`, log `/var/log/lodestar-agent.log`

Server:

- config: `/etc/lodestar-server/config.json`, hosts: `/etc/lodestar-server/hosts.json`,
  service links: `/etc/lodestar-server/links.json`
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
- **macOS keeps asking for permissions after each update**: check
  `codesign -dr - /var/lib/lodestar-agent/bin/lodestar-agent` shows
  `identifier "org.lodestar.agent" and certificate leaf = H"..."` (not ad-hoc)
  and look for `lodestar-agent: warning:` lines in the switch output. To start
  over with a new certificate (only without `macos.stableSigning.certificate`;
  with it, a missing key is reported as `no signing key`):
  `sudo rm -rf /var/lib/lodestar-agent/signing`,
  switch, `sudo launchctl kickstart -k system/org.lodestar.agent`, and allow the
  prompts once more (old entries in System Settings → Privacy & Security can be
  removed). Automatic granting needs an MDM PPPC profile (not supported).
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
nix build .#checks.aarch64-darwin.agent-module   # darwin module eval + shellcheck of the activation snippets
```

Linux-only checks need an x86_64-linux builder (KVM for the VM test):
`checks.x86_64-linux.vm` (server, two agents and a git remote in VMs, plus Kanidm and a
router stand-in that log in through the router entrance) and
`checks.x86_64-linux.firewall`.
