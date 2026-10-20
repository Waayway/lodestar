use super::ServerConfig;

impl ServerConfig {
    /// Refuses a router entrance that could not work or would expose the dashboard.
    pub fn check_proxy(&self) -> anyhow::Result<()> {
        let Some(proxy) = &self.proxy else {
            return Ok(());
        };
        if proxy.trusted_proxies.is_empty() {
            anyhow::bail!(
                "proxy.trusted_proxies is empty; the router entrance would refuse every request"
            );
        }
        let public_url = reqwest::Url::parse(&proxy.public_url).map_err(|error| {
            anyhow::anyhow!(
                "proxy.public_url {:?} is not a URL: {error}",
                proxy.public_url
            )
        })?;
        if !matches!(public_url.scheme(), "http" | "https") || public_url.host().is_none() {
            anyhow::bail!(
                "proxy.public_url {:?} must be an http or https URL with a host",
                proxy.public_url
            );
        }
        if proxy.listen == self.listen {
            anyhow::bail!(
                "proxy.listen {} is the same as listen; the router entrance needs its own address",
                proxy.listen
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(proxy_overrides: &str) -> ServerConfig {
        serde_json::from_str(&format!(
            r#"{{"listen":"100.100.1.2:8080","agent_port":7461,"hosts_file":"/h.json",
            "repo_url":"u","push_url":"p","state_dir":"/s",
            "access":{{"kind":"disabled"}},"online":{{"kind":"tailscale"}},
            "proxy":{{"listen":"10.0.10.170:8081","trusted_proxies":["10.0.10.110"],
            "public_url":"https://lodestar.example.com",
            "oidc":{{"issuer":"https://id.example.com/oauth2/openid/lodestar",
            "client_id":"lodestar","client_secret_file":"/secret",
            "reader_group":"lodestar-users@example.com","admin_group":"admins@example.com"}}
            {proxy_overrides}}}}}"#
        ))
        .unwrap()
    }

    #[test]
    fn valid_and_absent_proxy_pass() {
        config("").check_proxy().unwrap();
        let mut absent = config("");
        absent.proxy = None;
        absent.check_proxy().unwrap();
    }

    #[test]
    fn empty_trusted_proxies_refused() {
        let mut config = config("");
        config.proxy.as_mut().unwrap().trusted_proxies.clear();
        let error = config.check_proxy().unwrap_err().to_string();
        assert!(error.contains("trusted_proxies"), "{error}");
    }

    #[test]
    fn public_url_must_be_http_or_https() {
        let mut config = config("");
        config.proxy.as_mut().unwrap().public_url = "ftp://x".into();
        assert!(config.check_proxy().is_err());
        config.proxy.as_mut().unwrap().public_url = "not a url".into();
        assert!(config.check_proxy().is_err());
        config.proxy.as_mut().unwrap().public_url = "https://lodestar.example.com/".into();
        config.check_proxy().unwrap();
    }

    #[test]
    fn proxy_listen_differs_from_listen() {
        let mut config = config("");
        config.proxy.as_mut().unwrap().listen = config.listen;
        let error = config.check_proxy().unwrap_err().to_string();
        assert!(error.contains("proxy.listen"), "{error}");
    }

    #[test]
    fn public_origin_strips_path_and_slash() {
        let mut config = config("");
        let proxy = config.proxy.as_mut().unwrap();
        proxy.public_url = "https://lodestar.example.com/".into();
        assert_eq!(proxy.public_origin(), "https://lodestar.example.com");
        proxy.public_url = "http://lodestar.example.com:8443/a/b?x=1".into();
        assert_eq!(proxy.public_origin(), "http://lodestar.example.com:8443");
    }
}
