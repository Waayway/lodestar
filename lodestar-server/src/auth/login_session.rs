use lodestar_proto::Role;

/// A logged-in user as stored in the `sessions` table (the id is kept hashed).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoginSession {
    pub id_hash: String,
    pub user: String,
    pub role: Role,
    pub expires_at: i64,
}
