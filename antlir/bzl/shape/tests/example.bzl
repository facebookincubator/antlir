# @generated

load("@bazel_skylib//lib:types.bzl", "types")
load("//antlir/bzl/shape:defs.bzl", "fail_with_context", "add_context")

def _check_bool(x, context=None):
    if not types.is_bool(x):
        fail_with_context(msg="{} is not a bool".format(repr(x)), context=context)

def _check_int(x, context=None):
    if not types.is_int(x):
        fail_with_context(msg="{} is not a int".format(repr(x)), context=context)

def _check_string(x, context=None):
    if not types.is_string(x):
        fail_with_context(msg="{} is not a string".format(repr(x)), context=context)

def _check_dict(x, context=None):
    if not types.is_dict(x):
        fail_with_context(msg="{} is not a dict".format(repr(x)), context=context)

def _check_list(x, context=None):
    if not types.is_list(x):
        fail_with_context(msg="{} is not a list".format(repr(x)), context=context)



# Affiliations Struct
def Affiliations(
    *,
    faction = None,
    err_context=None,
):
    """
    Groupings in which a character may belong.

    """
    # prepare a dictionary with all the shape fields, starting with any default values
    data = {
    }

    data["faction"] = faction

    s = struct(__type__ = Affiliations, **data)
    __typecheck_Affiliations(s, _add_context("Constructing struct 'Affiliations'", err_context))
    return s

def __typecheck_Affiliations(object, err_context=None):

    faction = object.faction
    if faction == None:
        _fail_with_context("faction: required but is None", context=err_context)
    else:
        _check_string(faction, context=add_context("Validating 'faction'", err_context))

# Character Struct
def Character(
    *,
    affiliations = None,
    appears_in = None,
    friends = None,
    metadata = None,
    name = None,
    weapon = None,
    err_context=None,
):
    """
    A character that exists in the Star Wars universe.
Test data adapted from the GraphQL examples

    """
    # prepare a dictionary with all the shape fields, starting with any default values
    data = {
        "affiliations": Affiliations(faction="Rebellion"),
        "appears_in": [4,5,6],
        "metadata": {"species": "human"},
    }

    if affiliations != None:
        data["affiliations"] = affiliations

    if appears_in != None:
        data["appears_in"] = appears_in

    data["friends"] = friends

    if metadata != None:
        data["metadata"] = metadata

    data["name"] = name

    data["weapon"] = weapon

    s = struct(__type__ = Character, **data)
    __typecheck_Character(s, _add_context("Constructing struct 'Character'", err_context))
    return s

def __typecheck_Character(object, err_context=None):

    affiliations = object.affiliations
    if affiliations == None:
        _fail_with_context("affiliations: required but is None", context=err_context)
    else:
        __typecheck_Affiliations(affiliations, err_context=err_context)

    appears_in = object.appears_in
    if appears_in == None:
        _fail_with_context("appears_in: required but is None", context=err_context)
    else:
        _check_list(appears_in, context=err_context)
        for (i, appears_in_item) in enumerate(appears_in):
            inner_context = err_context + ["Checking index {} for field 'appears_in'".format(i)]
            _check_int(appears_in_item, context=add_context("Validating 'appears_in_item'", inner_context))

    friends = object.friends
    if friends == None:
        _fail_with_context("friends: required but is None", context=err_context)
    else:
        _check_list(friends, context=err_context)
        for (i, friends_item) in enumerate(friends):
            inner_context = err_context + ["Checking index {} for field 'friends'".format(i)]
            __typecheck_Friend(friends_item, err_context=inner_context)

    metadata = object.metadata
    if metadata == None:
        _fail_with_context("metadata: required but is None", context=err_context)
    else:
        _check_dict(metadata, context=err_context)
        for metadata_key, metadata_value in metadata.items():
            inner_context = err_context + ["Checking key '{}' for field 'metadata'".format(metadata_key)]
            _check_string(metadata_key, context=add_context("Validating 'metadata_key'", inner_context))
            _check_string(metadata_value, context=add_context("Validating 'metadata_value'", inner_context))

    name = object.name
    if name == None:
        _fail_with_context("name: required but is None", context=err_context)
    else:
        _check_string(name, context=add_context("Validating 'name'", err_context))

    weapon = object.weapon
    if weapon != None:
        __typecheck_Weapon(weapon, err_context=err_context)

# Color Enum
ColorVariants = struct(
    BLUE = 2,
    GREEN = 1,
    RED = 3,
)

__Color_name_to_value = {
    "BLUE": 2,
    "GREEN": 1,
    "RED": 3,
}
__Color_value_to_name = {
    2: "BLUE",
    1: "GREEN",
    3: "RED",
}

# Construct a new instance of the Color enum, checking that it is a valid
# variant name or value. Returns a struct with .name and .value
def Color(name_or_value, context=None):
    """
    A color that a lightsaber may come in.

    """
    if types.is_int(name_or_value):
        value = name_or_value
        if value not in __Color_value_to_name:
            fail_with_context(
                "{} not one of (2, 1, 3, )".format(value),
                context=context,
            )
        name = __Color_value_to_name[value]
    elif types.is_string(name_or_value):
        name = name_or_value.upper()
        if name not in __Color_name_to_value:
            fail_with_context(
                "{} not one of (BLUE, GREEN, RED, )".format(name),
                context=context,
            )

        value = __Color_name_to_value[name]
    else:
        fail_with_context(
            "Provided value {} to Color constructor was neither an int or string".format(name_or_value),
            context=context,
        )

    return struct(
        name = name,
        value = value,
        __type__ = Color,
    )

def __typecheck_Color(e, err_context=None):
    if not hasattr(e, "__type__"):
        fail_with_context(
            "Provided value {} is not a struct or enum".format(e),
            context=err_context,
        )
    if e.__type__ != Color:
        fail_with_context(
            "Provided value {} is not an instance of this enum: {}".format(e, e.__type__),
            context=err_context,
        )

# Friend Struct
def Friend(
    *,
    name = None,
    err_context=None,
):
    # prepare a dictionary with all the shape fields, starting with any default values
    data = {
    }

    data["name"] = name

    s = struct(__type__ = Friend, **data)
    __typecheck_Friend(s, _add_context("Constructing struct 'Friend'", err_context))
    return s

def __typecheck_Friend(object, err_context=None):

    name = object.name
    if name == None:
        _fail_with_context("name: required but is None", context=err_context)
    else:
        _check_string(name, context=add_context("Validating 'name'", err_context))

# Lightsaber Struct
def Lightsaber(
    *,
    color = None,
    handmedown = None,
    err_context=None,
):
    # prepare a dictionary with all the shape fields, starting with any default values
    data = {
        "color": Color(2),
        "handmedown": True,
    }

    if color != None:
        data["color"] = color

    if handmedown != None:
        data["handmedown"] = handmedown

    s = struct(__type__ = Lightsaber, **data)
    __typecheck_Lightsaber(s, _add_context("Constructing struct 'Lightsaber'", err_context))
    return s

def __typecheck_Lightsaber(object, err_context=None):

    color = object.color
    if color == None:
        _fail_with_context("color: required but is None", context=err_context)
    else:
        __typecheck_Color(color, err_context=err_context)

    handmedown = object.handmedown
    if handmedown == None:
        _fail_with_context("handmedown: required but is None", context=err_context)
    else:
        _check_bool(handmedown, context=add_context("Validating 'handmedown'", err_context))

# Weapon Union
def Weapon(
    *,
    lightsaber = None,
    other = None,
    err_context=None,
):
    data = {}

    if lightsaber != None:
        data["lightsaber"] = lightsaber

    if other != None:
        data["other"] = other

    u = struct(__type__ = Weapon, **data)
    __typecheck_Weapon(u, add_context("Constructing union 'Weapon'", err_context))
    return u


def __typecheck_Weapon(object, err_context=None):
    seen = []

    if hasattr(object, "lightsaber") and object.lightsaber != None:
        seen.append("lightsaber")

    if hasattr(object, "other") and object.other != None:
        seen.append("other")

    if len(seen) == 0:
        fail_with_context(
            "All fields for union Weapon were None", context=err_context
        )

    if len(seen) > 1:
        fail_with_context(
            "Multiple different values provided for union Weapon: {}".format(
                ",".join(seen)
            ),
            context=err_context,
        )

    if hasattr(object, "lightsaber") and object.lightsaber != None:
        lightsaber = object.lightsaber
        if lightsaber == None:
            _fail_with_context("lightsaber: required but is None", context=err_context)
        else:
            __typecheck_Lightsaber(lightsaber, err_context=err_context)

    if hasattr(object, "other") and object.other != None:
        other = object.other
        if other == None:
            _fail_with_context("other: required but is None", context=err_context)
        else:
            _check_string(other, context=add_context("Validating 'other'", err_context))
