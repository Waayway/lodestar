/// Logs what the manager host resolves to: the only sources the agent accepts.
pub(super) async fn log_manager_addrs(host: &str) {
    match tokio::net::lookup_host((host, 0)).await {
        Ok(addresses) => {
            let mut ips: Vec<String> = addresses
                .map(|address| address.ip().to_canonical().to_string())
                .collect();
            ips.sort();
            ips.dedup();
            eprintln!(
                "lodestar-agent: manager {host} resolves to: {}",
                if ips.is_empty() {
                    "(nothing)".to_string()
                } else {
                    ips.join(", ")
                }
            );
        }
        Err(error) => eprintln!("lodestar-agent: manager {host} does not resolve yet: {error}"),
    }
}
