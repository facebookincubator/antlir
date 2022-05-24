#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"Utilities to make Python systems programming more palatable."
import array
import asyncio
import inspect
import logging
import os
import platform
import random
import re
import socket
import subprocess
import tempfile
import time
from contextlib import contextmanager
from functools import wraps
from typing import (
    AnyStr,
    Callable,
    Iterable,
    Iterator,
    List,
    NamedTuple,
    Optional,
    Tuple,
    TypeVar,
    Union,
)


T = TypeVar("T")
_mockable_retry_fn_sleep = time.sleep
_mockable_platform_release = platform.release


# Bite me, Python3.
def byteme(s: AnyStr) -> bytes:
    "Byte literals are tiring, just promote strings as needed."
    # pyre-fixme[16]: `bytes` has no attribute `encode`.
    return s.encode() if isinstance(s, str) else s


# It's possible that `get_logger` is obtained, **and** logs, before
# `init_logging` is called.  Such usage is a minor bug; rather than hide the
# bug by never showing the debug logs, we'll make those logs visible.
_INITIALIZED_LOGGING = False
_ANTLIR_ROOT_LOGGER = "antlir"


class ColorFormatter(logging.Formatter):
    _base_fmt = (
        "\x1b[90m %(asctime)s.%(msecs)03d %(filename)s:%(lineno)d "
        "\x1b[0m%(message)s"
    )
    # pyre-fixme[4]: Attribute must be annotated.
    _level_to_prefix = {
        logging.DEBUG: "\x1b[37mD",  # White
        logging.INFO: "\x1b[94mI",  # Blue
        logging.WARNING: "\x1b[93mW",  # Yellow
        logging.ERROR: "\x1b[91mE",  # Red
        logging.CRITICAL: "\x1b[95mF",  # Magenta
    }

    def __init__(self) -> None:
        super().__init__(datefmt="%Y%m%d %H:%M:%S")

    # pyre-fixme[2]: Parameter must be annotated.
    def format(self, record) -> str:
        try:
            self._style._fmt = (
                self._level_to_prefix[record.levelno] + self._base_fmt
            )
        except KeyError:
            # Fall back to just prepending the log level int
            self._style._fmt = str(record.levelno) + self._base_fmt
        return logging.Formatter.format(self, record)


# NB: Many callsites in antlir rely on the assumption that this function will
# result in logging to the default stream of StreamHandler, which is stderr.
def init_logging(*, debug: bool = False) -> None:
    global _INITIALIZED_LOGGING
    level = logging.DEBUG if debug else logging.INFO
    logger = logging.getLogger(_ANTLIR_ROOT_LOGGER)
    # The first time around, just set up the stream handler & formatter --
    # this will be inherited by all `get_logger` instances.
    if not _INITIALIZED_LOGGING:
        _INITIALIZED_LOGGING = True
        hdlr = logging.StreamHandler()
        hdlr.setFormatter(ColorFormatter())
        logger.addHandler(hdlr)
        return
    # Logging is being "explicitly" re-initialized, so we may need to update
    # the level.  We only need to touch the root logger because all others
    # use `NOTSET` per `get_logger`.
    logger.setLevel(level)


# pyre-fixme[3]: Return type must be annotated.
def get_logger():
    # Default-initialize with `debug=True` until the user tells us otherwise.
    if not _INITIALIZED_LOGGING:
        init_logging(debug=True)
    calling_file = os.path.basename(inspect.stack()[1].filename)
    # Strip extension from name of logger
    if calling_file.endswith(".py"):
        calling_file = calling_file[: -len(".py")]
    logger = logging.getLogger(_ANTLIR_ROOT_LOGGER + "." + calling_file)
    logger.setLevel(logging.NOTSET)
    return logger


# pyre-fixme[5]: Global expression must be annotated.
log = get_logger()


# pyre-fixme[24]: Generic type `subprocess.Popen` expects 1 type parameter.
def check_popen_returncode(proc: subprocess.Popen) -> None:
    if proc.returncode != 0:  # pragma: no cover
        # Providing a meaningful coverage test for this is annoying, so I just
        # tested manually:
        #   >>> import subprocess
        #   >>> raise subprocess.CalledProcessError(returncode=5, cmd=['a'])
        #   Traceback (most recent call last):
        #     File "<stdin>", line 1, in <module>
        #   subprocess.CalledProcessError: Command '['a']' returned non-zero
        #   exit status 5.
        raise subprocess.CalledProcessError(
            returncode=proc.returncode, cmd=proc.args
        )


# pyre-fixme[2]: Parameter must be annotated.
def set_new_key(d, k, v) -> None:
    "`d[k] = v` that raises if it would it would overwrite an existing value"
    if k in d:
        raise KeyError(f"{k} was already set to {d[k]}, new value: {v}")
    d[k] = v


def shuffled(it: Iterable[T]) -> List[T]:
    l = list(it)
    random.shuffle(l)
    return l


@contextmanager
def listen_temporary_unix_socket() -> Iterator[Tuple[str, socket.socket]]:
    # Hardcoding /tmp is ugly, but Buck sets $TMP to fairly long paths,
    # which can cause `AF_UNIX path too long`.
    with tempfile.TemporaryDirectory(dir="/tmp") as td, socket.socket(
        socket.AF_UNIX, socket.SOCK_STREAM
    ) as lsock:
        sock_path = os.path.join(td, "sock")
        lsock.bind(sock_path)
        lsock.listen()
        yield sock_path, lsock


# pyre-fixme[3]: Return type must be annotated.
# pyre-fixme[2]: Parameter must be annotated.
def recv_fds(sock, msglen, maxfds, inheritable: bool = False):
    """
    Receives via a Unix domain socket a message of at most `msglen` bytes,
    with at most `maxfds` file descriptors in the ancillary data.  The file
    descriptors will be marked O_CLOEXEC unless inheritable is set to False.
    """
    fds = array.array("i")
    msg, ancdata, msg_flags, _addr = sock.recvmsg(
        msglen,
        maxfds * socket.CMSG_SPACE(fds.itemsize),
        0 if inheritable else socket.MSG_CMSG_CLOEXEC,
    )
    assert not (msg_flags & socket.MSG_TRUNC), msg_flags
    assert not (msg_flags & socket.MSG_CTRUNC), msg_flags
    assert not (msg_flags & socket.MSG_ERRQUEUE), msg_flags
    for cmsg_level, cmsg_type, cmsg_data in ancdata:
        assert cmsg_level == socket.SOL_SOCKET, cmsg_level
        assert cmsg_type == socket.SCM_RIGHTS, cmsg_type
        assert len(cmsg_data) % fds.itemsize == 0, cmsg_data
        fds.frombytes(cmsg_data)
    return msg, list(fds)


# Don't wait forever if the `send_fds` side crashes.  This is 2.5 minutes so
# we still make progress on overloaded hosts.
FD_UNIX_SOCK_TIMEOUT = 150


# pyre-fixme[3]: Return type must be annotated.
# pyre-fixme[2]: Parameter must be annotated.
def recv_fds_from_unix_sock(sock_path, max_fds):
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as conn_sock:
        # Don't wait forever if the `send_fds` side crashes.  This is 3
        # minutes so we still make progress on overloaded hosts.
        conn_sock.settimeout(FD_UNIX_SOCK_TIMEOUT)
        conn_sock.connect(sock_path)
        ignored_msg_len = 128
        _msg, fds = recv_fds(conn_sock, ignored_msg_len, max_fds)
        return fds


def run_stdout_to_err(
    args: Iterable[Union[str, bytes]],
    *,
    stdout: None = None,
    # pyre-fixme[2]: Parameter must be annotated.
    **kwargs
    # pyre-fixme[24]: Generic type `subprocess.CompletedProcess` expects 1 type parameter.
) -> subprocess.CompletedProcess:
    """
    Use this instead of `subprocess.{run,call,check_call}()` to prevent
    subprocesses from accidentally polluting stdout.
    """
    assert stdout is None, "run_stdout_to_err does not take a stdout kwarg"
    # pyre-fixme[6]: Expected `Union[os.PathLike[bytes], os.PathLike[str],
    #  typing.Sequence[typing.Union[os.PathLike[bytes], os.PathLike[str], bytes,
    #  str]], bytes, str]` for 1st param but got `Iterable[Variable[AnyStr <:
    #  [str, bytes]]]`.
    return subprocess.run(args, **kwargs, stdout=2)  # Redirect to stderr


@contextmanager
# pyre-fixme[3]: Return type must be annotated.
def pipe():
    r_fd, w_fd = os.pipe2(os.O_CLOEXEC)
    with os.fdopen(r_fd, "rb") as r, os.fdopen(w_fd, "wb") as w:
        yield r, w


@contextmanager
# pyre-fixme[2]: Parameter must be annotated.
def open_fd(path: AnyStr, flags) -> Iterator[int]:
    # If you ever need **NOT** to set one of these very sane defaults, add a
    # clearly named keyword-only arg.
    fd = os.open(path, flags=flags | os.O_NOCTTY | os.O_CLOEXEC)
    try:
        yield fd
    finally:
        os.close(fd)


def not_none(
    var: Optional[T], var_name: str = "", detail: Optional[str] = None
) -> T:
    """Used for type-refinement with `Optional`s."""
    if var is not None:
        return var
    expr_str = f"`{var_name}`" if var_name else "Expression"
    detail_str = "" if detail is None else f": {detail}"
    raise AssertionError(f"{expr_str} must not be None{detail_str}")


def retry_fn(
    retryable_fn: Callable[[], T],
    is_exception_retryable: Optional[Callable[[Exception], bool]] = None,
    *,
    delays: Iterable[float],
    what: str,
    log_exception: bool = True,
) -> T:
    """Allows functions to be retried `len(delays)` times, with each iteration
    sleeping for its respective index into `delays`. `is_exception_retryable`
    is an optional function that takes the raised exception and potentially
    evaluate to False, at which case no retry will occur and the exception will
    be re-raised. If the exception is not re-raised, the retry message will be
    logged to either DEBUG or ERROR depending whether `log_exception` is True.

    Delays are in seconds.
    """
    for i, delay in enumerate(delays):
        try:
            return retryable_fn()
        except Exception as e:
            if is_exception_retryable and not is_exception_retryable(e):
                raise
            log.log(
                logging.ERROR if log_exception else logging.DEBUG,
                # pyre-fixme[6]: Expected `Sized` for 1st param but got
                #  `Iterable[float]`.
                f"\n\n[Retry {i + 1} of {len(delays)}] {what} -- waiting "
                f"{delay} seconds.\n\n",
                exc_info=log_exception,
            )
            _mockable_retry_fn_sleep(delay)
    return retryable_fn()  # With 0 retries, we should still run the function.


# pyre-fixme[3]: Return type must be annotated.
def retryable(
    format_msg: str,
    delays: Iterable[float],
    *,
    is_exception_retryable: Optional[Callable[[Exception], bool]] = None,
    log_exception: bool = True,
):
    """Decorator used to retry a function if exceptions are thrown. `format_msg`
    should be a format string that can access any args provided to the
    decorated function. `delays` are the delays between retries, in seconds.
    `is_exception_retryable` and `log_exception` are forwarded to `retry_fn`,
    see its docblock.
    """
    # Prevent aliasing, iterator exhaustion, and other weirdness.
    # Indeterminate retry would require changing the API anyway.
    delays = list(delays)

    # pyre-fixme[3]: Return type must be annotated.
    # pyre-fixme[2]: Parameter must be annotated.
    def wrapper(fn):
        @wraps(fn)
        # pyre-fixme[53]: Captured variable `fn` is not annotated.
        # pyre-fixme[3]: Return type must be annotated.
        # pyre-fixme[2]: Parameter must be annotated.
        def decorated(*args, **kwargs):
            fn_args = inspect.getcallargs(fn, *args, **kwargs)
            return retry_fn(
                lambda: fn(*args, **kwargs),
                is_exception_retryable,
                delays=delays,
                what=format_msg.format(**fn_args),
                log_exception=log_exception,
            )

        return decorated

    return wrapper


async def async_retry_fn(
    retryable_fn: Callable[[], T],
    is_exception_retryable: Optional[Callable[[Exception], bool]] = None,
    *,
    delays: Iterable[float],
    what: str,
    log_exception: bool = True,
) -> T:
    """Similar to retry_fn except the function is executed asynchronously.
    See retry_fn docblock for details.
    """
    for i, delay in enumerate(delays):
        try:
            # pyre-fixme[12]: Expected an awaitable but got `T`.
            return await retryable_fn()
        except Exception as e:
            if is_exception_retryable and not is_exception_retryable(e):
                raise
            log.log(
                logging.ERROR if log_exception else logging.DEBUG,
                # pyre-fixme[6]: Expected `Sized` for 1st param but got
                #  `Iterable[float]`.
                f"\n\n[Retry {i + 1} of {len(delays)}] {what} -- waiting "
                f"{delay} seconds.\n\n",
                exc_info=log_exception,
            )
            time.sleep(delay)
    return (
        # pyre-fixme[12]: Expected an awaitable but got `T`.
        await retryable_fn()
    )  # With 0 retries, we should still run the function.


# pyre-fixme[3]: Return type must be annotated.
def async_retryable(
    format_msg: str,
    delays: Iterable[float],
    *,
    is_exception_retryable: Optional[Callable[[Exception], bool]] = None,
    log_exception: bool = True,
):
    """Decorator used to retry an asynchronous function if exceptions are
    thrown. `format_msg` should be a format string that can access any args
    provided to the decorated function. `delays` are the delays between
    retries, in seconds. `is_exception_retryable` and `log_exception` are
    forwarded to `async_retry_fn`, see its docblock.
    """
    # Prevent aliasing, iterator exhaustion, and other weirdness.
    # Indeterminate retry would require changing the API anyway.
    delays = list(delays)

    # pyre-fixme[3]: Return type must be annotated.
    # pyre-fixme[2]: Parameter must be annotated.
    def wrapper(fn):
        @wraps(fn)
        # pyre-fixme[53]: Captured variable `fn` is not annotated.
        # pyre-fixme[3]: Return type must be annotated.
        # pyre-fixme[2]: Parameter must be annotated.
        async def decorated(*args, **kwargs):
            fn_args = inspect.getcallargs(fn, *args, **kwargs)
            return await async_retry_fn(
                lambda: fn(*args, **kwargs),
                is_exception_retryable,
                delays=delays,
                what=format_msg.format(**fn_args),
                log_exception=log_exception,
            )

        return decorated

    return wrapper


def kernel_version() -> Tuple[int, int]:
    """
    Parse the current running kernel version and return a tuple representing
    the (MAJOR, MINOR) version.
    """
    m = re.match(r"(\d+)\.(\d+)\.\d+.*", _mockable_platform_release())
    if not m or len(m.groups()) != 2:
        raise ValueError(
            f"Invalid kernel version format '{platform.release()}'"
        )
    return int(m.group(1)), int(m.group(2))


class AsyncCompletedProc(NamedTuple):
    args: List[Union[str, bytes]]
    returncode: int
    stdout: Optional[bytes]
    stderr: Optional[bytes]

    def check_returncode(self) -> None:
        if self.returncode != 0:
            raise subprocess.CalledProcessError(
                returncode=self.returncode,
                cmd=self.args,
            )


async def async_run(
    cmd: List[Union[str, bytes]],
    input: Optional[bytes] = None,
    check: bool = True,
    # pyre-fixme[2]: Parameter must be annotated.
    **kwargs,
) -> AsyncCompletedProc:
    """Helper function to run an async subprocess and report the result in a
    canonical format.
    """
    if input is not None:
        assert kwargs.get("stdin") == asyncio.subprocess.PIPE, (
            "You must set `stdin=asyncio.subprocess.PIPE` for the provided "
            "`input` to be sent to the process' stdin."
        )
    proc = await asyncio.create_subprocess_exec(*cmd, **kwargs)
    stdout, stderr = await proc.communicate(input)
    ret = AsyncCompletedProc(
        args=cmd,
        returncode=not_none(proc.returncode),
        stdout=stdout,
        stderr=stderr,
    )
    if check:
        ret.check_returncode()
    return ret
