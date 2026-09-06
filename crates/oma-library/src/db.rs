// SPDX-License-Identifier: GPL-3.0-or-later
//! SQLite schema + CRUD. WAL mode, short transactions, `UNIQUE(path)`.

use oma_core::{Error, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::query::{Filter, SortOrder};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct App {
    pub id: i64,
    pub path: String,
    pub name: String,
    pub comment: String,
    pub icon: String,
    pub desktop: String,
    pub update_info: String,
    pub categories: Vec<String>,
    pub favorite: bool,
    pub hidden: bool,
    pub play_count: i64,
    pub last_played: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewApp {
    pub path: String,
    pub name: String,
    pub comment: String,
    pub icon: String,
    pub desktop: String,
    pub update_info: String,
    pub categories: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Collection {
    pub id: i64,
    pub name: String,
}

pub struct Library {
    conn: Connection,
}

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS apps (
    id INTEGER PRIMARY KEY,
    path TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL DEFAULT '',
    comment TEXT NOT NULL DEFAULT '',
    icon TEXT NOT NULL DEFAULT '',
    desktop TEXT NOT NULL DEFAULT '',
    update_info TEXT NOT NULL DEFAULT '',
    categories TEXT NOT NULL DEFAULT '',
    favorite INTEGER NOT NULL DEFAULT 0,
    hidden INTEGER NOT NULL DEFAULT 0,
    play_count INTEGER NOT NULL DEFAULT 0,
    last_played INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_apps_name ON apps(name);
CREATE TABLE IF NOT EXISTS collections (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL UNIQUE
);
CREATE TABLE IF NOT EXISTS app_collections (
    app_id INTEGER NOT NULL REFERENCES apps(id) ON DELETE CASCADE,
    collection_id INTEGER NOT NULL REFERENCES collections(id) ON DELETE CASCADE,
    PRIMARY KEY (app_id, collection_id)
);
CREATE TABLE IF NOT EXISTS tags (
    app_id INTEGER NOT NULL REFERENCES apps(id) ON DELETE CASCADE,
    tag TEXT NOT NULL,
    PRIMARY KEY (app_id, tag)
);
";

fn split_csv(s: &str) -> Vec<String> {
    s.split(';')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

fn join_csv(v: &[String]) -> String {
    v.join(";")
}

impl Library {
    pub fn open(path: &std::path::Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| Error::Io(parent.to_path_buf(), e))?;
        }
        let conn = Connection::open(path).map_err(|e| Error::Config(format!("sqlite: {e}")))?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")
            .map_err(|e| Error::Config(format!("sqlite: {e}")))?;
        conn.execute_batch(SCHEMA)
            .map_err(|e| Error::Config(format!("sqlite: {e}")))?;
        Ok(Self { conn })
    }

    pub fn open_memory() -> Result<Self> {
        let conn =
            Connection::open_in_memory().map_err(|e| Error::Config(format!("sqlite: {e}")))?;
        conn.execute_batch(SCHEMA)
            .map_err(|e| Error::Config(format!("sqlite: {e}")))?;
        Ok(Self { conn })
    }

    pub fn upsert(&self, app: &NewApp) -> Result<i64> {
        self.conn
            .execute(
                "INSERT INTO apps (path, name, comment, icon, desktop, update_info, categories)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(path) DO UPDATE SET
                   name=excluded.name, comment=excluded.comment, icon=excluded.icon,
                   desktop=excluded.desktop, update_info=excluded.update_info,
                   categories=excluded.categories",
                params![
                    app.path,
                    app.name,
                    app.comment,
                    app.icon,
                    app.desktop,
                    app.update_info,
                    join_csv(&app.categories)
                ],
            )
            .map_err(|e| Error::Config(format!("sqlite: {e}")))?;
        self.conn
            .query_row("SELECT id FROM apps WHERE path = ?1", [&app.path], |r| {
                r.get(0)
            })
            .map_err(|e| Error::Config(format!("sqlite: {e}")))
    }

    pub fn get(&self, id: i64) -> Result<Option<App>> {
        self.conn
            .query_row(
                "SELECT id, path, name, comment, icon, desktop, update_info, categories,
                        favorite, hidden, play_count, last_played FROM apps WHERE id = ?1",
                [id],
                row_to_app,
            )
            .optional()
            .map_err(|e| Error::Config(format!("sqlite: {e}")))
    }

    pub fn get_by_path(&self, path: &str) -> Result<Option<App>> {
        self.conn
            .query_row(
                "SELECT id, path, name, comment, icon, desktop, update_info, categories,
                        favorite, hidden, play_count, last_played FROM apps WHERE path = ?1",
                [path],
                row_to_app,
            )
            .optional()
            .map_err(|e| Error::Config(format!("sqlite: {e}")))
    }

    pub fn remove_by_path(&self, path: &str) -> Result<bool> {
        let n = self
            .conn
            .execute("DELETE FROM apps WHERE path = ?1", [path])
            .map_err(|e| Error::Config(format!("sqlite: {e}")))?;
        Ok(n > 0)
    }

    pub fn set_favorite(&self, id: i64, favorite: bool) -> Result<()> {
        self.conn
            .execute(
                "UPDATE apps SET favorite = ?1 WHERE id = ?2",
                params![favorite, id],
            )
            .map(|_| ())
            .map_err(|e| Error::Config(format!("sqlite: {e}")))
    }

    pub fn set_hidden(&self, id: i64, hidden: bool) -> Result<()> {
        self.conn
            .execute(
                "UPDATE apps SET hidden = ?1 WHERE id = ?2",
                params![hidden, id],
            )
            .map(|_| ())
            .map_err(|e| Error::Config(format!("sqlite: {e}")))
    }

    pub fn record_play(&self, id: i64, now_secs: i64) -> Result<()> {
        self.conn
            .execute(
                "UPDATE apps SET play_count = play_count + 1, last_played = ?1 WHERE id = ?2",
                params![now_secs, id],
            )
            .map(|_| ())
            .map_err(|e| Error::Config(format!("sqlite: {e}")))
    }

    pub fn tags_of(&self, id: i64) -> Result<Vec<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT tag FROM tags WHERE app_id = ?1 ORDER BY tag")
            .map_err(|e| Error::Config(format!("sqlite: {e}")))?;
        let tags = stmt
            .query_map([id], |r| r.get(0))
            .map_err(|e| Error::Config(format!("sqlite: {e}")))?
            .collect::<std::result::Result<Vec<String>, _>>()
            .map_err(|e| Error::Config(format!("sqlite: {e}")))?;
        Ok(tags)
    }

    pub fn set_tags(&self, id: i64, tags: &[String]) -> Result<()> {
        self.conn
            .execute("DELETE FROM tags WHERE app_id = ?1", [id])
            .map_err(|e| Error::Config(format!("sqlite: {e}")))?;
        for tag in tags {
            let tag = tag.trim();
            if tag.is_empty() {
                continue;
            }
            self.conn
                .execute(
                    "INSERT OR IGNORE INTO tags (app_id, tag) VALUES (?1, ?2)",
                    params![id, tag],
                )
                .map_err(|e| Error::Config(format!("sqlite: {e}")))?;
        }
        Ok(())
    }

    pub fn create_collection(&self, name: &str) -> Result<i64> {
        self.conn
            .execute(
                "INSERT OR IGNORE INTO collections (name) VALUES (?1)",
                [name],
            )
            .map_err(|e| Error::Config(format!("sqlite: {e}")))?;
        self.conn
            .query_row("SELECT id FROM collections WHERE name = ?1", [name], |r| {
                r.get(0)
            })
            .map_err(|e| Error::Config(format!("sqlite: {e}")))
    }

    pub fn delete_collection(&self, id: i64) -> Result<()> {
        self.conn
            .execute("DELETE FROM collections WHERE id = ?1", [id])
            .map(|_| ())
            .map_err(|e| Error::Config(format!("sqlite: {e}")))
    }

    pub fn collections(&self) -> Result<Vec<Collection>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, name FROM collections ORDER BY name")
            .map_err(|e| Error::Config(format!("sqlite: {e}")))?;
        let rows = stmt
            .query_map([], |r| {
                Ok(Collection {
                    id: r.get(0)?,
                    name: r.get(1)?,
                })
            })
            .map_err(|e| Error::Config(format!("sqlite: {e}")))?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| Error::Config(format!("sqlite: {e}")))?;
        Ok(rows)
    }

    pub fn add_to_collection(&self, app_id: i64, collection_id: i64) -> Result<()> {
        self.conn
            .execute(
                "INSERT OR IGNORE INTO app_collections (app_id, collection_id) VALUES (?1, ?2)",
                params![app_id, collection_id],
            )
            .map(|_| ())
            .map_err(|e| Error::Config(format!("sqlite: {e}")))
    }

    pub fn remove_from_collection(&self, app_id: i64, collection_id: i64) -> Result<()> {
        self.conn
            .execute(
                "DELETE FROM app_collections WHERE app_id = ?1 AND collection_id = ?2",
                params![app_id, collection_id],
            )
            .map(|_| ())
            .map_err(|e| Error::Config(format!("sqlite: {e}")))
    }

    pub fn members(&self, collection_id: i64) -> Result<Vec<App>> {
        let mut stmt = self.conn.prepare(
            "SELECT a.id, a.path, a.name, a.comment, a.icon, a.desktop, a.update_info, a.categories,
                    a.favorite, a.hidden, a.play_count, a.last_played
             FROM apps a JOIN app_collections m ON m.app_id = a.id
             WHERE m.collection_id = ?1 ORDER BY a.name",
        ).map_err(|e| Error::Config(format!("sqlite: {e}")))?;
        let rows = stmt
            .query_map([collection_id], row_to_app)
            .map_err(|e| Error::Config(format!("sqlite: {e}")))?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| Error::Config(format!("sqlite: {e}")))?;
        Ok(rows)
    }

    /// Query apps with filter + sort. Text matches name/comment/path
    /// (case-insensitive substring, quote-escaped); fuzzy ranking stays in
    /// the UI model.
    pub fn query(&self, filter: &Filter) -> Result<Vec<App>> {
        let mut sql = String::from(
            "SELECT a.id, a.path, a.name, a.comment, a.icon, a.desktop, a.update_info, a.categories,
                    a.favorite, a.hidden, a.play_count, a.last_played FROM apps a",
        );
        let mut clauses: Vec<String> = Vec::new();
        if filter.collection.is_some() {
            sql.push_str(" JOIN app_collections m ON m.app_id = a.id");
        }
        if filter.tag.is_some() {
            sql.push_str(" JOIN tags t ON t.app_id = a.id");
        }
        if !filter.show_hidden {
            clauses.push("a.hidden = 0".to_string());
        }
        if filter.favorites_only {
            clauses.push("a.favorite = 1".to_string());
        }
        if filter.updates_only {
            clauses.push("a.update_info <> ''".to_string());
        }
        if let Some(collection) = filter.collection {
            clauses.push(format!("m.collection_id = {collection}"));
        }
        if let Some(tag) = &filter.tag {
            clauses.push(format!("t.tag = '{}'", tag.replace('\'', "''")));
        }
        if !filter.text.trim().is_empty() {
            let escaped = filter.text.replace('\'', "''").to_lowercase();
            clauses.push(format!(
                "(LOWER(a.name) LIKE '%{escaped}%' OR LOWER(a.comment) LIKE '%{escaped}%' OR LOWER(a.path) LIKE '%{escaped}%')"
            ));
        }
        if !clauses.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&clauses.join(" AND "));
        }
        sql.push_str(match filter.sort {
            SortOrder::UpdateFirst => " ORDER BY (a.update_info <> '') DESC, LOWER(a.name)",
            SortOrder::Name => " ORDER BY LOWER(a.name)",
            SortOrder::Recent => " ORDER BY a.last_played DESC, LOWER(a.name)",
            SortOrder::Played => " ORDER BY a.play_count DESC, LOWER(a.name)",
        });
        let mut stmt = self
            .conn
            .prepare(&sql)
            .map_err(|e| Error::Config(format!("sqlite: {e}")))?;
        let rows = stmt
            .query_map([], row_to_app)
            .map_err(|e| Error::Config(format!("sqlite: {e}")))?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| Error::Config(format!("sqlite: {e}")))?;
        Ok(rows)
    }
}

fn row_to_app(row: &rusqlite::Row<'_>) -> rusqlite::Result<App> {
    let categories: String = row.get(7)?;
    Ok(App {
        id: row.get(0)?,
        path: row.get(1)?,
        name: row.get(2)?,
        comment: row.get(3)?,
        icon: row.get(4)?,
        desktop: row.get(5)?,
        update_info: row.get(6)?,
        categories: split_csv(&categories),
        favorite: row.get::<_, i64>(8)? != 0,
        hidden: row.get::<_, i64>(9)? != 0,
        play_count: row.get(10)?,
        last_played: row.get(11)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::query::Filter;

    fn app(path: &str, name: &str) -> NewApp {
        NewApp {
            path: path.to_string(),
            name: name.to_string(),
            comment: String::new(),
            icon: String::new(),
            desktop: String::new(),
            update_info: String::new(),
            categories: vec!["Utility".to_string()],
        }
    }

    #[test]
    fn upsert_round_trip_and_unique_path() {
        let db = Library::open_memory().expect("db");
        let id = db.upsert(&app("/a/App.AppImage", "First")).expect("insert");
        let again = db
            .upsert(&app("/a/App.AppImage", "Renamed"))
            .expect("upsert");
        assert_eq!(id, again, "same path keeps row");
        assert_eq!(db.get(id).expect("get").expect("row").name, "Renamed");
    }

    #[test]
    fn favorite_hide_play_and_filters() {
        let db = Library::open_memory().expect("db");
        let a = db.upsert(&app("/a/A.AppImage", "Alpha")).expect("a");
        let b = db.upsert(&app("/a/B.AppImage", "Beta")).expect("b");
        db.set_favorite(a, true).expect("fav");
        db.set_hidden(b, true).expect("hide");
        db.record_play(a, 1000).expect("play");
        db.record_play(a, 2000).expect("play");
        let row = db.get(a).expect("get").expect("row");
        assert!(row.favorite && row.play_count == 2 && row.last_played == 2000);
        let visible = db.query(&Filter::default()).expect("q");
        assert_eq!(visible.len(), 1, "hidden excluded by default");
        let favs = db
            .query(&Filter {
                favorites_only: true,
                ..Filter::default()
            })
            .expect("q");
        assert_eq!(favs.len(), 1);
        let all = db
            .query(&Filter {
                show_hidden: true,
                ..Filter::default()
            })
            .expect("q");
        assert_eq!(all.len(), 2);
        let found = db
            .query(&Filter {
                text: "alp".to_string(),
                show_hidden: true,
                ..Filter::default()
            })
            .expect("q");
        assert_eq!(found.len(), 1);
    }

    #[test]
    fn collections_keep_apps_on_delete() {
        let db = Library::open_memory().expect("db");
        let a = db.upsert(&app("/a/A.AppImage", "Alpha")).expect("a");
        let col = db.create_collection("Work").expect("create");
        assert_eq!(db.create_collection("Work").expect("idempotent"), col);
        db.add_to_collection(a, col).expect("add");
        assert_eq!(db.members(col).expect("members").len(), 1);
        db.delete_collection(col).expect("delete");
        assert!(db.collections().expect("cols").is_empty());
        assert!(db.get(a).expect("get").is_some(), "app survives");
    }

    #[test]
    fn tags_round_trip() {
        let db = Library::open_memory().expect("db");
        let a = db.upsert(&app("/a/A.AppImage", "Alpha")).expect("a");
        db.set_tags(
            a,
            &["fast".to_string(), "  ".to_string(), "fast".to_string()],
        )
        .expect("tags");
        assert_eq!(db.tags_of(a).expect("tags"), vec!["fast".to_string()]);
        let tagged = db
            .query(&Filter {
                tag: Some("fast".to_string()),
                ..Filter::default()
            })
            .expect("q");
        assert_eq!(tagged.len(), 1);
    }
    #[test]
    fn remove_by_path() {
        let db = Library::open_memory().expect("db");
        db.upsert(&app("/a/A.AppImage", "Alpha")).expect("a");
        assert!(db.remove_by_path("/a/A.AppImage").expect("rm"));
        assert!(!db.remove_by_path("/a/A.AppImage").expect("rm-again"));
    }

    #[test]
    fn bulk_query_stays_fast() {
        use crate::query::Filter;
        let db = Library::open_memory().expect("db");
        for i in 0..2000 {
            db.upsert(&app(
                &format!("/a/App{i:05}.AppImage"),
                &format!("App{i:05}"),
            ))
            .expect("insert");
        }
        let start = std::time::Instant::now();
        let rows = db
            .query(&Filter {
                text: "app019".to_string(),
                ..Filter::default()
            })
            .expect("query");
        assert!(!rows.is_empty());
        assert!(
            start.elapsed() < std::time::Duration::from_millis(1000),
            "2k-row filtered query must stay interactive"
        );
    }
}
