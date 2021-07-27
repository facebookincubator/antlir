# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

# @lint-ignore-every BUCKRESTRICTEDSYNTAX

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
  enums with string values
  unions via shape.union(type1, type2, ...)

If using a union, use the most specific type first as Pydantic will attempt to
coerce to the types in the order listed
(see https://pydantic-docs.helpmanual.io/usage/types/#unions) for more info.

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
    if _is_enum(t):
        return "_".join([str(v.capitalize()) for v in t.enum])
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
    if _is_union(t):
        type_names = [_python_type(union_t) for union_t in t.union_types]
        return "Union[{}]".format(", ".join(type_names))

    # If t is a string, then it should be the name of a type that will exist in
    # the Shape generated code context
    if types.is_string(t):
        return t
    fail("unknown type {}".format(t))  # pragma: no cover

# Poor man's debug pretty-printing. Better version coming on a stack.
def _pretty(x):
    return structs.to_dict(x) if structs.is_struct(x) else x

def _get_is_instance_error(val, t):
    if not _is_instance(val, t):
        return (
            (
                "{} is not an instance of {} -- note that structs & dicts " +
                "are NOT currently automatically promoted to shape"
            ).format(
                _pretty(val),
                _pretty(t),
            )
        )
    return None

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
    if _is_enum(t):
        if x in t.enum:
            return None
        return "expected one of {}, got {}".format(t.enum, x)
    if t == "Path":
        return _check_type(x, str)
    if t == "Target":
        type_error = _check_type(x, str)
        if not type_error:
            # If parsing the target works, we don't have an error
            if target_utils.parse_target(x):
                return None
        else:
            return type_error
    if _is_field(t):
        if t.optional and x == None:
            return None
        return _check_type(x, t.type)
    if _is_shape(t):
        # Don't need type-check the internals of `x` because we trust it to
        # have been type-checked at the time of construction.
        return _get_is_instance_error(x, t)
    if _is_collection(t):
        return _check_collection_type(x, t)
    if _is_union(t):
        _matched_type, error = _find_union_type(x, t)
        return error
    return "unsupported type {}".format(t)  # pragma: no cover

# Returns a mutually exclusive tuple:
#   ("matched type" or None, "error if no type matched" or None)
def _find_union_type(x, t):
    type_errors = []
    for union_t in t.union_types:
        type_error = _check_type(x, union_t)
        if type_error == None:
            return union_t, None
        type_errors.append(type_error)
    return None, "{} not matched in union {}: {}".format(
        x,
        t.union_types,
        "; ".join(type_errors),
    )

# Returns a mutually exclusive tuple:
#   ([tuple type, tuple element] or None, "type error" or None)
def _values_and_types_for_tuple(x, t):
    if not _is_collection(t) or t.collection != tuple:  # pragma: no cover
        # This is an assertion, not a user error.
        fail("{} is not a tuple type (value {})".format(_pretty(t), _pretty(x)))
    if not types.is_list(x) and not types.is_tuple(x):
        return None, "{} is not tuple".format(x)
    if len(x) != len(t.item_type):
        return None, "length of {} does not match {}".format(
            _pretty(x),
            _pretty(t.item_type),
        )

    # Explicit `list` since the tests run as Python, where `zip` is a generator
    values_and_types = list(zip(x, t.item_type))
    for i, (val, item_type) in enumerate(values_and_types):
        type_error = _check_type(val, item_type)
        if type_error:
            return None, "item {}: {}".format(i, type_error)
    return values_and_types, None

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
    if t.collection == list:
        if not types.is_list(x) and not types.is_tuple(x):
            return "{} is not list".format(x)
        for i, val in enumerate(x):
            type_error = _check_type(val, t.item_type)
            if type_error:
                return "item {}: {}".format(i, type_error)
        return None
    if t.collection == tuple:
        _values_and_types, error = _values_and_types_for_tuple(x, t)
        return error
    return "unsupported collection type {}".format(t.collection)  # pragma: no cover

def _nesting(type, path):
    return struct(
        type = type,
        path = path,
    )

def _codegen_type_aliases(nestings):
    return [
        "typeof_{field} = {type}".format(
            field="__".join(nesting.path),
            type=_python_type(nesting.type),
        )
        for nesting in nestings
    ]

def _nested_types(type, path = None):
    # recursively flatten a nesting of composed types, such that the result is
    # in reverse topological ordering, that is, deepest first
    basepath = path or []
    nestings = []

    # this is needed due to recursion on types inside collections
    type = type.type if _is_field(type) else type

    if _is_shape(type):
        for name, field in type.fields.items():
            path = basepath + [name]
            nestings.extend(_nested_types(field.type, path))
            nestings.append(_nesting(field.type, path))
    elif _is_collection(type):
        item_types = []
        if types.is_list(type.item_type) or types.is_tuple(type.item_type):
            item_types = list(type.item_type)
        else:
            item_types = [type.item_type]

        for i, t in enumerate(item_types):
            # when types are not associated to any field name, we use indexes
            path = basepath + [str(i)]
            nestings.extend(_nested_types(t, path))
            nestings.append(_nesting(t, path))

    return nestings

def _codegen_type(type, generated):
    src = []

    # avoid duplicates
    python_type = _python_type(type)
    if python_type in generated:
        return src
    else:
        generated[python_type] = True

    # nested shapes like `shape.list(Y)` with Y = `shape.list(X)` end up in the
    # recursion with Y (the nested shape) as a field, this gets around that
    type = type.type if _is_field(type) else type

    if _is_shape(type):
        src.extend(_codegen_shape(type))
    elif _is_collection(type):
        item_types = []

        # some collections have multiple types and some have only one
        if types.is_list(type.item_type) or types.is_tuple(type.item_type):
            item_types = list(type.item_type)
        else:
            item_types = [type.item_type]

        for t in item_types:
            src.extend(_codegen_type(t, generated))
    elif _is_enum(type):
        src.extend(_codegen_enum(type))

    return src

def _codegen_nested_classes(nestings):
    # assumes nestings are in the correct order (ie from `_nested_types`).
    # we also take care not to generate the same class more than once
    src = []
    generated = {}
    for nesting in nestings:
        src.extend(_codegen_type(nesting.type, generated))
    return src

def _codegen_field(name, field):
    src = []
    python_type = _python_type(field)
    if field.default == _NO_DEFAULT:
        src.append("{}: {}".format(name, python_type))
    else:
        default_repr = repr(field.default)
        if structs.is_struct(field.default):
            default_repr = "{}(**{})".format(python_type, repr(
                _as_serializable_dict(field.default),
            ))
        src.append("{}: {} = {}".format(name, python_type, default_repr))
    return src

def _codegen_shape(shape, classname = None):
    # generates a shape, but none of its nested types (if any)
    if classname == None:
        classname = _python_type(shape)
    src = [
        "class {}(Shape):".format(classname),
        "  __GENERATED_SHAPE__ = True",
    ]

    for name, field in shape.fields.items():
        src.extend(["  " + line for line in _codegen_field(name, field)])
    return src

def _codegen_enum(enum):
    classname = _python_type(enum)
    src = [
        "class {}(Enum):".format(classname),
    ]
    src.extend(["  {} = {}".format(value.upper(), repr(value)) for value in enum.enum])
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

def _is_union(x):
    return structs.is_struct(x) and sorted(structs.to_dict(x).keys()) == sorted(["union_types"])

def _union_type(*union_types):
    """
    Define a new union type that can be used when defining a field. Most
    useful when a union type is meant to be typedef'd and reused. To define
    a shape field directly, see shape.union.

    Example usage:
    ```
    mode_t = shape.union_t(int, str)  # could be 0o644 or "a+rw"

    type_a = shape.shape(mode=mode_t)
    type_b = shape.shape(mode=shape.field(mode_t, optional=True))
    ```
    """
    if len(union_types) == 0:
        fail("union must specify at one type")
    return struct(
        union_types = union_types,
    )

def _union(*union_types, **field_kwargs):
    return _field(
        type = _union_type(*union_types),
        **field_kwargs
    )

def _enum(*values, **field_kwargs):
    # since enum values go into class member names, they must be strings
    for val in values:
        if not types.is_string(val):
            fail("all enum values must be strings, got {}".format(_pretty(val)))
    return _field(
        type = struct(
            enum = tuple(values),
        ),
        **field_kwargs
    )

def _is_enum(t):
    return structs.is_struct(t) and sorted(structs.to_dict(t).keys()) == sorted(["enum"])

def _path(**field_kwargs):
    return _field(type = "Path", **field_kwargs)

# A target is special kind of Path in that it will be resolved to an on-disk location
# when the shape is rendered to json.  But when the shape instance is being
# used in bzl macros, the field will be a valid buck target.
def _target(**field_kwargs):
    return _field(type = "Target", **field_kwargs)

def _shape(**fields):
    """
    Define a new shape type with the fields as given by the kwargs.

    Example usage:
    ```
    shape.shape(hello=str)
    ```
    """
    for name, f in fields.items():
        # Avoid colliding with `__shape__`. Also, in Python, `_name` is private.
        if name.startswith("_"):
            fail("Shape field name {} must not start with _: {}".format(
                name,
                _pretty(fields),
            ))

        # transparently convert fields that are just a type have no options to
        # the rich field type for internal use
        if not hasattr(f, "type") or _is_union(f):
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

    for field, value in fields.items():
        if field not in shape.fields:
            fail("field `{}` is not defined in the shape".format(field))
        error = _check_type(value, shape.fields[field])
        if error:
            fail(error)

    return struct(__shape__ = shape, **with_defaults)

def _loader(name, shape, classname = "shape", **kwargs):  # pragma: no cover
    """codegen a fully type-hinted python source file to load the given shape"""
    if not _is_shape(shape):
        fail("expected shape type, got {}".format(shape))

    # generation order: innermost types -> outermost types -> aliases
    lines = []
    nested = _nested_types(shape)
    lines.extend(_codegen_nested_classes(nested))
    lines.extend(_codegen_shape(shape, classname))
    lines.extend(_codegen_type_aliases(nested))
    python_src = "from typing import *\nfrom antlir.shape import *\n"
    python_src += "\n".join(lines)

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

# Does a recursive (deep) copy of `val` which is expected to be of type
# `t` (in the `shape` sense of type compatibility).
#
# `opts` changes the output as follows:
#
#   - Set `opts.include_dunder_shape == False` to strip `__shape__` from the
#     resulting instance structs.  This is desirable when serializing,
#     because that field will e.g. fail with `struct.to_json()`.
#
#   - `opts.on_target_fields` has 3 possible values:
#
#     * "preserve": Leave the field as a `//target:path` string.
#
#     * "fail": Fails at Buck parse-time. Used for scenarios that cannot
#       reasonably support target -> buck output path resolution, like
#       `shape.json_file()`.  But, in the future, we should be able to
#       migrate these to a `target_tagger.bzl`-style approach.
#
#     * "uncacheable_location_macro"`, this will replace fields of
#       type `Target` with a struct that has the target name and its on-disk
#       path generated via a `$(location )` macro.  This MUST NOT be
#       included in cacheable Buck outputs.
def _recursive_copy_transform(val, t, opts):
    if _is_shape(t):
        error = _get_is_instance_error(val, t)
        if error:  # pragma: no cover -- an internal invariant, not a user error
            fail(error)
        new = {}
        for name, field in t.fields.items():
            new[name] = _recursive_copy_transform(
                # The `_is_instance` above will ensure that `getattr` succeeds
                getattr(val, name),
                field,
                opts,
            )
        if opts.include_dunder_shape:
            if val.__shape__ != t:  # pragma: no cover
                fail("__shape__ {} didn't match type {}".format(
                    _pretty(val.__shape__),
                    _pretty(t),
                ))
            new["__shape__"] = t
        return struct(**new)
    elif _is_field(t):
        if t.optional and val == None:
            return None
        return _recursive_copy_transform(val, t.type, opts)
    elif _is_collection(t):
        if t.collection == dict:
            return {
                k: _recursive_copy_transform(v, t.item_type[1], opts)
                for k, v in val.items()
            }
        elif t.collection == list:
            return [
                _recursive_copy_transform(v, t.item_type, opts)
                for v in val
            ]
        elif t.collection == tuple:
            values_and_types, error = _values_and_types_for_tuple(val, t)
            if error:  # pragma: no cover
                fail(error)
            return [
                _recursive_copy_transform(item_val, item_t, opts)
                for (item_val, item_t) in values_and_types
            ]

        # fall through to fail
    elif _is_union(t):
        matched_type, error = _find_union_type(val, t)
        if error:  # pragma: no cover
            fail(error)
        return _recursive_copy_transform(val, matched_type, opts)
    elif t == "Target":
        if opts.on_target_fields == "fail":
            fail(_SERIALIZING_LOCATION_MSG)
        elif opts.on_target_fields == "uncacheable_location_macro":
            return struct(
                name = val,
                path = "$(location {})".format(val),
            )
        elif opts.on_target_fields == "preserve":
            return val
        fail(
            # pragma: no cover
            "Unknown on_target_fields: {}".format(opts.on_target_fields),
        )
    elif t == int or t == bool or t == str or t == "Path" or _is_enum(t):
        return val
    fail(
        # pragma: no cover
        "Unknown type {} for {}".format(_pretty(t), _pretty(val)),
    )

def _safe_to_serialize_instance(instance):
    return _recursive_copy_transform(
        instance,
        instance.__shape__,
        struct(include_dunder_shape = False, on_target_fields = "fail"),
    )

def _python_data(
        name,
        instance,
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
            ),
        ],
        ...
    )
    ```

    can then be imported as:

        from .bin_bzl_args import data
    """
    shape = instance.__shape__
    instance = _safe_to_serialize_instance(instance)

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

def _json_file(name, instance, visibility = None):  # pragma: no cover
    """
    Serialize the given shape instance to a JSON file that can be used in the
    `resources` section of a `python_binary` or a `$(location)` macro in a
    `buck_genrule`.

    Warning: this will fail to serialize any shape type that contains a
    reference to a target location, as that cannot be safely cached by buck.
    """
    instance = _safe_to_serialize_instance(instance).to_json()
    buck_genrule(
        name = name,
        out = "out.json",
        cmd = "echo {} > $OUT".format(shell.quote(instance)),
        # Antlir users should not directly use `shape`, but we do use it
        # as an implementation detail of "builder" / "publisher" targets.
        antlir_rule = "user-internal",
        visibility = visibility,
    )
    return normalize_target(":" + name)

def _do_not_cache_me_json(instance):
    """
    Serialize the given shape instance to a JSON string, which is the only
    way to safely refer to other Buck targets' locations in the case where
    the binary being invoked with a certain shape instance is cached.

    Warning: Do not ever put this into a target that can be cached, it should
    only be used in cmdline args or environment variables.
    """
    return _recursive_copy_transform(
        instance,
        instance.__shape__,
        struct(
            include_dunder_shape = False,
            on_target_fields = "uncacheable_location_macro",
        ),
    ).to_json()

def _render_template(name, instance, template):  # pragma: no cover
    """
    Render the given Jinja2 template with the shape instance data to a file.

    Warning: this will fail to serialize any shape type that contains a
    reference to a target location, as that cannot be safely cached by buck.
    """
    _json_file(name + "--data.json", instance)

    buck_genrule(
        name = name,
        out = "unused",
        cmd = "$(exe {}-render) <$(location :{}--data.json) > $OUT".format(template, name),
        antlir_rule = "user-internal",
    )
    return normalize_target(":" + name)

# Asserts that there are no "Buck target" in the shape.  Contrast with
# `do_not_cache_me_json`.
#
# Converts a shape to a dict, as you would expected (field names are keys,
# values are scalars & collections as in the shape -- and nested shapes are
# also dicts).
def _as_serializable_dict(instance):
    return structs.to_dict(_safe_to_serialize_instance(instance))

# Do not use this outside of `target_tagger.bzl`.  Eventually, target tagger
# should be replaced by shape, so this is meant as a temporary shim.
#
# Unlike `as_serializable_dict`, does not fail on "Buck target" fields. Instead,
# these get represented as the target path (avoiding cacheability issues).
#
# target_tagger.bzl is the original form of matching target paths with their
# corresponding `$(location)`.  Ideally, we should fold this functionality
# into shape.  In the current implementation, it just needs to get the raw
# target path out of the shape, and nothing else.
def _as_dict_for_target_tagger(instance):
    return structs.to_dict(_recursive_copy_transform(
        instance,
        instance.__shape__,
        struct(
            include_dunder_shape = False,
            on_target_fields = "preserve",
        ),
    ))

# Returns True iff `instance` is a shape instance of any type.
def _is_any_instance(instance):
    return structs.is_struct(instance) and hasattr(instance, "__shape__")

# Returns True iff `instance` is a `shape.new(shape, ...)`.
def _is_instance(instance, shape):
    if not _is_shape(shape):
        fail("Checking if {} is a shape instance, but {} is not a shape".format(
            _pretty(instance),
            _pretty(shape),
        ))
    return (
        structs.is_struct(instance) and
        getattr(instance, "__shape__", None) == shape
    )

# Converts `shape.new(foo_t, x='a', y=shape.new(bar_t, z=3))` to
# `{'x': 'a', 'y': shape.new(bar_t, z=3)}`.
#
# The primary use-case is unpacking a shape in order to construct a modified
# variant.  E.g.
#
#   def new_foo(a, b=3):
#       if (a + b) % 1:
#           fail("a + b must be even, got {} + {}".format(a, b))
#       return shape.new(_foo_t, a=a, b=b, c=a+b)
#
#   def modify_foo(foo, ... some overrides ...):
#       d = shape.as_dict_shallow(instance)
#       d.update(... some overrides ...)
#       d.pop('c')
#       return new_foo(**d)
#
# Notes:
#   - This dict is NOT intended for serialization, since nested shape remain
#     as shapes, and are not converted to `dict`.
#   - There is no special treament for `shape.target` fields, they remain as
#     `//target:path` strings.
#   - `shape.new` is the mathematical inverse of `_as_dict_shallow`.  On the
#     other hand, we do not yet provide `_as_dict_deep`.  The latter would
#     NOT be invertible, since `shape` does not yet have a way of
#     recursively converting nested dicts into nested shapes.
def _as_dict_shallow(instance):
    return {
        field: getattr(instance, field)
        for field in instance.__shape__.fields
    }

shape = struct(
    shape = _shape,
    new = _new_shape,
    field = _field,
    dict = _dict,
    list = _list,
    tuple = _tuple,
    union = _union,
    union_t = _union_type,
    enum = _enum,
    path = _path,
    target = _target,
    loader = _loader,
    json_file = _json_file,
    python_data = _python_data,
    do_not_cache_me_json = _do_not_cache_me_json,
    render_template = _render_template,
    struct = struct,
    # There is no vanilla "as_dict" because:
    #
    #   (a) There are many different possible use-cases, and one size does
    #       not fit all.  The variants below handle the existing uses, but
    #       there can be more.  For example, if you want to mutate an
    #       existing shape, you currently cannot do that correctly without
    #       recursively constructing a new one.  We would need to provide a
    #       proper recursive "new from dict" to allow that to happen.
    #
    #   (b) It's usually the wrong tool for the job / a sign of tech debt.
    #       For example, it should be possible to convert all features to
    #       shape, and make target_tagger a first-class feature of shape.
    #       At that point, both of the below uses disappear.
    as_dict_shallow = _as_dict_shallow,
    as_dict_for_target_tagger = _as_dict_for_target_tagger,
    as_serializable_dict = _as_serializable_dict,
    is_instance = _is_instance,
    is_any_instance = _is_any_instance,
    pretty = _pretty,
)
