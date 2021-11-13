/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

namespace py3 antlir.bzl.shape.tests

/// Groupings in which a character may belong.
struct Affiliations {
  1: string faction;
}

/// A character that exists in the Star Wars universe.
/// Test data adapted from the GraphQL examples
struct Character {
  1: required string name;
  2: list<i32> appears_in = [4, 5, 6];
  3: list<Friend> friends;
  4: optional Weapon weapon;
  5: map<string, string> metadata = {"species": "human"};
  6: Affiliations affiliations = Affiliations{faction = "Rebellion"};
} (docs = "hello")

struct Friend {
  1: string name;
}

union Weapon {
  1: Lightsaber lightsaber;
  2: string other;
}

struct Lightsaber {
  1: Color color = "Color.BLUE";
  2: bool handmedown = true;
}

/// A color that a lightsaber may come in.
enum Color {
  GREEN = 1,
  BLUE = 2,
  RED = 3,
}
