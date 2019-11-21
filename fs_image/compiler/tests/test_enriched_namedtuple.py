#!/usr/bin/env python3
import unittest

from ..enriched_namedtuple import (
    metaclass_new_enriched_namedtuple, NonConstructibleField
)


class PlantType(type):
    def __new__(metacls, classname, bases, dct):
        return metaclass_new_enriched_namedtuple(
            __class__,
            # attributes / fields shared by all plants
            [('color', 'green'), 'has_roots'],
            metacls, classname, bases, dct,
        )


class Algae(metaclass=PlantType):
    fields = ['is_saltwater']


class FloweringPlant:
    __slots__ = ()
    fields = ['flower_color']


class Grain(FloweringPlant, metaclass=PlantType):
    fields = ['grain_size_mm', 'is_edible']


class EnrichedNamedtupleTestCase(unittest.TestCase):

    def _check_fields(self, cls, fields):
        self.assertEqual(set(cls._fields), {'DO_NOT_USE_type', *fields})

    def test_fields(self):
        self._check_fields(Algae, ['color', 'has_roots', 'is_saltwater'])
        self._check_fields(Grain, [
            'color', 'has_roots', 'flower_color', 'grain_size_mm', 'is_edible'
        ])

    def _check_values(self, ent, field_to_value):
        self.assertEqual(
            set(ent._fields), {'DO_NOT_USE_type', *field_to_value.keys()},
        )
        self.assertEqual(ent.DO_NOT_USE_type, ent.__class__)
        self.assertEqual(
            {f: getattr(ent, f) for f in field_to_value.keys()},
            field_to_value,
        )

    def test_values(self):
        self._check_values(
            Algae(has_roots=False, is_saltwater=True),
            {'color': 'green', 'has_roots': False, 'is_saltwater': True},
        )
        self._check_values(
            Algae(color='red', has_roots=False, is_saltwater=True),
            {'color': 'red', 'has_roots': False, 'is_saltwater': True},
        )
        self._check_values(
            Grain(
                has_roots=True,
                flower_color='yellow',
                grain_size_mm=5,
                is_edible=False,
            ),
            {
                'color': 'green',
                'has_roots': True,
                'flower_color': 'yellow',
                'grain_size_mm': 5,
                'is_edible': False,
            },
        )

    def test_field_value_errors(self):
        with self.assertRaisesRegex(
            AssertionError, '^.* requires the field is_saltwater$'
        ):
            Algae(has_roots=False)

        with self.assertRaisesRegex(AssertionError, '^.* unknown field foo$'):
            Algae(has_roots=False, is_saltwater=False, foo='cat')

        with self.assertRaisesRegex(
            AssertionError, '^.* unknown field DO_NOT_USE_type$'
        ):
            Algae(has_roots=False, is_saltwater=False, DO_NOT_USE_type='cat')

        with self.assertRaisesRegex(
            AssertionError, '^.* requires the field flower_color$'
        ):
            Grain(has_roots=True, grain_size_mm=5, is_edible=False),

    def test_field_declaration_errors(self):
        for redundant_field in ['color', 'flower_color', 'DO_NOT_USE_type']:
            with self.assertRaisesRegex(
                AssertionError, f'^.* both specify field {redundant_field}$'
            ):
                class BadGrain(FloweringPlant, metaclass=PlantType):
                    fields = ['is_edible', redundant_field]

    def test_customize_fields_fn(self):

        class TypeGrowsN(type):
            def __new__(metacls, classname, bases, dct):

                def customize_fields(field_to_value):
                    field_to_value['n'] += 1
                    return field_to_value

                return metaclass_new_enriched_namedtuple(
                    __class__, ['n'], metacls, classname, bases, dct,
                    customize_fields
                )

        class GrowsN(metaclass=TypeGrowsN):
            pass

        self.assertEqual(GrowsN(n=3).n, 4)

    def test_customize_fields_fn_errors(self):

        class TypeDiscardsFields(type):
            def __new__(metacls, classname, bases, dct):
                return metaclass_new_enriched_namedtuple(
                    __class__, ['n'], metacls, classname, bases, dct,
                    lambda _: {}
                )

        class DiscardsFields(metaclass=TypeDiscardsFields):
            pass

        with self.assertRaisesRegex(
            TypeError,
            '^.* missing 2 required positional arguments: '
                "'DO_NOT_USE_type' and 'n'$",
        ):
            DiscardsFields(n=3)

        class TypeFailsToSetNonConstructible(type):
            def __new__(metacls, classname, bases, dct):
                return metaclass_new_enriched_namedtuple(
                    __class__, [], metacls, classname, bases, dct,
                )

        class FailsToSetNonConstructible(
            metaclass=TypeFailsToSetNonConstructible
        ):
            fields = [('must_be_set_by_customize', NonConstructibleField)]

        with self.assertRaisesRegex(
            AssertionError,
            '^customize_fields_fn for FailsToSetNonConstructible failed to '
                'construct field must_be_set_by_customize$',
        ):
            print(FailsToSetNonConstructible())

    def test_slots_errors(self):

        class BadBase:
            pass

        with self.assertRaisesRegex(
            AssertionError, '^Base BadBase of APlant must set empty __slots__$'
        ):
            class APlant(BadBase, metaclass=PlantType):
                pass

        with self.assertRaisesRegex(
            AssertionError, '^Do not set __slots__ on enriched namedtuples.*$'
        ):
            class AnotherPlant(metaclass=PlantType):
                __slots__ = ()

        g = Grain(
            has_roots=True, grain_size_mm=3, is_edible=True, flower_color='red'
        )
        with self.assertRaises(AttributeError):
            g.boof = 3

    def test_repr(self):
        self.assertEqual(
            "Algae(color='green', has_roots=True, is_saltwater=False)",
            repr(Algae(has_roots=True, is_saltwater=False)),
        )


if __name__ == '__main__':
    unittest.main()
