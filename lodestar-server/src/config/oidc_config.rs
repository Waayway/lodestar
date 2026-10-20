use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// The OpenID Connect client the router entrance logs in with.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct OidcConfig {
    pub issuer: String,
    pub client_id: String,
    /// Read at each login; surrounding whitespace is trimmed.
    pub client_secret_file: PathBuf,
    pub reader_group: String,
    pub admin_group: String,
}
