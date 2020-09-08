load("//antlir/bzl:shape.bzl", "shape")
# some simple self-tests, that should be included in //antlir/bzl/tests
# the generated python is tested in test_shape.py, these tests exercise some of
# the parse-time field validation

def assert_equal(a, b):
    if a != b:
        fail("'{}' != '{}'".format(a, b))

def assert_not_equal(a, b):
    if a == b:
        fail("'{}' == '{}'".format(a, b))

def assert_validates(spec, data):
    assert_equal(spec.validate(spec, data), "")

def assert_validate_fail(spec, data, failure = None):
    if not failure:
        assert_not_equal(spec.validate(spec, data), "")
    else:
        assert_equal(spec.validate(spec, data), failure)

def run_shape_selftests():
    dict_spec = shape.dict(str, int)
    assert_validates(dict_spec, {"hello": 42})
    assert_validate_fail(dict_spec, {"hello": "a"})

    tuple_spec = shape.tuple(str, int, bool)
    assert_validates(tuple_spec, ("hello", 1, True))
    assert_validate_fail(tuple_spec, ("hello", 1))
    assert_validate_fail(tuple_spec, ("hello", 1, 2))

    optional_tuple_spec = shape.tuple(str, optional = True)
    assert_validates(optional_tuple_spec, ("hello",))
    assert_validates(optional_tuple_spec, None)
    assert_validate_fail(optional_tuple_spec, ("hello", "world"))

    primitive_spec = shape.field(int)
    assert_validates(primitive_spec, 1)
    assert_validate_fail(primitive_spec, "a")

    optional_primitive_spec = shape.field(int, optional = True)
    assert_validates(optional_primitive_spec, 1)
    assert_validates(optional_primitive_spec, None)
    assert_validate_fail(optional_primitive_spec, "a")

    list_of_shapes = shape.list(shape.shape(list_item = bool))

    shape_spec = shape.shape(
        single_int = shape.field(int, default = 42),
        set_of_str = shape.set(str),
        dict_str_to_int = shape.dict(str, int),
        nested = shape.shape(is_nested = bool),
    )
    assert_validates(
        shape_spec,
        {
            "dict_str_to_int": {"answer": 42},
            "nested": shape.new(shape_spec.nested, is_nested = True),
            "set_of_str": ("hello",),
            "single_int": 1,
        },
    )
    assert_validate_fail(
        shape_spec,
        {
            # field of wrong type
            "dict_str_to_int": {"answer": "42"},
            "nested": shape.new(shape_spec.nested, is_nested = True),
            "set_of_str": ("hello",),
            "single_int": 1,
        },
    )
    assert_validate_fail(
        shape_spec,
        {
            # missing required field
            "nested": shape.new(shape_spec.nested, is_nested = True),
            "set_of_str": ("hello",),
            "single_int": 1,
        },
    )
    assert_validate_fail(
        shape_spec,
        {
            "dict_str_to_int": {"answer": 42},
            # different nested shape type
            "nested": shape.new(shape.shape(different = bool), different = True),
            "set_of_str": ("hello",),
            "single_int": 1,
        },
    )
