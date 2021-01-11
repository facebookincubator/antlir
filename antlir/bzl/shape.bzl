# @lint-ignore-every BUCKRESTRICTEDSYNTAX
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

`shape.python_data` dumps a shape object to a raw python source file. This
is useful for some cases where a python_binary is expected to be fully
self-contained, but still require some build-time information. It is also
useful in cases when shapes are being dynamically generated based on inputs
to a macro. See the docblock of the function for an example.

## Naming Conventions
Shape types should be named with a suffix of '_t' to denote that it is a
shape type.
Shape instances should conform to whatever convention is used where they are
declared (usually snake_case variables).

## Example usage

Inspired by `image_actions/mount.bzl`:
```
mount_t = shape.shape(
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

mount = shape.new(
    mount_t,
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
load(":oss_shim.bzl", "buck_genrule", "python_library", "target_utils", "third_party")
load(":sha256.bzl", "sha256_b64")
load(":structs.bzl", "structs")
load(":target_helpers.bzl", "normalize_target")

_SERIALIZING_LOCATION_MSG = (
    "shapes with layer/target fields cannot safely be serialized in the" +
    " output of a buck target.\n" +
    "For buck_genrule uses, consider passing an argument with the (shell quoted)" +
    " result of 'shape.do_not_cache_me_json'\n" +
    "For unit tests, consider setting an environment variable with the same" +
    " JSON string"
)

_NO_DEFAULT = struct(no_default = True)

def _python_type(t):
    if t == int:
        return "int"
    if t == bool:
        return "bool"
    if t == str:
        return "str"
    if _is_collection(t):
        if t.collection == dict:
            k, v = t.item_type
            return "Mapping[{}, {}]".format(_python_type(k), _python_type(v))
        if t.collection == list:
            # list input is codegened as a homogenous tuple so that the
            # resulting field in the python class reflects the readonly nature
            # of the source
            return "Tuple[{}, ...]".format(_python_type(t.item_type))
        if t.collection == tuple:
            return "Tuple[{}]".format(", ".join([_python_type(x) for x in t.item_type]))
    if _is_field(t):
        python_type = _python_type(t.type)
        if t.optional:
            python_type = "Optional[{}]".format(python_type)
        return python_type
    if _is_shape(t):
        # deterministically name the class based on the shape field names and types
        # to allow for buck caching and proper starlark runtime compatibility
        return "_" + sha256_b64(
            str({key: _python_type(field) for key, field in t.fields.items()}),
        ).replace("-", "_")

    # If t is a string, then it should be the name of a type that will exist in
    # the Shape generated code context
    if types.is_string(t):
        return t
    fail("unknown type {}".format(t))  # pragma: no cover

def _check_type(x, t):
    """Check that x is an instance of t.
    This is a little more complicated than `isinstance(x, t)`, and supports
    more use cases. _check_type handles primitive types (bool, int, str),
    shapes and collections (dict, list, tuple).

    Return: None if successful, otherwise a str to be passed to `fail` at a
                site that has more context for the user
    """
    if t == int:
        if types.is_int(x):
            return None
        return "expected int, got {}".format(x)
    if t == bool:
        if types.is_bool(x):
            return None
        return "expected bool, got {}".format(x)
    if t == str:
        if types.is_string(x):
            return None
        return "expected str, got {}".format(x)
    if t == "Path":
        return _check_type(x, str)
    if t == "Target" or t == "LayerTarget":
        type_error = _check_type(x, str)
        if not type_error:
            # If parsing the target works, we don't have an error
            if target_utils.parse_target(x):
                return None
        else:
            return type_error
    if _is_field(t):
        if t.optional and type(x) == type(None):
            return None
        return _check_type(x, t.type)
    if _is_shape(t):
        for name, field in t.fields.items():
            type_error = _check_type(getattr(x, name, None), field)
            if type_error:
                return "{}: {}".format(name, type_error)
        return None
    if _is_collection(t):
        return _check_collection_type(x, t)

    return "unsupported type {}".format(t)  # pragma: no cover

def _check_collection_type(x, t):
    if t.collection == dict:
        if not types.is_dict(x):
            return "{} is not dict".format(x)
        key_type, val_type = t.item_type
        for key, val in x.items():
            key_type_error = _check_type(key, key_type)
            if key_type_error:
                return "key: " + key_type_error
            val_type_error = _check_type(val, val_type)
            if val_type_error:
                return "val: " + val_type_error
        return None
    elif t.collection == list:
        if not types.is_list(x) and not types.is_tuple(x):
            return "{} is not list".format(x)
        for i, val in enumerate(x):
            type_error = _check_type(val, t.item_type)
            if type_error:
                return "item {}: {}".format(i, type_error)
        return None
    elif t.collection == tuple:
        if not types.is_list(x) and not types.is_tuple(x):
            return "{} is not tuple".format(x)
        for i, (val, item_type) in enumerate(zip(x, t.item_type)):
            type_error = _check_type(val, item_type)
            if type_error:
                return "item {}: {}".format(i, type_error)
        return None
    return "unsupported collection type {}".format(t.collection)  # pragma: no cover

def _shapes_for_field(field_or_type):
    # recursively codegen classes for every shape that is contained in this
    # field, or any level of nesting beneath
    src = []
    if _is_field(field_or_type):
        field = field_or_type
        if _is_shape(field.type):
            src.extend(_codegen_shape(field.type))
        if _is_collection(field.type):
            item_types = []

            # some collections have multiple types and some have only one
            if types.is_list(field.type.item_type) or types.is_tuple(field.type.item_type):
                item_types = list(field.type.item_type)
            else:
                item_types = [field.type.item_type]

            for t in item_types:
                src.extend(_shapes_for_field(t))
    elif _is_shape(field_or_type):
        src.extend(_codegen_shape(field_or_type))
    return src

def _codegen_field(name, field):
    # for nested shapes, the class definitions must be listed in the body
    # before the fields, so that forward references are avoided
    src = []
    python_type = _python_type(field)
    src.extend(_shapes_for_field(field))

    if field.default == _NO_DEFAULT:
        src.append("{}: {}".format(name, python_type))
    else:
        default_repr = repr(field.default)
        if structs.is_struct(field.default):
            default_repr = "{}(**{})".format(python_type, repr(structs.to_dict(field.default)))
        src.append("{}: {} = {}".format(name, python_type, default_repr))
    return src

def _codegen_shape(shape, classname = None):
    if classname == None:
        classname = _python_type(shape)
    src = [
        "class {}(Shape):".format(classname),
        "  __GENERATED_SHAPE__ = True",
    ]

    for name, field in shape.fields.items():
        src.extend(["  " + line for line in _codegen_field(name, field)])
    return src

def _field(type, optional = False, default = _NO_DEFAULT):
    if optional and default == _NO_DEFAULT:
        default = None
    return struct(
        type = type,
        optional = optional,
        default = default,
    )

def _is_field(x):
    return structs.is_struct(x) and sorted(structs.to_dict(x).keys()) == sorted(["type", "optional", "default"])

def _dict(key_type, val_type, **field_kwargs):
    return _field(
        type = struct(
            collection = dict,
            item_type = (key_type, val_type),
        ),
        **field_kwargs
    )

def _list(item_type, **field_kwargs):
    return _field(
        type = struct(
            collection = list,
            item_type = item_type,
        ),
        **field_kwargs
    )

def _tuple(*item_types, **field_kwargs):
    return _field(
        type = struct(
            collection = tuple,
            item_type = item_types,
        ),
        **field_kwargs
    )

def _is_collection(x):
    return structs.is_struct(x) and sorted(structs.to_dict(x).keys()) == sorted(["collection", "item_type"])

def _path(**field_kwargs):
    return _field(type = "Path", **field_kwargs)

# A target is special kind of Path in that it will be resolved to an on-disk location
# when the shape is rendered to json.  But when the shape instance is being
# used in bzl macros, the field will be a valid buck target.
def _target(**field_kwargs):
    return _field(type = "Target", **field_kwargs)

def _layer(**field_kwargs):
    return _field(type = "LayerTarget", **field_kwargs)

def _shape(**fields):
    """
    Define a new shape type with the fields as given by the kwargs.

    Example usage:
    ```
    shape.shape(hello=str)
    ```
    """
    for name, f in fields.items():
        # transparently convert fields that are just a type have no options to
        # the rich field type for internal use
        if not hasattr(f, "type"):
            fields[name] = _field(f)
    return struct(
        fields = fields,
        # for external usage, make the fields top-level attributes
        **{key: f.type for key, f in fields.items()}
    )

def _is_shape(x):
    if not structs.is_struct(x):
        return False
    if not hasattr(x, "fields"):
        return False
    return sorted(structs.to_dict(x).keys()) == sorted(["fields"] + list(x.fields.keys()))

def _shape_defaults_dict(shape):
    defaults = {}
    for key, field in shape.fields.items():
        if field.default != _NO_DEFAULT:
            defaults[key] = field.default
    return defaults

def _new_shape(shape, **fields):
    """
    Type check and instantiate a struct of the given shape type using the
    values from the **fields kwargs.

    Example usage:
    ```
    example_t = shape.shape(hello=str)
    example = shape.new(example_t, hello="world")
    ```
    """
    with_defaults = _shape_defaults_dict(shape)
    with_defaults.update(fields)
    instance = struct(**with_defaults)
    type_error = _check_type(instance, shape)
    if type_error:
        fail(type_error)
    return instance

def _loader(name, shape, classname = "shape", **kwargs):  # pragma: no cover
    """codegen a fully type-hinted python source file to load the given shape"""
    if not _is_shape(shape):
        fail("expected shape type, got {}".format(shape))
    python_src = "from typing import *\nfrom antlir.shape import *\n"
    python_src += "\n".join(_codegen_shape(shape, classname))
    buck_genrule(
        name = "{}.py".format(name),
        out = "unused.py",
        cmd = "echo {} > $OUT".format(shell.quote(python_src)),
        # Antlir users should not directly use `shape`, but we do use it
        # as an implementation detail of "builder" / "publisher" targets.
        antlir_rule = "user-internal",
    )
    python_library(
        name = name,
        srcs = {":{}.py".format(name): "{}.py".format(name)},
        deps = ["//antlir:shape"],
        # Antlir users should not directly use `shape`, but we do use it
        # as an implementation detail of "builder" / "publisher" targets.
        antlir_rule = "user-internal",
        **kwargs
    )
    return normalize_target(":" + name)

# This will traverse all of the elements of the provided shape instance looking for
# fields that are of type `Target` and then replace the value with a struct that
# has the target name and it's on-disk path generated via a `$(location )` macro.
#
# Note: This is not covered by the python unittest coverage metric because the only
# callsite is also not covered.  However, this is exercised by the tests
# via the actual shape types being built, loaded, and tested.
def _translate_targets(val, t):  # pragma: no cover
    if _is_shape(t):
        new = {}
        for name, field in t.fields.items():
            new[name] = _translate_targets(getattr(val, name, None), field)
        return struct(**new)
    elif _is_field(t):
        if t.optional and type(val) == type(None):
            return None
        return _translate_targets(val, t.type)
    elif _is_collection(t):
        if t.collection == dict:
            return {
                k: _translate_targets(v, t.item_type)
                for k, v in val.items()
            }
        elif t.collection == list or t.collection == tuple:
            return [
                _translate_targets(v, t.item_type)
                for v in val
            ]
        else:
            return None
    elif t in ("Target", "LayerTarget"):
        return struct(
            name = val,
            path = "$(location {})".format(val),
        )
    else:
        return val

def _type_has_location(t):
    if _is_field(t):
        if t.type in ("Target", "LayerTarget"):
            return True
        return _type_has_location(t.type)
    if _is_collection(t):
        if t.collection == dict:
            kt, vt = t.item_type
            return _type_has_location(kt) or _type_has_location(vt)
        elif t.collection == list:
            return _type_has_location(t.item_type)
        elif t.collection == tuple:
            for it in t.item_type:
                if _type_has_location(it):
                    return True
    if not _is_shape(t):
        return False
    for name, field in t.fields.items():
        if _type_has_location(field):
            return True
    return False  # pragma: no cover

def _python_data(
        name,
        instance,
        shape,
        module = None,
        classname = "shape",
        **python_library_kwargs):  # pragma: no cover
    """
    Codegen a static shape data structure that can be directly 'import'ed by
    Python. The object is available under the name "data". A common use case
    is to call shape.python_data inline in a target's `deps`, with `module`
    (defaults to `name`) then representing the name of the module that can be
    imported in the underlying file.

    Example usage:
    ```
    python_binary(
        name = provided_name,
        deps = [
            shape.python_data(
                name = "bin_bzl_args",
                instance = shape.new(
                    some_shape_t,
                    var = input_var,
                ),
                shape = some_shape_t,
            ),
        ],
        ...
    )
    ```

    can then be imported as:

        from .bin_bzl_args import data
    """
    if _type_has_location(shape):
        fail(_SERIALIZING_LOCATION_MSG)
    python_src = "from typing import *\nfrom antlir.shape import *\n"
    python_src += "\n".join(_codegen_shape(shape, classname))
    python_src += "\ndata = {classname}.parse_raw('{shape_json}')".format(
        classname = classname,
        shape_json = instance.to_json(),
    )

    if not module:
        module = name

    buck_genrule(
        name = "{}.py".format(name),
        out = "unused.py",
        cmd = "echo {} >> $OUT".format(shell.quote(python_src)),
        # Antlir users should not directly use `shape`, but we do use it
        # as an implementation detail of "builder" / "publisher" targets.
        antlir_rule = "user-internal",
    )
    python_library(
        name = name,
        srcs = {":{}.py".format(name): "{}.py".format(module)},
        deps = [
            "//antlir:shape",
            third_party.library("pydantic", platform = "python"),
        ],
        # Antlir users should not directly use `shape`, but we do use it
        # as an implementation detail of "builder" / "publisher" targets.
        antlir_rule = "user-internal",
        **python_library_kwargs
    )
    return normalize_target(":" + name)

def _json_file(name, instance, shape):  # pragma: no cover
    """
    Serialize the given shape instance to a JSON file that can be used in the
    `resources` section of a `python_binary` or a `$(location)` macro in a
    `buck_genrule`.

    Warning: this will fail to serialize any shape type that contains a
    reference to a target location, as that cannot be safely cached by buck.
    """
    if _type_has_location(shape):
        fail(_SERIALIZING_LOCATION_MSG)

    buck_genrule(
        name = name,
        out = "out.json",
        cmd = "echo {} > $OUT".format(shell.quote(instance.to_json())),
        # Antlir users should not directly use `shape`, but we do use it
        # as an implementation detail of "builder" / "publisher" targets.
        antlir_rule = "user-internal",
    )
    return normalize_target(":" + name)

def _do_not_cache_me_json(instance, shape):
    """
    Serialize the given shape instance to a JSON string, which is the only
    way to safely refer to other Buck targets' locations in the case where
    the binary being invoked with a certain shape instance is cached.

    Warning: Do not ever put this into a target that can be cached, it should
    only be used in cmdline args or environment variables.
    """
    instance = _translate_targets(instance, shape)
    return instance.to_json()

shape = struct(
    shape = _shape,
    new = _new_shape,
    field = _field,
    dict = _dict,
    list = _list,
    tuple = _tuple,
    path = _path,
    target = _target,
    layer = _layer,
    loader = _loader,
    json_file = _json_file,
    python_data = _python_data,
    as_dict = structs.to_dict,
    do_not_cache_me_json = _do_not_cache_me_json,
)
