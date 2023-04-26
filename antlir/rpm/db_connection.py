#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import sqlite3
import urllib.parse
from contextlib import AbstractContextManager
from typing import Optional

from antlir.common import get_logger

from antlir.rpm.pluggable import Pluggable
from antlir.rpm.repo_db import SQLDialect


log = get_logger()


class DBConnectionContext(AbstractContextManager, Pluggable):
    """
    RepoDBContext gets its database connections from DBConnectionContext.
    This context is entered for a burst of database operations, and exited
    when there might be a lull in database accesses.  This lets your context
    to reconnect as needed, or to reuse the same connection.
    """

    @property
    def SQL_DIALECT(self) -> SQLDialect:
        raise NotImplementedError


class SQLiteConnectionContext(DBConnectionContext, plugin_kind="sqlite"):
    SQL_DIALECT = SQLDialect.SQLITE3
    _warned_about_sqlite_force_master = False

    def __init__(
        self,
        db_path: str,
        readonly: bool = False,
        force_master: Optional[bool] = None,
    ) -> None:
        if (
            force_master is not None
            and not type(self)._warned_about_sqlite_force_master
        ):  # pragma: no cover
            log.warning("`force_master` is not supported for SQLite - ignoring")
            type(self)._warned_about_sqlite_force_master = True
        self.readonly = readonly
        self.db_path = db_path
        self._conn = None

    def __enter__(self):
        if self.readonly:
            self._conn = sqlite3.connect(
                f"file:{urllib.parse.quote(self.db_path)}?mode=ro", uri=True
            )
        else:
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
    from antlir.rpm.facebook import db_connection as _fb_db_connection  # noqa: F401
except ImportError:  # pragma: no cover
    pass
