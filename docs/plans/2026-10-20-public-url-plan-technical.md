# Lodestar public URL with Kanidm login: Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `https://lodestar.vanwaaij.com` works from anywhere: anonymous visitors see a
reduced Services feed, Kanidm readers see the dashboard read-only, admins act only when
the router marks the request as arriving over Tailscale; the tailnet listener and the CLI
are unchanged.

**Architecture:** lodestar-server gets an optional second listener (the router entrance)
with its own axum router: a trusted-peer gate, a viewer loaded from a session cookie
(sessions in SQLite, id hashed) plus the router's `X-Homelab-Via: tailnet` mark, and
per-route needs (public, read, act). Login is an OIDC authorization-code flow with PKCE
against Kanidm (`openidconnect` 4 on the existing reqwest 0.12). New wire types
`PublicService` and `Me`; the web asks `/api/me` first and renders one of three states.
The flake (`~/.flake`) adds the route, the Kanidm client, the secret and the server options.

**Tech Stack:** Rust (axum 0.8, reqwest 0.12 rustls, SeaORM/SQLite, `openidconnect` 4,
sha2, base64, getrandom; tests: p256), ts-rs, Preact + vitest, NixOS module + VM test
(nixpkgs Kanidm 1.11), the nixos-config flake.

**Spec:** `docs/plans/2026-10-20-public-url-design.md` (binding; read it first). Flake side:
`~/.flake/AGENTS.md`, `~/.flake/docs/identity.md`, `~/.flake/docs/lodestar.md`.

## Global Constraints

- Lodestar work in `~/lodestar` on `main`; flake work in `~/.flake` on `unstable`. Read each
  repo's `AGENTS.md` first. No worktrees, no stash/reset; the trees may hold the user's
  uncommitted work: commit only your paths (`git commit -m "…" -- <paths>`).
- Commits: conventional (`feat(server): …`, `feat(web): …`, `feat(nix): …`,
  `test(vm): …`, `feat(lodestar): …` in the flake), author `Thijs van Waaij
  <thijs@vanwaaij.com>` (check `git config user.email`), **no `Co-Authored-By` or any AI
  trailer** (the user's instruction overrides both AGENTS.md files).
- Never push without asking the user (lodestar `main`, flake `unstable`).
- Lodestar commands through the dev shell (`nix develop -c …`, AGENTS.md "Working"). New
  files `git add`ed before Nix sees them. Bindings regenerated after any proto change:
  `cd lodestar-proto && TS_RS_EXPORT_DIR=../web/src/bindings nix develop .. -c cargo test -p lodestar-proto`
  (remove a stray `lodestar-proto/bindings/`).
- Rust style: one struct per file named after it, full names, `anyhow` with context, log
  lines start `lodestar-server: `, clippy `-D warnings` clean, `cargo fmt`. No module named
  like its parent (`auth/auth.rs` trips clippy): the shared struct is `AuthState`.
- Test data neutral: users `alice` (reader), `bob` (admin), `carol` (neither); groups
  `lodestar-users@example.com`, `admins@example.com`; public URL
  `https://lodestar.example.com`. Real values appear only in the flake.
- Cookie (exact): `__Host-lodestar_session=<value>; Path=/; Max-Age=43200; Secure;
  HttpOnly; SameSite=Lax`. Login binding cookie: `__Host-lodestar_login=<state>; Path=/;
  Max-Age=600; Secure; HttpOnly; SameSite=Lax`. Clearing: same name, `Max-Age=0`.
- Session value: 32 random bytes, base64url without padding (43 chars); stored as
  lowercase hex sha256. TTL 12 h (`SESSION_TTL_SECS = 43_200`); pending login TTL 10 min
  (`LOGIN_TTL_SECS = 600`); at most `MAX_PENDING_LOGINS = 256`; open WebSockets of a
  session recheck it every 60 s (`SESSION_RECHECK`).
- OIDC: scopes `openid profile groups`; PKCE S256; `state` and `nonce`; ID token algs
  allowed: ES256 and RS256 (openidconnect allows only RS256 by default: set it);
  discovery cached 1 h (`DISCOVERY_TTL`); HTTP timeout 10 s, redirects not followed;
  redirect URI `<publicUrl>/auth/callback`; user = `preferred_username`; groups = the ID
  token's `groups` claim, compared exactly. Admin group wins over reader group.
  Client secret read from `client_secret_file` at each login, surrounding whitespace trimmed.
- Mark (exact): header `X-Homelab-Via` with value `tailnet`, honoured only from a trusted
  peer (all requests from other peers already get 403).
- `config.json` contract (Nix writes, Rust reads), absent or `null` = today's behaviour:
  `"proxy": {"listen": "10.0.10.170:8081", "trusted_proxies": ["10.0.10.110"],
  "public_url": "https://lodestar.vanwaaij.com", "oidc": {"issuer": "…", "client_id":
  "lodestar", "client_secret_file": "/…", "reader_group": "…", "admin_group": "…"}}`.
- Wire JSON is snake_case like every existing type: `/api/me` is
  `{"viewer": "bob"|null, "role": "anonymous"|"reader"|"admin", "session": bool,
  "can_act": bool, "reason": null|"anonymous"|"not_admin"|"not_tailnet"}`;
  `/api/public/services` is a list of `{"title","group","description","icon","state":
  "up"|"down"|"unknown","since"}` sorted by `(group, title)`.
- API refusals (`{"error": …}`): anonymous on a read/act route → 401 `log in first`;
  reader acting → 403 `only admins can do this`; admin without the mark → 403
  `only over Tailscale`; untrusted peer → 403 `forbidden` (plain, before anything else).
- Copy (exact, web): `Log in`, `Log out`, `Only over Tailscale` (title/tooltip of a
  disabled action button), public states `Up`, `Down`, `Unknown`. Error pages: issuer
  down `Can't reach the login service` / `Try again in a moment.`; no group `No access` /
  `Your account is not in a group that may use Lodestar.`; any other callback failure
  `Login failed` / `Start the login again.`; each with a `Try again` link to `/auth/login`.
- Flake values: route `lodestar` host `lodestar.vanwaaij.com`, backend
  `http://10.0.10.170:8081`, public and tailnet, not gated; access `lodestar` members
  `[ "thijs" ]`, redirect `https://lodestar.vanwaaij.com/auth/callback`, landing
  `https://lodestar.vanwaaij.com`, `preferShortUsername = true`, scopes
  `[ "openid" "profile" "groups" ]`; 1Password `op://Homelab/kanidm-oauth2-lodestar/credential`;
  issuer `https://id.vanwaaij.com/oauth2/openid/lodestar`; groups
  `lodestar-users@id.vanwaaij.com`, `admins@id.vanwaaij.com`; trustedProxies
  `[ "10.0.10.110" ]`.

## Review Focus

- **Another `*.vanwaaij.com` site opening the WebSocket with the user's cookie** (same-site,
  so SameSite=Lax sends it; WebSockets have no CORS): the proxy entrance must refuse a
  `/api/ws` upgrade whose `Origin` is not the public URL's origin (test in Task 6).
- **A WebSocket outliving its session** (logout in another tab, or 12 h expiry while the tab
  stays open): the socket closes within one recheck after the row is gone (test in Task 6,
  with the recheck interval shortened through `AuthState`).
- **The secret file ends in a newline** (opnix and hand-written files do): the token request
  must authenticate with the trimmed secret (test in Task 4).
- **Server restarted between login and callback** (pending logins are in memory) or a
  bookmarked callback URL: the callback shows `Login failed` with `Try again`, status 400,
  never a 500 and never a session (test in Task 7).
- **Anonymous clients hammering `/auth/login`**: pending logins stay capped at 256 (oldest
  dropped), expired ones pruned on insert (test in Task 4).

---

### Task 1: Proto: `PublicService`, `Me`, `Role`, `ActDenied`

**Files:**
- Create: `lodestar-proto/src/dashboard/{public_service.rs,me.rs,role.rs}`
- Modify: `lodestar-proto/src/dashboard/mod.rs`, `lodestar-proto/src/lib.rs` (re-exports),
  generated `web/src/bindings/*`

**Interfaces:**
- Produces (all `Serialize, Deserialize, Clone, Debug, PartialEq, TS`, `#[ts(export)]`,
  `rename_all = "snake_case"`):
  - `enum Role { Anonymous, Reader, Admin }` (`role.rs`; also `Copy, Eq`).
  - `struct PublicService { title: String, group: String, description: Option<String>,
    icon: Option<String>, state: PublicServiceState, since: Option<i64> }`
    (`#[ts(type = "number | null")]` on `since`) and `enum PublicServiceState { Up, Down,
    Unknown }` with `fn from_check(state: &ServiceState) -> PublicServiceState`
    (`Up→Up`, `Down→Down`, `Pending|Unchecked→Unknown`).
  - `struct Me { viewer: Option<String>, role: Role, session: bool, can_act: bool,
    reason: Option<ActDenied> }` and `enum ActDenied { Anonymous, NotAdmin, NotTailnet }`.

- [ ] **Step 1: Failing tests** in the new files: `public_service_json` — a value
  serialises to exactly `{"title":"Grafana","group":"Monitoring","description":null,"icon":null,"state":"unknown","since":null}`;
  `state_from_check` — the four mappings; `me_json` — `Me { viewer: Some("bob"), role:
  Admin, session: true, can_act: false, reason: Some(NotTailnet) }` →
  `{"viewer":"bob","role":"admin","session":true,"can_act":false,"reason":"not_tailnet"}`.
- [ ] **Step 2: Run** `nix develop -c cargo test -p lodestar-proto` → FAIL (types missing).
- [ ] **Step 3: Implement** the types and re-exports.
- [ ] **Step 4: Regenerate bindings**; `web/src/bindings/` gains `PublicService.ts`,
  `PublicServiceState.ts`, `Me.ts`, `Role.ts`, `ActDenied.ts`. Run
  `nix develop -c cargo test --workspace` and `cd web && nix develop .. -c npx tsc --noEmit` → PASS.
- [ ] **Step 5: Commit** `feat(proto): PublicService and Me`

### Task 2: Configuration: `ProxyConfig` and the module options

**Files:**
- Create: `lodestar-server/src/config/{proxy_config.rs,oidc_config.rs,proxy_check.rs}`
- Modify: `lodestar-server/src/config/{mod.rs,server_config.rs}`, `lodestar-server/src/server.rs`
  (call the check), every `ServerConfig { … }` literal (`demo/start.rs`, `tests/{api,poller,mirror,service_check}.rs`:
  `proxy: None`), `nix/modules/server.nix`, `flake.nix` (`firewallCheck`)

**Interfaces:**
- Produces:
  - `pub struct ProxyConfig { pub listen: SocketAddr, pub trusted_proxies: Vec<IpAddr>,
    pub public_url: String, pub oidc: OidcConfig }`;
    `pub struct OidcConfig { pub issuer: String, pub client_id: String, pub
    client_secret_file: PathBuf, pub reader_group: String, pub admin_group: String }`
    (both `Serialize, Deserialize, Clone, Debug, PartialEq`).
  - `ServerConfig.proxy: Option<ProxyConfig>` (`#[serde(default)]`).
  - `impl ProxyConfig { pub fn public_origin(&self) -> String }` (scheme://host[:port],
    no trailing slash) and `impl ServerConfig { pub fn check_proxy(&self) -> anyhow::Result<()> }`.
  - Module options under `services.lodestar-server`: `proxyListen.address` (nullOr str,
    default null = off), `proxyListen.port` (port, 8081), `trustedProxies` (listOf str, []),
    `publicUrl` (nullOr str, null), `oidc.issuer` (nullOr str), `oidc.clientId` (str,
    `"lodestar"`), `oidc.clientSecretFile` (nullOr str; doc: opnix with
    `owner = "lodestar-server"`), `oidc.readerGroup`, `oidc.adminGroup` (nullOr str).

- [ ] **Step 1: Failing Rust tests**: in `server_config.rs` extend `config_defaults_and_tags`
  with `config.proxy == None`; new `proxy_parses` (the contract JSON with test values →
  equal struct); in `proxy_check.rs`: `empty_trusted_proxies_refused` (error mentions
  `trusted_proxies`), `public_url_must_be_http_or_https` (`ftp://x` refused, `https://lodestar.example.com/`
  accepted), `proxy_listen_differs_from_listen`, `public_origin_strips_path_and_slash`
  (`https://lodestar.example.com/` → `https://lodestar.example.com`).
- [ ] **Step 2: Run** `nix develop -c cargo test -p lodestar-server config::` → FAIL.
- [ ] **Step 3: Implement**; `server::start` calls `check_proxy()` after `check_listen()`.
- [ ] **Step 4: Failing Nix check**: in `flake.nix` add a second evaluated system to
  `firewallCheck` (same base, plus `proxyListen.address = "10.0.10.170"`,
  `trustedProxies = [ "10.0.10.110" ]`, `publicUrl`, all `oidc.*` set) and assert:
  8081 is in neither `allowedTCPPorts` nor `interfaces.tailscale0.allowedTCPPorts`;
  `networking.firewall.extraCommands` contains
  `-p tcp -s 10.0.10.110 --dport 8081 -j nixos-fw-accept`; `config.json`'s
  `proxy.trusted_proxies == [ "10.0.10.110" ]` and `proxy.oidc.client_id == "lodestar"`;
  the existing system's `config.json` has no `proxy` (or `null`); a third system with only
  `proxyListen.address` set has a failed assertion whose message names `trustedProxies`.
  Run `nix eval .#checks.x86_64-linux.firewall.drvPath` → FAIL.
- [ ] **Step 5: Implement the module**: `proxy` in `config.json` only when
  `proxyListen.address != null`; assertions (address set ⇒ `trustedProxies != [ ]`,
  `publicUrl`, `oidc.issuer`, `oidc.clientSecretFile`, `oidc.readerGroup`,
  `oidc.adminGroup` non-null); firewall: with `networking.nftables.enable` add
  `ip saddr { … } tcp dport <port> accept` (`ip6 saddr` for addresses with `:`) to
  `extraInputRules`, else one `iptables -A nixos-fw -p tcp -s <a> --dport <port> -j nixos-fw-accept`
  per address (`ip6tables` for IPv6) in `extraCommands`.
- [ ] **Step 6: Verify**: `nix develop -c cargo test --workspace`, `nix eval
  .#checks.x86_64-linux.firewall.drvPath`, `nix flake check --no-build --keep-going` → PASS.
- [ ] **Step 7: Commit** `feat(server): router entrance configuration and module options`

### Task 3: Sessions table and session primitives

**Files:**
- Create: `lodestar-server/migration/src/m20261020_000001_create_sessions.rs`,
  `lodestar-server/src/db/{sessions.rs,entities/sessions.rs}`,
  `lodestar-server/src/auth/{mod.rs,login_session.rs,session_id.rs,cookie.rs,next_path.rs}`
- Modify: `lodestar-server/migration/src/lib.rs`, `lodestar-server/src/db/{mod.rs,entities/mod.rs}`,
  `lodestar-server/src/lib.rs` (`pub mod auth;`), `lodestar-server/Cargo.toml`
  (`sha2 = "0.10"`, `base64 = "0.22"`, `getrandom = "0.3"`)
- Test: `lodestar-server/tests/db.rs` (extend), unit tests in the new `auth/` files

**Interfaces:**
- Produces:
  - Table `sessions`: `id_hash` text PK, `user` text, `role` text (`"reader"`/`"admin"`,
    the serde string), `created_at` big int, `expires_at` big int.
  - `pub struct LoginSession { pub id_hash: String, pub user: String, pub role: Role,
    pub expires_at: i64 }`.
  - `db::insert_session(db, session: &LoginSession, created_at: i64) -> anyhow::Result<()>`,
    `db::find_session(db, id_hash: &str, now: i64) -> anyhow::Result<Option<LoginSession>>`
    (an expired row is deleted and `None` returned), `db::delete_session(db, id_hash: &str)
    -> anyhow::Result<()>`, `db::delete_expired_sessions(db, now: i64) -> anyhow::Result<u64>`.
  - `pub struct SessionId(String)`: `new_random() -> SessionId`, `from_cookie(value: &str)
    -> Option<SessionId>` (exactly 43 base64url chars), `as_str(&self) -> &str`,
    `hash(&self) -> String`; `pub fn hash_session_value(value: &str) -> String`.
  - `cookie.rs`: `SESSION_COOKIE`, `LOGIN_COOKIE`, `SESSION_TTL_SECS`, `LOGIN_TTL_SECS`;
    `pub fn set_cookie(name: &str, value: &str, max_age: i64) -> HeaderValue`,
    `pub fn clear_cookie(name: &str) -> HeaderValue`,
    `pub fn read_cookie(headers: &HeaderMap, name: &str) -> Option<String>`.
  - `pub fn safe_next(raw: Option<&str>) -> String`.

- [ ] **Step 1: Failing tests**:
  - `tests/db.rs`: `session_round_trip` (insert, find before expiry → equal),
    `expired_session_is_deleted_on_find` (find at `expires_at` → `None`, a second raw
    query finds no row), `delete_expired_counts` (two expired, one live → 2),
    `migration_runs_on_existing_database` (connect twice to the same dir → OK).
  - `session_id.rs`: `hash_is_sha256_hex` (`hash_session_value("abc")` ==
    `ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad`);
    `random_ids_are_43_chars_and_differ`; `from_cookie_rejects` `""`, 42 chars, `+`/`/`.
  - `cookie.rs`: `session_cookie_attributes` (exact string from Global Constraints),
    `clear_cookie_has_max_age_0`, `read_cookie_finds_among_several` (`a=1; __Host-lodestar_session=x; b=2` → `x`).
  - `next_path.rs`: `/` and `/#/services` kept; `None`, `""`, `//evil.example.com`,
    `/\evil.example.com`, `https://evil.example.com`, `services`, a value with a control
    character, a value longer than 512 bytes → `/`.
- [ ] **Step 2: Run** `nix develop -c cargo test -p lodestar-server` → FAIL.
- [ ] **Step 3: Implement** (migration in the style of `m20261012_000001_create_tables.rs`;
  `SessionId::new_random` with `getrandom::fill`, `URL_SAFE_NO_PAD`).
- [ ] **Step 4: Run** the tests → PASS; clippy clean.
- [ ] **Step 5: Commit** `feat(server): login sessions in SQLite`

### Task 4: OIDC client against a fake issuer

**Files:**
- Create: `lodestar-server/src/auth/{oidc_client.rs,groups_claim.rs,login_identity.rs,login_error.rs,login_start.rs,pending_login.rs,pending_logins.rs}`,
  `lodestar-server/tests/support/{mod.rs,fake_issuer.rs}`, `lodestar-server/tests/oidc.rs`
- Modify: `lodestar-server/src/auth/mod.rs`, `lodestar-server/Cargo.toml`
  (`openidconnect = { version = "4", default-features = false, features = ["reqwest"] }`;
  reqwest gains feature `rustls-tls-native-roots` so the system CA store is trusted;
  dev: `p256 = { version = "0.13", features = ["ecdsa", "jwk"] }`)

**Interfaces:**
- Consumes: `OidcConfig` (Task 2), `Role` (Task 1), `LOGIN_TTL_SECS` (Task 3).
- Produces:
  - `pub struct GroupsClaim { #[serde(default)] pub groups: Vec<String> }` (`AdditionalClaims`).
  - `pub struct LoginIdentity { pub user: String, pub groups: Vec<String> }` with
    `pub fn role(&self, config: &OidcConfig) -> Option<Role>` (admin group → `Admin`,
    else reader group → `Reader`, else `None`).
  - `pub enum LoginError { IssuerUnreachable(String), Misconfigured(String), UnknownState,
    Refused(String) }` (`Display`; never contains token contents).
  - `pub struct PendingLogin { pub nonce: String, pub pkce_verifier: String, pub next:
    String, pub created_at: i64 }`; `pub struct PendingLogins` with `new() -> Self`,
    `insert(&self, state: String, pending: PendingLogin, now: i64)` (prunes older than
    `LOGIN_TTL_SECS`, then drops the oldest beyond `MAX_PENDING_LOGINS`),
    `take(&self, state: &str, now: i64) -> Option<PendingLogin>` (removes; expired → `None`).
  - `pub struct LoginStart { pub authorize_url: String, pub state: String, pub pending: PendingLogin }`.
  - `pub struct OidcClient` with `pub fn new(config: OidcConfig, public_url: &str) ->
    anyhow::Result<Self>`, `pub async fn begin(&self, next: String, now: i64) ->
    Result<LoginStart, LoginError>`, `pub async fn finish(&self, code: &str, pending:
    PendingLogin) -> Result<LoginIdentity, LoginError>`; discovery cached `DISCOVERY_TTL`
    behind a `tokio::sync::Mutex`; custom-claims client type via
    `openidconnect::Client<GroupsClaim, …>` (openidconnect's custom-claims docs).
  - Test support: `pub struct FakeIssuer` with `start() -> FakeIssuer` (axum on
    `127.0.0.1:0`: discovery, JWKS from a fixed P-256 key, token endpoint), `issuer(&self)
    -> String`, `issue(&self, code: &str, claims: serde_json::Value)`, `issue_signed_by_other_key(&self, code, claims)`,
    `discovery_hits(&self) -> usize`, `last_client_secret(&self) -> Option<String>`; it
    checks `code_verifier` against the stored S256 challenge and answers 400
    `invalid_grant` for an unknown code or a wrong verifier. ES256 JWS signed by hand
    (`p256::ecdsa::SigningKey::from_bytes` of a fixed 32-byte value, base64url
    `header.payload.signature`, signature as raw `r‖s`).

- [ ] **Step 1: Failing tests** in `tests/oidc.rs` (config: issuer = fake, client
  `lodestar`, secret file containing `s3cret\n`, groups as in Global Constraints):
  - `begin_builds_pkce_url` — the URL starts with the fake's authorize endpoint; query has
    `response_type=code`, `client_id=lodestar`, `redirect_uri=https://lodestar.example.com/auth/callback`,
    `scope` containing `openid`, `profile`, `groups`, `code_challenge_method=S256`, a
    `state` equal to `LoginStart.state`, a `nonce` equal to `pending.nonce`.
  - `finish_accepts_es256_token` — `issue("c1", {sub, preferred_username: "alice", nonce, groups: ["lodestar-users@example.com", "<some uuid>"]})`
    → `LoginIdentity { user: "alice", groups: [both] }`; `last_client_secret() == Some("s3cret")`.
  - `finish_refuses_wrong_nonce`, `finish_refuses_bad_signature`,
    `finish_refuses_unknown_code`, `finish_refuses_missing_preferred_username` → `Refused(_)`.
  - `missing_groups_claim_gives_no_role` — token without `groups` → identity with `[]`,
    `role(..) == None`.
  - `discovery_failure_is_issuer_unreachable` — issuer on a closed port → `IssuerUnreachable(_)`.
  - `discovery_is_cached` — two `begin`s → `discovery_hits() == 1`.
  - Unit (`login_identity.rs`): `role` table: admin only → Admin; both → Admin; reader →
    Reader; `admins@example.com.evil`/`LODESTAR-USERS@example.com` → None.
  - Unit (`pending_logins.rs`): `take_twice_is_none` (replay), `expired_is_none`,
    `capped_at_256_dropping_oldest`.
- [ ] **Step 2: Run** `nix develop -c cargo test -p lodestar-server --test oidc` → FAIL.
- [ ] **Step 3: Implement**. `reqwest::Client` with `redirect(Policy::none())`, `timeout(10 s)`;
  `set_allowed_algs([EcdsaP256Sha256, RsaSsaPkcs1V15Sha256])` on the ID token verifier;
  the secret file is read in `finish` (unreadable → `Misconfigured`).
- [ ] **Step 4: Run** → PASS; clippy clean.
- [ ] **Step 5: Commit** `feat(server): OpenID Connect client with PKCE`

### Task 5: Viewer, access rules, `/api/me` and the public feed

**Files:**
- Create: `lodestar-server/src/access/viewer.rs`, `lodestar-server/src/api/{me.rs,public_services.rs}`,
  `lodestar-server/src/state/public_services.rs`
- Modify: `lodestar-server/src/access/{mod.rs,middleware.rs}`, `lodestar-server/src/api/{mod.rs,app.rs}`,
  `lodestar-server/src/state/mod.rs`
- Test: unit tests in `viewer.rs`, `lodestar-server/tests/api.rs` (extend)

**Interfaces:**
- Consumes: `Me`, `Role`, `ActDenied`, `PublicService`, `PublicServiceState::from_check` (Task 1);
  `LoginSession` (Task 3).
- Produces:
  - `pub struct Viewer { pub name: Option<String>, pub role: Role, pub session_hash:
    Option<String>, pub can_act: bool, pub reason: Option<ActDenied> }` with
    `pub fn tailnet(actor: String) -> Viewer` (Admin, can act), `pub fn anonymous() -> Viewer`,
    `pub fn from_session(session: &LoginSession, via_tailnet: bool) -> Viewer`,
    `pub fn me(&self) -> Me` (`session = session_hash.is_some()`),
    `pub fn permits(&self, need: Need) -> Result<(), (StatusCode, &'static str)>`;
    `pub enum Need { Public, Read, Act }` (same file). Refusals per Global Constraints.
  - `require_access` (tailnet) inserts `Viewer::tailnet(actor)` next to `Actor`.
  - `pub fn public_services(state: &AppState) -> Vec<PublicService>`: built from
    `state.links` and `state.checks` (not from `ServiceView`), sorted by `(group, title)`;
    no check result → `Unknown`, `since: None`.
  - Routes `GET /api/me` (`Json<Me>` from `Extension<Viewer>`) and
    `GET /api/public/services`; `api/app.rs` split into `fn public_api_routes() ->
    Router<AppState>` (these two), `fn read_routes()` (snapshot, apply log, audit),
    `fn act_routes()` (host apply, channel apply, promote); `app(state)` composes them plus
    `/ws` exactly as today (behaviour unchanged).

- [ ] **Step 1: Failing tests**:
  - `viewer.rs` table `permits_table`, rows (viewer → Public / Read / Act):
    anonymous → Ok / 401 / 401; reader marked → Ok / Ok / 403 `only admins can do this`;
    admin unmarked → Ok / Ok / 403 `only over Tailscale`; admin marked → Ok / Ok / Ok;
    tailnet → Ok / Ok / Ok.
  - `me_table`: anonymous → `{None, Anonymous, false, false, Some(Anonymous)}`; reader
    marked → `can_act false, reason NotAdmin`; admin unmarked → `NotTailnet`; admin marked
    → `can_act true, reason None, session true`; tailnet → `{Some(actor), Admin, false, true, None}`.
  - `tests/api.rs`: `me_on_tailnet_entrance_is_admin` (access disabled → viewer `dev`,
    role admin, can act); `public_services_has_only_public_fields` (one link with a stored
    `Down` check → keys exactly `description, group, icon, since, state, title`,
    `state == "down"`, `since` equals the check's).
- [ ] **Step 2: Run** `nix develop -c cargo test -p lodestar-server` → FAIL.
- [ ] **Step 3: Implement**.
- [ ] **Step 4: Run** → PASS (all existing `tests/api.rs` cases unchanged).
- [ ] **Step 5: Commit** `feat(server): viewer roles, /api/me and the public services feed`

### Task 6: The router entrance listener

**Files:**
- Create: `lodestar-server/src/access/proxy_middleware.rs`, `lodestar-server/src/auth/auth_state.rs`,
  `lodestar-server/tests/proxy.rs` (move `git`/`Origin` from `tests/api.rs` into
  `tests/support/origin.rs` if both need them)
- Modify: `lodestar-server/src/api/app.rs` (`proxy_app`), `lodestar-server/src/server.rs`,
  `lodestar-server/src/state/app_state.rs`, `lodestar-server/src/websocket/{handler.rs,session.rs}`

**Interfaces:**
- Consumes: `ProxyConfig` (Task 2), `db::find_session`, cookies (Task 3), `OidcClient`,
  `PendingLogins` (Task 4), `Viewer`, `Need`, route groups (Task 5).
- Produces:
  - `pub struct AuthState { pub config: ProxyConfig, pub oidc: OidcClient, pub pending:
    PendingLogins, pub session_recheck: Duration }` with `pub fn new(config: ProxyConfig)
    -> anyhow::Result<Self>` (`session_recheck = SESSION_RECHECK`, a 60 s const in that file); `AppState.auth:
    Option<Arc<AuthState>>` (`None` in `new`).
  - Middleware: `require_trusted_peer` (403 `forbidden` unless
    `peer.ip().to_canonical()` is in `trusted_proxies`), `load_viewer` (cookie →
    `find_session` → `Viewer::from_session(.., mark)` or `Viewer::anonymous()`; inserts
    `Viewer`, and `Actor(user)` when signed in), `require_read`, `require_act`
    (`Viewer::permits`), `require_public_origin` (WebSocket upgrade: `Origin` must equal
    `public_origin()`, else 403).
  - `pub fn proxy_app(state: AppState) -> Router`: `/api` = public routes + read routes
    (`require_read`) + act routes (`require_act`) + `/ws` (`require_read`,
    `require_public_origin`), CSRF as today; `/auth` routes from Task 7 (stubbed 404 here);
    static fallback; then `load_viewer`, outermost `require_trusted_peer`.
  - `ServerHandle.proxy_addr: Option<SocketAddr>`; `server::start` binds `proxy.listen`
    when set (log `lodestar-server: router entrance on <addr>`); both listeners share one
    shutdown (`tokio::sync::watch`); `wait()` ends when either serve task ends.
  - `websocket::session::run(state, viewer: Viewer, socket)`: when
    `viewer.session_hash` is set, every `auth.session_recheck` it calls `find_session`
    and closes the socket when the session is gone.

- [ ] **Step 1: Failing tests** in `tests/proxy.rs` (an `AppState` like `tests/api.rs`
  with `auth` set, trusted `["127.0.0.1"]`, `proxy_app` served on `127.0.0.1:0`;
  sessions inserted with `db::insert_session` for `alice` (Reader) and `bob` (Admin)):
  - `untrusted_peer_gets_403_everywhere` — trusted `["192.0.2.1"]`: `/`, `/api/me`,
    `/api/public/services`, `/auth/login` → 403.
  - `anonymous_gets_ui_me_and_public_feed_only` — `/` 200 (index), `/api/me` role
    `anonymous`, `/api/public/services` 200, `/api/snapshot`, `/api/audit` 401
    `log in first`, `/api/ws` upgrade refused 401.
  - `reader_reads_but_cannot_act` — alice's cookie with the mark: snapshot 200 with
    `viewer == "alice"`; `POST /api/hosts/h/apply` (with `X-Lodestar: 1`) → 403
    `only admins can do this`.
  - `admin_acts_only_with_exact_mark` — bob without mark → 403 `only over Tailscale`;
    `X-Homelab-Via: Tailnet` or `yes` → 403; `tailnet` → 202 and the audit row's actor is `bob`.
  - `expired_session_is_anonymous` — row with `expires_at` in the past → `/api/me`
    anonymous, row gone.
  - `websocket_origin_must_be_public_url` — bob's cookie, `Origin:
    https://evil.example.com` → 403; `Origin: https://lodestar.example.com` → 101 and a
    snapshot arrives.
  - `websocket_closes_when_session_deleted` — `session_recheck = 200 ms`; open, delete the
    row, the stream ends within 2 s.
  - `server_start_binds_both_listeners` — `server::start` with `proxy.listen =
    127.0.0.1:0` → `proxy_addr.is_some()`, `/api/me` there answers anonymous, the tailnet
    address still answers admin.
- [ ] **Step 2: Run** `nix develop -c cargo test -p lodestar-server --test proxy` → FAIL.
- [ ] **Step 3: Implement**.
- [ ] **Step 4: Run** `nix develop -c cargo test --workspace` → PASS; clippy clean.
- [ ] **Step 5: Commit** `feat(server): router entrance listener with session roles`

### Task 7: `/auth/login`, `/auth/callback`, `/auth/logout`

**Files:**
- Create: `lodestar-server/src/auth/{login.rs,callback.rs,logout.rs,error_page.rs}`
- Modify: `lodestar-server/src/auth/mod.rs`, `lodestar-server/src/api/app.rs` (mount under
  `/auth`, logout behind CSRF), `AGENTS.md` (layout: `auth/` = OIDC login and sessions;
  `access/` gains the router entrance)
- Test: `lodestar-server/tests/proxy.rs` (extend, with `FakeIssuer` from Task 4)

**Interfaces:**
- Consumes: everything from Tasks 3, 4 and 6.
- Produces:
  - `GET /auth/login?next=` → `safe_next`, `oidc.begin`, `pending.insert`, 303 to the
    authorize URL with `Set-Cookie` login cookie = state. `IssuerUnreachable` → 502 page.
  - `GET /auth/callback?code=&state=` (or `error=` from Kanidm → 403 `No access` page):
    login cookie must equal `state`, `pending.take` must hit, `oidc.finish`, `role` must be
    `Some`; then `delete_expired_sessions`, `insert_session` (expiry now + 12 h), 303 to
    `pending.next` with the session cookie set and the login cookie cleared. Failures:
    `UnknownState`/cookie mismatch/`Refused` → 400 `Login failed`, no group → 403
    `No access`, `IssuerUnreachable` → 502, `Misconfigured` → 500 (same page shape).
    Logs: `lodestar-server: login: <user> as <role>`; `lodestar-server: login refused: <reason>`.
  - `POST /auth/logout` → delete the row (if any), clear the session cookie, 204.
  - `pub fn error_page(status: StatusCode, title: &str, detail: &str) -> Response` (HTML,
    escaped, with the `Try again` link).

- [ ] **Step 1: Failing tests** (drive the flow by reading `Location` and `Set-Cookie`
  by hand; reqwest without redirects or cookie store):
  - `login_flow_sets_session_cookie` — `/auth/login?next=/%23/services` → 303 to the fake;
    fake issues a token for alice with the URL's nonce; callback with the login cookie →
    303 `Location: /#/services`, `Set-Cookie` matches the exact session attributes,
    `/api/me` with it → `{"viewer":"alice","role":"reader","session":true,…}`.
  - `callback_replay_is_refused` — the same callback twice → second 400 `Login failed`, no
    session cookie.
  - `callback_state_must_match_login_cookie` — missing or different login cookie → 400.
  - `callback_unknown_state_after_restart` — fresh `AuthState`, valid-looking params → 400
    page containing `Try again`, never 500.
  - `no_group_means_no_access_and_no_session` — carol (groups `[]`) → 403 page `No access`,
    no session row.
  - `kanidm_error_param_is_no_access` — `?error=access_denied&state=…` → 403.
  - `issuer_down_shows_error_page` — issuer on a closed port → 502 containing
    `Can't reach the login service`; `/api/public/services` still 200.
  - `next_is_sanitised` — `next=//evil.example.com` → after callback `Location: /`.
  - `logout_needs_csrf_header_and_deletes_session` — without `X-Lodestar` → 403; with →
    204, `Set-Cookie` clears, `/api/me` with the old cookie → anonymous.
- [ ] **Step 2: Run** → FAIL.
- [ ] **Step 3: Implement**.
- [ ] **Step 4: Run** `nix develop -c cargo test --workspace`, clippy, `cargo fmt --check` → PASS.
- [ ] **Step 5: Commit** `feat(server): Kanidm login, callback and logout`

### Task 8: Web connection: viewer, public feed, login and logout

**Files:**
- Create: `web/src/lib/viewer.ts`, `web/tests/viewer.test.tsx`
- Modify: `web/src/connection/{client.ts,ConnectionProvider.tsx,hooks.ts}`, `web/tests/helpers.tsx`
  (`fakeActions` gains `login`, `logout`; fixtures `me(over)`, `publicService(over)`)

**Interfaces:**
- Consumes: bindings `Me`, `PublicService` (Task 1); routes from Tasks 5–7.
- Produces:
  - `client.ts`: `getMe(): Promise<Me>`, `getPublicServices(): Promise<PublicService[]>`,
    `logout(): Promise<void>` (POST `/auth/logout`), `loginUrl(next: string): string`
    (`/auth/login?next=` + `encodeURIComponent`).
  - `lib/viewer.ts`: `type ViewMode = "loading" | "anonymous" | "signed_in" | "forbidden"`;
    `viewMode(me: Me | null, forbidden: boolean): ViewMode`;
    `actPermission(me: Me | null): { visible: boolean; disabledReason: string | null }`
    (null/anonymous/reader → hidden; admin without `can_act` → visible, `Only over Tailscale`;
    admin with → visible, null); `PUBLIC_POLL_MS = 30_000`.
  - `ConnectionState` gains `viewer: Me | null`, `mode: ViewMode`, `publicServices:
    PublicService[] | null`; `Actions` gains `login(): void` (navigates to
    `loginUrl(location.pathname + location.hash)`) and `logout(): Promise<void>` (then
    navigates to `/`).
  - Provider flow: `getMe` on mount (403 → `forbidden`; network error → retry with the
    feed's backoff); `signed_in` → feed as today; `anonymous` → `getPublicServices` now
    and every `PUBLIC_POLL_MS`, no WebSocket; a feed status `waiting` or any `ApiError` 401
    from an action refetches `/api/me` (anonymous → the feed stops, polling starts).
  - Hooks: `useViewer(): Me | null`, `useViewMode(): ViewMode`,
    `usePublicServices(): PublicService[] | null`, `useActPermission()`.

- [ ] **Step 1: Failing tests** in `web/tests/viewer.test.tsx`:
  - `viewMode` and `actPermission` tables (all roles, `can_act` both ways, `null`).
  - `loginUrl("/#/services") === "/auth/login?next=%2F%23%2Fservices"`.
  - Provider with a stubbed `fetch` and a counting `WebSocket` stub, fake timers:
    anonymous `/api/me` → `/api/public/services` fetched, no WebSocket constructed, fetched
    again after 30 s; reader → one WebSocket, no public fetch; reader whose feed goes
    `waiting` while `/api/me` now answers anonymous → mode becomes `anonymous` and the public
    feed is fetched; `/api/me` 403 → mode `forbidden`.
  - `logout` posts `/auth/logout` with `x-lodestar: 1`.
- [ ] **Step 2: Run** `cd web && nix develop .. -c npx vitest run tests/viewer.test.tsx` → FAIL.
- [ ] **Step 3: Implement**; components still compile (Task 9 uses the new state).
- [ ] **Step 4: Run** `npx vitest run` and `npx tsc --noEmit` → PASS.
- [ ] **Step 5: Commit** `feat(web): viewer state, public feed and login in the connection`

### Task 9: Web: three states and the action buttons

**Files:**
- Create: `web/src/components/services/PublicServiceCard.tsx`, `web/src/pages/PublicServicesPage.tsx`,
  `web/tests/public.test.tsx`
- Modify: `web/src/App.tsx`, `web/src/components/layout/{PageHeader.tsx,NavTabs.tsx,PageShell.tsx}`,
  `web/src/components/fleet/{HostParts.tsx,ChannelCard.tsx,PromoteCard.tsx}`,
  `web/src/lib/{format.ts,tones.ts}`, `web/src/icons/index.ts` (`LogInIcon as LogIn`,
  `LogOutIcon as LogOut`), `web/tests/actions.test.tsx`, `docs/style-guide.md`

**Interfaces:**
- Consumes: hooks and actions from Task 8.
- Produces:
  - `groupServices<T extends { group: string; title: string }>(list: T[]): [string, T[]][]`
    (generic, same order); `publicServiceLabel(state): "Up" | "Down" | "Unknown"`;
    `publicServiceCounts(list) -> { up; down; unknown }`;
    `stateSince(state: string, since: number | null, now: number): string | null` (the
    existing sub-line wording; `serviceSubLine` uses it); `publicServiceStyle(state)`
    (`up` ok/Up, `down` bad/Down, `unknown` neutral/Idle).
  - `PublicServiceCard({ service, now })`: icon, title as text (no link, no URL, no host),
    description, badge, sub line, tone stripe.
  - `PublicServicesPage()`: header with logo, theme toggle and `Log in` button
    (`actions.login`), `NavTabs` with only Services, summary tiles up/down/unknown, groups
    as on `ServicesPage` (`groupColumns`), empty state `No services declared`.
  - `PageHeader` prop `session: boolean` → `Log out` button (`actions.logout`);
    `NavTabs` prop `fleet?: boolean` (default true).
  - `App`: mode `anonymous` → `PublicServicesPage` whatever the route.
  - `HostActions` apply button, `ChannelCard` button, `PromoteDialog` trigger:
    `useActPermission()` → hidden when not `visible`; `disabled` with
    `title="Only over Tailscale"` when `disabledReason`. The log button stays for readers.

- [ ] **Step 1: Failing tests**:
  - `actions.test.tsx`: for `HostRow`, `ChannelCard`, `PromoteDialog` under three viewers:
    reader → no apply/promote button; admin without `can_act` → button disabled with
    title `Only over Tailscale` and clicking calls nothing; admin with → enabled, calls
    the action. The `Show log for desktop` button exists for the reader.
  - `public.test.tsx`: `PublicServicesPage` with two services → titles, descriptions,
    badges `Up` / `Unknown`, no link inside the cards, no Fleet tab, `Log in`
    calls `login`; empty list → `No services declared`; `PageHeader` with `session` shows
    `Log out` and calls `logout`, without `session` shows none; `publicServiceLabel`,
    `publicServiceCounts`, generic `groupServices` order.
- [ ] **Step 2: Run** → FAIL.
- [ ] **Step 3: Implement** with existing tokens, tones and components (`docs/style-guide.md`);
  add the new components, icons and the three states to the style guide.
- [ ] **Step 4: Run** `npx vitest run`, `npx tsc --noEmit`, `npm run build` → PASS; then
  check the demo (`nix run .#lodestar-demo -- --listen 127.0.0.1:18080`) is unchanged
  (tailnet-style admin), light and dark, 375 px wide.
- [ ] **Step 5: Commit** `feat(web): anonymous, reader and admin views`

### Task 10: VM test with Kanidm, operations docs

**Files:**
- Create: `nix/tests/kanidm-login.py`
- Modify: `nix/tests/vm.nix`, `docs/operations.md` (section "Public URL"), `README.md` (one line)

**Interfaces:**
- Consumes: module options (Task 2), the whole server flow (Tasks 4–7).
- Produces: nodes `kanidm` and `router` in `checks.x86_64-linux.vm`;
  `kanidm-login` (`pkgs.writers.writePython3Bin`, stdlib only):
  `kanidm-login --server http://<manager>:8081 --kanidm https://acme.test --user <u>
  --password-file <f>` prints the session cookie value, exits 1 naming the failed stage.

- [ ] **Step 1: Write the test changes** (mirror nixpkgs `nixos/tests/kanidm-provisioning.nix`
  for TLS and provisioning):
  - `kanidm` node: `services.kanidm.package = pkgs.kanidmWithSecretProvisioning_1_11`;
    server on `[::]:443`, `origin = "https://acme.test"`, `domain = "acme.test"`, TLS from
    `${pkgs.path}/nixos/tests/common/acme/server/snakeoil-certs.nix` (copied to the
    store as nixpkgs does); firewall 443; `provision`: persons `alice`, `bob`, `carol`;
    groups `lodestar-users` (alice), `admins` (bob); `systems.oauth2.lodestar` with
    `originUrl = "https://lodestar.example.test/auth/callback"`, `originLanding`,
    `basicSecretFile` (a fixed test secret), `preferShortUsername = true`, `scopeMaps` for
    `lodestar-users` and `admins` = `[ "openid" "profile" "groups" ]`.
  - `manager`: `security.pki.certificateFiles = [ certs.ca.cert ]`, `networking.hosts`
    `acme.test` → kanidm's IP; `services.lodestar-server`: `proxyListen.address =
    config.networking.primaryIPAddress`, `trustedProxies = [ router's IP ]`, `publicUrl =
    "https://lodestar.example.test"`, `oidc.issuer = "https://acme.test/oauth2/openid/lodestar"`,
    the same secret file, groups `lodestar-users@acme.test` / `admins@acme.test`.
  - `router`: CA, hosts entry, `kanidm-login`, curl, jq.
  - `kanidm-login.py`: GET `/auth/login` (no redirects; keep the login cookie) → POST
    Kanidm `/v1/auth` `init2` (`issue: "token"`), `begin: "password"`, `cred.password`
    (session id header `X-KANIDM-AUTH-SESSION-ID`) → bearer token → POST
    `/oauth2/authorise` with the authorize URL's query as a JSON object → on
    `ConsentRequested` POST `/oauth2/authorise/permit` with the consent token → the
    redirect's `code`/`state` → GET `/auth/callback` with the login cookie as a `Cookie`
    header → print the session cookie. Cookies are sent by hand (`__Host-`/`Secure`
    cookies are never stored over http). If a Kanidm step answers 4xx, check it against
    the packaged Kanidm's client library.
  - Script: passwords with `kanidmd recover-account -c <config> <name>` as nixpkgs does
    for `idm_admin` (if 1.11 refuses persons, use a credential reset token over the tty).
    Checks: from `router`, anonymous `/api/public/services` → first element's keys exactly
    `["description","group","icon","since","state","title"]`, `/api/snapshot` → 401;
    alice → `/api/me` role `reader`, snapshot 200, apply with the mark → 403; bob → apply
    without the mark → 403 `only over Tailscale`, with it → 202 and `/api/audit` shows
    actor `bob`; carol → login fails at Kanidm or Lodestar, no cookie; bob's logout → his
    cookie is anonymous; `manager` curling its own `<ip>:8081/api/me` with the mark →
    403 (untrusted peer); `kanidm` curling `manager:8081` with `--connect-timeout 5` →
    exit code 28 (dropped). The existing steps stay as they are.
- [ ] **Step 2: Evaluate locally**: `nix eval .#checks.x86_64-linux.vm.drvPath` → a path.
- [ ] **Step 3: Run on the Linux builder** (bundle `main` to nixos-manager as in
  `~/.flake/AGENTS.md` "Linux builder", clone to `~/build`): `nix build -L
  .#checks.x86_64-linux.vm .#checks.x86_64-linux.firewall` → PASS. Record the actual
  `groups` claim values seen in the test log if a group check fails.
- [ ] **Step 4: Docs**: `operations.md` "Public URL": what the entrance serves, the router
  contract (forward to `proxyListen`, set `X-Homelab-Via: tailnet` on the tailnet entrance,
  remove it on the public one), the options, the Kanidm client (issuer form, redirect,
  `preferShortUsername`, scopes, group names with `@domain`), sessions (12 h, groups read
  at login), troubleshooting (`login refused:` lines, 403 `forbidden` = untrusted peer).
- [ ] **Step 5: Full verification**: `cargo test --workspace`, clippy, `cargo fmt --check`,
  web `vitest`/`tsc`/`build`, `nix build .#lodestar .#lodestar-agent .#lodestar-server
  --no-link`, `nix flake check --no-build --keep-going` → all pass.
- [ ] **Step 6: Commit** `test(vm): router entrance with Kanidm login; docs: public URL`

### Task 11: Flake: Lodestar on `lodestar.vanwaaij.com`

**Files (in `~/.flake`):**
- Modify: `flake.lock`, `modules/lodestar/server.nix`, `modules/identity/kanidm.nix`
  (`servicesUrl` default and description), `tests/identity.nix`, `tests/router.nix`,
  `docs/lodestar.md`, `docs/identity.md`

**Interfaces:**
- Consumes: the module options (Task 2) through `inputs.lodestar`.
- Produces (in `modules/lodestar/server.nix`, inside the existing `mkIf … enable`, values
  `mkDefault` where the host may override): `homelab.routes.lodestar` (backend from
  `proxyListen`); `homelab.access.lodestar` (Global Constraints values; `oidc.icon` only if
  an icon exists in `modules/identity/branding/icons/`); opnix secret
  `kanidmOauth2Lodestar = { reference = "op://Homelab/kanidm-oauth2-lodestar/credential";
  owner = "lodestar-server"; mode = "0400"; }` (no `services`: read at login);
  `services.lodestar-server.{proxyListen.address = config.homelab.lan.address;
  trustedProxies; publicUrl; oidc.*}`; `lodestar.links.lodestar.url =
  "https://lodestar.vanwaaij.com"`. `servicesUrl` default
  `https://lodestar.vanwaaij.com/#/services`.

- [ ] **Step 1: Ask the user to push lodestar `main`**, then push with the Waayway token
  (as in the flake AGENTS.md, remote `origin`, `HEAD:main`) and run `nix flake update lodestar`.
- [ ] **Step 2: Failing checks**: in `tests/identity.nix` change the gate landing assert to
  `"https://lodestar.vanwaaij.com/#/services"` and add: `state.groups.lodestar-users.members
  == [ "thijs" ]`; `state.systems.oauth2.lodestar.originUrl == [
  "https://lodestar.vanwaaij.com/auth/callback" ]`; `.preferShortUsername`;
  `.scopeMaps == { lodestar-users = [ "openid" "profile" "groups" ]; }`;
  `.originLanding == "https://lodestar.vanwaaij.com/"`. In `tests/router.nix` on `router`:
  route `lodestar` exists with backend `http://10.0.10.170:8081`, `public`,
  `tailnet.enable`, no gate middleware. Run `nix build .#checks.x86_64-linux.identity
  .#checks.x86_64-linux.router --no-link` (or `nix eval …drvPath`) → FAIL.
- [ ] **Step 3: Implement**; format with the repo's nixfmt (AGENTS.md "Formatting").
- [ ] **Step 4: Verify**: the five host evaluations from AGENTS.md; `nix flake check
  --no-build --keep-going`; `nix eval .#nixosConfigurations.nixos-manager.config.environment.etc."lodestar-server/config.json".text`
  shows `proxy` with `10.0.10.170:8081`, `["10.0.10.110"]`, the issuer and both groups;
  `…nixos-manager.config.networking.firewall.extraCommands` contains `-s 10.0.10.110 --dport 8081`.
- [ ] **Step 5: Docs**: `docs/lodestar.md` (public URL, who sees what, the secret, the new
  policy grants, dashboard link); `docs/identity.md` (Lodestar row in the app table and
  the secret table: read from 1Password at runtime via opnix, like the gate).
- [ ] **Step 6: Commit** `feat(lodestar): public URL with Kanidm login` (only these paths).

### Task 12: Live rollout

No router-test step: router-test serves every flake route, but it is not a trusted proxy
(adding it would let a test box mint the tailnet mark), so through it Lodestar only ever
answers 403/timeout; the VM test covers the router role. The route is new, so the real
router carries no risk for existing sites. Verify the server side before the route exists.

- [ ] **Step 1: Ask the user to push `unstable`**; push; promote to `stable` (`lodestar promote --yes`; if it fails with "Host key verification failed" — known 2026-10-20 — push the same commit to `stable` from atlas with the Waayway token).
- [ ] **Step 2: Apply `identity`** (VM 111) through Lodestar; the `lodestar` client and
  `lodestar-users` group exist (`kanidm-provision` log).
- [ ] **Step 3: The user runs** `scripts/kanidm-oauth2-secret.sh lodestar` on atlas (Touch ID).
- [ ] **Step 4: Kanidm reachable from nixos-manager.** The LAN resolves `*.vanwaaij.com`
  to the router (UniFi local DNS, 2026-10-20), which passes `id` through to VM 111, so no
  Tailscale grant is needed. Check from nixos-manager:
  `curl -sf https://id.vanwaaij.com/oauth2/openid/lodestar/.well-known/openid-configuration | jq -r .issuer`
  → the issuer. If it resolves to a Tailscale address instead, stop and report.
- [ ] **Step 5: Apply `nixos-manager`**; `/run/…/kanidmOauth2Lodestar` exists, owned by
  `lodestar-server`; `journalctl -u lodestar-server` shows `router entrance on 10.0.10.170:8081`.
  From the router VM: `curl -s http://10.0.10.170:8081/api/me` → anonymous JSON; from
  prod (10.0.10.100): `curl --connect-timeout 5 http://10.0.10.170:8081/` → exit 28.
- [ ] **Step 6: Apply `router`**. Public: `curl -s https://lodestar.vanwaaij.com/api/public/services | jq '.[0] | keys'`
  → the six public keys; `curl -s -H 'X-Homelab-Via: tailnet' https://lodestar.vanwaaij.com/api/me`
  from outside the tailnet → still no tailnet mark (role anonymous, `can_act` false).
- [ ] **Step 7: The user creates the Tailscale Service `lodestar`** (tcp:443); the controller
  adds its grant with the same sources as `svc:auth`, then on the router
  `tailscale serve drain svc:lodestar` and `tailscale serve advertise svc:lodestar`
  (homelab finding 22). Check: `dig lodestar.vanwaaij.com` on a tailnet laptop → the Service IP.
- [ ] **Step 8: The user logs in** in a browser: on a phone off the tailnet → dashboard,
  action buttons disabled with `Only over Tailscale`; on a tailnet laptop through
  `https://lodestar.vanwaaij.com` → actions enabled; one harmless apply shows actor `thijs`
  in Audit; Kanidm's Services tile opens `https://lodestar.vanwaaij.com/#/services`. If
  login says `No access`, compare `readerGroup`/`adminGroup` with the claim format the VM
  test recorded.
- [ ] **Step 9: Docs**: if anything differed live, fix `docs/lodestar.md`; commit
  `docs(lodestar): public URL rollout notes` (flake), ask before pushing.
