//! Prometheus metrics for tracked software: the pure renderer and the `/metrics` listener.

mod support;

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::Arc;

use lodestar_server::builds::BuildMetric;
use lodestar_server::config::{
    AccessConfig, MetricsConfig, OnlineSource, ServerConfig, SoftwareConfig, UpdatesConfig,
    Upstream,
};
use lodestar_server::server;
use lodestar_server::tailscale::StaticOnline;
use lodestar_server::updates::metrics::{MetricsInput, render};
use lodestar_server::updates::update_run::{UpdateOutcome, UpdateRun, VersionChange};
use lodestar_server::updates::upstream::LatestVersion;
use support::Origin;

fn running(pairs: &[(&str, &str, &str)]) -> BTreeMap<String, BTreeMap<String, String>> {
    let mut out: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
    for (host, software, version) in pairs {
        out.entry(host.to_string())
            .or_default()
            .insert(software.to_string(), version.to_string());
    }
    out
}

fn latest(version: Option<&str>, checked_at: Option<i64>) -> BTreeMap<String, LatestVersion> {
    BTreeMap::from([(
        "tailscale".to_string(),
        LatestVersion {
            version: version.map(String::from),
            checked_at,
            error: None,
            errors: 0,
        },
    )])
}

#[test]
fn upstream_check_errors_are_a_counter() {
    let mut known = latest(None, None);
    known.get_mut("tailscale").unwrap().errors = 3;
    let out = render(&MetricsInput {
        running: BTreeMap::new(),
        latest: known,
        last_runs: BTreeMap::new(),
        builds: BTreeMap::new(),
    });
    assert!(
        out.contains("# TYPE lodestar_software_latest_check_errors_total counter\n"),
        "{out}"
    );
    assert!(
        out.contains("lodestar_software_latest_check_errors_total{software=\"tailscale\"} 3\n"),
        "{out}"
    );
    // Zero is rendered too (a counter that exists from the first check).
    let out = render(&MetricsInput {
        running: BTreeMap::new(),
        latest: latest(Some("1.104.2"), Some(1)),
        last_runs: BTreeMap::new(),
        builds: BTreeMap::new(),
    });
    assert!(
        out.contains("lodestar_software_latest_check_errors_total{software=\"tailscale\"} 0\n"),
        "{out}"
    );
}

#[test]
fn renders_running_latest_and_up_to_date() {
    let out = render(&MetricsInput {
        running: running(&[
            ("a", "tailscale", "1.104.2"),
            ("b", "tailscale", "1.98.10"),
            ("c", "tailscale", "1.x"),
        ]),
        latest: latest(Some("1.104.2"), Some(1234)),
        last_runs: BTreeMap::new(),
        builds: BTreeMap::new(),
    });
    for line in [
        r#"lodestar_software_running_info{host="a",software="tailscale",version="1.104.2"} 1"#,
        r#"lodestar_software_running_info{host="c",software="tailscale",version="1.x"} 1"#,
        r#"lodestar_software_up_to_date{host="a",software="tailscale"} 1"#,
        r#"lodestar_software_up_to_date{host="b",software="tailscale"} 0"#,
        r#"lodestar_software_latest_info{software="tailscale",version="1.104.2"} 1"#,
        r#"lodestar_software_latest_checked_timestamp_seconds{software="tailscale"} 1234"#,
        "# TYPE lodestar_software_running_info gauge",
        "# TYPE lodestar_software_up_to_date gauge",
        "# TYPE lodestar_software_latest_info gauge",
        "# TYPE lodestar_software_latest_checked_timestamp_seconds gauge",
    ] {
        assert!(out.lines().any(|l| l == line), "missing {line}\n{out}");
    }
    assert!(!out.contains(r#"up_to_date{host="c""#), "{out}");
    assert!(out.ends_with('\n'));
    // sorted by labels within a family
    let a = out.find(r#"running_info{host="a""#).unwrap();
    let b = out.find(r#"running_info{host="b""#).unwrap();
    assert!(a < b);
}

#[test]
fn newer_than_latest_counts_as_up_to_date() {
    let out = render(&MetricsInput {
        running: running(&[("a", "tailscale", "1.105.0")]),
        latest: latest(Some("1.104.2"), Some(1)),
        last_runs: BTreeMap::new(),
        builds: BTreeMap::new(),
    });
    assert!(out.contains(r#"up_to_date{host="a",software="tailscale"} 1"#));
}

#[test]
fn unknown_latest_gives_running_lines_only() {
    let out = render(&MetricsInput {
        running: running(&[("a", "tailscale", "1.104.2")]),
        latest: latest(None, None),
        last_runs: BTreeMap::new(),
        builds: BTreeMap::new(),
    });
    assert!(out.contains("lodestar_software_running_info{"));
    assert!(!out.contains("lodestar_software_up_to_date{"));
    assert!(!out.contains("lodestar_software_latest_info{"));
    assert!(!out.contains("latest_checked_timestamp_seconds{"));
}

#[test]
fn checked_timestamp_only_when_checked() {
    let out = render(&MetricsInput {
        running: BTreeMap::new(),
        latest: latest(Some("1.0"), None),
        last_runs: BTreeMap::new(),
        builds: BTreeMap::new(),
    });
    assert!(out.contains(r#"latest_info{software="tailscale",version="1.0"} 1"#));
    assert!(!out.contains("latest_checked_timestamp_seconds{"));
}

#[test]
fn label_values_are_escaped() {
    let out = render(&MetricsInput {
        running: running(&[("a\"b", "tailscale", "1\\2\n3")]),
        latest: BTreeMap::new(),
        last_runs: BTreeMap::new(),
        builds: BTreeMap::new(),
    });
    assert!(
        out.contains(r#"host="a\"b",software="tailscale",version="1\\2\n3""#),
        "{out}"
    );
}

#[test]
fn renders_last_update_run_per_input() {
    let run = |input: &str, finished_at: i64, outcome: UpdateOutcome| {
        (
            input.to_string(),
            UpdateRun {
                input: input.into(),
                finished_at,
                outcome,
            },
        )
    };
    let out = render(&MetricsInput {
        running: BTreeMap::new(),
        latest: BTreeMap::new(),
        last_runs: BTreeMap::from([
            run(
                "nixpkgs-unstable",
                1234,
                UpdateOutcome::Pushed {
                    commit: "a".repeat(40),
                    changes: vec![VersionChange {
                        software: "tailscale".into(),
                        from: "1.98.10".into(),
                        to: "1.104.2".into(),
                    }],
                },
            ),
            run(
                "home-manager",
                99,
                UpdateOutcome::BuildFailed {
                    host: "a".into(),
                    log: "boom".into(),
                },
            ),
        ]),
        builds: BTreeMap::new(),
    });
    for line in [
        r#"lodestar_update_last_result{input="nixpkgs-unstable",result="pushed"} 1"#,
        r#"lodestar_update_last_run_timestamp_seconds{input="nixpkgs-unstable"} 1234"#,
        r#"lodestar_update_last_result{input="home-manager",result="build-failed"} 1"#,
        r#"lodestar_update_last_run_timestamp_seconds{input="home-manager"} 99"#,
        "# TYPE lodestar_update_last_result gauge",
        "# TYPE lodestar_update_last_run_timestamp_seconds gauge",
    ] {
        assert!(out.lines().any(|l| l == line), "missing {line}\n{out}");
    }
    // Only the last result per input is present.
    assert_eq!(
        out.matches("lodestar_update_last_result{").count(),
        2,
        "{out}"
    );
}

#[test]
fn renders_the_build_families() {
    let out = render(&MetricsInput {
        running: BTreeMap::new(),
        latest: BTreeMap::new(),
        last_runs: BTreeMap::new(),
        builds: BTreeMap::from([
            (
                "stable".to_string(),
                BuildMetric {
                    result: "ok".into(),
                    finished_at: 1700,
                    duration_seconds: Some(42.5),
                },
            ),
            (
                "unstable".to_string(),
                BuildMetric {
                    result: "failed".into(),
                    finished_at: 1800,
                    duration_seconds: None,
                },
            ),
        ]),
    });
    for line in [
        r#"lodestar_build_last_result{channel="stable",result="ok"} 1"#,
        r#"lodestar_build_last_result{channel="unstable",result="failed"} 1"#,
        r#"lodestar_build_last_timestamp_seconds{channel="stable"} 1700"#,
        r#"lodestar_build_last_timestamp_seconds{channel="unstable"} 1800"#,
        r#"lodestar_build_duration_seconds{channel="stable"} 42.5"#,
        "# TYPE lodestar_build_last_result gauge",
        "# TYPE lodestar_build_last_timestamp_seconds gauge",
        "# TYPE lodestar_build_duration_seconds gauge",
    ] {
        assert!(out.lines().any(|l| l == line), "missing {line}\n{out}");
    }
    assert_eq!(out.matches("lodestar_build_duration_seconds{").count(), 1);
}

#[tokio::test]
async fn metrics_listener_serves_and_dashboard_does_not() {
    let dir = tempfile::tempdir().unwrap();
    let origin = Origin::new(dir.path());
    origin.commit("one", "main");
    let config = ServerConfig {
        listen: "127.0.0.1:0".parse().unwrap(),
        agent_port: 7461,
        hosts_file: dir.path().join("hosts.json"),
        links_file: None,
        repo_url: origin.bare.to_str().unwrap().into(),
        push_url: origin.bare.to_str().unwrap().into(),
        deploy_key_file: None,
        state_dir: dir.path().join("state"),
        access: AccessConfig::Disabled,
        online: OnlineSource::Static {
            addresses: HashMap::new(),
        },
        mirror_fetch_secs: 300,
        poll_secs: 60,
        tailscale_bin: PathBuf::from("tailscale"),
        promote: None,
        proxy: None,
        updates: UpdatesConfig {
            software: BTreeMap::from([(
                "tailscale".to_string(),
                SoftwareConfig {
                    input: "nixpkgs".into(),
                    upstream: Upstream::Github("tailscale/tailscale".into()),
                },
            )]),
            ..Default::default()
        },
        builds: Default::default(),
        metrics: Some(MetricsConfig {
            listen: "127.0.0.1:0".parse().unwrap(),
        }),
        nix_bin: "nix".into(),
        agent_token_files: Default::default(),
    };
    let online = Arc::new(StaticOnline::new(&HashMap::new()).unwrap());
    let handle = server::start(config, vec![], vec![], online, false)
        .await
        .unwrap();
    handle.state.versions.lock().unwrap().insert(
        "a".into(),
        BTreeMap::from([("tailscale".into(), "1.0".into())]),
    );
    let metrics = handle.metrics_addr.expect("metrics listener");

    let response = reqwest::get(format!("http://{metrics}/metrics"))
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let content_type = response.headers()["content-type"]
        .to_str()
        .unwrap()
        .to_string();
    assert!(
        content_type.starts_with("text/plain; version=0.0.4"),
        "{content_type}"
    );
    let body = response.text().await.unwrap();
    assert!(body.contains(
        r#"lodestar_software_running_info{host="a",software="tailscale",version="1.0"} 1"#
    ));

    let other = reqwest::get(format!("http://{metrics}/api/snapshot"))
        .await
        .unwrap();
    assert_eq!(other.status(), 404);
    let dashboard = reqwest::get(format!("http://{}/metrics", handle.addr))
        .await
        .unwrap();
    assert_eq!(dashboard.status(), 404);
    handle.stop().await;
}
