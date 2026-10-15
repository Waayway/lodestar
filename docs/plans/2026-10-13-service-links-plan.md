# Service links (Services tab) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A Services tab in the dashboard listing every link declared in Nix
(`lodestar.links`), grouped, with a live HTTP check and the host's online state.

**Architecture:** Nix collects `lodestar.links` from every configuration
(`lib.linksFrom`) into `services.lodestar-server.links`, written to
`/etc/lodestar-server/links.json`. The server loads it, probes each link every 60 s in a
background task, keeps results in `AppState`, adds `services` to the `Snapshot` and sends
`services_updated` on change. The web gets a hash route and a Services page built from the
existing tokens, tones and components.

**Tech Stack:** Nix module system, Rust (axum 0.8, reqwest 0.12 rustls, tokio, ts-rs),
Preact + Tailwind v4 + vitest.

**Spec:** `docs/plans/2026-10-13-service-links-design.md` (read it first).

## Global Constraints

- Commands run through the dev shell (`nix develop -c …`, see `AGENTS.md` "Working").
- New files must be `git add`ed before Nix sees them.
- Bindings are generated, never edited: after a `lodestar-proto` change run
  `cd lodestar-proto && TS_RS_EXPORT_DIR=../web/src/bindings nix develop .. -c cargo test -p lodestar-proto`
  and remove any stray `lodestar-proto/bindings/`.
- The working tree may hold the user's uncommitted work (e.g. `nix/modules/agent/*`).
  Never stash/reset it; commit only your paths: `git commit -m "…" -- <paths>`.
- Commits: conventional (`feat(server): …`, `feat(web): …`, `feat(nix): …`), ending with
  `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`. Author
  `Thijs van Waaij <thijs@vanwaaij.com>` (check `git config user.email`).
- `links.json` contract (Nix writes, Rust reads): a JSON list sorted by id of
  `{"id","title","url","group","description","icon","host","check":{"enable","url","expect_status","insecure_tls"}}`;
  `description`, `icon`, `check.url` may be `null`.
- Default `expect_status`: `[200 204 301 302 303 307 308 401 403]`. Default `group`:
  `"Services"`. Default `title`: the attribute name.
- Check constants: interval 60 s (`CHECK_INTERVAL`), timeout 5 s (`CHECK_TIMEOUT`),
  concurrency 8 (`CHECK_CONCURRENCY`), redirects not followed, body not read.
  Slow threshold for the `warn` tone: 1000 ms (`SLOW_MS`, web only).
- Copy (exact): `Up · 84 ms`, `Down: HTTP 502`, `Down: timed out`,
  `Down: connection refused`, `Down: unreachable`, `Down: TLS error`, `Down: <message>`,
  `Checking…`, `Not checked`, `Down · host off`, `Down · agent not answering`, empty state
  title `No services declared`.
- Colours only via tokens/tone classes from `lib/tones.ts`; icons only from `@/icons` /
  `BrandIcon`; no emoji; every state has icon + words.

## Review Focus

- A link whose host is a fleet host that goes off **after** the snapshot: the card must show
  `Down · host off` from the `host_updated` message alone (web joins host state; test in
  Task 7).
- Many links timing out at once: a round must still take ≈ `ceil(n/8) × 5 s`, never block
  the host poller (separate task; test in Task 5 that a round with a hanging server
  finishes within timeout + margin).
- A link URL that redirects to an external login (302 to another host): counts as up and
  never follows to the other host (test in Task 4).
- An old `config.json` without `links_file`, or a server with no links: the server starts,
  `services` is `[]`, the tab shows the empty state (tests in Task 3 and Task 8).
- A very long URL or title on a phone: the card truncates the URL (tooltip has it) and
  the page never scrolls sideways (test in Task 8 checks the `truncate`/`min-w-0`
  classes and the tooltip text).

---

### Task 1: Nix — `lodestar.links`, `linksFrom`, server option

**Files:**
- Create: `nix/modules/links.nix`, `nix/lib/linksFrom.nix`, `nix/tests/links-from.nix`
- Modify: `flake.nix` (modules, `lib.linksFrom`, checks), `nix/modules/server.nix`

**Interfaces:**
- Produces: option `lodestar.links.<id>` (fields per spec table: `title`, `url`, `group`,
  `description`, `icon`, `host`, `check.{enable,url,expectStatus,insecureTls}`);
  `nixosModules.links`, `darwinModules.links`, both in the `default`s;
  `lib.linksFrom { nixosConfigurations ? {}, darwinConfigurations ? {} } -> attrsOf link`
  with `host` filled; `services.lodestar-server.links` (same submodule, `host` a `str`);
  `/etc/lodestar-server/links.json` (contract above); `links_file =
  "/etc/lodestar-server/links.json"` in `config.json`; `links.json` in `restartTriggers`.

- [ ] **Step 1: Write the failing check** `nix/tests/links-from.nix` (`{ pkgs, lib, linksFrom }` →
  `pkgs.runCommand` that fails unless all asserts hold). Build two stub configurations with
  `lib.evalModules` over `nix/modules/links.nix` plus a stub
  `options.services.lodestar-agent.hostName` (values `"alpha"`, `"beta"`):
  - alpha declares `a = { url = "http://a"; }` → result `a.host == "alpha"`,
    `a.title == "a"`, `a.group == "Services"`,
    `a.check.expectStatus == [200 204 301 302 303 307 308 401 403]`.
  - beta declares `b = { url = "http://b"; host = "pve"; }` → `b.host == "pve"`.
  - alpha and beta both declaring `dup` → `(builtins.tryEval (builtins.deepSeq r r)).success == false`.
  - a configuration without the option is skipped.
  Wire it as `checks.<system>.links-from` in `flake.nix`.

- [ ] **Step 2: Run it, expect failure**
  Run: `nix build .#checks.aarch64-darwin.links-from --no-link` (or the current system).
  Expected: evaluation error, `nix/lib/linksFrom.nix` missing.

- [ ] **Step 3: Implement** `nix/modules/links.nix` (pure options, no config; works on NixOS
  and nix-darwin), `nix/lib/linksFrom.nix` (mirror `hostsFrom.nix`: `filterAttrs (c:
  c.config ? lodestar)`, `host = if l.host != null then l.host else
  c.config.services.lodestar-agent.hostName`, assert non-empty host, assert no duplicate ids
  with message `lodestar.lib.linksFrom: link ids declared by more than one configuration: …`),
  export from `flake.nix` and add `links` to `nixosModules.default` imports and make
  `darwinModules.default` import `[ agentDarwin links ]`.

- [ ] **Step 4: Server module** — in `nix/modules/server.nix` add `links` (`attrsOf`
  the link submodule with `host = types.str`, default `{}`), write
  `environment.etc."lodestar-server/links.json".text` (list from `mapAttrsToList`, keys
  snake_case: `expect_status`, `insecure_tls`, plus `id`), add `links_file` to `config.json`
  and the file to `restartTriggers`. Reuse the submodule from `links.nix` (export it via a
  `let` file or `nix/lib/linkType.nix`) so the two can't drift.

- [ ] **Step 5: Verify**
  Run: `nix build .#checks.<system>.links-from --no-link && nix flake check --no-build --keep-going`
  Expected: success; `checks.x86_64-linux.firewall` and `agent-module` still evaluate.

- [ ] **Step 6: Commit** `feat(nix): lodestar.links option, lib.linksFrom and the server's links.json`

### Task 2: Proto — service views and `services_updated`

**Files:**
- Create: `lodestar-proto/src/dashboard/service_view.rs`
- Modify: `lodestar-proto/src/dashboard/{mod.rs,snapshot.rs,ws_message.rs}`, generated
  `web/src/bindings/*`

**Interfaces:**
- Produces (all `Serialize, Deserialize, Clone, Debug, PartialEq, TS`, `#[ts(export)]`,
  snake_case):
  - `enum ServiceState { Up, Down, Pending, Unchecked }`
  - `#[serde(tag = "kind", content = "detail")] enum DownReason { HttpStatus(u16), Timeout,
    ConnectionRefused, Unreachable, Tls, Other(String) }`
  - `struct ServiceCheck { state, reason: Option<DownReason>, latency_ms: Option<u32>,
    checked_at: Option<i64>, since: Option<i64> }` (`#[ts(type = "number | null")]` on the
    `i64`s, as `HostView.last_seen`)
  - `struct ServiceView { id, title, url, group, description: Option<String>, icon:
    Option<String>, host: String, host_online: Option<OnlineState>, check: ServiceCheck }`
  - `Snapshot.services: Vec<ServiceView>` with `#[serde(default)]`
  - `WsMessage::ServicesUpdated(Vec<ServiceView>)` → `{"type":"services_updated","data":[…]}`

- [ ] **Step 1: Failing tests** in `service_view.rs` / `ws_message.rs`:
  `down_reason_is_tagged` — `HttpStatus(502)` serialises to
  `{"kind":"http_status","detail":502}`, `Timeout` to `{"kind":"timeout"}`;
  `services_updated_is_tagged` — JSON contains `"type":"services_updated"`;
  `snapshot_without_services_parses` — an existing snapshot JSON without `services` parses
  with `services == []`.
- [ ] **Step 2: Run** `nix develop -c cargo test -p lodestar-proto` → FAIL (types missing).
- [ ] **Step 3: Implement** the types; fill `services: vec![]` where the server builds
  `Snapshot` so the workspace compiles (Task 5 fills it).
- [ ] **Step 4: Regenerate bindings** (Global Constraints command); `web/src/bindings/`
  gains `ServiceView.ts`, `ServiceCheck.ts`, `ServiceState.ts`, `DownReason.ts`; `Snapshot.ts`
  and `WsMessage.ts` change. Run `nix develop -c cargo test --workspace` and
  `cd web && nix develop .. -c npx tsc --noEmit` → PASS (fix test fixtures in
  `web/tests/helpers.tsx` that build a `Snapshot`: add `services: []`).
- [ ] **Step 5: Commit** `feat(proto): service views and services_updated`

### Task 3: Server config — load links

**Files:**
- Create: `lodestar-server/src/config/link.rs`
- Modify: `lodestar-server/src/config/{mod.rs,server_config.rs}`,
  `lodestar-server/src/state/app_state.rs`, `lodestar-server/src/server.rs`,
  `lodestar-server/src/main.rs`, `lodestar-server/src/demo/{start.rs,demo_handle.rs}`

**Interfaces:**
- Produces: `pub struct Link { pub id: String, pub title: String, pub url: String, pub
  group: String, pub description: Option<String>, pub icon: Option<String>, pub host:
  String, pub check: LinkCheck }`; `pub struct LinkCheck { pub enable: bool, pub url:
  Option<String>, pub expect_status: Vec<u16>, pub insecure_tls: bool }` (both
  `Deserialize, Serialize, Clone, Debug, PartialEq`); `pub fn load_links(path: &Path) ->
  anyhow::Result<Vec<Link>>`; `ServerConfig.links_file: Option<PathBuf>`
  (`#[serde(default)]`); `AppState.links: Arc<Vec<Link>>` (empty in `new`);
  `server::start(config, hosts, links: Vec<Link>, online, demo)`.

- [ ] **Step 1: Failing tests** in `link.rs`: `links_load` (temp file with one link in the
  contract shape → fields equal), `links_parse_error_names_file` (invalid JSON → error
  string contains the path). In `server_config.rs`, extend `config_defaults_and_tags`:
  `config.links_file == None`.
- [ ] **Step 2: Run** `nix develop -c cargo test -p lodestar-server config::` → FAIL.
- [ ] **Step 3: Implement** `link.rs` like `managed_host.rs` (`reading links file …` /
  `parsing links file …` contexts); `main.rs` loads `links_file` when set, else `vec![]`;
  pass to `server::start`, which sets `state.links = Arc::new(links)`; demo passes its own
  list (Task 6 fills it; `vec![]` for now).
- [ ] **Step 4: Run** `nix develop -c cargo test --workspace` → PASS.
- [ ] **Step 5: Commit** `feat(server): load links.json`

### Task 4: Probe and classification

**Files:**
- Create: `lodestar-server/src/poller/service_check.rs`
- Modify: `lodestar-server/src/poller/mod.rs`
- Test: `lodestar-server/tests/service_check.rs`

**Interfaces:**
- Consumes: `Link`, `LinkCheck` (Task 3); `ServiceState`, `DownReason` (Task 2).
- Produces:
  - `pub struct Probers { normal: reqwest::Client, insecure: reqwest::Client }` with
    `pub fn new() -> anyhow::Result<Self>` (both: `redirect(Policy::none())`,
    `timeout(CHECK_TIMEOUT)`, `insecure` adds `danger_accept_invalid_certs(true)`).
  - `pub struct ProbeOutcome { pub state: ServiceState, pub reason: Option<DownReason>,
    pub latency_ms: Option<u32> }`
  - `pub async fn probe(probers: &Probers, link: &Link) -> ProbeOutcome` (GET on
    `check.url.unwrap_or(url)`, latency measured to response headers).
  - `pub fn classify_error(error: &reqwest::Error) -> DownReason` (`is_timeout` →
    `Timeout`; `is_connect` with an `io::ErrorKind::ConnectionRefused` source →
    `ConnectionRefused`; other connect / DNS → `Unreachable`; a rustls error in the source
    chain → `Tls`; else `Other(error.to_string())`).
  - `pub const CHECK_INTERVAL: Duration = 60 s; CHECK_TIMEOUT = 5 s; CHECK_CONCURRENCY: usize = 8;`

- [ ] **Step 1: Failing tests** in `tests/service_check.rs` against an axum app on
  `127.0.0.1:0` with routes `/ok` (200), `/login` (401), `/bad` (502), `/redir` (302,
  `Location: http://10.255.255.1/` — an unroutable host, proving it is not followed),
  `/hang` (sleeps 10 s):
  - `ok_is_up` → `Up`, `latency_ms.is_some()`, `reason == None`
  - `unauthorized_counts_as_up` → `Up`
  - `bad_gateway_is_down` → `Down`, `Some(HttpStatus(502))`
  - `redirect_is_not_followed` → `Up` within 1 s
  - `hang_times_out` → `Down`, `Some(Timeout)`, elapsed < 6 s
  - `closed_port_is_refused` → bind then drop a listener, probe its port → `Down`,
    `Some(ConnectionRefused)`
  - `status_outside_expect_is_down` → `/ok` with `expect_status = [204]` → `Down`,
    `Some(HttpStatus(200))`
  - `check_url_overrides_url` → `url` closed port, `check.url` `/ok` → `Up`
- [ ] **Step 2: Run** `nix develop -c cargo test -p lodestar-server --test service_check` → FAIL.
- [ ] **Step 3: Implement** in `service_check.rs`; export from `poller/mod.rs`.
- [ ] **Step 4: Run** the test again → PASS; `cargo clippy --workspace --all-targets -- -D warnings` clean.
- [ ] **Step 5: Commit** `feat(server): HTTP probe for service links`

### Task 5: Checker loop, state, snapshot and broadcast

**Files:**
- Modify: `lodestar-server/src/poller/{service_check.rs,poll_loop.rs}`,
  `lodestar-server/src/state/{app_state.rs,snapshot.rs,mod.rs}`
- Create: `lodestar-server/src/state/service_view.rs`
- Test: `lodestar-server/tests/service_check.rs` (extend)

**Interfaces:**
- Consumes: `probe`, `Probers`, constants (Task 4); `AppState.links` (Task 3).
- Produces:
  - `AppState.checks: Arc<Mutex<HashMap<String, ServiceCheck>>>` (empty in `new`).
  - `pub async fn check_round(state: &AppState, probers: &Probers) -> bool` — probes all
    enabled links (`buffer_unordered(CHECK_CONCURRENCY)`), stores results (`since` set to
    `checked_at` when `state` differs from the stored one, kept otherwise), returns whether
    any `state` or `reason` changed; broadcasts
    `WsMessage::ServicesUpdated(service_views(state, &state.online.peers().await))` when it did.
  - `pub(crate) fn online_state(state: &AppState, name: &str, peers: &HashMap<String,
    PeerInfo>) -> OnlineState` extracted from `host_view_with` (which then calls it).
  - `pub fn service_views(state: &AppState, peers: &HashMap<String, PeerInfo>) ->
    Vec<ServiceView>` in `state/service_view.rs`: one per link, sorted by `(group, title)`;
    check from `checks` or `Pending` (enabled, no result) / `Unchecked` (disabled);
    `host_online = Some(online_state(…))` when `link.host` is in `state.hosts`, else `None`.
  - `snapshot()` fills `services`.
  - `poller::spawn` starts a third task: `Probers::new()`, `interval(CHECK_INTERVAL)`
    (first tick immediate), `check_round` each tick; it never awaits the host poller.

- [ ] **Step 1: Failing tests** (extend `tests/service_check.rs`, building an `AppState`
  like `tests/poller.rs` does, with links to the axum app):
  - `round_records_and_broadcasts_once` — subscribe to `state.tx`; first round → one
    `ServicesUpdated`; second identical round → `check_round` returns `false`, no message.
  - `since_moves_only_on_change` — `/flip` returns 200 then 502: `since` equal across two
    `Up` rounds, new value after the `Down` round.
  - `disabled_link_is_unchecked_and_not_probed` — route counter stays 0, view state
    `Unchecked`.
  - `snapshot_lists_services_sorted` — links in groups `B`/`A` → snapshot order `A`, `B`;
    a link with `host` = a managed host has `host_online == Some(_)`, an unknown host `None`.
  - `round_with_hanging_links_is_bounded` — 9 links to `/hang` → `check_round` completes
    in < 2 × `CHECK_TIMEOUT` + 1 s.
- [ ] **Step 2: Run** → FAIL.
- [ ] **Step 3: Implement** per Interfaces.
- [ ] **Step 4: Run** `nix develop -c cargo test --workspace` → PASS; clippy clean.
- [ ] **Step 5: Commit** `feat(server): periodic service checks in the snapshot and feed`

### Task 6: Demo services

**Files:**
- Create: `lodestar-server/src/demo/demo_services.rs`
- Modify: `lodestar-server/src/demo/{mod.rs,start.rs,demo_handle.rs}`

**Interfaces:**
- Produces: `pub(crate) async fn start_demo_services() -> anyhow::Result<(Vec<Link>,
  JoinHandle<()>)>` — an in-process axum server on `127.0.0.1:0` (`/` 200, `/slow` 200 after
  1.2 s, `/bad` 502) and links covering: up (group *Apps*), slow up, down 502 (group
  *Media*), refused (closed port), `check.enable = false`, and one whose `host` is a demo
  host that is off. Task joined into the demo's tasks so it stops with the handle.

- [ ] **Step 1: Failing test** in `tests/demo.rs`: `demo_has_services_in_every_state` —
  start the demo, wait (≤ 10 s) until `/api/snapshot` has services with states covering
  `up`, `down`, `unchecked`, and one `host_online == "off"`.
- [ ] **Step 2: Run** `nix develop -c cargo test -p lodestar-server --test demo` → FAIL.
- [ ] **Step 3: Implement**; pass the links to `server::start` in `start.rs` and
  `demo_handle.rs` (restart path keeps the same links).
- [ ] **Step 4: Run** → PASS.
- [ ] **Step 5: Commit** `feat(demo): simulated services`

### Task 7: Web data — route, reducer, `useServices`

**Files:**
- Create: `web/src/lib/route.ts`, `web/tests/services.test.tsx`
- Modify: `web/src/connection/{ConnectionProvider.tsx,hooks.ts}`, `web/tests/helpers.tsx`

**Interfaces:**
- Consumes: bindings from Task 2.
- Produces:
  - `export type Route = "fleet" | "services"`; `export function parseRoute(hash: string): Route`
    (`"#/services"` → services, anything else → fleet); `export function useRoute(): Route`
    (listens to `hashchange`).
  - Reducer action `{ type: "services"; data: ServiceView[] }` handled in
    `reduceSnapshot`; the feed's `services_updated` dispatches it.
  - `export function useServices(): ServiceView[] | null` — snapshot services with
    `host_online` replaced by the current `snapshot.hosts` entry's `online` when that host
    exists (so `host_updated` alone updates cards), else kept.
  - `helpers.tsx`: `export function service(over: Partial<ServiceView>): ServiceView`.

- [ ] **Step 1: Failing tests** in `services.test.tsx`: `parseRoute` cases (`""`,
  `"#/"`, `"#/services"`, `"#/nope"`); `reduceSnapshot` with `services` replaces the list
  and keeps hosts; `useServices` joins a host that went `off` via a `host` action (render a
  probe component under a provider fed by the reducer, as `connection.test.ts` does).
- [ ] **Step 2: Run** `cd web && nix develop .. -c npx vitest run tests/services.test.tsx` → FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** all web tests and `tsc --noEmit` → PASS.
- [ ] **Step 5: Commit** `feat(web): services in the connection, hash route`

### Task 8: Web UI — tones, words, icons, card, page, tabs

**Files:**
- Create: `web/src/components/services/ServiceCard.tsx`,
  `web/src/components/services/ServiceIcon.tsx`, `web/src/pages/ServicesPage.tsx`,
  `web/src/components/layout/NavTabs.tsx`
- Modify: `web/src/lib/{tones.ts,format.ts}`, `web/src/icons/{index.ts,BrandIcon.tsx}`,
  `web/src/App.tsx`, `web/src/pages/DashboardPage.tsx`,
  `web/src/components/layout/PageHeader.tsx`, `web/tests/services.test.tsx`

**Interfaces:**
- Consumes: `useServices`, `useRoute`, `service()` fixture (Task 7).
- Produces:
  - `@/icons`: `LayoutGridIcon as Services`, `CircleCheckIcon as Up`, `CircleXIcon as Down`,
    `ExternalLinkIcon as External`.
  - `BrandIcon` brands extended with `jellyfin`, `nextcloud`, `proxmox`, `truenas`,
    `traefikproxy`, `immich` (named imports only), `className: ""` (currentColor); export
    `isBrand(slug: string): slug is Brand`.
  - `format.ts`: `downReasonLabel(r: DownReason): string` (`timed out`, `connection
    refused`, `unreachable`, `TLS error`, `HTTP 502`, message); `serviceLabel(s:
    ServiceView): string` (exact copy in Global Constraints); `serviceCounts(s:
    ServiceView[]): { up: number; down: number; pending: number; unchecked: number }`;
    `groupServices(s: ServiceView[]): [string, ServiceView[]][]` (groups by name, cards by
    title).
  - `tones.ts`: `serviceStyle(s: ServiceView): StateStyle` per the spec's table (slow =
    `latency_ms >= 1000`).
  - `ServiceIcon { icon: string | null; title: string }` → `BrandIcon` if `isBrand`, else
    the `@/icons` export of that name if any, else the first letter (serif).
  - `ServiceCard { service: ServiceView; now: number }`; `ServicesPage()`;
    `NavTabs { route: Route; downCount: number }` (links `#/` and `#/services`,
    `aria-current="page"`, a `bad` count badge when `downCount > 0`).
  - `App.tsx` renders `DashboardPage` or `ServicesPage` by `useRoute()`; the header with
    `NavTabs` is shared (move the page shell — banner, padding, `PageHeader`, error
    states — into a `components/layout/PageShell.tsx` used by both pages, not duplicated).

- [ ] **Step 1: Failing tests** (extend `services.test.tsx`):
  - `serviceStyle` table: up → ok/Up; up 1200 ms → warn; down → bad/Down; down + host off
    → warn/Off; down + agent_unreachable → warn/Unreachable; pending → info/Building spin;
    unchecked → neutral/Idle.
  - `serviceLabel`: `Up · 84 ms`, `Down: HTTP 502`, `Down · host off`, `Checking…`,
    `Not checked`.
  - `groupServices` order; `serviceCounts`.
  - `ServiceCard` render: the title is a link with `href` = url, `target="_blank"`,
    `rel` contains `noopener`; badge text; URL element has class `truncate` and its
    tooltip/title holds the full URL; `data-tone` on the card.
  - `ServicesPage` with `[]` → "No services declared"; with two groups → two section
    headings in order.
  - `NavTabs` shows the down count and marks the current tab.
- [ ] **Step 2: Run** → FAIL.
- [ ] **Step 3: Implement** following `docs/style-guide.md` (Section, StatTile, Badge,
  OnlineState, Tooltip, the row-stripe tone classes, grid
  `grid-cols-[repeat(auto-fill,minmax(280px,1fr))] gap-3`, `min-w-0` on card children).
- [ ] **Step 4: Run** `npx vitest run`, `npx tsc --noEmit`, `npm run build` → PASS; then
  `nix run .#lodestar-demo` and look at `#/services` in light and dark, desktop and 375 px
  width: no horizontal scroll, every state visible.
- [ ] **Step 5: Commit** `feat(web): Services tab`

### Task 9: VM test, package hash, docs

**Files:**
- Modify: `nix/tests/vm.nix`, `default.nix` (`npmDepsHash` only if `package-lock.json`
  changed — it should not), `docs/operations.md`, `docs/style-guide.md`, `README.md`

- [ ] **Step 1: Extend the VM test**: on `manager`, set `services.lodestar-server.links` to
  `agent = { url = "http://laptop1:7461/"; host = "laptop1"; group = "Fleet"; check.expectStatus = [ 200 404 ]; }`
  and `closed = { url = "http://localhost:1/"; host = "manager"; }`; assert
  `/etc/lodestar-server/links.json` exists and
  `wait_until_succeeds` that `/api/snapshot` has `agent` `up` with `host_online == "on"` and
  `closed` `down` with `reason.kind == "connection_refused"`.
- [ ] **Step 2: Run** on the Linux builder (bundle to `~/build` on nixos-manager as in the
  nixos-config `AGENTS.md`): `nix build -L .#checks.x86_64-linux.vm` → PASS.
- [ ] **Step 3: Docs**: `operations.md` section "Service links" (declaring `lodestar.links`,
  `linksFrom`, what counts as up, `check.url`/`insecureTls`, 60 s / 5 s);
  `style-guide.md`: Services page, `ServiceCard`, `ServiceIcon`, `NavTabs`, the state-table
  rows and new icon names; `README.md`: one feature line.
- [ ] **Step 4: Full verification**: `cargo test --workspace`, clippy, `cargo fmt --check`,
  web `vitest`/`tsc`/`build`, `nix build .#lodestar .#lodestar-agent .#lodestar-server
  --no-link`, `nix flake check --no-build --keep-going` → all pass.
- [ ] **Step 5: Commit** `test(vm): service links; docs: Services tab`
