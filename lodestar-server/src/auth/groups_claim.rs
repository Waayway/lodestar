use openidconnect::AdditionalClaims;
use serde::{Deserialize, Serialize};

/// The ID token's `groups` claim (Kanidm sends names like `admins@example.com` and UUIDs);
/// absent means no groups.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct GroupsClaim {
    #[serde(default)]
    pub groups: Vec<String>,
}

impl AdditionalClaims for GroupsClaim {}
