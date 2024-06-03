# Copyright (c) Meta Platforms, Inc. and affiliates.
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
at Buck parse time and at runtime (aka image build time).

## Field Types
A shape field is a named member of a shape type. There are a variety of field
types available:
  primitive types (bool, int, float, str)
  other shapes
  homogenous lists of a single `field` element type
  dicts with homogenous key `field` types and homogenous `field` value type
  enums with string values
  unions via shape.union(type1, type2, ...)

If using a union, use the most specific type first as Pydantic will attempt to
coerce to the types in the order listed
(see https://pydantic-docs.helpmanual.io/usage/types/#unions) for more info.

## Optional and Defaulted Fields
By default, fields are required to be set at instantiation time.

Fields declared with `shape.field(..., default='val')` do not have to be
instantiated explicitly.

Additionally, fields can be marked optional by using the `optional` kwarg in
`shape.field`.

For example, `shape.field(int, optional=True)` denotes an integer field that
may or may not be set in a shape object.

Obviously, optional fields are still subject to the same type validation as
non-optional fields, but only if they have a non-None value.

## Runtime Implementations
`shape.impl` codegens runtime parser/validation libraries in Rust and Python.
The `name` argument must match the name of the `*.shape.bzl` file without the
'.bzl' suffix.

`shape.impl` behaves like any other Buck target, and requires dependencies to be
explicitly set.

NOTE: `shape.bzl` can be used strictly for Buck-time safety without any runtime
library implementation, in which case a separated `.shape.bzl` file and
`shape.impl` targets are not required.

## Serialization formats
shape.bzl provides two mechanisms to pass shape objects to runtime code.

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

```
build_source_t = shape.shape(
    source=str,
    type=str,
)

mount_config_t = shape.shape(
    build_source = build_source_t,
    default_mountpoint=str,
    is_directory=bool,
)

mount_t = shape.shape(
    mount_config = mount_config_t,
    mountpoint = shape.field(str, optional=True),
    target = shape.field(str, optional=True),
)

mount = mount_t(
    mount_config=mount_config_t(
        build_source=build_source_t(
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

load("//antlir/bzl:build_defs.bzl", "buck_genrule", "export_file", "python_library", "rust_library", "target_utils", "third_party")
load(":shell.bzl", "shell")
load(":target_helpers.bzl", "antlir_dep", "normalize_target")

_NO_DEFAULT = struct(no_default = True)

_shape_field = record(
    ty = typing.Any,
    default = typing.Any,
)

_union_rec = record(
    ty = typing.Any,
    __thrift = field(list[int] | None, default = None),
)

def _normalize_type(typ):
    if typ == _path:
        return str

    return typ

def _field(typ, optional = False, default = _NO_DEFAULT) -> _shape_field:
    # Optional fields may be given a non-None default value, but if not, it will
    # be defaulted to None
    if optional and default == _NO_DEFAULT:
        default = None

    typ = _normalize_type(typ)

    if isinstance(typ, _union_rec):
        typ = typ.ty

    if optional:
        typ = typ | None
    return _shape_field(
        ty = typ,
        default = default,
    )

def _dict(key_type, val_type, **field_kwargs):
    typ = dict[_normalize_type(key_type), _normalize_type(val_type)]
    if field_kwargs:
        return _field(
            typ = typ,
            **field_kwargs
        )
    return typ

def _list(item_type, **field_kwargs):
    typ = list[_normalize_type(item_type)]
    if field_kwargs:
        return _field(
            typ = typ,
            **field_kwargs
        )
    return typ

def _union(*union_types, __thrift = None):
    union_types = list(union_types)
    union = union_types.pop()
    for t in union_types:
        union = union | t
    return _union_rec(
        ty = union,
        __thrift = __thrift,
    )

def _enum(*values):
    return enum(*values)

def _path(**_field_kwargs):
    fail("shape.path() is no longer supported, use `shape.path` directly, or wrap in `shape.field()`")

def _uniq(iterable):
    return {el: None for el in iterable}.keys()

def _shape(__thrift = None, **fields):
    """
    Define a new shape type with the fields as given by the kwargs.

    Example usage:
    ```
    shape.shape(hello=str)
    ```
    """

    for name, f in fields.items():
        # Avoid colliding with `__shape__`. Also, in Python, `_name` is "private".
        if name.startswith("_"):
            fail("Shape field name {} must not start with _: {}".format(
                name,
            ))

        # transparently convert fields that are just a type have no options to
        # the rich field type for internal use
        if not isinstance(f, _shape_field):
            f = _field(f)

        fields[name] = field(
            f.ty,
            **(
                {"default": f.default} if f.default != _NO_DEFAULT else {}
            )
        )

    if __thrift != None:
        thrift_names = _uniq(__thrift.values())
        if thrift_names != _uniq(fields.keys()):
            fail("thrift mapping field names must match exactly with field names ({} != {})".format(fields.keys(), thrift_names))

        # It would be even better if we could recursively check that all the
        # included shape types also support thrift, but that's hairy to do in
        # starlark and the rust compilation will fail fast enough, and prevent
        # the implementation from being unsafe in the first place

    return record(**fields)

def _impl(name, deps = (), visibility = None, expert_only_custom_impl = False, test_only_rc_bzl2_ir: bool = False, **kwargs):  # pragma: no cover
    if not name.endswith(".shape"):
        fail("shape.impl target must be named with a .shape suffix")
    export_file(
        name = name + ".bzl",
    )

    # @oss-disable

    bzl2ir = antlir_dep("bzl/shape2:bzl2ir") # @oss-enable
    if test_only_rc_bzl2_ir:
        bzl2ir = antlir_dep("bzl/shape2:bzl2ir")

    buck_genrule(
        name = name,
        cmd = """
            $(exe {}) {} $(location :{}.bzl) {} > $OUT
        """.format(
            bzl2ir,
            normalize_target(":" + name),
            name,
            shell.quote(repr({d: "$(location {})".format(d) for d in deps})),
        ),
    )

    ir2code_prefix = "$(exe {}) --templates $(location {})/templates".format(antlir_dep("bzl/shape2:ir2code"), antlir_dep("bzl/shape2:templates"))

    if not expert_only_custom_impl:
        buck_genrule(
            name = "{}.py".format(name),
            cmd = "{} pydantic $(location :{}) > $OUT".format(ir2code_prefix, name),
        )
        python_library(
            name = "{}-python".format(name),
            srcs = {":{}.py".format(name): "__init__.py"},
            base_module = native.package_name() + "." + name.replace(".shape", ""),
            deps = [antlir_dep(":shape")] + ["{}-python".format(d) for d in deps],
            visibility = visibility,
            **{k.replace("python_", ""): v for k, v in kwargs.items() if k.startswith("python_")}
        )
        buck_genrule(
            name = "{}.rs".format(name),
            cmd = "{} rust $(location :{}) > $OUT".format(ir2code_prefix, name),
        )
        rust_library(
            name = "{}-rust".format(name),
            crate = kwargs.pop("rust_crate", name[:-len(".shape")]),
            mapped_srcs = {":{}.rs".format(name): "src/lib.rs"},
            deps = ["{}-rust".format(d) for d in deps] + [antlir_dep("bzl/shape2:shape")] + third_party.libraries(
                [
                    "anyhow",
                    "fbthrift",
                    "pyo3",
                    "serde",
                    "serde_json",
                    "typed-builder",
                ],
                platform = "rust",
            ),
            visibility = visibility,
            unittests = False,
            allow_unused_crate_dependencies = True,
            **{k.replace("rust_", ""): v for k, v in kwargs.items() if k.startswith("rust_")}
        )

def _json_string(instance):
    """
    Serialize the given shape instance to a JSON string.
    """
    return json.encode(instance)

def _json_file(name, instance, visibility = None, labels = None):  # pragma: no cover
    """
    Serialize the given shape instance to a JSON file that can be used in the
    `resources` section of a `python_binary` or a `$(location)` macro in a
    `buck_genrule`.
    """
    buck_genrule(
        name = name,
        cmd = "echo {} > $OUT".format(shell.quote(_json_string(instance))),
        visibility = visibility,
        labels = labels or [],
    )
    return normalize_target(":" + name)

def _render_template(name, instance, template, visibility = None):  # pragma: no cover
    """
    Render the given Jinja2 template with the shape instance data to a file.
    """
    if native.rule_exists(name):
        # TODO(T191162975): this is a horrible footgun that has already been
        # fired in at least one place. However, it's not a new bug, so keep it
        # papered over in D58021063 and address it in a later stacked diff.
        return normalize_target(":" + name)
    buck_genrule(
        name = name,
        cmd = """
            $(exe {}-render) > $OUT <<< {}
        """.format(template, shell.quote(_json_string(instance))),
        visibility = visibility,
    )
    return normalize_target(":" + name)

def _python_data(
        name,
        instance,
        shape_impl,
        type_name,
        module = None,
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
    module = module or name

    shape_target = target_utils.parse_target(normalize_target(shape_impl))
    shape_module = shape_target.base_path.replace("/", ".") + "." + shape_target.name.replace(".shape", "")

    buck_genrule(
        name = "{}.py".format(name),
        # Antlir users should not directly use `shape`, but we do use it
        # as an implementation detail of "builder" / "publisher" targets.
        cmd = '''
            echo "from {module} import {type_name}" > $OUT
            echo {data_start} >> $OUT
            echo {json} >> $OUT
            echo '""")' >> $OUT
        '''.format(
            data_start = shell.quote('data = {classname}.parse_raw("""'.format(
                classname = type_name,
            )),
            json = shell.quote(_json_string(instance)),
            module = shape_module,
            type_name = type_name,
        ),
        compatible_with = ["ovr_config//os:linux"],
    )

    python_library(
        name = name,
        compatible_with = ["ovr_config//os:linux"],
        srcs = {":{}.py".format(name): "{}.py".format(module)},
        deps = [shape_impl + "-python"],
        # Antlir users should not directly use `shape`, but we do use it
        # as an implementation detail of "builder" / "publisher" targets.
        **python_library_kwargs
    )
    return normalize_target(":" + name)

shape = struct(
    # generate implementation of various client libraries
    impl = _impl,
    # output target macros and other conversion helpers
    dict = _dict,
    json_string = _json_string,
    enum = _enum,
    field = _field,
    json_file = _json_file,
    list = _list,
    path = _path,
    python_data = _python_data,
    render_template = _render_template,
    shape = _shape,
    union = _union,
)
