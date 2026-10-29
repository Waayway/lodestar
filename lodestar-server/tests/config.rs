use lodestar_server::config::{ServerConfig, Upstream};
use std::path::PathBuf;

const BASE: &str = r#""listen":"127.0.0.1:8080","agent_port":7461,"hosts_file":"/h.json",
    "repo_url":"u","push_url":"p","state_dir":"/s","access":{"kind":"disabled"},
    "online":{"kind":"tailscale"}"#;

fn parse(extra: &str) -> ServerConfig {
    serde_json::from_str(&format!("{{{BASE}{extra}}}")).unwrap()
}

fn with_updates(updates: &str) -> ServerConfig {
    parse(&format!(",\"updates\":{updates}"))
}

#[test]
fn nix_shape_parses() {
    let config = parse(
        r#","updates":{"channel":"unstable","interval_secs":86400,"upstream_interval_secs":21600,
        "software":{"tailscale":{"input":"nixpkgs-unstable","upstream":{"github":"tailscale/tailscale"}}}},
        "metrics":{"listen":"10.0.10.170:9466"}"#,
    );
    let software = &config.updates.software["tailscale"];
    assert_eq!(software.input, "nixpkgs-unstable");
    assert_eq!(
        software.upstream,
        Upstream::Github("tailscale/tailscale".into())
    );
    assert_eq!(
        config.metrics.unwrap().listen,
        "10.0.10.170:9466".parse().unwrap()
    );
    let v6 = parse(r#","metrics":{"listen":"[fd00::1]:9466"}"#);
    assert_eq!(v6.metrics.unwrap().listen.port(), 9466);
}

#[test]
fn absent_updates_and_metrics_default() {
    let config = parse("");
    assert_eq!(config.updates.channel, "unstable");
    assert_eq!(config.updates.interval_secs, 86400);
    assert_eq!(config.updates.upstream_interval_secs, 21600);
    assert!(config.updates.software.is_empty());
    assert_eq!(config.metrics, None);
    assert_eq!(config.nix_bin, PathBuf::from("nix"));
    config.check_updates().unwrap();
    let partial = with_updates(r#"{"channel":"next"}"#);
    assert_eq!(partial.updates.interval_secs, 86400);
}

fn software(name: &str, input: &str, github: &str) -> String {
    format!(
        r#"{{"software":{{"{name}":{{"input":"{input}","upstream":{{"github":"{github}"}}}}}}}}"#
    )
}

#[test]
fn check_accepts_valid() {
    with_updates(&software(
        "tail-scale_1",
        "nixpkgs-unstable",
        "tailscale/tail.scale",
    ))
    .check_updates()
    .unwrap();
}

#[test]
fn check_rejects_bad_values_naming_them() {
    let cases = [
        (
            software("tailscale", "nixpkgs unstable", "a/b"),
            "nixpkgs unstable",
        ),
        (software("tailscale", "nixpkgs", "tailscale"), "tailscale"),
        (software("Tail scale", "nixpkgs", "a/b"), "Tail scale"),
        (r#"{"channel":"-x"}"#.into(), "-x"),
        (r#"{"channel":"stable"}"#.into(), "stable"),
    ];
    for (updates, needle) in cases {
        let error = with_updates(&updates).check_updates().unwrap_err();
        assert!(error.to_string().contains(needle), "{error} lacks {needle}");
    }
}

#[test]
fn channel_may_not_be_the_configured_promote_target() {
    let config =
        parse(r#","promote":{"from":"main","to":"release"},"updates":{"channel":"release"}"#);
    assert!(config.check_updates().is_err());
    let ok = parse(r#","promote":{"from":"main","to":"release"},"updates":{"channel":"stable"}"#);
    ok.check_updates().unwrap();
    let off = parse(r#","promote":null,"updates":{"channel":"stable"}"#);
    assert!(off.check_updates().is_err());
}

#[test]
fn inputs_group_software() {
    let config = with_updates(
        r#"{"software":{
        "tailscale":{"input":"nixpkgs","upstream":{"github":"a/b"}},
        "caddy":{"input":"nixpkgs","upstream":{"github":"c/d"}},
        "other":{"input":"x","upstream":{"github":"e/f"}}}}"#,
    );
    let inputs = config.updates.inputs();
    assert_eq!(inputs["nixpkgs"], vec!["caddy", "tailscale"]);
    assert_eq!(inputs["x"], vec!["other"]);
}
