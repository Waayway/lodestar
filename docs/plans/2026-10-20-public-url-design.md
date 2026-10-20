# Lodestar on a public URL with Kanidm login

Status: agreed 2026-10-20. Follows section B of the homelab front-door design
(`~/.flake/docs/plans/2026-10-19-homelab-front-door-design.md`), with two refinements
from the 2026-10-20 review: anonymous visitors get no links, and reading and acting are
separate groups.

## Goal

`https://lodestar.vanwaaij.com` works from anywhere. Anyone sees what the homelab hosts
and whether it's up; logged-in people see the full dashboard; admins can act, but only
when they arrive over Tailscale. The existing tailnet address and the CLI keep working
unchanged.

## Who sees what

| Visitor | Entrance | Gets |
|---|---|---|
| anyone | router entrance, no session | Services tab only, from a reduced feed: title, group, description, icon, state (up / down / unknown), since. No URL, host, latency or error text. No Fleet tab. |
| member of the reader group (`lodestar-users`) | router entrance, session | everything read-only: snapshot, WebSocket feed, Services with links, audit, apply logs |
| member of the admin group (`admins`) | router entrance, session, marked as tailnet | also the actions: host apply, channel apply, promote |
| member of the admin group | router entrance, session, from the internet | read-only; the action buttons are disabled with "only over Tailscale" |
| allowed tailnet user or tag | tailnet entrance (today's listener) | unchanged: `tailscale whois`, full access, no login |

An admin is also a reader (the admin group grants reading even if the person is not in
the reader group).

## Components

### Login (lodestar-server)

- OpenID Connect authorization code flow with PKCE (S256), `state` and `nonce`, against
  one issuer. Kanidm signs ID tokens with ES256; the client must accept it.
- Routes on the router entrance: `GET /auth/login` (optional `?next=` path, only
  same-origin relative paths), `GET /auth/callback`, `POST /auth/logout` (CSRF rules
  as for `/api`).
- Groups come from the ID token's `groups` claim, compared exactly against the
  configured names (Kanidm sends `name@domain`, e.g. `lodestar-users@id.vanwaaij.com`).
  Someone in neither group gets "no access" after login and no session.
- Session: a random 256-bit id in cookie `__Host-lodestar_session` (Secure, HttpOnly,
  SameSite=Lax, Path=/), 12 hours, stored hashed in SQLite with the user name, the
  role (reader/admin) and the expiry. Group membership is read at login only; a change
  takes effect at the next login (at most 12 hours). Logout and expiry delete the row.
- Audit actor for actions through this entrance: the Kanidm user name (`thijs`).

### Router entrance (lodestar-server)

- A second listener, separate router: `proxyListen` address and port (nixos-manager:
  `10.0.10.170:8081`). Requests whose peer is not in `trustedProxies` are refused (403)
  before any other handling, in addition to the firewall.
- "Arrived over Tailscale" = header `X-Homelab-Via: tailnet` on a request from a trusted
  proxy. The router sets this header on its tailnet entrance and removes it on its
  public entrance, so it can't be forged through the router.
- It serves the web UI, the public feed, `/auth/*`, `/api/me`, and the existing API
  behind the session and role rules above. The tailnet listener and the agent protocol
  don't change; the agents never talk to this entrance.

### Public feed and `/api/me`

- `GET /api/public/services`: a new reduced type `PublicService` (title, group,
  description, icon, state, since), its own type in `lodestar-proto`, never derived by
  stripping `ServiceView` at the edge. Polled by the logged-out page every 30 seconds.
- `GET /api/me`: `{ viewer, role: anonymous | reader | admin, canAct, reason }`, on both
  entrances (the tailnet entrance always answers admin with `canAct = true`).

### Web

- One more connection state from `/api/me`. Anonymous: only the Services tab, fed by the
  public feed, plus a "Log in" button. Reader: the full dashboard, action buttons hidden.
  Admin without `canAct`: action buttons disabled with "only over Tailscale". Log out in
  the header for session users.

### NixOS module (`services.lodestar-server`)

New options: `proxyListen.{address, port}`, `trustedProxies` (list of addresses),
`oidc.{issuer, clientId, clientSecretFile, readerGroup, adminGroup}`, `publicUrl`
(redirect URI is `<publicUrl>/auth/callback`). The firewall opens the proxy port only
for `trustedProxies`. All unset = today's behaviour.

## This flake (`~/.flake`)

- `homelab.routes.lodestar`: host `lodestar.vanwaaij.com`, public and tailnet, backend
  `http://10.0.10.170:8081`. Not gated (Lodestar does its own login).
- `homelab.access.lodestar`: members `thijs` (group `lodestar-users`), OAuth2 client with
  redirect `https://lodestar.vanwaaij.com/auth/callback`, short user names, scopes
  `openid profile groups`. Secret copied with `scripts/kanidm-oauth2-secret.sh lodestar`
  to 1Password `kanidm-oauth2-lodestar`, read on nixos-manager through opnix.
- The Lodestar link and the Kanidm Services tile point to `https://lodestar.vanwaaij.com`.
- Tailscale: the user creates the Service `lodestar`; the policy grants it like the other
  Services.

## Error handling

- Issuer unreachable at login: a plain error page with "try again"; the public page and
  existing sessions keep working (discovery is fetched at login time and cached).
- Callback with a bad or replayed `state`, wrong `nonce`, expired code or bad signature:
  refused, no session, logged with the reason (no token contents).
- Session expired mid-use: API answers 401; the web falls back to the anonymous view
  with "Log in".

## Testing

- Rust: role decisions as a table (entrance × session × role × tailnet mark × peer), the
  `next` validation, cookie attributes, session expiry, and the callback against a fake
  issuer (ES256 keys, wrong nonce, wrong state, missing groups).
- Web: vitest for the three states and the disabled buttons.
- VM test: a Kanidm node (nixpkgs module, provisioned person and groups), the server with
  the router entrance, and a "router" client node. Checks: anonymous gets only
  `PublicService` fields; login works end to end; a reader can't act; an admin can't act
  without the tailnet mark; the mark from an untrusted address is refused; the firewall
  drops the proxy port for other addresses.
- Live: on the router-test VM first, then the real route.

## Not in scope

Changing the tailnet listener or the CLI; replacing `tailscale whois`; per-host
permissions; refresh tokens.
