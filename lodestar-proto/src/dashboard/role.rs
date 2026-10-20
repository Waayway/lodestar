use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Role {
    Anonymous,
    Reader,
    Admin,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn role_is_snake_case() {
        assert_eq!(
            serde_json::to_string(&Role::Anonymous).unwrap(),
            r#""anonymous""#
        );
    }
}
