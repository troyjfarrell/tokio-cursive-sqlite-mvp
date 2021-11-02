use std::num::NonZeroUsize;

use lazy_static::lazy_static;
use rusqlite_migration::{Migrations, SchemaVersion as SqliteSchemaVersion, M};

// Test migrations
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_test() {
        assert!(MIGRATIONS.validate().is_ok());
    }
}

lazy_static! {
    pub static ref MIGRATIONS: Migrations<'static> = Migrations::new(vec![
        M::up(include_str!("sql/0001-up.sql")).down(include_str!("sql/0001-down.sql")),
        M::up(include_str!("sql/0002-up.sql")).down(include_str!("sql/0002-down.sql")),
    ]);
}

pub const MAX_VERSION: usize = 2;

#[derive(Debug)]
pub enum SchemaVersion {
    NoneSet,
    Expected(usize),
    Unexpected(usize),
}

#[derive(Debug)]
pub struct SchemaVersionInfo {
    pub current_version: SchemaVersion,
    pub max_version: NonZeroUsize,
}

impl SchemaVersionInfo {
    pub fn from_sqlite_schema_version(current_version: SqliteSchemaVersion) -> Self {
        let version = match current_version {
            SqliteSchemaVersion::NoneSet => SchemaVersion::NoneSet,
            SqliteSchemaVersion::Inside(v) => SchemaVersion::Expected(v.get()),
            SqliteSchemaVersion::Outside(v) => SchemaVersion::Unexpected(v.get()),
        };
        Self {
            current_version: version,
            max_version: NonZeroUsize::new(MAX_VERSION).unwrap(),
        }
    }
}
