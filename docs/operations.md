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
get in). Use the server's Tailscale IP or a name in `allowedHostnames` (default: the
server's `networking.hostName`, its MagicDNS short name); add the MagicDNS FQDN
(`manager.<tailnet>.ts.net`) and whatever name the agents' `managerHost` / `serverUrl`
uses, or the CLI on those hosts is refused too. Any other `Host` gets a plain 403
`forbidden` and logs `lodestar-server: refused a request for host ...`: this stops a page
on another site from reaching the dashboard by pointing its own DNS name at the server
(DNS rebinding). Names are matched exactly, never resolved.

```sh
lodestar status                          # host, os, channel, online, running, behind, last apply
lodestar rebuild desktop --wait          # force rebuild: re-apply the channel head even if the host
                                         # already runs it; exit 0 only if it ended ok
lodestar rebuild --channel stable --wait [--timeout 900]
lodestar log desktop [APPLY_ID]          # default: the host's last apply
lodestar promote [--yes]                 # shows the candidate; --yes moves the target channel
lodestar updates [run [--wait]]          # tracked software; run the updater now
lodestar build status                    # each channel's build on the builder (build gate)
lodestar build retry <commit>            # build the failed hosts of <commit> again
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
A browser's `Origin`, when sent (on POSTs and the WebSocket), must be the dashboard's own
(`http://` + the `Host` it was loaded from); a request without one (curl, the CLI) is
allowed. Every response carries `X-Frame-Options: DENY`,
`Content-Security-Policy: frame-ancestors 'none'` and `X-Content-Type-Options: nosniff`:
the dashboard cannot be framed by another page.

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
WebSocket `/api/ws`, which also needs the public URL as its `Origin`; so does every
state-changing request that sends an `Origin`), the act routes,
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
- Pass the browser's `Origin` through unchanged. The server does not check `Host` on this
  listener (only trusted proxies reach it): the router must route by the public host.
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

## Software updates

The server can keep chosen software (first: Tailscale) current: it knows which version
every host runs, which release is out upstream, and once a day it bumps the flake input
that provides the software and pushes the new `flake.lock` when something got newer and
everything still builds. It only moves inputs; it never overrides a package.

```nix
# every host: what it runs, by software name
services.lodestar-agent.versions.tailscale = config.services.tailscale.package.version;

# the server host
services.lodestar-server = {
  updates.software.tailscale = {
    input = "nixpkgs-unstable";             # the flake input that provides it
    upstream.github = "tailscale/tailscale";
  };
  updates.channel = "unstable";             # default; never the promote target
  updates.intervalSecs = 86400;             # default: one run a day
  updates.upstreamIntervalSecs = 21600;     # default: check releases every 6 h
  metrics.address = "10.0.10.170";          # optional Prometheus listener, port 9466
};
```

Pushing needs `pushUrl` and `deployKeyFile`, as for promote. `updates.software = { }`
(the default) turns the updater and the upstream checks off; that is also how to stop it
(switch the server host; the card disappears and `lodestar updates run` answers 409
`updater off`).

### A run

One run at a time, for every input that some tracked software uses, one after the other:

1. A scratch worktree of the channel's head under `/var/lib/lodestar-server/updater/<input>`
   (removed afterwards).
2. Every host's `versions` at the current lock (`nix eval`).
3. `nix flake update <input>`; lock unchanged: `no-change`.
4. The versions again; nothing tracked got newer on any host (or something got older):
   `no-change`, nothing is committed.
5. Build every Linux host whose tracked versions changed (darwin hosts are evaluated
   only).
6. Commit `flake.lock` only, as `Lodestar <lodestar@vanwaaij.com>`, message
   `chore(<input>): bump for tailscale 1.98.10 -> 1.104.2`, and push fast-forward only,
   if the channel's head is still the one from step 1.

The first scheduled run is one interval after a fresh server starts (never at boot);
after that the next run is one interval after the newest finished run, also across
restarts (an overdue run starts at once). A run started with "Check now" or `lodestar
updates run` counts. Every run is an `update` audit event.

```sh
lodestar updates                         # software, latest release, input, last result, age
lodestar updates run                     # start a run now (409 while one is running)
lodestar updates run --wait [--timeout 7200]   # exit 0 only if every input ended pushed or no-change
```

The dashboard's **Software updates** card (shown only while software is tracked) has the
same and a **Check now** button; the fleet table shows each host's versions, marked when
behind the latest release.

### Results

- **pushed**: a bump is on the channel; hosts get it through the normal apply (and
  servers through promote).
- **no-change**: the lock did not move, or it moved but no tracked version got newer
  (or one got older, noted as `downgrade` in the audit event). Nothing to do.
- **eval-failed**: reading a host's versions failed (the host is named), or the input
  could not be updated or left no `flake.lock` (no host). A host or input problem:
  run the `nix eval` of that host on the channel's head, fix it in the repository; the
  next run retries.
- **build-failed**: a changed host did not build with the new lock; nothing was pushed.
  `lodestar updates` and the audit event name the host, the event holds the log tail
  (`lodestar audit`, dashboard audit log). Fix the host or wait for upstream; the next
  run retries.
- **push-failed**: the worktree could not be made or the push did not happen. "channel
  moved" means someone pushed to the channel during the run: nothing was forced, and the
  next run starts from the new head. Any other reason (key, network, remote refusal)
  needs a look at `journalctl -u lodestar-server` and `deployKeyFile`.

### Metrics

With `metrics.address` set, `GET http://<address>:9466/metrics` (no identity check;
the module does not open the port, open it to your Prometheus only):

| Metric | Labels | Value |
|---|---|---|
| `lodestar_software_running_info` | `host`, `software`, `version` | 1 |
| `lodestar_software_latest_info` | `software`, `version` | 1 |
| `lodestar_software_up_to_date` | `host`, `software` | 1 up to date, 0 behind; absent when unknown |
| `lodestar_software_latest_checked_timestamp_seconds` | `software` | unix time of the last successful upstream check |
| `lodestar_software_latest_check_errors_total` | `software` | counter of failed upstream checks since start |
| `lodestar_update_last_run_timestamp_seconds` | `input` | unix time of the last finished run |
| `lodestar_update_last_result` | `input`, `result` | 1 for the last run's result |

A failed upstream check (GitHub rate limit, error, no stable release) keeps the last
known version and does not advance the checked timestamp.

## Build cache

The cache host (`services.lodestar-cache`, `nixosModules.cache`) is the machine the
server builds on (`services.lodestar-server.builds`, ssh-ng as `lodestar`) and the file
binary cache every Linux host substitutes from. It holds only paths **built there**:
everything signed by cache.nixos.org stays there, and clients fetch each path
independently from whichever substituter has it.

- **Serving**: nginx serves `cacheDir` at `/nix/` on `listen.port`, opened to
  `allowedClients` only (e.g. the reverse proxy). `*.narinfo` is sent with
  `Cache-Control: no-cache`; dotfiles (`.roots/`, `.lock`, temporary files) are 404.
  `nix-cache-info` says `Priority: 50`, so clients try cache.nixos.org (40) first.
- **Publish** (`cache-publish <commit> <store-path>…`, run by the server after a build,
  channel index JSON on stdin): validates the arguments like the server (40-hex commit,
  plain store paths) and the index, takes `<cacheDir>/.lock`. A given path whose narinfo
  the cache has already is not looked up in the local store at all (the builder may have
  collected it): it and the closure the cache has of it (the narinfos' `References`)
  go into the roots as they are. For the other paths it keeps the closure's
  paths that are `ultimate` (built on this host) or signed with the cache's own key
  (substituted back from this cache after a store GC), and for each one not yet in the cache signs it
  in the local store (`nix store sign`), writes `nar/<filehash>.nar.zst` and
  `<hash>.narinfo` (both via temp file and rename; the given paths last, so a top-level
  narinfo exists only with its built-here closure). It does not use `nix copy`: a file
  cache refuses a path whose references it lacks. Then it **merges** the paths into
  `.roots/<commit>` (a commit is published in several calls when hosts are built in
  different rounds, e.g. after `lodestar build retry`), writes the index to
  `.roots/channels.json` and prunes, keeping `<commit>` as well.
- **Prune** (`cache-prune [dir [keep-commit]]`, after every publish and daily via
  `lodestar-cache-prune.timer`): the keep set is the union of `.roots/<commit>` for the
  `current` and `previous` commit of every channel in `channels.json` (and
  `keep-commit`, which `cache-publish` passes). Every other
  narinfo is deleted with its NAR (only plain files under `nar/`, never a NAR a kept
  narinfo also names), then the `.roots` files of unkept commits. A missing or
  unreadable index, an invalid commit in it, or a kept commit without a roots file
  deletes nothing. The paths of a commit the index does not keep (an older commit built
  for a re-send, a bump whose other channel is not built yet) stay until the next
  publish of another commit; that is intended.
- **Where**: `cacheDir` (`nix-cache-info`, `*.narinfo`, `nar/`, `.roots/`),
  `journalctl -u lodestar-cache-prune`; publish output lands in the server's build log.
- **Recovery**: the cache is rebuildable. If it is lost or corrupt, empty `cacheDir`
  (keep the mount), then on the server remove the gate's index and restart:
  `systemctl stop lodestar-server; rm /var/lib/lodestar-server/builds/channels.json;
  systemctl start lodestar-server`. The prebuild at start finds each channel head built
  but not in the index, realises every host of the channel on the builder again
  (instant for what its store still has, rebuilt for what its GC removed), advances the
  channel and publishes all its paths. A restart alone republishes nothing: the index
  still names the heads, so the gate has nothing to advance. Each channel's `previous`
  commit is not kept until its next head. Clients build or use cache.nixos.org
  meanwhile. A narinfo whose NAR is missing: delete the narinfo and recover as above. A
  new signing key: replace `signingKeyFile`, empty the cache, update the clients'
  `trusted-public-keys`, recover as above.

## Build gate

With `services.lodestar-server.builds.enable`, every Linux host's system for a commit is
built once on the builder (`builds.store`, ssh-ng as the cache host's `lodestar` user) and
published to its cache before any agent may apply that commit; darwin hosts are never
built here (they build themselves, and their applies never wait). Every path to building
goes through the gate:

- **Prebuild**: after each mirror fetch (and once at server start) every channel whose
  head moved is built in the background, one at a time.
- **Apply** (dashboard, `lodestar rebuild`, host or channel): the server waits for the
  build of the channel's head for **every Linux host of the channel**, then records and
  sends the apply. `lodestar rebuild` prints `waiting for the build of <commit> on the
  builder…` meanwhile and waits up to two hours for the answer.
- **Re-send** of a desired apply the agent never got: built first, in the background.
- **Updater**: builds the changed Linux hosts on the builder before it pushes and records
  the results for the pushed commit.

States of a channel's head (`lodestar build status`, dashboard channel card and Builds
tab, `GET /api/builds`):

- **building**: the gate is building it for this channel right now.
- **ok**: every Linux host of the channel built.
- **failed**: at least one Linux host failed to build (the hosts are listed). Results are
  final: an apply of that commit on that channel is refused with HTTP 409 `build of
  <commit> failed on the builder: <host>, …` and an `apply-refused` audit event (with
  `commit`, `channel`, `failed_hosts`), and nothing is sent to any agent. Fix the config
  and push, or, for a failure that was not the commit's fault (out of disk, a flaky
  download), `lodestar build retry <commit>` (audit `build-retry`): the failed hosts are
  forgotten and, when the commit is a channel head, rebuilt at once; otherwise at the next
  apply.
- no state: the head has no complete result yet (not built, or its build could not run).

A build that **could not run** is not a result: when the builder does not answer (the
cache VM reboots, ssh fails) or the publish fails, nothing is recorded for those hosts
and an apply is refused with 409 `build of <commit> failed: builder unreachable: …` or
`build of <commit> failed: publish failed: …` (audit `apply-refused` with `error`). No
`lodestar build retry` is needed: the next apply builds and publishes those hosts again,
and the prebuild and the re-send try such a commit again on their own after a pause per
channel and commit that doubles from 30 s to 10 min (checked at each mirror fetch and
poll).

The channel index (what the cache keeps) moves to a commit only when it is the channel's
current head and every Linux host of the channel is ok there; each move publishes every
path of the channel again, so the cache has the whole head even when its hosts were
built in several rounds (an updater bump, a retry, a channel reset to an earlier
commit). The builder keeps no GC roots for published paths, so before such a move the
gate **realises every host of the channel again** on the builder, in the same call as
any host still to build (instant for what its store still has, a rebuild or a
substitution from the cache otherwise), and only then publishes. That call counts like
a build: an unreachable builder records nothing and the channel does not move (409,
backoff as above); a host that fails now is recorded as failed. Once the index names the
commit, later prebuilds and applies of it neither build nor publish.

Files in `/var/lib/lodestar-server/builds/`:

- `<commit>.json`: the result per host (`ok`, store `path`, `log_tail` of a failure,
  `built_at`), written atomically after build and publish.
- `<commit>.lock`: the commit's lock (`flock`), held for check, build, publish and write;
  a second caller waits on it. Released by the kernel if the server dies.
- `channels.json` (+ `channels.lock`): the index, `{ <channel>: { current, previous } }`,
  sent with every publish; results older than 30 days that it does not name are deleted.
- `work/<commit>`: the worktree of a build in progress; removed after the build and at
  server start.

Metrics (with `metrics.address`), per channel with a complete result for its head:

| Metric | Labels | Value |
|---|---|---|
| `lodestar_build_last_result` | `channel`, `result` | 1 for `ok` or `failed` |
| `lodestar_build_last_timestamp_seconds` | `channel` | unix time the last host result of the head was recorded |
| `lodestar_build_duration_seconds` | `channel` | seconds the last build the server ran for the channel took (absent until one ran since start) |

The dashboard's **Builds** tab (`#/builds`, `GET /api/builds/recent`) lists the 20 newest
commits: those building now first, then results by when they were written. Each shows its
subject, the channels it is the head of and the channels whose cache keeps it, and every
Linux host with its store path, or, for a failed host, its log behind "Show log". Retry
build sits on any commit with a failed host. The tab carries a red count of channels
whose head failed. It asks the server every 10 s while open, and at once when a channel's
build state changes.

Build logs: the Builds tab, `journalctl -u lodestar-server` (prebuild and re-send
failures, `apply refused` lines) and the `log_tail` in the result file. A failure keeps
the errors that are not about a failed dependency (up to 80 lines: Nix prints the real
error once and then one "Cannot build … dependency failed" per dependent) followed by the
last 40 lines.

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
- tracked versions: `/etc/lodestar/versions.json` (from `versions`)

Server:

- config: `/etc/lodestar-server/config.json`, hosts: `/etc/lodestar-server/hosts.json`,
  service links: `/etc/lodestar-server/links.json`
- state: `/var/lib/lodestar-server/` (`lodestar.db`, `mirror.git`, ssh `known_hosts`,
  `updater/<input>` while an updater run works on that input, `builds/` for the build
  gate)
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
- **Plain 403 `forbidden` for a name that resolves to the server**: the name is not in
  `allowedHostnames` (see "Applying").
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
  write access to `pushUrl`, readable by the `lodestar-server` user. The updater's
  `push-failed` (other than "channel moved") has the same causes.
- **Updater results**: see "Software updates" above.

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
