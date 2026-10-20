pub use sea_orm_migration::prelude::*;

mod m20261012_000001_create_tables;
mod m20261020_000001_create_sessions;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m20261012_000001_create_tables::Migration),
            Box::new(m20261020_000001_create_sessions::Migration),
        ]
    }
}
