use std::path::Path;

use anyhow::Context;
use serde::{Deserialize, Serialize};

/// One service link, as listed in `links.json` (written by the Nix module).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Link {
    pub id: String,
    pub title: String,
    pub url: String,
    pub group: String,
    pub description: Option<String>,
    pub icon: Option<String>,
    pub host: String,
    pub check: LinkCheck,
}

/// How a link is checked over HTTP.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct LinkCheck {
    pub enable: bool,
    pub url: Option<String>,
    pub expect_status: Vec<u16>,
    pub insecure_tls: bool,
}

pub fn load_links(path: &Path) -> anyhow::Result<Vec<Link>> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("reading links file {}", path.display()))?;
    serde_json::from_str(&text).with_context(|| format!("parsing links file {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_file(name: &str, content: &str) -> (std::path::PathBuf, std::path::PathBuf) {
        let dir =
            std::env::temp_dir().join(format!("lodestar-links-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("links.json");
        std::fs::write(&path, content).unwrap();
        (dir, path)
    }

    #[test]
    fn links_load() {
        let (dir, path) = temp_file(
            "load",
            r#"[{"id":"grafana","title":"Grafana","url":"https://g.example","group":"Monitoring",
            "description":null,"icon":"chart","host":"nixos-manager",
            "check":{"enable":true,"url":null,"expect_status":[200,302],"insecure_tls":false}}]"#,
        );
        let links = load_links(&path).unwrap();
        assert_eq!(
            links,
            vec![Link {
                id: "grafana".into(),
                title: "Grafana".into(),
                url: "https://g.example".into(),
                group: "Monitoring".into(),
                description: None,
                icon: Some("chart".into()),
                host: "nixos-manager".into(),
                check: LinkCheck {
                    enable: true,
                    url: None,
                    expect_status: vec![200, 302],
                    insecure_tls: false,
                },
            }]
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn links_parse_error_names_file() {
        let (dir, path) = temp_file("bad", "not json");
        let error = format!("{:#}", load_links(&path).unwrap_err());
        assert!(error.contains(&path.display().to_string()), "{error}");
        std::fs::remove_dir_all(&dir).ok();
    }
}
