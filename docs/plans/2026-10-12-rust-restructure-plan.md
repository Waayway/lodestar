# Rust restructure plan (2026-10-12)

The crates grew as flat folders with large files (`lodestar-server/src/api.rs`
holds routing, handlers, errors, CSRF and helpers). Restructure for readability
without changing behaviour.

## Rules (apply to every crate: lodestar-proto, lodestar-agent, lodestar-server
(+ migration), lodestar-cli)

1. **One struct per file**, with the types that belong to it (its enums,
   its error type, small private helpers it alone uses). A file named after its
   struct (`apply_record.rs` → `ApplyRecord`). Free functions that form one
   concern share a file named after the concern.
2. **Modules are folders.** A concern with more than one file becomes a folder
   with `mod.rs` (or `<name>.rs` + `<name>/`) that only declares submodules and
   re-exports the public items; no logic in `mod.rs` beyond that.
3. **Full, descriptive names** for variables, parameters, fields and
   functions: `transaction` not `txn`, `config` not `cfg`, `host` not `h`,
   `state` not `st`, `request`/`response` not `req`/`resp`, `error` not `e`
   (closures included), `index` not `i` except trivial numeric loops,
   `repository`/`mirror` not `repo`/`m`. Single-letter names only for
   generic type parameters.
4. Keep public API paths working for tests and the binaries (re-export from
   the module root); update `use` paths where needed.
5. **No behaviour change.** The diff is moves, splits and renames. All tests
   keep passing unchanged except for `use` paths.

## Server layout (target)

```
lodestar-server/src/
  main.rs                 CLI parsing + calls into server::start / demo::start
  lib.rs                  module declarations + re-exports
  config/                 server_config.rs, access_config.rs, online_source.rs, fleet_host.rs, listen_check.rs
  api/
    mod.rs                router() assembly only
    error.rs              ApiError + IntoResponse
    csrf.rs               require_same_site layer
    snapshot.rs           GET /api/snapshot
    hosts.rs              POST /api/hosts/{name}/apply, GET log
    channels.rs           POST /api/channels/{channel}/apply
    promote.rs            POST /api/promote
    audit.rs              GET /api/audit
    apply_request.rs      shared create-applies / send-apply logic
  websocket/              handler, session (snapshot first, lagged → snapshot, close)
  access/                 identity.rs, decision.rs, middleware.rs
  tailscale/              peer_info.rs, status_parser.rs, whois_parser.rs, cli_provider.rs, static_provider.rs, online_provider.rs (trait)
  mirror/                 mirror.rs (struct), promote_error.rs, git_command.rs
  poller/                 poll_loop.rs, host_poll.rs, reachability.rs, apply_recording.rs, activation_ingest.rs, log_follower.rs
  resend.rs               should_resend (pure)
  state/                  app_state.rs, host_view.rs, snapshot.rs
  agent_client.rs         AgentClient
  db/                     connection.rs + one file per repository concern (applies.rs, desired.rs, status.rs, audit.rs, activations.rs), entities/ unchanged
  embed.rs                static assets
  server.rs               ServerHandle + start
  demo/                   unchanged layout, same rules inside
```

Agent and CLI: same rules; split any file over ~250 lines by struct/concern
(e.g. agent `apply.rs` → `apply/` with `record.rs`, `lock.rs`, `wrapper.rs`,
`log_reader.rs`, `prune.rs`; `api.rs` → `api/` like the server; CLI `lib.rs` →
`commands/` one file per subcommand + `client.rs`, `wait.rs`, `table.rs`).

## Verification

- `nix develop -c cargo test --workspace` (same test count as before)
- `nix develop -c cargo clippy --workspace --all-targets -- -D warnings`
- `nix develop -c cargo fmt --check`
- `rg -n "\btxn\b|\bcfg\b|\breq\b|\bresp\b|\|e\||\(e\)|\bst\b"` over `*/src` returns nothing
  (or only justified hits listed in the report)
