/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under both the MIT license found in the
 * LICENSE-MIT file in the root directory of this source tree and the Apache
 * License, Version 2.0 found in the LICENSE-APACHE file in the root directory
 * of this source tree.
 */

//! Contains basic definitions for the sql crate and for any crate that wish
//! to implement traits to be used with the sql's queries macro.

#![deny(warnings, missing_docs, clippy::all, broken_intra_doc_links)]

pub mod deprecated_mysql;
pub mod error;
pub mod ext;
pub mod mysql;
pub mod sqlite;
pub mod transaction;

use std::fmt::{self, Debug};
use std::sync::Arc;

// Used in docs
#[cfg(test)]
mod _unused {
    use sql as _;
    use sql_tests_lib as _;
}

/// Struct to store a set of write, read and read-only connections for a shard.
#[derive(Clone)]
pub struct SqlConnections {
    /// Write connection to the master
    pub write_connection: Connection,
    /// Read connection
    pub read_connection: Connection,
    /// Read master connection
    pub read_master_connection: Connection,
}

impl SqlConnections {
    /// Create SqlConnections from a single connection.
    pub fn new_single(connection: Connection) -> Self {
        Self {
            write_connection: connection.clone(),
            read_connection: connection.clone(),
            read_master_connection: connection,
        }
    }
}

/// Struct to store a set of write, read and read-only connections for multiple shards.
#[derive(Clone)]
pub struct SqlShardedConnections {
    /// Write connections to the master for each shard
    pub write_connections: Vec<Connection>,
    /// Read connections for each shard
    pub read_connections: Vec<Connection>,
    /// Read master connections for each shard
    pub read_master_connections: Vec<Connection>,
}

impl SqlShardedConnections {
    /// Check if the struct is empty.
    pub fn is_empty(&self) -> bool {
        self.write_connections.is_empty()
    }
}

impl From<Vec<SqlConnections>> for SqlShardedConnections {
    fn from(shard_connections: Vec<SqlConnections>) -> Self {
        let mut write_connections = Vec::with_capacity(shard_connections.len());
        let mut read_connections = Vec::with_capacity(shard_connections.len());
        let mut read_master_connections = Vec::with_capacity(shard_connections.len());
        for connections in shard_connections.into_iter() {
            write_connections.push(connections.write_connection);
            read_connections.push(connections.read_connection);
            read_master_connections.push(connections.read_master_connection);
        }

        Self {
            read_connections,
            read_master_connections,
            write_connections,
        }
    }
}

/// Enum that generalizes over connections to Sqlite and MyRouter.
#[derive(Clone)]
pub enum Connection {
    /// Sqlite lets you use this crate with rusqlite connections such as in memory or on disk Sqlite
    /// databases, both useful in case of testing or local sql db use cases.
    Sqlite(Arc<sqlite::SqliteMultithreaded>),
    /// An enum variant for the mysql-based connections, your structure have to
    /// implement [deprecated_mysql::MysqlConnection] in order to be usable here.
    ///
    /// This backend is based on MyRouter connections and is deprecated soon. Please
    /// use new Mysql client instead.
    DeprecatedMysql(deprecated_mysql::BoxMysqlConnection),
    /// A variant used for the new Mysql client connection factory.
    Mysql(mysql::Connection),
}

impl From<sqlite::SqliteMultithreaded> for Connection {
    fn from(con: sqlite::SqliteMultithreaded) -> Self {
        Connection::Sqlite(Arc::new(con))
    }
}

impl From<deprecated_mysql::BoxMysqlConnection> for Connection {
    fn from(con: deprecated_mysql::BoxMysqlConnection) -> Self {
        Connection::DeprecatedMysql(con)
    }
}

impl From<mysql::Connection> for Connection {
    fn from(conn: mysql::Connection) -> Self {
        Connection::Mysql(conn)
    }
}

impl Debug for Connection {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Connection::Sqlite(..) => write!(f, "Sqlite"),
            Connection::DeprecatedMysql(ref con) => con.fmt(f),
            Connection::Mysql(..) => write!(f, "Mysql client"),
        }
    }
}

/// Value returned from a `write` type of query
pub struct WriteResult {
    last_insert_id: Option<u64>,
    affected_rows: u64,
}

impl WriteResult {
    /// Method made public for access from inside macros, you probably don't want to use it.
    pub fn new(last_insert_id: Option<u64>, affected_rows: u64) -> Self {
        WriteResult {
            last_insert_id,
            affected_rows,
        }
    }

    /// Return the id of last inserted row if any.
    pub fn last_insert_id(&self) -> Option<u64> {
        self.last_insert_id
    }

    /// Return number of rows affected by the `write` query
    pub fn affected_rows(&self) -> u64 {
        self.affected_rows
    }
}
