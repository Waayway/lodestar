use std::path::Path;

use anyhow::Context;
use lodestar_migration::{Migrator, MigratorTrait};
use sea_orm::{Database, DatabaseConnection};

/// Opens (creating it when missing) `<state_dir>/lodestar.db` and runs the migrations.
pub async fn connect(state_dir: &Path) -> anyhow::Result<DatabaseConnection> {
    std::fs::create_dir_all(state_dir)
        .with_context(|| format!("creating state dir {}", state_dir.display()))?;
    let url = format!(
        "sqlite://{}?mode=rwc",
        state_dir.join("lodestar.db").display()
    );
    let connection = Database::connect(&url).await.context("opening database")?;
    Migrator::up(&connection, None)
        .await
        .context("running migrations")?;
    Ok(connection)
}
