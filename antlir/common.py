#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"Utilities to make Python systems programming more palatable."
import inspect
import logging
import os
import platform
import sys
import time
from functools import wraps
from typing import Callable, Iterable, Optional, TypeVar


T = TypeVar("T")
_mockable_retry_fn_sleep = time.sleep
_mockable_platform_release = platform.release


# It's possible that `get_logger` is obtained, **and** logs, before
# `init_logging` is called.  Such usage is a minor bug; rather than hide the
# bug by never showing the debug logs, we'll make those logs visible.
_INITIALIZED_LOGGING = False
_ANTLIR_ROOT_LOGGER = "antlir"


class ColorFormatter(logging.Formatter):
    _base_fmt = (
        "\x1b[90m %(asctime)s.%(msecs)03d %(process)d %(filename)s:%(lineno)d "
        "\x1b[0m%(message)s"
    )
    _level_to_prefix = {
        logging.DEBUG: "\x1b[37mD",  # White
        logging.INFO: "\x1b[94mI",  # Blue
        logging.WARNING: "\x1b[93mW",  # Yellow
        logging.ERROR: "\x1b[91mE",  # Red
        logging.CRITICAL: "\x1b[95mF",  # Magenta
    }

    def __init__(self) -> None:
        super().__init__(datefmt="%Y%m%d %H:%M:%S")

    def format(self, record) -> str:
        try:
            self._style._fmt = self._level_to_prefix[record.levelno] + self._base_fmt
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
    logger.setLevel(level)

    # The first time around, just set up the stream handler & formatter --
    # this will be inherited by all `get_logger` instances.
    if not _INITIALIZED_LOGGING:
        _INITIALIZED_LOGGING = True
        hdlr = logging.StreamHandler()
        hdlr.setFormatter(ColorFormatter())
        logger.addHandler(hdlr)


def get_logger():
    calling_file = os.path.basename(inspect.getframeinfo(sys._getframe(1)).filename)
    # Strip extension from name of logger
    if calling_file.endswith(".py"):
        calling_file = calling_file[: -len(".py")]
    logger = logging.getLogger(_ANTLIR_ROOT_LOGGER + "." + calling_file)
    return logger


log = get_logger()


def not_none(var: Optional[T], var_name: str = "", detail: Optional[str] = None) -> T:
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

    def wrapper(fn):
        @wraps(fn)
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

    def wrapper(fn):
        @wraps(fn)
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
