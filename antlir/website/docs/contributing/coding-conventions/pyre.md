---
id: pyre
title: Pyre
---

We've begun incrementally adopting Pyre into `antlir/`. The following are some noteworthy snags of Pyre that we've discovered through this adoption, and recommended conventions to handle them.

## Generators

- We commonly use "generators" for the purpose of lazily returning values, but rarely in the sense of sending/receiving. Consider the simple function below:

  ```
  def fn(limit: int):
      for i in range(limit):
          yield i
  ```

- Both `Generator[int, None, None]` and `Iterator[int]` would be valid return type annotations for this. If the 2nd and 3rd arguments to the `Generator` annotation will be `None` (i.e. messaging is unused), then the preferred annotation is `Iterator`.

## AnyStr =\> MehStr

- When calling `subprocess`, a list containing both `str` and `bytes` can be provided (e.g. `["ls", b"-a"`]).
- Because of our heavy interaction with the OS, we often find ourselves interacting with both `bytes` (typically abstracted as `Path` in `fs_utils`) and `str`, and providing a combination of these to `subprocess.`
- Unfortunately, the existing [AnyStr](https://docs.python.org/3/library/typing.html#typing.AnyStr) is only meant to be used for functions accepting a mix of string types, but not for mutable type declarations.
- For this reason, we've created a new type `MehStr` which is simply a `Union[str, bytes]`, and can be used to e.g. annotate arg lists containing both string types being passed to `subprocess` as described above.
- NB: `MehStr` should only be used in specific cases required to satisfy Pyre, otherwise `AnyStr` should be used
- See <https://fb.prod.workplace.com/groups/pyreqa/3065532970203181> for more context
