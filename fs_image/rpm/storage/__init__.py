#!/usr/bin/env python3

from .storage import Storage, StorageInput, StorageOutput

__all__ = [Storage, StorageInput, StorageOutput]

# Register implementations with Storage
from . import filesystem_storage  # noqa: F401
try:
    # Import FB-specific implementations if available
    from . import facebook  # noqa: F401
except ImportError:  # pragma: no cover
    pass
