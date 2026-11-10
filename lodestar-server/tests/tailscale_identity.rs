//! Managed hosts are bound to a Tailscale identity (stable node ID or MagicDNS name), never
//! to the HostName a device reports about itself.

use std::net::IpAddr;
use std::os::unix::fs::PermissionsExt;

use lodestar_server::config::{ManagedHost, OnlineSource, check_tailscale_identities};
use lodestar_server::tailscale::{
    OnlineProvider, PeerInfo, TailscaleCli, match_peers, parse_status, provider_from_config,
};

fn managed(name: &str, node_id: Option<&str>, dns_name: Option<&str>) -> ManagedHost {
    ManagedHost {
        name: name.into(),
        system: "x86_64-linux".into(),
        user: "alice".into(),
        channel: "main".into(),
        os: "linux".into(),
        node_id: node_id.map(Into::into),
        dns_name: dns_name.map(Into::into),
    }
}

fn ip(text: &str) -> Option<IpAddr> {
    Some(text.parse().unwrap())
}

/// A peer entry of `tailscale status --json`.
fn peer(id: &str, host_name: &str, dns_name: &str, address: &str, online: bool) -> String {
    format!(
        r#""nodekey:{id}":{{"ID":"{id}","HostName":"{host_name}","DNSName":"{dns_name}","OS":"linux","TailscaleIPs":["{address}"],"Online":{online},"LastSeen":"2026-10-12T09:00:00Z"}}"#
    )
}

fn status(peers: &[String]) -> Vec<PeerInfo> {
    parse_status(&format!(r#"{{"Peer":{{{}}}}}"#, peers.join(","))).unwrap()
}

#[test]
fn status_keeps_every_node_with_its_identity() {
    let peers = parse_status(include_str!("fixtures/status.json")).unwrap();
    assert_eq!(peers.len(), 4);
    let me = peers.iter().find(|peer| peer.node_id == "n1").unwrap();
    assert_eq!(me.dns_name, "manager.tail1234.ts.net");
    assert_eq!(me.addr, ip("100.64.0.1"));
    let laptop = peers.iter().find(|peer| peer.node_id == "n2").unwrap();
    assert_eq!(laptop.host, "laptop");
    assert_eq!(laptop.dns_name, "laptop.tail1234.ts.net");
    assert_eq!(laptop.os, "macOS");
    assert_eq!(laptop.addr, ip("100.64.0.2"));
    assert_eq!(laptop.last_seen, Some(1_791_792_000));
    let ghost = peers.iter().find(|peer| peer.node_id == "n4").unwrap();
    assert_eq!(ghost.last_seen, None);
    assert_eq!(ghost.addr, ip("fd7a:115c:a1e0::4"));
}

#[test]
fn impostor_with_the_same_host_name_is_never_picked_by_node_id() {
    let real = peer(
        "nREAL",
        "laptop",
        "laptop.example.ts.net.",
        "100.64.0.2",
        false,
    );
    // Online, seen later, smaller DNSName: it would have won every old tie-break.
    let impostor = peer("nEVIL", "laptop", "a.example.ts.net.", "100.64.0.66", true);
    for order in [[&real, &impostor], [&impostor, &real]] {
        let peers = status(&[order[0].clone(), order[1].clone()]);
        let matched = match_peers(&[managed("Laptop", Some("nREAL"), None)], &peers);
        let laptop = &matched.peers["laptop"];
        assert_eq!(laptop.addr, ip("100.64.0.2"));
        assert!(!laptop.online);
        assert!(matched.problems.is_empty(), "{:?}", matched.problems);
    }
}

#[test]
fn impostor_with_the_same_host_name_is_never_picked_by_dns_name() {
    let real = peer(
        "nREAL",
        "laptop",
        "laptop.example.ts.net.",
        "100.64.0.2",
        true,
    );
    let impostor = peer(
        "nEVIL",
        "laptop",
        "laptop-1.example.ts.net.",
        "100.64.0.66",
        true,
    );
    let peers = status(&[impostor, real]);
    let matched = match_peers(
        &[managed("laptop", None, Some("Laptop.Example.ts.net."))],
        &peers,
    );
    assert_eq!(matched.peers["laptop"].addr, ip("100.64.0.2"));
}

#[test]
fn the_identity_matches_whatever_host_name_the_node_reports() {
    let peers = status(&[peer(
        "n7",
        "renamed",
        "laptop.example.ts.net.",
        "100.64.0.7",
        true,
    )]);
    let matched = match_peers(&[managed("laptop", Some("n7"), None)], &peers);
    assert_eq!(matched.peers["laptop"].addr, ip("100.64.0.7"));
}

#[test]
fn a_host_name_alone_matches_nothing() {
    let peers = status(&[peer(
        "n7",
        "laptop",
        "laptop.example.ts.net.",
        "100.64.0.7",
        true,
    )]);
    let matched = match_peers(&[managed("laptop", None, None)], &peers);
    assert!(matched.peers.is_empty());
    assert_eq!(matched.problems.len(), 1, "{:?}", matched.problems);
    assert!(matched.problems[0].contains("laptop"));
}

#[test]
fn unmatched_identity_leaves_the_host_off() {
    let peers = status(&[peer(
        "nEVIL",
        "laptop",
        "laptop.example.ts.net.",
        "100.64.0.66",
        true,
    )]);
    let matched = match_peers(&[managed("laptop", Some("nREAL"), None)], &peers);
    assert!(matched.peers.is_empty());
    assert!(
        matched.problems[0].contains("nREAL"),
        "{:?}",
        matched.problems
    );
}

#[test]
fn ambiguous_dns_name_fails_closed() {
    let peers = status(&[
        peer("n1", "laptop", "laptop.example.ts.net.", "100.64.0.2", true),
        peer(
            "n2",
            "laptop",
            "laptop.example.ts.net.",
            "100.64.0.66",
            true,
        ),
    ]);
    let matched = match_peers(
        &[managed("laptop", None, Some("laptop.example.ts.net"))],
        &peers,
    );
    assert!(matched.peers.is_empty());
    assert!(
        matched.problems[0].contains("ambiguous"),
        "{:?}",
        matched.problems
    );
}

#[test]
fn node_id_and_dns_name_must_agree() {
    let peers = status(&[peer(
        "n7",
        "laptop",
        "evil.example.ts.net.",
        "100.64.0.7",
        true,
    )]);
    let matched = match_peers(
        &[managed("laptop", Some("n7"), Some("laptop.example.ts.net"))],
        &peers,
    );
    assert!(matched.peers.is_empty());
    assert_eq!(matched.problems.len(), 1, "{:?}", matched.problems);
}

#[test]
fn one_node_for_two_hosts_fails_closed_for_both() {
    let peers = status(&[peer(
        "n7",
        "laptop",
        "laptop.example.ts.net.",
        "100.64.0.7",
        true,
    )]);
    let matched = match_peers(
        &[
            managed("laptop", Some("n7"), None),
            managed("desktop", None, Some("laptop.example.ts.net")),
        ],
        &peers,
    );
    assert!(matched.peers.is_empty(), "{:?}", matched.peers);
}

#[test]
fn the_server_itself_matches_through_self() {
    let me = r#""Self":{"ID":"nSELF","HostName":"manager","DNSName":"manager.example.ts.net.","OS":"linux","TailscaleIPs":["100.64.0.1"],"Online":true,"LastSeen":"0001-01-01T00:00:00Z"}"#;
    let impostor = peer(
        "nEVIL",
        "manager",
        "manager-1.example.ts.net.",
        "100.64.0.66",
        true,
    );
    let peers = parse_status(&format!(r#"{{{me},"Peer":{{{impostor}}}}}"#)).unwrap();
    for host in [
        managed("manager", Some("nSELF"), None),
        managed("manager", None, Some("manager.example.ts.net")),
    ] {
        let matched = match_peers(&[host], &peers);
        let manager = &matched.peers["manager"];
        assert_eq!(manager.addr, ip("100.64.0.1"));
        assert!(manager.online);
    }
}

#[test]
fn identities_are_required_valid_and_unique() {
    assert!(
        check_tailscale_identities(&[
            managed("a", Some("nA1"), None),
            managed("b", None, Some("b.example.ts.net")),
            managed("c", Some("nC"), Some("c.example.ts.net.")),
        ])
        .is_ok()
    );
    let missing = check_tailscale_identities(&[managed("a", None, None)]).unwrap_err();
    assert!(format!("{missing:#}").contains("a"), "{missing:#}");
    for bad in [
        vec![managed("a", Some(""), None)],
        vec![managed("a", Some("-n1"), None)],
        vec![managed("a", None, Some("laptop"))],
        vec![managed("a", None, Some("bad name.example.ts.net"))],
        vec![
            managed("a", Some("n1"), None),
            managed("b", Some("n1"), None),
        ],
        vec![
            managed("a", None, Some("x.example.ts.net")),
            managed("b", None, Some("X.example.ts.net.")),
        ],
    ] {
        assert!(check_tailscale_identities(&bad).is_err(), "{bad:?}");
    }
}

#[test]
fn tailscale_provider_refuses_hosts_without_identity() {
    let hosts = [managed("laptop", None, None)];
    let result = provider_from_config(
        &OnlineSource::Tailscale,
        std::path::Path::new("tailscale"),
        &hosts,
    );
    assert!(result.is_err());
    // Static addresses (tests/dev) stay keyed by name.
    let addresses = [("laptop".to_string(), "127.0.0.1".to_string())].into();
    assert!(
        provider_from_config(
            &OnlineSource::Static { addresses },
            std::path::Path::new("tailscale"),
            &hosts,
        )
        .is_ok()
    );
}

#[test]
fn hosts_file_carries_the_identity() {
    let hosts: Vec<ManagedHost> = serde_json::from_str(
        r#"[{"name":"a","system":"x86_64-linux","user":"alice","channel":"main","os":"linux",
            "node_id":"nA1","dns_name":"a.example.ts.net"},
           {"name":"b","system":"x86_64-linux","user":"alice","channel":"main","os":"linux"}]"#,
    )
    .unwrap();
    assert_eq!(hosts[0].node_id.as_deref(), Some("nA1"));
    assert_eq!(hosts[0].dns_name.as_deref(), Some("a.example.ts.net"));
    assert_eq!(
        (hosts[1].node_id.clone(), hosts[1].dns_name.clone()),
        (None, None)
    );
}

#[tokio::test]
async fn cli_provider_reports_managed_hosts_by_identity() {
    let dir = tempfile::tempdir().unwrap();
    let json = format!(
        r#"{{"Peer":{{{},{}}}}}"#,
        peer(
            "nREAL",
            "laptop",
            "laptop.example.ts.net.",
            "100.64.0.2",
            true
        ),
        peer("nEVIL", "laptop", "a.example.ts.net.", "100.64.0.66", true),
    );
    std::fs::write(dir.path().join("status.json"), json).unwrap();
    let bin = dir.path().join("tailscale");
    std::fs::write(
        &bin,
        format!(
            "#!/bin/sh\ncat {}\n",
            dir.path().join("status.json").display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
    let cli = TailscaleCli::new(bin, vec![managed("laptop", Some("nREAL"), None)]);
    let peers = cli.peers().await;
    assert_eq!(peers.len(), 1, "{peers:?}");
    assert_eq!(peers["laptop"].addr, ip("100.64.0.2"));
}

#[test]
fn host_names_differing_only_in_case_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("hosts.json");
    std::fs::write(
        &path,
        r#"[{"name":"laptop","system":"x86_64-linux","user":"alice","channel":"main","os":"linux","node_id":"n1"},
           {"name":"Laptop","system":"x86_64-linux","user":"alice","channel":"main","os":"linux","node_id":"n2"}]"#,
    )
    .unwrap();
    let error = lodestar_server::config::load_hosts(&path).unwrap_err();
    assert!(format!("{error:#}").contains("Laptop"), "{error:#}");
}

#[test]
fn hosts_with_the_same_name_get_no_node() {
    let peers = status(&[
        peer("n1", "laptop", "laptop.example.ts.net.", "100.64.0.2", true),
        peer("n2", "evil", "evil.example.ts.net.", "100.64.0.66", true),
    ]);
    let matched = match_peers(
        &[
            managed("laptop", Some("n1"), None),
            managed("Laptop", Some("n2"), None),
        ],
        &peers,
    );
    assert!(matched.peers.is_empty(), "{:?}", matched.peers);
}
