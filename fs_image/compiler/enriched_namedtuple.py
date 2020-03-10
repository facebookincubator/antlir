#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

'''

This module extends `namedtuple` to allow coping with more complex data models.

A typical user of `enriched_namedtuple` wants to produce a family (or class
hierarchy) of related record types.  Each of the types will behave much like
`namedtuple`, but they will also systematically share some functionality.


## Basic usage

1) Define a metaclass that will be used to define your sub-types. E.g.

    class PlantType(type):
        def __new__(metacls, classname, bases, attr_dict):
            return metaclass_new_enriched_namedtuple(
                __class__,
                # attributes / fields shared by all plants
                [('color', GREEN), 'has_roots'],
                metacls, classname, bases, attr_dict,
            )

   You can make fields optional by passing a tuple with the default value as
   the second member (see `GREEN` above).  You can require a field by
   specifying either its name or `('field_name', RequiredField)`.

   Above, we chose only to add a couple of shared fields to each of the
   concrete namedtuple types.  Since this is a metaclass, you could
   customize most other aspects of class construction.  To preprocess the
   values of fields prior to construction, simply pass the callback
   `customize_fields_fn` to `metaclass_new_enriched_namedtuple`.  You can
   add synthetic fields by declaring them as `NonConstructibleField`, and
   emitting them from `customize_fields_fn`.

2) One big advantage of enriched namedtuples is that they support inheritance
   of fields across a class hierarchy.

    class FloweringPlant:
        fields = ['flower_color']

    class Grain(FloweringPlant, metaclass=PlantType):
        fields = ['grain_size_mm', 'edible']

   `Grain` will inherit fields from `PlantType` and `FloweringPlant` to yield:

    [('color', GREEN), 'has_roots', 'flower_color', 'grain_size_mm', 'edible']

   You will get an error if the same fields comes from two sources.

3) Initialization via positional arguments would be fragile, so enriched
   namedtuples must always be constructed via keyword args:

    Grain(has_roots=True, grain_size_mm=5, flower_color=WHITE, edible=False)


## Other features

 - Enriched namedtuples prevent type-punning: regular namedtuples use the
   tuple comparator, by default, distinct namedtuple types storing the same
   data in the same order will hash and compare equal.  By contrast,
   different enriched namedtuple types hash & compare differently, even if
   they store the same data.

   At present, the fix is simply to add a `DO_NOT_USE_type` field to each
   enriched namedtuple.  It is equal to the `__class__` attribute, which
   means that we waste some memory.  Do not rely on this implementation
   detail, it may change.


## Possible improvements

 - Like with other defaults in Python, you will have aliasing issues if you
   set them to mutable objects like [].

 - It would likely lead to cleaner, safer code to force enriched namedtuples
   to be recursively immutable, instead of just providing shallow
   immutability.  See https://fburl.com/recursively_immutable for a sample
   implementation.

 - It is probably not great that namedtuples are positionally iterable, or
   indexable by integers. Field indexing is quite fragile in a world with
   inheritance, so it might be best to forbit it.  Instead, add `.items()`?

'''
from collections import namedtuple


class NonConstructibleField:
    pass


class RequiredField:
    pass


def _normalize_enriched_namedtuple_fields(
    cls, field_to_value, field_to_base_and_default
):
    '''
    When constructing an enriched namedtuple instance, the user passes
    a number of keyword arguments to populate the namedtuple's fields.
    This helper takes the user-supplied keyword arguments as the
    dictionary `field_to_value`, and:
     - validates that all the keys are fields of this enriched namedtuple,
     - populates defaults for any keys that the user did not specify,
     - errors when a field is required, but the user did not supply a key,
     - adds `DO_NOT_USE_type` to prevent type-punning (see doc above).

    After this helper is done modifying `field_to_value`, the dictionary
    is additionally passed into the user-supplied `customize_fields_fn`,
    and only then is the namedtuple instantiated.

    If the namedtuple defines `NonConstructibleFields`, the user's
    `customize_fields_fn` will have to supply them.

    `field_to_base_and_default` has the form:

      {'field_name': (BaseClassDefiningField, default_value_or_RequiredField)}

    This dictionary is built by the metaclass via `_merge_fields_across_bases`
    at the time that your enriched type is being instantiated.

    DANGER: This **MUTATES** field_to_value.
    '''
    # Make sure all arguments are known.
    for field, _value in field_to_value.items():
        base_and_default = field_to_base_and_default.get(field)
        assert (
            (base_and_default is not None) and
            (base_and_default[1] is not NonConstructibleField)
        ), 'Constructing {} with unknown field {}'.format(cls, field)

    # Check we have required args, and back-fill optional ones.
    for field, (_base, default) in field_to_base_and_default.items():
        if field not in field_to_value:
            assert default is not RequiredField, (
                '{} requires the field {}'.format(cls, field)
            )
            field_to_value[field] = default

    # `customize_fields_fn` can do theas sorts of checks and assignments for
    # other, non-builtin fields.
    assert field_to_value['DO_NOT_USE_type'] is NonConstructibleField
    field_to_value['DO_NOT_USE_type'] = cls


def _merge_fields_across_bases(
    metaclass, class_name, bases, attr_dict, core_fields_and_defaults,
):
    field_to_base_and_default = {}

    def add(cls, field_and_default):
        # A vanilla string is equivalent to ('field', RequiredField)
        if isinstance(field_and_default, str):
            field, default = field_and_default, RequiredField
        else:
            field, default = field_and_default
        if field in field_to_base_and_default:
            raise AssertionError(
                'Classes {} and {} both specify field {}'.format(
                    cls.__name__, field_to_base_and_default[field][0].__name__,
                    field,
                )
            )
        field_to_base_and_default[field] = (cls, default)

    for field_and_default in core_fields_and_defaults:
        add(metaclass, field_and_default)

    # Make a fake version of the class we're instantiating just so we can
    # combine the fields according to the MRO. The ordering will matter
    # if we later allow "shadowing" declarations, e.g.
    #     ShadowsField('field_name', PreviousClass, default=RequiredField)
    for cls in type(
        class_name, bases, {'fields': attr_dict.get('fields', [])}
    ).mro():
        for field_and_default in getattr(cls, 'fields', []):
            add(cls, field_and_default)

    return field_to_base_and_default


def _assert_all_fields_constructible(class_name, field_to_value):
    'We do not check that all fields are set, since namedtuple will.'
    for field, value in field_to_value.items():
        if value is NonConstructibleField:
            raise AssertionError(
                'customize_fields_fn for {} failed to construct field {}'
                .format(class_name, field)
            )
    return field_to_value


def metaclass_new_enriched_namedtuple(
    calling_metaclass,  # Yes, we could do stack inspection, no, we won't.
    core_fields_and_defaults,
    metacls, class_name, bases, attr_dict,
    # Returns {'field': 'value'}. Ok to mutate the dict that was passed to it.
    customize_fields_fn=lambda kwargs: kwargs,
):
    'Supports inheritance hierarchies of enriched namedtuples'
    field_to_base_and_default = _merge_fields_across_bases(
        calling_metaclass, class_name, bases, attr_dict,
        [
            ('DO_NOT_USE_type', NonConstructibleField),
            *core_fields_and_defaults,
        ],
    )

    class EnrichedNamedtupleBase(
        namedtuple(class_name, field_to_base_and_default.keys())
    ):
        __slots__ = ()  # Forbid adding new attributes

        def __new__(cls, **field_to_value):  # Forbid positional arguments
            # MUTATES field_to_value, OK since ** always makes a new dict
            _normalize_enriched_namedtuple_fields(
                cls, field_to_value, field_to_base_and_default
            )
            return super(EnrichedNamedtupleBase, cls).__new__(
                cls,
                **_assert_all_fields_constructible(
                    class_name, customize_fields_fn(field_to_value),
                ),
            )

        def __repr__(self):
            return class_name + '(' + ', '.join(
                f'{f}={repr(getattr(self, f))}'
                    for f in self._fields if f != 'DO_NOT_USE_type'
            ) + ')'

    # Since we're inheriting from `tuple`, __slots__ must be empty if set.
    #
    # Enriched namedtuples like PathObject below are supposed to be immutable,
    # so we should not allow people to add attributes after construction.
    # Without __slots__ the subclass would end up with a `__dict__` attribute,
    # which would allow setting arbitrary data on a created object -- and
    # worse yet, the data would be excluded from ==, `hash` computations,
    # serialization, etc.  In other words, such data would be "semantically
    # invisible".
    assert '__slots__' not in attr_dict, (
        'Do not set __slots__ on enriched namedtuples. Got {} for {}'
        .format(attr_dict['__slots__'], class_name)
    )
    for base in bases:
        assert getattr(base, '__slots__', None) in [(), []], (
            'Base {} of {} must set empty __slots__'.format(
                base.__name__, class_name
            )
        )

    # NB: We own `attr_dict` as per the metaclass contract.
    attr_dict['__slots__'] = ()
    attr_dict.pop('fields', None)  # namedtuple provides _fields
    # Useful for testing if we have an instance of this namedtuple metatype.
    attr_dict['_enriched_namedtuple_base'] = EnrichedNamedtupleBase

    return super(calling_metaclass, metacls).__new__(
        metacls, class_name, (EnrichedNamedtupleBase,) + bases, attr_dict
    )
