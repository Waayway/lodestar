/// Who is behind a tailnet address, as `tailscale whois` reports it.
#[derive(Clone, Debug, PartialEq)]
pub struct Identity {
    pub login: Option<String>,
    pub tags: Vec<String>,
    /// Short host name of the node the request came from.
    pub node: String,
}
