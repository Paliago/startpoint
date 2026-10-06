use rusqlite::{Connection, OptionalExtension, params};
use startpoint_sdk::{ActivityDay, ApiError, ApplicationPaths, Game, GameId, JournalEntry};
use std::{collections::BTreeMap, fs, time::Duration};

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
        if version > 2 {
            return Err(ApiError::Storage(
                "This library was created by a newer application. It has not been modified.".into(),
            ));
        }
        connection.execute_batch("PRAGMA journal_mode=WAL;
            PRAGMA foreign_keys=ON;
            BEGIN IMMEDIATE;
            CREATE TABLE IF NOT EXISTS games (id TEXT PRIMARY KEY, document TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS settings (name TEXT PRIMARY KEY, document TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS metadata_cache (id TEXT PRIMARY KEY, document TEXT NOT NULL, updated INTEGER NOT NULL);
            CREATE TABLE IF NOT EXISTS launch_events (id INTEGER PRIMARY KEY, game_id TEXT NOT NULL REFERENCES games(id) ON DELETE CASCADE, launched_at INTEGER NOT NULL);
            CREATE INDEX IF NOT EXISTS launch_events_timestamp ON launch_events(launched_at);
            CREATE TABLE IF NOT EXISTS journal_entries (id TEXT PRIMARY KEY, game_id TEXT NOT NULL REFERENCES games(id) ON DELETE CASCADE, created_at INTEGER NOT NULL, text TEXT NOT NULL);
            CREATE INDEX IF NOT EXISTS journal_entries_game ON journal_entries(game_id, created_at);
            PRAGMA user_version=2;
            COMMIT;").map_err(database_error)?;
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

    pub fn activity(&self) -> Result<Vec<ActivityDay>, ApiError> {
        read_activity(&self.connection)
    }

    pub fn record_launch(
        &mut self,
        game: &Game,
        timestamp: u64,
    ) -> Result<Vec<ActivityDay>, ApiError> {
        let timestamp = i64::try_from(timestamp).map_err(database_error)?;
        let document = serde_json::to_string(game).map_err(database_error)?;
        let transaction = self.connection.transaction().map_err(database_error)?;
        if transaction
            .execute(
                "UPDATE games SET document=?1 WHERE id=?2",
                params![document, game.id.0],
            )
            .map_err(database_error)?
            != 1
        {
            return Err(ApiError::NotFound);
        }
        transaction
            .execute(
                "INSERT INTO launch_events(game_id, launched_at) VALUES (?1, ?2)",
                params![game.id.0, timestamp],
            )
            .map_err(database_error)?;
        let activity = read_activity(&transaction)?;
        transaction.commit().map_err(database_error)?;
        Ok(activity)
    }

    pub fn journal(&self) -> Result<Vec<JournalEntry>, ApiError> {
        let mut statement = self.connection
            .prepare("SELECT id, game_id, created_at, text FROM journal_entries ORDER BY created_at DESC, id")
            .map_err(database_error)?;
        statement
            .query_map([], |row| {
                Ok(JournalEntry {
                    id: row.get(0)?,
                    game_id: GameId(row.get(1)?),
                    created_at: u64::try_from(row.get::<_, i64>(2)?).map_err(|error| {
                        rusqlite::Error::FromSqlConversionFailure(
                            2,
                            rusqlite::types::Type::Integer,
                            Box::new(error),
                        )
                    })?,
                    text: row.get(3)?,
                })
            })
            .map_err(database_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(database_error)
    }

    pub fn save_journal_entry(&self, entry: &JournalEntry, existing: bool) -> Result<(), ApiError> {
        let timestamp = i64::try_from(entry.created_at).map_err(database_error)?;
        if existing {
            if self
                .connection
                .execute(
                    "UPDATE journal_entries SET text=?1 WHERE id=?2 AND game_id=?3",
                    params![entry.text, entry.id, entry.game_id.0],
                )
                .map_err(database_error)?
                != 1
            {
                return Err(ApiError::NotFound);
            }
        } else {
            self.connection.execute("INSERT INTO journal_entries(id, game_id, created_at, text) VALUES (?1, ?2, ?3, ?4)", params![entry.id, entry.game_id.0, timestamp, entry.text]).map_err(database_error)?;
        }
        Ok(())
    }

    pub fn delete_journal_entry(&self, game_id: &GameId, entry_id: &str) -> Result<(), ApiError> {
        if self
            .connection
            .execute(
                "DELETE FROM journal_entries WHERE id=?1 AND game_id=?2",
                params![entry_id, game_id.0],
            )
            .map_err(database_error)?
            != 1
        {
            return Err(ApiError::NotFound);
        }
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

fn read_activity(connection: &Connection) -> Result<Vec<ActivityDay>, ApiError> {
    let mut statement = connection.prepare("SELECT date(launched_at, 'unixepoch', 'localtime'), game_id, count(*) FROM launch_events GROUP BY 1, 2 ORDER BY 1").map_err(database_error)?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                GameId(row.get(1)?),
                row.get::<_, u32>(2)?,
            ))
        })
        .map_err(database_error)?;
    let mut days = BTreeMap::<String, ActivityDay>::new();
    for row in rows {
        let (date, game_id, launches) = row.map_err(database_error)?;
        let day = days.entry(date.clone()).or_insert_with(|| ActivityDay {
            date,
            launches: 0,
            games: BTreeMap::new(),
        });
        day.launches += launches;
        day.games.insert(game_id, launches);
    }
    Ok(days.into_values().collect())
}
