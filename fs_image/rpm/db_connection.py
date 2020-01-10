#!/usr/bin/env python3
import sqlite3

from contextlib import AbstractContextManager

from .pluggable import Pluggable
from .repo_db import SQLDialect


class DBConnectionContext(AbstractContextManager, Pluggable):
    '''
    RepoDBContext gets its database connections from DBConnectionContext.
    This context is entered for a burst of database operations, and exited
    when there might be a lull in database accesses.  This lets your context
    to reconnect as needed, or to reuse the same connection.
    '''
    pass


class SQLiteConnectionContext(DBConnectionContext, plugin_kind='sqlite'):
    SQL_DIALECT = SQLDialect.SQLITE3

    def __init__(self, db_path: str):
        self.db_path = db_path
        self._conn = None

    def __enter__(self):
        self._conn = sqlite3.connect(self.db_path)
        return self._conn

    # Does not suppress exceptions
    def __exit__(self, exc_type, exc_val, exc_tb) -> None:
        # We must act equivalently to the MySQL context in rolling back
        # uncommitted changes on context exit.
        self._conn.rollback()
        self._conn.close()
        self._conn = None


# NB: If needed, it would be trivial to add a plain MySQL context. I'm
# leaving it commented-out since I have no plans of testing it soon.
#
#   import MySQLdb
#   class MySQLConnectionContext(DBConnectionContext, plugin_kind='mysql'):
#       SQL_DIALECT = SQLDialect.MYSQL
#
#       def __init__(self, ...):
#           self.... = ...
#           self._conn = None
#
#       def __enter__(self):
#           assert self._conn is None, 'MySQLConnectionContext not reentrant'
#           # Reconnect every time, since MySQL connections go away quickly.
#           self._conn = MySQLdb.connect(
#               ..., charset="ascii", use_unicode=True,
#           )
#           return self._conn
#
#       # Does not suppress exceptions
#       def __exit__(self, exc_type, exc_val, exc_tb) -> None:
#           self._conn.close()
#           self._conn = None

try:
    # Import FB-specific implementations if available. Do this last in the
    # file so that DBConnectionContext is already available to them.
    from .facebook import db_connection as _fb_db_connection  # noqa: F401
except ImportError:  # pragma: no cover
    pass
