---
sidebar_position: 1
---

# Features

"Features" is the term that `antlir2` uses to describe user-provided
instructions for how an image is to be built.

A common misconception is that the order in which features are written in buck
matter. They do not. Features are ordered by a
[dependency graph](../internals/depgraph.md), so you don't have to concern
yourself with the order in which you write your features.

## API documentation

The API documentation for all of the builtin antlir2 features can be found
[here](../api/features.md)
