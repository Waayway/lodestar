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
        proxy.parse_public_url()?;
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
    fn public_url_must_be_https() {
        let mut config = config("");
        config.proxy.as_mut().unwrap().public_url = "ftp://x".into();
        assert!(config.check_proxy().is_err());
        // The cookies are `__Host-…; Secure`: a browser never keeps them over plain http.
        config.proxy.as_mut().unwrap().public_url = "http://lodestar.example.com".into();
        let error = config.check_proxy().unwrap_err().to_string();
        assert!(error.contains("https://"), "{error}");
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
    }

    #[test]
    fn public_origin_is_normalised_like_a_browser_origin() {
        for (url, origin) in [
            (
                "HTTPS://Lodestar.Example.com",
                "https://lodestar.example.com",
            ),
            (
                "https://lodestar.example.com:443",
                "https://lodestar.example.com",
            ),
            (
                "https://lodestar.example.com:8443/",
                "https://lodestar.example.com:8443",
            ),
        ] {
            config_ok(&config_with(url));
            assert_eq!(
                config_with(url).proxy.unwrap().public_origin(),
                origin,
                "{url}"
            );
        }
    }

    #[test]
    fn malformed_or_decorated_public_urls_refused() {
        for url in [
            "https://user:pw@lodestar.example.com/",
            "https://user@lodestar.example.com",
            "https://lodestar.example.com/sub",
            "https://lodestar.example.com/?x=1",
            "https://lodestar.example.com/#top",
            "https://lodestar.example.com?",
            "https:lodestar.example.com",
            "https:/lodestar.example.com",
            "https://",
            "mailto:alice@example.com",
            "http://lodestar.example.com",
            "http://lodestar.example.com:8080",
            "",
        ] {
            let config = config_with(url);
            assert!(config.check_proxy().is_err(), "{url:?} must be refused");
            assert_eq!(config.proxy.unwrap().public_origin(), "", "{url:?}");
        }
    }

    fn config_with(public_url: &str) -> ServerConfig {
        let mut config = config("");
        config.proxy.as_mut().unwrap().public_url = public_url.into();
        config
    }

    fn config_ok(config: &ServerConfig) {
        config.check_proxy().unwrap();
    }
}
