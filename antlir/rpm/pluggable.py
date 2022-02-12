#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"See `Storage` subclasses for examples of plugins."
import inspect
import json
from typing import Any, Dict, Mapping


class Pluggable:
    """
    If C inherits from Pluggable, then C's subclasses must be declared with
    a class kwarg of `plugin_kind`.  That string must be unique among the
    subclasses of C.

    You can then use `C.make(kind=...)` to create plugins by kind, or
    `C.from_json({'kind': ...})` to create plugins from JSON configs.
    Because of the JSON-config feature, `__init__` for all plugins should
    accept only plain-old-data kwargs.
    """

    # pyre-fixme[9]: plugin_kind has type `str`; used as `None`.
    def __init_subclass__(cls, plugin_kind: str = None, **kwargs) -> None:
        super().__init_subclass__(**kwargs)
        # We're in the base of this family of plugins, set it up.
        if Pluggable in cls.__bases__:
            assert plugin_kind is None
            # pyre-fixme[11]: Annotation `cls` is not defined as a type.
            cls._pluggable_kind_to_cls: Mapping[str, cls] = {}
            cls._pluggable_base = cls
        else:  # Register plugin class on its base
            d = cls._pluggable_base._pluggable_kind_to_cls
            if plugin_kind in d:
                raise AssertionError(
                    f"{cls} and {d[plugin_kind]} have the same plugin kind"
                )
            d[plugin_kind] = cls
            cls._pluggable_kind = plugin_kind

    @classmethod
    def from_json(cls, json_cfg: Dict[str, Any]) -> "Pluggable":
        "Uniform parsing for Storage configs e.g. on the command-line."
        json_cfg["kind"]  # KeyError if not set, or if not a dict
        return cls.make(**json_cfg)

    @classmethod
    def make(cls, kind: str, **kwargs) -> "Pluggable":
        return cls._pluggable_base._pluggable_kind_to_cls[kind](**kwargs)

    @classmethod
    def argparse_json(cls, arg_json: str) -> Dict[str, str]:
        # Make bad JSON fail at argument parse-time
        json_cfg = json.loads(arg_json)
        cls._pluggable_base.from_json(json_cfg)
        return json_cfg

    @classmethod
    def add_argparse_arg(cls, parser, *args, help: str = "", **kwargs) -> None:
        plugins = "; ".join(
            f"""`{n}` taking {', '.join(
                f'`{p}`' for p in list(
                    inspect.signature(c.__init__).parameters.values()
                )[1:]
            ) or 'no args'}"""
            for n, c in cls._pluggable_base._pluggable_kind_to_cls.items()
        )
        parser.add_argument(
            *args,
            type=cls.argparse_json,
            help=f'{help}A JSON dictionary containing the key "kind", which '
            f"identifies a {cls._pluggable_base.__name__} subclass, plus "
            f'"key", which is a user-specified [-_a-zA-Z0-9]+ string '
            "that marks all storage IDs emitted by this instantiation of "
            "the storage engine. Lastly, the JSON may contain additional "
            "keyword arguments for that class. Available plugins: "
            f"{plugins}.",
            **kwargs,
        )
