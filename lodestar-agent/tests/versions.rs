use std::path::Path;

use lodestar_agent::versions::read_versions;

fn read(content: Option<&str>) -> std::collections::BTreeMap<String, String> {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("versions.json");
    if let Some(content) = content {
        std::fs::write(&path, content).unwrap();
    }
    read_versions(&path)
}

#[test]
fn reads_a_map_of_strings() {
    let versions = read(Some(r#"{"tailscale":"1.104.2"}"#));
    assert_eq!(versions.len(), 1);
    assert_eq!(versions["tailscale"], "1.104.2");
}

#[test]
fn missing_file_is_empty() {
    assert!(read(None).is_empty());
    assert!(read_versions(Path::new("/nonexistent/versions.json")).is_empty());
}

#[test]
fn anything_but_an_object_of_strings_is_empty() {
    for content in ["[]", r#"{"a":1}"#, r#"{"a":"1","b":2}"#, "not json", ""] {
        assert!(read(Some(content)).is_empty(), "{content:?}");
    }
}
