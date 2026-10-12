# AGENTS.md

Guide for coding agents working on Lodestar. Read `README.md` first for what it
does; `docs/operations.md` for how it is run.

## Layout

- `Cargo.toml`: workspace (edition 2024, `publish = false`, `MIT OR Apache-2.0`).
  - `lodestar-proto/`: wire types shared by agent, server and web. Exported to
    TypeScript with ts-rs into `web/src/bindings/` (generated, never edit).
  - `lodestar-agent/`: the agent daemon (`serve`) and the apply wrapper
    (`apply.rs`: state files under `/var/lib/lodestar-agent`, `launcher.rs`:
    systemd-run / detach), git as the user (`git.rs`), pre-apply checks
    (`checks.rs`), manager source check (`net.rs`).
  - `lodestar-server/`: axum API + WebSocket (`api.rs`, `ws.rs`), access via
    `tailscale whois` (`access.rs`, `tailscale.rs`), git mirror and promote
    (`mirror.rs`), poller (`poller.rs`), SeaORM/SQLite (`db/`, `migration/`),
    the embedded frontend (`embed.rs`, `build.rs`: `LODESTAR_WEB_DIST`, else
    `../web/dist`), and demo mode (`demo/`: throwaway repo, simulated agents).
  - `lodestar-cli/`: the `lodestar` CLI (library + thin `main.rs`).
- `web/`: Preact + shadcn/ui dashboard (Vite, Tailwind v4, vitest). Look and
  components are described in `docs/style-guide.md`. In `web/src/`:
  - `main.tsx` renders `App.tsx`, which only composes providers (theme,
    tooltips, toasts, connection) around `pages/DashboardPage.tsx`.
  - `connection/`: the only code that talks to the server: `client.ts` (HTTP,
    `X-Lodestar`, `ApiError`, 403), `feed.ts` (WebSocket, backoff, 403 stop),
    `ConnectionProvider.tsx` (snapshot, open log, audit, actions) and
    `hooks.ts` (`useFleet`, `useHost`, `useApplyLog`, `useAudit`, `useActions`).
    No component calls `fetch` or opens a socket.
  - `lib/`: pure helpers (`format.ts` labels, `applyLog.ts` log merge,
    `time.ts`, `theme.ts`, `cn.ts`).
  - `components/ui/` (shadcn primitives in the tokens), `components/brand/`,
    `components/fleet/`, `components/layout/`; `icons/` (Lucide under
    Lodestar names, `BrandIcon`); `styles/tokens.css` + `styles/base.css`;
    `bindings/` (generated).
  - Tests in `web/tests/`, by behaviour (`formatting`, `log-streaming`,
    `connection`, `components`).
- `default.nix`: crane builds of the crates, `buildNpmPackage` of `web/`
  (`npmDepsHash`), and the bindings check.
- `flake.nix`: packages, apps, modules, `lib.hostsFrom`, checks, dev shell.
- `nix/modules/agent/{common,nixos,darwin}.nix`: `services.lodestar-agent`
  (shared options in `common.nix`; systemd in `nixos.nix`, launchd in
  `darwin.nix`); `audit-hook.nix`: the activation audit snippet.
- `nix/modules/server.nix`: `services.lodestar-server` (NixOS only).
- `nix/lib/hostsFrom.nix`, `nix/tests/vm.nix` (NixOS VM test).
- `docs/`: `operations.md`, `style-guide.md` (the dashboard's look and components); `plans/` (dated design and plan documents; the
  `fleet-manager` ones are historical).

## Working

Use the dev shell for Rust and Node (`cargo`/`node` on PATH may be missing or
broken):

```sh
nix develop -c cargo test --workspace
nix develop -c cargo clippy --workspace --all-targets -- -D warnings
nix develop -c cargo fmt --check
cd web && nix develop .. -c npx vitest run
cd web && nix develop .. -c npx tsc --noEmit
cd web && nix develop .. -c npm run build
nix build .#lodestar .#lodestar-agent .#lodestar-server --no-link
nix flake check --no-build --keep-going
```

- New files must be `git add`ed before Nix sees them.
- After changing a type in `lodestar-proto`, regenerate the bindings:
  `cd lodestar-proto && TS_RS_EXPORT_DIR=../web/src/bindings nix develop .. -c cargo test -p lodestar-proto`
  (remove any stray `lodestar-proto/bindings/`). `checks.<system>.bindings`
  fails when they are stale.
- After changing `web/package-lock.json`, update `npmDepsHash` in
  `default.nix` (take the `got:` hash from the failing build).
- `checks.x86_64-linux.vm` and `.firewall` need an x86_64-linux builder (the VM
  test needs KVM); on other machines check that they evaluate:
  `nix eval .#checks.x86_64-linux.vm.drvPath`.
- Demo: `nix run .#lodestar-demo -- --listen 127.0.0.1:18080 --speed 4`.

## Conventions

- Modules: every user-specific value is an option; no `specialArgs` from the
  consumer. Platform split by separate modules (`nixosModules.agent` /
  `darwinModules.agent`), not by conditions on `pkgs` (that recurses).
  Format Nix with the flake's formatter (`nix fmt`, RFC-style nixfmt).
- Rust: `cargo fmt`, clippy clean with `-D warnings`. Errors with `anyhow`
  and context. Log lines start with the binary name (`lodestar-agent: ...`).
- Every git argument from a caller is validated (`is_commit_hash`, channel
  name check) and refs are fully qualified, so nothing can be read as an
  option.
- Tests next to the code they cover: Rust integration tests in
  `<crate>/tests/`, vitest files in `web/tests/` (grouped by behaviour, importing via `@/`). Test data is neutral
  (`alice`, `example.com`, `100.64.0.0/10`).
- Commits: conventional style (`feat(agent): ...`, `fix(cli): ...`), small and
  focused.
