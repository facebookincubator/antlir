import unittest

import antlir.bzl.shape.tests.example as example

# import antlir.bzl.shape.tests.example.types as example
# from thrift.py3.serializer import deserialize, Protocol


class CharacterSub(example.Character):
    def extra_method(self):
        pass


class TestExample(unittest.TestCase):
    def test_example(self):
        pass
        # print(dir(example.Character))
        # print(dir(Protocol))
        # self.assertEquals(example.luke.name, "Luke Skywalker")
        # example.Character(name="Luke Skywalker", blah=True)
        # ch = deserialize(
        #     example.Character, b'{"name": "Luke Skywalker"}', Protocol.JSON
        # )
