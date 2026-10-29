use lodestar_server::updates::version::{compare_versions, tag_to_version};
use std::cmp::Ordering::{Equal, Less};

#[test]
fn equal_after_suffix_cut() {
    assert_eq!(
        compare_versions("1.104.2-t7f4efe814-g4939ea2e4", "1.104.2"),
        Some(Equal)
    );
}
#[test]
fn numeric_not_lexical() {
    assert_eq!(compare_versions("1.98.10", "1.104.2"), Some(Less));
}
#[test]
fn missing_part_is_zero() {
    assert_eq!(compare_versions("1.104", "1.104.0"), Some(Equal));
}
#[test]
fn leading_v() {
    assert_eq!(compare_versions("v1.104.2", "1.104.2"), Some(Equal));
}
#[test]
fn plus_suffix() {
    assert_eq!(compare_versions("2.0.0+build5", "2.0.0"), Some(Equal));
}
#[test]
fn non_numeric_is_incomparable() {
    assert_eq!(compare_versions("1.104.x", "1.104.2"), None);
    assert_eq!(compare_versions("", "1"), None);
}
#[test]
fn tag() {
    assert_eq!(tag_to_version("v1.104.2"), "1.104.2");
    assert_eq!(tag_to_version("1.2"), "1.2");
}
