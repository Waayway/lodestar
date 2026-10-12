use std::collections::HashMap;
use std::net::IpAddr;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use anyhow::Context;

use super::{Identity, OnlineProvider, PeerInfo, parse_status, parse_whois};

const STATUS_TTL: Duration = Duration::from_secs(30);
const WHOIS_TTL: Duration = Duration::from_secs(60);
const CLI_TIMEOUT: Duration = Duration::from_secs(10);

/// Shells out to the `tailscale` binary; status cached 30 s, whois 60 s.
pub struct TailscaleCli {
    bin: PathBuf,
    status: Mutex<Option<(Instant, HashMap<String, PeerInfo>)>>,
    whois: Mutex<HashMap<IpAddr, (Instant, Identity)>>,
}

impl TailscaleCli {
    pub fn new(bin: PathBuf) -> Self {
        Self {
            bin,
            status: Mutex::new(None),
            whois: Mutex::new(HashMap::new()),
        }
    }

    async fn run(&self, args: &[&str]) -> anyhow::Result<String> {
        let mut command = tokio::process::Command::new(&self.bin);
        command.args(args).kill_on_drop(true);
        let output = tokio::time::timeout(CLI_TIMEOUT, command.output())
            .await
            .context("tailscale timed out")?
            .context("running tailscale")?;
        anyhow::ensure!(
            output.status.success(),
            "tailscale exited with {}",
            output.status
        );
        Ok(String::from_utf8(output.stdout)?)
    }
}

#[async_trait::async_trait]
impl OnlineProvider for TailscaleCli {
    async fn peers(&self) -> HashMap<String, PeerInfo> {
        let cached = self.status.lock().unwrap().clone();
        if let Some((fetched_at, peers)) = &cached
            && fetched_at.elapsed() < STATUS_TTL
        {
            return peers.clone();
        }
        let fresh = match self
            .run(&["status", "--json"])
            .await
            .and_then(|json| parse_status(&json))
        {
            Ok(peers) => peers,
            Err(_) => return cached.map(|(_, peers)| peers).unwrap_or_default(),
        };
        *self.status.lock().unwrap() = Some((Instant::now(), fresh.clone()));
        fresh
    }

    async fn whois(&self, ip: IpAddr) -> Option<Identity> {
        if let Some((fetched_at, identity)) = self.whois.lock().unwrap().get(&ip)
            && fetched_at.elapsed() < WHOIS_TTL
        {
            return Some(identity.clone());
        }
        let ip_text = ip.to_string();
        let identity = self
            .run(&["whois", "--json", &ip_text])
            .await
            .and_then(|json| parse_whois(&json))
            .ok()?;
        let mut cache = self.whois.lock().unwrap();
        cache.retain(|_, (fetched_at, _)| fetched_at.elapsed() < WHOIS_TTL);
        cache.insert(ip, (Instant::now(), identity.clone()));
        Some(identity)
    }
}
