# Service links: design

A **Services** tab in the dashboard: every web application the fleet hosts, with its URL,
grouped, and with a live state per link (an HTTP check plus the state of the host it runs
on). The links are declared in Nix next to the services, collected from the flake like the
host list, and reach the server through the normal apply, so the tab follows git.

Status: approved in conversation 2026-10-13. Consumer side (the declarations in the
`nixos-config` flake): `nixos-config/docs/plans/2026-10-13-homelab-dashboard-design.md`.

## Goals

- One place, in Lodestar's look, with every hosted application and a working link to it.
- Declared as code: a link lives in the module that runs the service and disappears with it.
  Services on machines not (yet) managed by Nix can be listed by hand.
- At a glance: is it up, how fast, and is the machine it runs on on.

Not in scope: check history or graphs, alerting/notifications, a "check now" button,
editing links from the UI, service widgets (counts, stats) like Homepage's.

## Data model (Nix)

### `lodestar.links.<id>` (every host)

A plain data option, defined by lodestar and imported on every NixOS and darwin
configuration (`nixosModules.links`, `darwinModules.links`; also part of
`nixosModules.default` / `darwinModules.default`). It configures nothing by itself.

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `title` | str | the attribute name | Card title |
| `url` | str | (required) | What the card links to |
| `group` | str | `"Services"` | Section the card is shown in |
| `description` | nullOr str | `null` | One line under the title |
| `icon` | nullOr str | `null` | A Simple Icons slug (`"jellyfin"`) or a Lodestar/Lucide icon name; `null` shows the title's first letter |
| `host` | nullOr str | `null` | Machine the service runs on; `null` = the configuration that declares it (filled in by `linksFrom`) |
| `check.enable` | bool | `true` | Probe this link |
| `check.url` | nullOr str | `null` | URL to probe instead of `url` (e.g. an internal address when the public name hairpins) |
| `check.expectStatus` | listOf int | `[ 200 204 301 302 303 307 308 401 403 ]` | Statuses that count as up (a login page is up) |
| `check.insecureTls` | bool | `false` | Accept self-signed certificates (PVE, TrueNAS) |

The id (attribute name) is the link's identity: stable, used as the React key and in
`links.json`.

### `lodestar.lib.linksFrom { nixosConfigurations ? {}, darwinConfigurations ? {} }`

In `nix/lib/linksFrom.nix`, next to `hostsFrom`. Returns `{ <id> = link; }` from every
configuration's `config.lodestar.links` (configurations without the option are skipped),
with `host` defaulted to that configuration's `services.lodestar-agent.hostName` (the
option exists wherever the modules are imported and defaults to `networking.hostName`, so
the name matches `hostsFrom`; an empty name fails evaluation). An id
declared by two configurations fails evaluation with
`lodestar.lib.linksFrom: link ids declared by more than one configuration: …`.

### `services.lodestar-server.links`

`attrsOf linkType` (the same submodule, `host` required by then), default `{ }`. The module
writes `/etc/lodestar-server/links.json` (a list sorted by id, snake_case keys like
`hosts.json`) and passes `links_file` in `config.json`. Usual value:

```nix
services.lodestar-server.links = lodestar.lib.linksFrom {
  inherit (self) nixosConfigurations darwinConfigurations;
};
```

As with `hosts.json`, a change restarts the server on switch (it is part of the unit's
`restartTriggers`).

## Server

### Config

`config/link.rs`: `Link { id, title, url, group, description, icon, host, check: LinkCheck }`
read from `links_file` at start. A missing `links_file` key means no links (older configs
keep working); a file that does not parse stops startup with the path and the serde error,
as `hosts.json` does.

### The checker

`poller/service_check.rs` (spawned beside the host poll loop, also in demo mode with
simulated results):

- Every 60 s, probe every link with `check.enable`, at most 8 at a time
  (`futures::stream::buffer_unordered`). Interval and timeout are constants, not options.
- Probe: `GET` on `check.url` or `url`, 5 s total timeout, redirects **not** followed
  (`redirect::Policy::none()`), body not read. One shared `reqwest::Client` per TLS mode
  (normal and `danger_accept_invalid_certs`).
- Classification (pure function, unit-tested):
  - status in `expect_status` → `up`, with latency in ms;
  - other status → `down`, reason `http_status(n)`;
  - timeout → `down`, `timeout`; connect error → `down`, `connection_refused` or
    `unreachable` (DNS / no route); TLS error → `down`, `tls`; anything else → `down`,
    `other(message)`.
- Results live in `AppState` only (`HashMap<id, CheckResult>`), no database table. A result
  keeps `state`, `reason`, `latency_ms`, `checked_at`, `since` (when `state` last changed).
- Before a link's first probe its state is `pending`; with `check.enable = false` it is
  `unchecked`.
- Broadcast: after each round, when any link's `state` or `reason` changed, send
  `WsMessage::ServicesUpdated(Vec<ServiceView>)` with all services. A latency change alone
  is not broadcast (the next snapshot or change carries it), so an idle dashboard sees no
  traffic every minute.

### Views (lodestar-proto, `dashboard/`)

```rust
pub struct ServiceView {
    pub id: String, pub title: String, pub url: String, pub group: String,
    pub description: Option<String>, pub icon: Option<String>, pub host: String,
    /// The host's online state when it is a fleet host; None otherwise.
    pub host_online: Option<OnlineState>,
    pub check: ServiceCheck,
}
pub struct ServiceCheck {
    pub state: ServiceState,        // "up" | "down" | "pending" | "unchecked"
    pub reason: Option<DownReason>, // tagged enum, as above
    pub latency_ms: Option<u32>,
    pub checked_at: Option<i64>,    // unix seconds, like the other views
    pub since: Option<i64>,
}
```

- `Snapshot` gains `services: Vec<ServiceView>` (`#[serde(default)]`), sorted by group
  then title.
- `WsMessage` gains `services_updated`. When a host's `OnlineState` changes the server
  already sends `host_updated`; the web derives `host_online` from its host list then, so no
  extra message is needed (the server still fills `host_online` in snapshots).
- Bindings regenerated (`ServiceView.ts`, `ServiceCheck.ts`, `ServiceState.ts`,
  `DownReason.ts`).

No new HTTP endpoint and no CLI command in this change (`lodestar services` can come later).

## Web

### Navigation

`App.tsx` gets a tiny hash router (`lib/route.ts`: `"fleet" | "services"`, from
`location.hash`, `#/services`; default fleet). The header (`components/layout/`) shows two
tabs under the wordmark row, **Fleet** (`Machines` icon) and **Services** (`Services`
icon), `aria-current="page"` on the active one. The Services tab carries a small `bad`
count badge when any service is down. Both pages share the connection, so switching is
instant.

### `pages/ServicesPage.tsx`

- Summary tiles (`StatTile`): **up** (`ok`), **down** (`bad`), **checking** (`info`,
  pending), and only when non-zero, **not checked** (neutral).
- One section per `group`, groups sorted by name, with the standard section heading
  (serif, uppercase, rule after) and a count note.
- A grid of cards: `grid-template-columns: repeat(auto-fill, minmax(280px, 1fr))`, 12 px
  gap; single column on phones, never sideways scrolling.
- Empty state: "No services declared" with one line on `lodestar.links`.

### `components/services/ServiceCard.tsx` `{ service: ServiceView, now }`

- Left-edge 3 px state stripe in the service tone (as on host rows).
- Icon tile (the OS-tile style): `BrandIcon` when the slug is a known Simple Icon,
  otherwise the Lodestar icon of that name, otherwise the title's first letter in the serif.
  Brand icons in `currentColor`. Simple Icons stay tree-shaken: `BrandIcon` gets a small
  explicit map of the slugs we use (`jellyfin`, `nextcloud`, `proxmox`, `truenas`,
  `traefikproxy`, `restic`, `immich`, …), not the whole set.
- Title as a link (`target="_blank" rel="noopener"`), with the `External` icon; the URL
  under it in mono, truncated with the full URL in a tooltip.
- Description (muted, 12 px).
- Status badge and host line: `Up · 84 ms`, `Down: HTTP 502`, `Down: timed out`,
  `Checking…`, `Not checked`; "on media" with an `OnlineState` dot for fleet hosts, plain
  text otherwise. When the host is off or its agent is not answering, the host state is
  the stated cause: `Down · host off`.
- "checked 30 s ago" / "down since 2 h" in the sub line.

### Tones and words

`lib/tones.ts` gets `serviceStyle(service)` → `{ tone, icon }`:

| Case | Tone | Icon |
| --- | --- | --- |
| up | `ok` | `Up` (circle-check) |
| up, latency ≥ 1000 ms | `warn` | `Up` |
| down | `bad` | `Down` (circle-x) |
| down and host off / agent not answering | `warn` | `Off` / `Unreachable` |
| pending | `info` | `Building` (spins) |
| unchecked | `neutral` | `Idle` |

Labels in `lib/format.ts` (`serviceLabel`, `downReasonLabel`). New icon names in `@/icons`:
`Services` (layout-grid), `Up` (circle-check), `Down` (circle-x), `External`
(external-link). The style guide's state table, icon list and component list get these
additions.

### Data flow

`ConnectionProvider` stores `services` from the snapshot and replaces them on
`services_updated`; `useServices()` in `hooks.ts` returns them joined with the current
host list's online state. Components never fetch.

## Errors and edge cases

- A link's `host` that is no fleet host: no host state, the name as text.
- A fleet host that is off: its links are still probed (the service may run elsewhere,
  e.g. behind a proxy); the card shows the host state as the cause when the probe fails.
- Server restart: all links `pending` until the first round (within ~5 s of start).
- Probing is outbound from the server only to URLs in Nix config; no user-supplied URLs.

## Testing

- **Nix:** `checks.<system>.links-from`: `linksFrom` over two stub configurations: host
  defaulting, explicit `host` kept, duplicate id fails (`builtins.tryEval`).
- **Rust:** classification unit tests; checker tests against a local axum server on an
  ephemeral port (200 up, 401 up, 502 down, 302 not followed, sleep → timeout, closed port
  → refused); `since` only moves on a state change; broadcast only on change.
- **Web (vitest):** `serviceStyle` table, `downReasonLabel`, grouping/sorting,
  `ServiceCard` render (link, badge words, host-off cause), route parsing, the tab count.
- **VM test:** the server gets one link to the agent VM's port and one to a closed port;
  assert `/snapshot` reaches `up` and `down` for them.
- **Demo:** simulated services in every state, so the tab can be reviewed with
  `nix run .#demo`.
- `cargo clippy`, `cargo fmt --check`, `tsc`, `nix flake check` as usual.

## Docs

`docs/operations.md`: declaring links, `linksFrom`, what the check counts as up.
`docs/style-guide.md`: Services page, `ServiceCard`, the new tones, icons and copy.
`README.md`: one line in the feature list.
