# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.
"""
shape.bzl provides a convenient strongly-typed bridge from Buck bzl parse
time to Python runtime.

## Shape objects
Shape objects are immutable instances of a shape type, that have been
validated to match the shape type spec as described below.

## Shape Types
Shape types are a collection of strongly typed fields that can be validated
at Buck parse time (by `shape.new`) and at Python runtime (by `shape.loader`
implementations).

## Field Types
A shape field is a named member of a shape type. There are a variety of field
types available:
  primitive types (bool, int, float, str)
  other shapes
  homogenous lists of a single `field` element type
  dicts with homogenous key `field` types and homogenous `field` value type
  heterogenous tuples with `field` element types

## Optional and Defaulted Fields
By default, fields are required to be set at instantiation time
(`shape.new`).

Fields declared with `shape.field(..., default='val')` do not have to be
instantiated explicitly.

Additionally, fields can be marked optional by using the `optional` kwarg in
`shape.field` (or any of the collection field types: `shape.list`,
`shape.tuple`, or `shape.dict`).

For example, `shape.field(int, optional=True)` denotes an integer field that
may or may not be set in a shape object.

Obviously, optional fields are still subject to the same type validation as
non-optional fields, but only if they have a non-None value.

## Loaders
`shape.loader` codegens a type-hinted Python library that is capable of
parsing and validating a shape object at runtime.
The return value of shape.loader is the fully-qualified name of the
`python_library` rule that contains the implementation of this loader.

## Serialization formats
shape.bzl provides two mechanisms to pass shape objects to Python runtime code.

`shape.json_file` dumps a shape object to an output file. This can be read
from a file or resource, using `read_resource` or `read_file` of the
generated loader class.

`shape.python_file` dumps a shape object to a raw python source file. This is
useful for some cases where a python_binary is expected to be fully
self-contained, but still require some build-time information. It is also useful
in cases when shapes are being dynamically generated based on inputs to a macro.
See the docblock of the function for an example.

## Example usage

Inspired by `image_actions/mount.bzl`:
```
mount = shape.shape(
    mount_config=shape.shape(
        build_source=shape.shape(
            source=str,
            type=str,
        ),
        default_mountpoint=str,
        is_directory=bool,
    ),
    mountpoint = shape.field(str, optional=True),
    target = shape.field(str, optional=True),
)

mount_instance = shape.new(
    mount,
    mount_config=shape.new(
        mount.mount_config,
        build_source=shape.new(
            mount.mount_config.build_source,
            source="/etc/fbwhoami",
            type="host",
        ),
        default_mountpoint="/etc/fbwhoami",
        is_directory=False,
    ),
)
```

See tests/shape_test.bzl for full example usage and selftests.
"""

load("@bazel_skylib//lib:shell.bzl", "shell")
load("@bazel_skylib//lib:types.bzl", "types")
load(":oss_shim.bzl", "buck_genrule", "python_library")
load(":sha256.bzl", "sha256_b64")
load(":structs.bzl", "structs")
load(":target_helpers.bzl", "normalize_target")

_NO_DEFAULT = object()

def _is_type(x):
    return type(x) == type

def _is_field(x):
    return hasattr(x, "_field")

def _is_shape(x):
    return hasattr(x, "_shape")

def _is_shape_instance(x):
    return hasattr(x, "_shape_type")

def _get_src(x):
    return getattr(x, "python_src", [])

def _isinstance(x, t):
    if _is_field(t):
        t = t.starlark_type
    if _is_shape_instance(x):
        return x._shape_type == t
    return type(x) == t

def _validate_shape(shape, data):
    if structs.is_struct(data):
        data = structs.to_dict(data)
    if not types.is_dict(data):
        return "expected dict, got '{}'".format(data)
    for key, field_spec in shape.fields.items():
        if (
            not field_spec.optional and
            field_spec.default == _NO_DEFAULT and
            key not in data
        ):
            return "{}: missing required field".format(key)
        error_msg = field_spec.validate(
            field_spec,
            data.get(
                key,
                None if field_spec.default == _NO_DEFAULT else field_spec.default,
            ),
        )
        if error_msg:
            return "{}: {}".format(key, error_msg)
    for given_key in data.keys():
        if given_key not in shape.fields:
            return "{}: field does not exist in shape definition".format(given_key)
    return ""

def _validate_shape_field(spec, data):
    if not _isinstance(data, spec.starlark_type):
        return "expected shape '{}', got '{}".format(spec.starlark_type, data)
    return ""

def _to_field(field_or_type, **field_kwargs):
    """convert raw types to fields, leaving existing field definitions alone"""
    if _is_field(field_or_type):
        return field_or_type
    if _is_shape(field_or_type):
        return _field(
            python_type = field_or_type.__name__,
            starlark_type = field_or_type,
            validate = _validate_shape_field,
            # shapes require an additional class definition on top of the
            # field member
            python_src = field_or_type.python_src,
            **field_kwargs
        )
    return _primitive_field(field_or_type, **field_kwargs)

def _define_shape(**fields):
    # expand top-level shape definitions as direct struct fields, so that they
    # don't need to be defined as standalone variables
    top_level_shape_fields = {key: val for key, val in fields.items()}
    fields = {key: _to_field(val) for key, val in fields.items()}

    # deterministically name the class based on the shape field names and types
    # to allow for buck caching and proper starlark runtime compatibility
    class_name = "_" + sha256_b64(
        str({key: field.python_type for key, field in fields.items()}),
    ).replace("-", "_")

    python_src = [
        "@shape_dataclass",
        "class {}(object, metaclass=ShapeMeta):".format(class_name),
    ]

    # fields with defaults must come after fields without default values
    fields_sorted_by_defaults = sorted([(f.default != _NO_DEFAULT, key, f) for key, f in fields.items()])
    for _, key, field in fields_sorted_by_defaults:
        python_src.extend(["  " + line for line in field.python_src])
        if field.default == _NO_DEFAULT:
            python_src.append("  {}: {}".format(key, field.python_type))
        else:
            python_src.append("  {}: {} = {}".format(key, field.python_type, repr(field.default)))
    python_src = [line for line in python_src if line.lstrip().rstrip()]
    return struct(
        _shape = True,
        fields = fields,
        validate = _validate_shape,
        python_src = python_src,
        __name__ = class_name,
        **top_level_shape_fields
    )

# structs serialize all their fields to the json string, but we don't want any
# of the internal typing information to be leaked into the output json, so it
# is "flattened" at struct instantiation time, preserving the original typing
# information for internal checks alongside a plain data representation for
# serialization
def _plain_data(x):
    if hasattr(x, "_data"):
        return x._data
    if types.is_list(x) or types.is_tuple(x):
        return [getattr(i, "_data", i) for i in x]
    if types.is_dict(x):
        return {k: getattr(v, "_data", v) for k, v in x.items()}
    return x

def _instantiate_shape(shape, **fields):
    error_msg = _validate_shape(shape, fields)
    if error_msg:
        fail(error_msg)
    plain_data = {k: _plain_data(v) for k, v in fields.items()}
    return struct(
        _shape_type = shape,
        # _data is internally used for all serialization, but expose the fields
        # as first-class members to enable easy access from starlark code after
        # the shape instance has been constructed and validated
        _data = struct(**plain_data),
        **plain_data
    )

def _primitive_field(type_, **field_kwargs):
    if not _is_type(type_):
        fail("field type '{}' is not a starlark type".format(type_))
    return _field(
        python_type = type_.__name__,
        starlark_type = type_,
        validate = _validate_primitive,
        **field_kwargs
    )

def _validate_primitive(spec, data):
    if not _isinstance(data, spec.starlark_type):
        return "'{}' is not '{}'".format(data, spec.starlark_type)
    return ""

def _field(python_type, starlark_type, validate, optional = False, default = _NO_DEFAULT, python_src = None, **kwargs):
    if optional:
        python_type = "typing.Optional[{}]".format(python_type)
        if default == _NO_DEFAULT:
            default = None
    return struct(
        _field = True,
        python_type = python_type,
        starlark_type = starlark_type,
        validate = _validate_optional if optional else validate,
        validate_non_none = validate,
        optional = optional,
        default = default,
        is_shape = _is_shape(starlark_type),
        python_src = python_src or [],
        **kwargs
    )

def _validate_optional(spec, data):
    if data == None:
        return ""
    return spec.validate_non_none(spec, data)

def _dict_field(key_type, val_type, **field_kwargs):
    if not _is_type(key_type):
        fail("dicts can only have primitives as keys", attr = "key_type")
    return _field(
        python_type = "typing.Mapping[{}, {}]".format(key_type.__name__, val_type.__name__),
        starlark_type = (key_type, val_type),
        validate = _validate_dict_field,
        val_type = val_type,
        python_src = _get_src(val_type),
        **field_kwargs
    )

def _validate_dict_field(spec, data):
    if not types.is_dict(data):
        return "expected dict, got '{}'".format(data)
    key_type, val_type = spec.starlark_type
    for key, val in data.items():
        if not _isinstance(key, key_type):
            return "key '{}' is not '{}'".format(key, key_type)
        if not _isinstance(val, val_type):
            return "val '{}' (from key '{}') is not '{}'".format(val, key, val_type)
    return ""

def _list_field(item_type, set_ = False, **field_kwargs):
    python_type = "typing.FrozenSet" if set_ else "typing.Sequence"
    item_type = _to_field(item_type)
    return _field(
        python_type = "{}[{}]".format(python_type, item_type.python_type),
        starlark_type = item_type.starlark_type,
        validate = _validate_list_field,
        item_type = item_type.starlark_type,
        python_src = _get_src(item_type),
        **field_kwargs
    )

def _validate_list_field(spec, data):
    if not types.is_list(data) and not types.is_tuple(data):
        return "expected list, got '{}'".format(data)
    item_type = spec.starlark_type
    for i, item in enumerate(data):
        if not _isinstance(item, item_type):
            return "item '{}' (at {}) is not '{}'".format(item, i, item_type)
    return ""

def _set_field(*args, **kwargs):
    return _list_field(set_ = True, *args, **kwargs)

def _tuple_field(*item_types, **field_kwargs):
    item_type_names = ",".join([t.__name__ for t in item_types])
    python_src = []
    for t in item_types:
        python_src.extend(_get_src(t))
    return _field(
        python_type = "typing.Tuple[{}]".format(item_type_names),
        starlark_type = item_types,
        validate = _validate_tuple_field,
        item_types = item_types,
        python_src = python_src,
        **field_kwargs
    )

def _validate_tuple_field(spec, data):
    if not types.is_tuple(data) and not types.is_list(data):
        return "expected tuple, got '{}'".format(data)
    field_types = spec.starlark_type
    if len(field_types) != len(data):
        return "expected {} items, got {}".format(len(field_types), len(data))
    for i, (item_type, item) in enumerate(zip(field_types, data)):
        if not _isinstance(item, item_type):
            return "item '{}' (at {}) is not '{}'".format(item, i, item_type)
    return ""

def _loader_src(shape, classname):
    """codegen a fully type-hinted python source file to load the given shape"""
    python_src = """import importlib.resources
import json
import os
import pathlib
import typing

# This generated code is used by both shape.python_file and
# shape.python_loader, which have different constraints around how things can
# be imported.
# We prefer to import from the shared antlir.shape library, but
# shape.python_file produces standalone python source that has the library
# copied in.
try:
    from antlir.shape import *
except ImportError:
    pass
"""

    # this is heavily dependent on the generated code structure, but tests will
    # easily catch if this breaks
    shape.python_src[1] = "class {}(object, metaclass=ShapeMeta):".format(classname)
    python_src += "\n".join(shape.python_src)

    python_src += """
  @classmethod
  def read_resource(cls, package: str, name: str) -> "{c}":
      with importlib.resources.open_text(package, name) as r:
          return cls.read_file(r)

  @classmethod
  def read_file(cls, f: typing.Union[os.PathLike, typing.IO]) -> "{c}":
      if isinstance(f, (str, pathlib.Path)):
          with open(f) as f:
              return cls(**json.load(f))
      return cls(**json.load(f))
""".format(c = classname)
    return python_src

def _loader(name, shape, classname = None, **kwargs):
    python_src = _loader_src(shape, classname or name)
    buck_genrule(
        name = "{}={}.py".format(name, name),
        out = "unused.py",
        cmd = "echo {} > $OUT".format(shell.quote(python_src)),
        # Antlir users should not directly use `shape`, but we do use it
        # as an implementation detail of "builder" / "publisher" targets.
        antlir_rule = "user-internal",
    )
    python_library(
        name = name,
        srcs = [":{}={}.py".format(name, name)],
        deps = ["//antlir:shape"],
        # Antlir users should not directly use `shape`, but we do use it
        # as an implementation detail of "builder" / "publisher" targets.
        antlir_rule = "user-internal",
        **kwargs
    )

def _python_file(name, shape):
    """Codegen a static shape data structure that can be directly 'import'ed by
    Python. The object is available under the name "data". A common use case is
    to use this in `srcs`, with `name` then representing the name of the module
    that can be imported in the underlying file.

    Example:

        python_binary(
            name = provided_name,
            srcs = [
                shape.python_file(
                    name = "bin_bzl_args",
                    shape = shape.new(
                        some_shape_t,
                        var = input_var,
                    ),
                ),
            ],
            ...
        )

    can then be imported as:

        from .bin_bzl_args import data
    """
    if not _is_shape_instance(shape):
        fail("'{}' is not a shape".format(shape), attr = "shape")
    python_src = _loader_src(shape._shape_type, "shape")
    json_str = shape._data.to_json()
    json_str = json_str.replace('"', '\\"')
    python_src += "\ndata = shape(**json.loads(\"{}\"))".format(json_str)

    # make it easier for direct inclusion in `srcs`
    name = "{}={}.py".format(name, name)
    buck_genrule(
        name = name,
        out = name,
        cmd = """
            cp $(location //antlir:shape.py) $OUT
            echo {} >> $OUT
        """.format(shell.quote(python_src)),
        # Antlir users should not directly use `shape`, but we do use it
        # as an implementation detail of "builder" / "publisher" targets.
        antlir_rule = "user-internal",
    )
    return normalize_target(":" + name)

def _json_file(name, shape):
    if not _is_shape_instance(shape):
        fail("'{}' is not a shape".format(shape), attr = "shape")
    buck_genrule(
        name = name,
        out = "out.json",
        cmd = "echo {} > $OUT".format(shell.quote(shape._data.to_json())),
        # Antlir users should not directly use `shape`, but we do use it
        # as an implementation detail of "builder" / "publisher" targets.
        antlir_rule = "user-internal",
    )
    return normalize_target(":" + name)

shape = struct(
    shape = _define_shape,
    new = _instantiate_shape,
    dict = _dict_field,
    field = _to_field,
    list = _list_field,
    set = _set_field,
    tuple = _tuple_field,
    loader = _loader,
    json_file = _json_file,
    python_file = _python_file,
)
