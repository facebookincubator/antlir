#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import sqlite3
import urllib.parse
from contextlib import AbstractContextManager
from typing import Optional

import boto3
import mysql.connector
from antlir.common import get_logger

from .pluggable import Pluggable
from .repo_db import SQLDialect


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
    _warned_about_force_master = False

    def __init__(
        self,
        db_path: str,
        readonly: bool = False,
        force_master: Optional[bool] = None,
    ):
        if (
            force_master is not None
            and not type(self)._warned_about_force_master
        ):  # pragma: no cover
            log.warning("`force_master` is not supported for SQLite - ignoring")
            type(self)._warned_about_force_master = True
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


class MySQLConnectionContext(DBConnectionContext, plugin_kind="mysql"):
    SQL_DIALECT = SQLDialect.MYSQL
    _warned_about_readonly = False
    _warned_about_force_master = False

    def __init__(
        self,
        endpoint: str,
        dbname: str,
        user: str,
        password: str,
        port: int = 3306,
        # readonly and force_master cannot be generically applied to MySQL
        # connections, it depends on the endpoint host and the user. The
        # parameters are accepted for consistency, but they are not used.
        readonly: bool = False,
        force_master: Optional[bool] = None,
    ):
        self.endpoint = endpoint
        self.dbname = dbname
        self.user = user
        self.password = password
        self.port = port

        if (
            readonly and not type(self)._warned_about_readonly
        ):  # pragma: no cover
            log.warning("`readonly` is not supported for MySQL - ignoring")
            type(self)._warned_about_readonly = True

        if (
            force_master is not None
            and not type(self)._warned_about_force_master
        ):  # pragma: no cover
            log.warning("`force_master` is not supported for MySQL - ignoring")
            type(self)._warned_about_force_master = True

        self._conn = None

    def __enter__(self):
        assert self._conn is None, "MySQLConnectionContext not reentrant"
        # Reconnect every time, since MySQL connections go away quickly.
        self._conn = mysql.connector.connect(
            host=self.endpoint,
            user=self.user,
            passwd=self.password,
            port=self.port,
            database=self.dbname,
        )
        return self._conn

    # Does not suppress exceptions
    def __exit__(self, exc_type, exc_val, exc_tb) -> None:
        # We must act equivalently to the MySQL context in rolling back
        # uncommitted changes on context exit.
        self._conn.rollback()
        self._conn.close()
        self._conn = None


class RDSMySQLConnectionContext(
    MySQLConnectionContext, plugin_kind="rds_mysql"
):
    SQL_DIALECT = SQLDialect.MYSQL

    def __init__(
        self,
        region: str,
        **kwargs,
    ):
        assert (
            "password" not in kwargs
        ), "password is not accepted for RDS connections"
        super().__init__(**kwargs)

        self._conn = None

    @property
    def password(self) -> str:
        # Override the password property that the parent uses to get a
        # connection token via boto3 and IAM.
        # These tokens are valid for 15 minutes, so regenerate on each
        # request to guarantee freshness.
        client = boto3.client("rds", region_name=self.region)
        return client.generate_db_auth_token(
            DBHostname=self.endpoint,
            Port=self.port,
            DBUsername=self.user,
            Region=self.region,
        )

    # noop on password property set, it always needs to be generated, but this
    # allows subclassing the base MySQL connector
    @password.setter
    def password(self, x: str) -> None:
        pass


try:
    # Import FB-specific implementations if available. Do this last in the
    # file so that DBConnectionContext is already available to them.
    from .facebook import db_connection as _fb_db_connection  # noqa: F401
except ImportError:  # pragma: no cover
    pass
