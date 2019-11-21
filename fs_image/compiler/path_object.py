#!/usr/bin/env python3
import os

from .enriched_namedtuple import metaclass_new_enriched_namedtuple


class PathObject(type):
    'Base metaclass for the Requires & Provides hierarchies. Both are '
    'enriched namedtuples that have an image-absolute path.'

    def __new__(metacls, classname, bases, dct):

        def customize_fields(kwargs):
            # Normalize paths as image-absolute. This is crucial since we
            # will use `path` as a dictionary key.
            kwargs['path'] = os.path.normpath(
                # The `lstrip` is needed because `normpath does not
                # normalize away leading slashes: //b/c
                os.path.join('/', kwargs['path'].lstrip('/'))
            )
            return kwargs

        return metaclass_new_enriched_namedtuple(
            __class__,
            ['path'],
            metacls, classname, bases, dct,
            customize_fields,
        )
