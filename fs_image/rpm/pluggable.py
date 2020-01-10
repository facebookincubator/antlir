#!/usr/bin/env python3
'See `Storage` subclasses for examples of plugins.'
import inspect
import json

from typing import Mapping


class Pluggable:
    '''
    If C inherits from Pluggable, then C's subclasses must be declared with
    a class kwarg of `plugin_kind`.  That string must be unique among the
    subclasses of C.

    You can then use `C.make(kind=...)` to create plugins by kind, or
    `C.from_json('{"kind": ...})` to create plugins from JSON configs.
    Because of the JSON-config feature, `__init__` for all plugins should
    accept only plain-old-data kwargs.
    '''

    def __init_subclass__(cls, plugin_kind: str=None, **kwargs):
        super().__init_subclass__(**kwargs)
        # We're in the base of this family of plugins, set it up.
        if Pluggable in cls.__bases__:
            assert plugin_kind is None
            cls._pluggable_kind_to_cls: Mapping[str, cls] = {}
            cls._pluggable_base = cls
        else:  # Register plugin class on its base
            d = cls._pluggable_base._pluggable_kind_to_cls
            if plugin_kind in d:
                raise AssertionError(
                    f'{cls} and {d[plugin_kind]} have the same plugin kind'
                )
            d[plugin_kind] = cls

    @classmethod
    def from_json(cls, json_cfg: str) -> 'Pluggable':
        'Uniform parsing for Storage configs e.g. on the command-line.'
        cfg = json.loads(json_cfg)
        cfg['kind']  # KeyError if not set, or if not a dict
        return cls.make(**cfg)

    @classmethod
    def make(cls, kind, **kwargs) -> 'Pluggable':
        return cls._pluggable_base._pluggable_kind_to_cls[kind](**kwargs)

    @classmethod
    def argparse_json(cls, arg_json: str) -> str:
        # Make bad JSON fail at argument parse-time
        cls._pluggable_base.from_json(arg_json)
        return arg_json

    @classmethod
    def add_argparse_arg(cls, parser, *args, help='', **kwargs):
        plugins = '; '.join(
            f'''`{n}` taking {', '.join(
                f'`{p}`' for p in list(
                    inspect.signature(c.__init__).parameters.values()
                )[1:]
            )}'''
                for n, c in cls._pluggable_base._pluggable_kind_to_cls.items()
        )
        parser.add_argument(
            *args,
            type=cls.argparse_json,
            help=f'{help}A JSON dictionary containing the key "kind", which '
                f'identifies a {cls._pluggable_base.__name__} subclass, plus '
                f'"key", which is a user-specified [-_a-zA-Z0-9]+ string '
                'that marks all storage IDs emitted by this instantiation of '
                'the storage engine. Lastly, the JSON may contain additional '
                'keyword arguments for that class. Available plugins: '
                f'{plugins}.',
            **kwargs
        )
