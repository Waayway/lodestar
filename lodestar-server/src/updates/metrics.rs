//! Prometheus text rendering of the tracked software: what runs where, what is latest
//! upstream, and whether each host is up to date.

use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::fmt::Write;

use super::upstream::LatestVersion;
use super::version::compare_versions;

/// What the metrics are rendered from.
pub struct MetricsInput {
    /// Host -> software -> running version, for every host and every reported software.
    pub running: BTreeMap<String, BTreeMap<String, String>>,
    /// Software -> latest upstream release; only configured software.
    pub latest: BTreeMap<String, LatestVersion>,
}

/// Escapes a label value: `\`, `"` and newline.
fn escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            c => out.push(c),
        }
    }
    out
}

fn family(out: &mut String, name: &str, help: &str, mut lines: Vec<(String, String)>) {
    lines.sort();
    let _ = writeln!(out, "# HELP {name} {help}");
    let _ = writeln!(out, "# TYPE {name} gauge");
    for (labels, value) in lines {
        let _ = writeln!(out, "{name}{{{labels}}} {value}");
    }
}

/// The metrics in Prometheus text format (version 0.0.4): families in a fixed order, lines
/// sorted by labels.
pub fn render(input: &MetricsInput) -> String {
    let mut running_info = Vec::new();
    let mut up_to_date = Vec::new();
    for (host, versions) in &input.running {
        for (software, version) in versions {
            let host = escape(host);
            let software_label = escape(software);
            running_info.push((
                format!(
                    r#"host="{host}",software="{software_label}",version="{}""#,
                    escape(version)
                ),
                "1".to_string(),
            ));
            let known = input
                .latest
                .get(software)
                .and_then(|latest| latest.version.as_deref());
            if let Some(latest) = known {
                let value = match compare_versions(version, latest) {
                    Some(Ordering::Less) => Some("0"),
                    Some(_) => Some("1"),
                    None => None,
                };
                if let Some(value) = value {
                    up_to_date.push((
                        format!(r#"host="{host}",software="{software_label}""#),
                        value.to_string(),
                    ));
                }
            }
        }
    }
    let mut latest_info = Vec::new();
    let mut checked = Vec::new();
    for (software, latest) in &input.latest {
        let software = escape(software);
        if let Some(version) = &latest.version {
            latest_info.push((
                format!(r#"software="{software}",version="{}""#, escape(version)),
                "1".to_string(),
            ));
        }
        if let Some(at) = latest.checked_at {
            checked.push((format!(r#"software="{software}""#), at.to_string()));
        }
    }
    let mut out = String::new();
    family(
        &mut out,
        "lodestar_software_running_info",
        "Version of tracked software running on a host.",
        running_info,
    );
    family(
        &mut out,
        "lodestar_software_latest_info",
        "Latest upstream release of tracked software.",
        latest_info,
    );
    family(
        &mut out,
        "lodestar_software_up_to_date",
        "1 when a host runs the latest release or newer, 0 when older; absent when unknown.",
        up_to_date,
    );
    family(
        &mut out,
        "lodestar_software_latest_checked_timestamp_seconds",
        "Unix time of the last successful upstream check.",
        checked,
    );
    out
}
