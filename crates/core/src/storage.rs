use rusqlite::{Connection, OptionalExtension, params};
use startpoint_sdk::{ApiError, ApplicationPaths, Game};
use std::{fs, time::Duration};

pub(crate) struct Store {
    connection: Connection,
}

fn database_error(error: impl std::fmt::Display) -> ApiError {
    ApiError::Storage(error.to_string())
}

impl Store {
    pub fn open(paths: &ApplicationPaths) -> Result<Self, ApiError> {
        fs::create_dir_all(&paths.data_directory).map_err(database_error)?;
        fs::create_dir_all(&paths.cache_directory).map_err(database_error)?;
        let connection = Connection::open(paths.data_directory.join("library.sqlite3"))
            .map_err(database_error)?;
        connection
            .busy_timeout(Duration::from_secs(3))
            .map_err(database_error)?;
        let version: u32 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .map_err(database_error)?;
        if version > 1 {
            return Err(ApiError::Storage(
                "This library was created by a newer application. It has not been modified.".into(),
            ));
        }
        connection.execute_batch("PRAGMA journal_mode=WAL;
            CREATE TABLE IF NOT EXISTS games (id TEXT PRIMARY KEY, document TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS settings (name TEXT PRIMARY KEY, document TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS metadata_cache (id TEXT PRIMARY KEY, document TEXT NOT NULL, updated INTEGER NOT NULL);
            PRAGMA user_version=1;").map_err(database_error)?;
        Ok(Self { connection })
    }

    pub fn games(&self) -> Result<Vec<Game>, ApiError> {
        let mut statement = self
            .connection
            .prepare("SELECT document FROM games")
            .map_err(database_error)?;
        let rows = statement
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(database_error)?;
        rows.map(|row| {
            let document = row.map_err(database_error)?;
            serde_json::from_str(&document).map_err(database_error)
        })
        .collect()
    }

    pub fn save_games(&mut self, games: &[Game]) -> Result<(), ApiError> {
        let transaction = self.connection.transaction().map_err(database_error)?;
        for game in games {
            let document = serde_json::to_string(game).map_err(database_error)?;
            transaction
                .execute(
                    "INSERT INTO games(id, document) VALUES (?1, ?2)
                ON CONFLICT(id) DO UPDATE SET document=excluded.document",
                    params![game.id.0, document],
                )
                .map_err(database_error)?;
        }
        transaction.commit().map_err(database_error)
    }

    pub fn remove_game(&self, id: &str) -> Result<(), ApiError> {
        self.connection
            .execute("DELETE FROM games WHERE id=?1", [id])
            .map_err(database_error)?;
        Ok(())
    }

    pub fn setting<T: serde::de::DeserializeOwned>(
        &self,
        name: &str,
    ) -> Result<Option<T>, ApiError> {
        let value: Option<String> = self
            .connection
            .query_row(
                "SELECT document FROM settings WHERE name=?1",
                [name],
                |row| row.get(0),
            )
            .optional()
            .map_err(database_error)?;
        value
            .map(|value| serde_json::from_str(&value).map_err(database_error))
            .transpose()
    }

    pub fn save_setting<T: serde::Serialize>(&self, name: &str, value: &T) -> Result<(), ApiError> {
        let document = serde_json::to_string(value).map_err(database_error)?;
        self.connection
            .execute(
                "INSERT INTO settings(name, document) VALUES (?1, ?2)
            ON CONFLICT(name) DO UPDATE SET document=excluded.document",
                params![name, document],
            )
            .map_err(database_error)?;
        Ok(())
    }

    pub fn cached<T: serde::de::DeserializeOwned>(
        &self,
        id: &str,
        now: u64,
    ) -> Result<Option<T>, ApiError> {
        let minimum_updated = i64::try_from(now.saturating_sub(86_400)).map_err(database_error)?;
        let document: Option<String> = self
            .connection
            .query_row(
                "SELECT document FROM metadata_cache WHERE id=?1 AND updated>=?2",
                params![id, minimum_updated],
                |row| row.get(0),
            )
            .optional()
            .map_err(database_error)?;
        document
            .map(|value| serde_json::from_str(&value).map_err(database_error))
            .transpose()
    }

    pub fn save_cache<T: serde::Serialize>(
        &self,
        id: &str,
        value: &T,
        now: u64,
    ) -> Result<(), ApiError> {
        let updated = i64::try_from(now).map_err(database_error)?;
        let document = serde_json::to_string(value).map_err(database_error)?;
        self.connection
            .execute(
                "INSERT INTO metadata_cache(id, document, updated) VALUES (?1, ?2, ?3)
            ON CONFLICT(id) DO UPDATE SET document=excluded.document, updated=excluded.updated",
                params![id, document, updated],
            )
            .map_err(database_error)?;
        Ok(())
    }
}
