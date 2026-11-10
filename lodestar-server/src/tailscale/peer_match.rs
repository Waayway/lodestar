//! Binds each managed host to its tailnet node.

use std::collections::HashMap;

use super::PeerInfo;
use crate::config::{ManagedHost, normalize_dns_name};

/// The nodes of the managed hosts, keyed by lowercased managed host name, and why the others
/// have none.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PeerMatch {
    pub peers: HashMap<String, PeerInfo>,
    pub problems: Vec<String>,
}

/// Finds each host's node by its configured identity: the node with the host's `node_id`
/// (whose DNSName must then also be `dns_name`, if set), else the node with its `dns_name`.
/// The HostName a node reports is never used. A host whose identity matches no node, or more
/// than one, or a node another host also matched, gets no node (it shows as off and is never
/// contacted) and a problem line.
pub fn match_peers(hosts: &[ManagedHost], peers: &[PeerInfo]) -> PeerMatch {
    let mut out = PeerMatch::default();
    let mut matched: Vec<(String, &PeerInfo)> = Vec::new();
    for host in hosts {
        let key = host.name.to_lowercase();
        // `load_hosts` refuses these; never let one host's node stand in for another's.
        let same_name = hosts
            .iter()
            .filter(|other| other.name.to_lowercase() == key)
            .count();
        if same_name > 1 {
            out.problems.push(format!(
                "{}: {same_name} hosts with this name; ignoring all of them",
                host.name
            ));
            continue;
        }
        match find_peer(host, peers) {
            Ok(peer) => matched.push((key, peer)),
            Err(problem) => out.problems.push(format!("{}: {problem}", host.name)),
        }
    }
    for (index, (name, peer)) in matched.iter().enumerate() {
        let shared: Vec<&str> = matched
            .iter()
            .enumerate()
            .filter(|(other_index, (_, other_peer))| {
                *other_index != index && std::ptr::eq(*other_peer, *peer)
            })
            .map(|(_, (other, _))| other.as_str())
            .collect();
        if shared.is_empty() {
            out.peers.insert(name.clone(), (*peer).clone());
        } else {
            out.problems.push(format!(
                "{name}: node {} is also matched by {}; ignoring it",
                describe(peer),
                shared.join(", ")
            ));
        }
    }
    out
}

fn find_peer<'a>(host: &ManagedHost, peers: &'a [PeerInfo]) -> Result<&'a PeerInfo, String> {
    let dns_name = host.dns_name.as_deref().map(normalize_dns_name);
    let candidates: Vec<&PeerInfo> = match (&host.node_id, &dns_name) {
        (Some(node_id), _) => peers
            .iter()
            .filter(|peer| !peer.node_id.is_empty() && &peer.node_id == node_id)
            .collect(),
        (None, Some(dns_name)) => peers
            .iter()
            .filter(|peer| !peer.dns_name.is_empty() && &peer.dns_name == dns_name)
            .collect(),
        (None, None) => return Err("no Tailscale identity (node_id or dns_name) configured".into()),
    };
    let wanted = match (&host.node_id, &dns_name) {
        (Some(node_id), _) => format!("node ID {node_id}"),
        (None, Some(dns_name)) => format!("DNS name {dns_name}"),
        (None, None) => unreachable!(),
    };
    let peer = match candidates.as_slice() {
        [] => return Err(format!("no tailnet node with {wanted}")),
        [peer] => *peer,
        _ => {
            return Err(format!(
                "ambiguous: {} tailnet nodes with {wanted}; ignoring all of them",
                candidates.len()
            ));
        }
    };
    if let Some(dns_name) = &dns_name
        && &peer.dns_name != dns_name
    {
        return Err(format!(
            "node {} has DNS name {:?}, not {dns_name:?}; ignoring it",
            describe(peer),
            peer.dns_name
        ));
    }
    Ok(peer)
}

fn describe(peer: &PeerInfo) -> String {
    if peer.node_id.is_empty() {
        peer.dns_name.clone()
    } else {
        peer.node_id.clone()
    }
}
