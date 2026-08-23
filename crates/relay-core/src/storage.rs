use std::{
    fs::{self, File, OpenOptions},
    path::Path,
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::Context;
use relay_protocol::{ChatScope, ChatTarget, CoreToAdapter, InboundMessage, PROTOCOL_VERSION};
use rusqlite::{Connection, OptionalExtension, params};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PurgeCounts {
    pub outbox: usize,
    pub inbox: usize,
    pub threads: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InboxDisposition {
    New,
    Duplicate,
    Conflict,
}

pub struct Store {
    connection: Mutex<Connection>,
    // Held for the lifetime of the store so one database cannot be served by
    // two relay processes at the same time.
    _instance_lock: File,
}

fn now_ms() -> i64 {
    i64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis(),
    )
    .unwrap_or(i64::MAX)
}

fn scope_name(scope: &ChatScope) -> &'static str {
    match scope {
        ChatScope::C2c => "c2c",
        ChatScope::Group => "group",
    }
}

fn parse_scope(value: &str) -> anyhow::Result<ChatScope> {
    match value {
        "c2c" => Ok(ChatScope::C2c),
        "group" => Ok(ChatScope::Group),
        _ => anyhow::bail!("invalid persisted chat scope"),
    }
}

fn acquire_instance_lock(path: &Path) -> anyhow::Result<File> {
    let mut lock_path = path.as_os_str().to_os_string();
    lock_path.push(".lock");
    let lock_path = std::path::PathBuf::from(lock_path);
    let instance_lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&lock_path)
        .with_context(|| format!("cannot open relay instance lock {}", lock_path.display()))?;
    match instance_lock.try_lock() {
        Ok(()) => Ok(instance_lock),
        Err(std::fs::TryLockError::WouldBlock) => anyhow::bail!(
            "relay database is already owned by another relay instance: {}",
            path.display()
        ),
        Err(std::fs::TryLockError::Error(error)) => Err(error)
            .with_context(|| format!("cannot acquire relay instance lock {}", lock_path.display())),
    }
}

impl Store {
    pub fn open(path: &Path, workspace: &Path) -> anyhow::Result<Self> {
        let resolved_workspace = fs::canonicalize(workspace)
            .with_context(|| format!("cannot resolve workspace {}", workspace.display()))?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("cannot create state directory {}", parent.display()))?;
            let resolved_parent = fs::canonicalize(parent)
                .with_context(|| format!("cannot resolve state directory {}", parent.display()))?;
            if resolved_parent.starts_with(&resolved_workspace) {
                anyhow::bail!("RELAY_DB must resolve outside CODEX_WORKSPACE");
            }
        }
        if path.exists() {
            let resolved_path = fs::canonicalize(path)
                .with_context(|| format!("cannot resolve relay database {}", path.display()))?;
            if resolved_path.starts_with(&resolved_workspace) {
                anyhow::bail!("RELAY_DB must resolve outside CODEX_WORKSPACE");
            }
        }
        let instance_lock = acquire_instance_lock(path)?;
        let mut connection = Connection::open(path)
            .with_context(|| format!("cannot open relay database {}", path.display()))?;
        connection.busy_timeout(std::time::Duration::from_secs(5))?;
        connection.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = FULL;
             PRAGMA foreign_keys = ON;
             CREATE TABLE IF NOT EXISTS thread_bindings (
               target_key TEXT PRIMARY KEY,
               thread_id TEXT NOT NULL,
               updated_at_ms INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS inbox (
               message_id TEXT PRIMARY KEY,
               target_key TEXT NOT NULL,
               target_scope TEXT NOT NULL,
               target_id TEXT NOT NULL,
               content_sha256 TEXT NOT NULL,
               status TEXT NOT NULL CHECK(status IN ('processing','completed','failed')),
               received_at_ms INTEGER NOT NULL,
               error_code TEXT
             );
             CREATE TABLE IF NOT EXISTS outbox (
               outbox_id TEXT PRIMARY KEY,
               message_id TEXT NOT NULL,
               target_scope TEXT NOT NULL,
               target_id TEXT NOT NULL,
               text TEXT,
               kind TEXT NOT NULL CHECK(kind IN ('reply','error')),
               error_code TEXT,
               status TEXT NOT NULL CHECK(status IN ('pending','delivered')),
               created_at_ms INTEGER NOT NULL,
               delivered_at_ms INTEGER,
               FOREIGN KEY(message_id) REFERENCES inbox(message_id)
             );
             CREATE INDEX IF NOT EXISTS outbox_status_idx ON outbox(status, created_at_ms);",
        )?;
        let schema_version: i64 =
            connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
        if schema_version > 1 {
            anyhow::bail!("relay database schema is newer than this binary");
        }
        if schema_version == 0 {
            let transaction = connection.transaction()?;
            if !has_column(&transaction, "inbox", "target_scope")? {
                transaction.execute(
                    "ALTER TABLE inbox ADD COLUMN target_scope TEXT NOT NULL DEFAULT ''",
                    [],
                )?;
            }
            if !has_column(&transaction, "inbox", "target_id")? {
                transaction.execute(
                    "ALTER TABLE inbox ADD COLUMN target_id TEXT NOT NULL DEFAULT ''",
                    [],
                )?;
            }
            transaction.execute(
                "UPDATE inbox
                    SET target_scope = substr(target_key, 1, instr(target_key, ':') - 1),
                        target_id = substr(target_key, instr(target_key, ':') + 1)
                  WHERE target_scope = '' OR target_id = ''",
                [],
            )?;
            transaction.pragma_update(None, "user_version", 1)?;
            transaction.commit()?;
        }
        if !has_column(&connection, "inbox", "target_scope")?
            || !has_column(&connection, "inbox", "target_id")?
        {
            anyhow::bail!("relay database schema is incomplete");
        }
        Ok(Self {
            connection: Mutex::new(connection),
            _instance_lock: instance_lock,
        })
    }

    pub fn recover_interrupted(&self) -> anyhow::Result<usize> {
        let mut connection = self.connection.lock().expect("database mutex poisoned");
        let transaction = connection.transaction()?;
        let interrupted = {
            let mut statement = transaction.prepare(
                "SELECT message_id, target_scope, target_id FROM inbox WHERE status = 'processing'",
            )?;
            statement
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };
        for (message_id, target_scope, target_id) in &interrupted {
            transaction.execute(
                "INSERT INTO outbox(outbox_id, message_id, target_scope, target_id, text, kind, error_code, status, created_at_ms)
                 VALUES (?1, ?2, ?3, ?4, ?5, 'error', 'interrupted', 'pending', ?6)",
                params![
                    Uuid::new_v4().to_string(),
                    message_id,
                    target_scope,
                    target_id,
                    "The relay restarted before this request completed. Please send it again.",
                    now_ms()
                ],
            )?;
        }
        transaction.execute(
            "UPDATE inbox SET status = 'failed', error_code = 'interrupted' WHERE status = 'processing'",
            [],
        )?;
        transaction.commit()?;
        Ok(interrupted.len())
    }

    pub fn purge_expired(&self, retention: std::time::Duration) -> anyhow::Result<PurgeCounts> {
        let retention_ms = i64::try_from(retention.as_millis()).unwrap_or(i64::MAX);
        self.purge_before(now_ms().saturating_sub(retention_ms))
    }

    fn purge_before(&self, cutoff_ms: i64) -> anyhow::Result<PurgeCounts> {
        let mut connection = self.connection.lock().expect("database mutex poisoned");
        let transaction = connection.transaction()?;
        let outbox = transaction.execute(
            "DELETE FROM outbox WHERE status = 'delivered' AND delivered_at_ms < ?1",
            [cutoff_ms],
        )?;
        let inbox = transaction.execute(
            "DELETE FROM inbox
             WHERE status != 'processing' AND received_at_ms < ?1
               AND NOT EXISTS (SELECT 1 FROM outbox WHERE outbox.message_id = inbox.message_id)",
            [cutoff_ms],
        )?;
        let threads = transaction.execute(
            "DELETE FROM thread_bindings WHERE updated_at_ms < ?1",
            [cutoff_ms],
        )?;
        transaction.commit()?;
        Ok(PurgeCounts {
            outbox,
            inbox,
            threads,
        })
    }

    pub fn begin_inbound(
        &self,
        message: &InboundMessage,
        target_key: &str,
        content_sha256: &str,
    ) -> anyhow::Result<InboxDisposition> {
        let connection = self.connection.lock().expect("database mutex poisoned");
        let inserted = connection.execute(
            "INSERT OR IGNORE INTO inbox(message_id, target_key, target_scope, target_id, content_sha256, status, received_at_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, 'processing', ?6)",
            params![
                message.message_id,
                target_key,
                scope_name(&message.target.scope),
                message.target.target_id,
                content_sha256,
                now_ms()
            ],
        )?;
        if inserted == 1 {
            return Ok(InboxDisposition::New);
        }
        let existing = connection.query_row(
            "SELECT target_key, content_sha256 FROM inbox WHERE message_id = ?1",
            [&message.message_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )?;
        Ok(
            if existing.0 == target_key && existing.1 == content_sha256 {
                InboxDisposition::Duplicate
            } else {
                InboxDisposition::Conflict
            },
        )
    }

    pub fn thread_id(&self, target_key: &str) -> anyhow::Result<Option<String>> {
        let connection = self.connection.lock().expect("database mutex poisoned");
        Ok(connection
            .query_row(
                "SELECT thread_id FROM thread_bindings WHERE target_key = ?1",
                [target_key],
                |row| row.get(0),
            )
            .optional()?)
    }

    pub fn set_thread_id(&self, target_key: &str, thread_id: &str) -> anyhow::Result<()> {
        let connection = self.connection.lock().expect("database mutex poisoned");
        connection.execute(
            "INSERT INTO thread_bindings(target_key, thread_id, updated_at_ms) VALUES (?1, ?2, ?3)
             ON CONFLICT(target_key) DO UPDATE SET thread_id = excluded.thread_id, updated_at_ms = excluded.updated_at_ms",
            params![target_key, thread_id, now_ms()],
        )?;
        Ok(())
    }

    pub fn reset_thread(&self, target_key: &str) -> anyhow::Result<()> {
        let connection = self.connection.lock().expect("database mutex poisoned");
        connection.execute(
            "DELETE FROM thread_bindings WHERE target_key = ?1",
            [target_key],
        )?;
        Ok(())
    }

    pub fn complete_reply(
        &self,
        message: &InboundMessage,
        text: &str,
    ) -> anyhow::Result<CoreToAdapter> {
        self.complete(message, text, "reply", None)
    }

    pub fn complete_error(
        &self,
        message: &InboundMessage,
        text: &str,
        error_code: &str,
    ) -> anyhow::Result<CoreToAdapter> {
        self.complete(message, text, "error", Some(error_code))
    }

    fn complete(
        &self,
        message: &InboundMessage,
        text: &str,
        kind: &str,
        error_code: Option<&str>,
    ) -> anyhow::Result<CoreToAdapter> {
        let mut connection = self.connection.lock().expect("database mutex poisoned");
        let transaction = connection.transaction()?;
        let changed = transaction.execute(
            "UPDATE inbox SET status = ?2, error_code = ?3 WHERE message_id = ?1 AND status = 'processing'",
            params![
                message.message_id,
                if error_code.is_some() {
                    "failed"
                } else {
                    "completed"
                },
                error_code
            ],
        )?;
        if changed != 1 {
            anyhow::bail!("cannot complete an unknown inbox message");
        }
        let outbox_id = Uuid::new_v4().to_string();
        transaction.execute(
            "INSERT INTO outbox(outbox_id, message_id, target_scope, target_id, text, kind, error_code, status, created_at_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'pending', ?8)",
            params![
                outbox_id,
                message.message_id,
                scope_name(&message.target.scope),
                message.target.target_id,
                text,
                kind,
                error_code,
                now_ms()
            ],
        )?;
        transaction.commit()?;

        if let Some(code) = error_code {
            Ok(CoreToAdapter::Error {
                protocol_version: PROTOCOL_VERSION.into(),
                outbox_id: Some(outbox_id),
                in_reply_to: Some(message.message_id.clone()),
                target: Some(message.target.clone()),
                code: code.into(),
                message: text.into(),
                retryable: false,
            })
        } else {
            Ok(CoreToAdapter::Reply {
                protocol_version: PROTOCOL_VERSION.into(),
                outbox_id,
                in_reply_to: message.message_id.clone(),
                target: message.target.clone(),
                text: text.into(),
            })
        }
    }

    pub fn acknowledge(&self, outbox_id: &str) -> anyhow::Result<bool> {
        let connection = self.connection.lock().expect("database mutex poisoned");
        let changed = connection.execute(
            "UPDATE outbox SET status = 'delivered', delivered_at_ms = ?2, text = NULL
             WHERE outbox_id = ?1 AND status = 'pending'",
            params![outbox_id, now_ms()],
        )?;
        Ok(changed == 1)
    }

    pub fn pending_outbox(&self) -> anyhow::Result<Vec<CoreToAdapter>> {
        let connection = self.connection.lock().expect("database mutex poisoned");
        let mut statement = connection.prepare(
            "SELECT outbox_id, message_id, target_scope, target_id, text, kind, error_code
             FROM outbox WHERE status = 'pending' ORDER BY created_at_ms, outbox_id",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, Option<String>>(6)?,
            ))
        })?;
        rows.map(|row| {
            let (outbox_id, message_id, scope, target_id, text, kind, error_code) = row?;
            let target = ChatTarget {
                scope: parse_scope(&scope)?,
                target_id,
            };
            let text = text.unwrap_or_else(|| "Response content was already redacted.".into());
            if kind == "reply" {
                Ok(CoreToAdapter::Reply {
                    protocol_version: PROTOCOL_VERSION.into(),
                    outbox_id,
                    in_reply_to: message_id,
                    target,
                    text,
                })
            } else {
                Ok(CoreToAdapter::Error {
                    protocol_version: PROTOCOL_VERSION.into(),
                    outbox_id: Some(outbox_id),
                    in_reply_to: Some(message_id),
                    target: Some(target),
                    code: error_code.unwrap_or_else(|| "relay_error".into()),
                    message: text,
                    retryable: false,
                })
            }
        })
        .collect()
    }
}

fn has_column(connection: &Connection, table: &str, column: &str) -> anyhow::Result<bool> {
    let mut statement = connection.prepare(&format!("PRAGMA table_info({table})"))?;
    let names = statement
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(names.iter().any(|name| name == column))
}

#[cfg(test)]
mod tests {
    use relay_protocol::{ChatScope, ChatTarget};
    use tempfile::tempdir;

    use super::*;

    fn message() -> InboundMessage {
        InboundMessage {
            message_id: "message-1".into(),
            sender_id: "sender-1".into(),
            sender_is_bot: false,
            target: ChatTarget {
                scope: ChatScope::C2c,
                target_id: "sender-1".into(),
            },
            content: "hello".into(),
            platform_timestamp: "2026-08-22T00:00:00Z".into(),
            received_at_ms: 1,
        }
    }

    #[test]
    fn inbox_and_outbox_are_idempotent_and_ack_redacts_text() {
        let directory = tempdir().expect("tempdir");
        let workspace = directory.path().join("workspace");
        fs::create_dir(&workspace).expect("workspace");
        let store =
            Store::open(&directory.path().join("relay.sqlite3"), &workspace).expect("store");
        let message = message();
        assert_eq!(
            store
                .begin_inbound(&message, "c2c:sender-1", "hash")
                .expect("insert"),
            InboxDisposition::New
        );
        assert_eq!(
            store
                .begin_inbound(&message, "c2c:sender-1", "hash")
                .expect("duplicate"),
            InboxDisposition::Duplicate
        );
        let output = store.complete_reply(&message, "reply").expect("complete");
        let CoreToAdapter::Reply { outbox_id, .. } = output else {
            panic!("expected reply");
        };
        assert_eq!(store.pending_outbox().expect("pending").len(), 1);
        assert!(store.acknowledge(&outbox_id).expect("ack"));
        assert!(
            store
                .pending_outbox()
                .expect("pending after ack")
                .is_empty()
        );
        assert!(!store.acknowledge(&outbox_id).expect("second ack"));
        let purged = store.purge_before(i64::MAX).expect("purge");
        assert_eq!(purged.outbox, 1);
        assert_eq!(purged.inbox, 1);
    }

    #[test]
    fn interrupted_inbox_becomes_a_replayable_error() {
        let directory = tempdir().expect("tempdir");
        let workspace = directory.path().join("workspace");
        fs::create_dir(&workspace).expect("workspace");
        let store =
            Store::open(&directory.path().join("relay.sqlite3"), &workspace).expect("store");
        let message = message();
        store
            .begin_inbound(&message, "c2c:sender-1", "hash")
            .expect("insert");
        assert_eq!(store.recover_interrupted().expect("recover"), 1);
        let pending = store.pending_outbox().expect("pending");
        assert!(matches!(
            pending.as_slice(),
            [CoreToAdapter::Error { code, .. }] if code == "interrupted"
        ));
    }

    #[test]
    fn state_database_cannot_resolve_inside_the_workspace() {
        let directory = tempdir().expect("tempdir");
        let workspace = directory.path().join("workspace");
        fs::create_dir(&workspace).expect("workspace");
        let result = Store::open(&workspace.join("relay.sqlite3"), &workspace);
        assert!(result.is_err());
    }

    #[test]
    fn only_one_store_can_own_a_database() {
        let directory = tempdir().expect("tempdir");
        let workspace = directory.path().join("workspace");
        fs::create_dir(&workspace).expect("workspace");
        let database = directory.path().join("relay.sqlite3");
        let first = Store::open(&database, &workspace).expect("first store");
        let second = Store::open(&database, &workspace);
        assert!(second.is_err());
        drop(first);
        Store::open(&database, &workspace).expect("lock released after drop");
    }

    #[test]
    fn partial_v0_schema_migrates_atomically_to_v1() {
        let directory = tempdir().expect("tempdir");
        let workspace = directory.path().join("workspace");
        fs::create_dir(&workspace).expect("workspace");
        let database = directory.path().join("relay.sqlite3");
        let connection = Connection::open(&database).expect("legacy database");
        connection
            .execute_batch(
                "CREATE TABLE inbox (
                   message_id TEXT PRIMARY KEY,
                   target_key TEXT NOT NULL,
                   target_scope TEXT NOT NULL DEFAULT '',
                   content_sha256 TEXT NOT NULL,
                   status TEXT NOT NULL,
                   received_at_ms INTEGER NOT NULL,
                   error_code TEXT
                 );
                 INSERT INTO inbox(
                   message_id, target_key, target_scope, content_sha256, status, received_at_ms
                 ) VALUES ('legacy-1', 'c2c:sender-1', '', 'hash', 'completed', 1);
                 PRAGMA user_version = 0;",
            )
            .expect("legacy schema");
        drop(connection);

        let store = Store::open(&database, &workspace).expect("migrated store");
        let connection = store.connection.lock().expect("database mutex");
        let version: i64 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .expect("schema version");
        let target: (String, String) = connection
            .query_row(
                "SELECT target_scope, target_id FROM inbox WHERE message_id = 'legacy-1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("migrated row");

        assert_eq!(version, 1);
        assert_eq!(target, ("c2c".to_owned(), "sender-1".to_owned()));
    }
}
