---
id: python
title: Python
---

### The codebase has legacy files, please send diffs to fix them

But, also, please do not mix no-op refactors with business logic changes. Plan ahead, or use `hg split`.

### Stay lint clean

If a linter rule is causing you problems, let's talk about fixing the lint rule instead.

## Testing

### All code should enforce 100% test coverage

This is Python. If code wasn't executed, it's definitely wrong (incompatible types, bad kwargs, etc). Once we fully adopt Pyre, it'll help catch some shallow bugs before you write tests, but the coverage requirement ought to remain.

Of course, you should aspire to actually break your code by thinking about what it does, what can go wrong, and how to best exercise it. 100% line coverage is just the bare minimum.

Specifically, Python library and binary targets should be mentioned in a test like so:

```
needed_coverage = [
    (":library_name", 100),
    (":binary-name-library", 100),  # `python_binary` defines an implicit `-library` target
]
```

There are scant exceptions to this, which justify the usage of `# pragma: no cover` on a line or block level:

- Tests can't assert coverage below `if __name__ == '__main__'`. Try to keep that block minimal, and to export most logic to a `main()` function. Also, mention above `__main__` the integration test that covers this code path (or that this code path is for human debugging only, in which case a manual smoke test is appreciated).
- If you have an error check that's essentially an assertion, then you can `# pragma: no cover` it. Note that using `assert` instead of `if foo: raise` has essentially the same effect. Be judicious about error cases that are left uncovered --consider at least these risks:
  - **bad:** we don't generate an error, or generate the wrong kind of error, and the program will continue normally when it should have failed,
  - **not great:** the untested error message makes it hard to debug a real failure -- e.g. you forgot the `f` in front of an f-string.

### Inherit from `AntlirTestCase`

Its `setUp()` turns off test output abbreviation (which otherwise complicates CI debugging unnecessarily), so be sure to call `super().setUp()` if overloading that.

This class also enables `async def test_foo()` for testing asyncio code.

### Avoid `unittest.mock` when possible

- To unit-test `antlir` code, design testable interfaces from day 1.
- Also design code to permit bottom-up easy integration tests -- these are worth far more than mock tests.
- When it's necessary to cut out a dependency from a test (because it's heavy, unreliable, or otherwise inaccessble to tests), prefer to **fake** it instead of mocking it. This entails configuring your code to point at a dependency that quacks like the one you expect, but is acceptable in tests. The main distinction from, and advantage over, mocks is that this tests the complete interaction you expect with the external system, instead of making sparse assertions about, the way a mock might. Note that in some cases, using `unittest.mock` to inject fakes is fine.
- When interfacing with external, unreliable, and hard-to-fake subsystems, it is acceptable to do mock tests.
- When using `unittest.mock`, watch out for over-generic mocks, which end up being fragile. I.e. instead of mocking `subprocess.check_output`, consider defining `_run_my_subprog = subprocess.check_output` in your module, and mocking `_run_my_subprog`.

## `TARGETS`, resources & imports

### Avoid `base_module`, prefer absolute imports

- Please work to eliminate `base_module` from `TARGETS` files. This is deprecated throughout Facebook, and generally complicates code comprehension. In other words, your module in `fbcode/antlir/foo.py` should be importable as `antlir.foo`.
- Always use absolute imports

### Use `Path.resource` and `layer_resource_subvol`; avoid `__file__`, `importlib.resources.path`

Your `python_unittest` or `python_binary` has `resources`. You want to access the files from the running code. In `@mode/dev`, most of the above will kind of work. However, in `@mode/opt` and open-source, it's hard to get it right. Our path towards fixing this disaster is as follows:

- Step 1: Standardize on `with Path.resource(__package__, 'path')`
- Step 2: Make that context manager work correctly in all settings.

#### Rationale

Our Python code runs in \~3 settings:

- `@mode/dev`: This is an in-place linktree, so `__file__` and `importlib` work.
- `@mode/opt` with `par_style = "fastzip"` (roughly equivalent to OSS PEX): Neither `__file__` and `importlib` work.
- `@mode/opt` with `par_style = "xar"`: In this mode, both `__file__` and `importlib` work, but it's hard to reproduce in OSS. Historically, when we found `@mode/opt` issues, we would paper over them by converting certain binaries as XARs (which, incidentally, do not compose well with `sudo`). Having these overrides is an ongoing cost to developers we have to keep in mind all of these constraints, and we often get it wrong and break `@mode/opt`.

For these reasons, we are standardizing on `Path.resource` to the exclusion of everything else.

#### Why are `target_location` / `load_location` deprecated?

`target_location` cannot work properly with distributed Buck caches. If your binary is built on Sandcastle trunk, it will have embedded inside an absolute path to a resource that is only valid in that Sandcastle container (or at any rate, in similar Sandcastle containers). Therefore, this binary will fail to run anywhere else, including devboxes. We haven't seen much of this because it only affects `@mode/opt` runs not on Sandcastle, and our usage is low.

### Import ordering

We recommend sectioned imports, separated by a single blank line. First come standard modules, then antlir modules. Each section splits into 2 subsections: first all `import x`, then `from y import z`. Example:

```
import standard_module

from other_standard import std_name

from antlir.foo import bar
```

## General

### Recommended `__main__` boilerplate

A main function of any complexity should be a separate function and covered by unit tests. Any main (simple or not) should be covered by _some_ (not necessarily dedicated) integration test, unless it can only invoked by `antlir` developers for debugging.

A typical main should use `init_cli` to set up logging & argparse:

```
if __name__ == '__main__':
    from antlir.cli import init_cli

    with init_cli(__doc__) as cli:
        cli.parser.add_argument("--your-arg")

    business_logic(cli.args)
```

In a "simple" main that has ample integration test coverage, it is OK to compose a few business logic functions inline, as long as you do no branching. **If in doubt, use a separate main function and write unit-tests.**

### Use `UserError`

Any user-facing Antlir binary (e.g. `compiler:compiler`) should check for user errors. Such binaries ought to have a top-level `except UserError` to highlight important messages for the user, and to hide confusing backtraces.

Conversely, any error scenario that is correctable by the user should `raise UserError`.

### Logging

It's the responsibility of any `__main__` to call `init_logging` (see the section on `__main__`).

```
from antlir.common import get_logger

log = get_logger()

...
log.info(f'foo {var}')
```

### Formatting: PEP-8, 80 chars

- If you need to churn formatting, do it on a separate diff. Don't mix re-formatting with logic changes. Super-small reformats may be begrudgingly tolerated.
- We don't nitpick formatting, as long as it's lint-clean, see [fs_image/.flake8](https://our.intern.facebook.com/intern/diffusion/FBS/browse/master/fbcode/antlir/.flake8).
- 80 chars optimizes for coding and review on smaller screens (laptop, WFH)
- Manual formatting: optimize for readability. Code is read far more than written.
- Auto-formatting, if you must: most of the codebase is single-quoted, so you can use `black -S -l 80` to retain ambient style. In new modules, you can use `"` if it makes you happy.

### Use `Path` from `antlir.fs_utils`, avoid `pathlib`

#### Why `Path`?

- Linux paths are not `str`, they are `bytes`.
- Path raises when compared to `str`, preventing bugs like `'a' == b'a'`.
- Once we adopt Pyre, the type separation should help detect bugs earlier.
- It has the syntax sugar of `pathlib` without adopting its over-complicated and error-prone object model. In `pathlib` if you use the wrong class from the hierarchy, your code fails at runtime, in `Path`, there is only one class.

#### How to `Path`ify code

- Take and return just `Path` in new interfacecs
- Take `AnyStr` in old interfaces, and immediately coerce to `Path` to simplify manipulations (Postel's law).

#### `.decode()` on `Path` is a code smell

We need this to interface with genrule modules (e.g. `requests`), but in most other circumstances, we have primitives for avoiding explicit `.decode()` calls and the associated waste of time fighting with `str` vs `bytes` issues. Specifically, be aware that:

- `Path` supports `__format__` letting it be spliced into f-strings and `str.format()`.
- `subprocess` allows mixing `Path` and `str` in arguments.
- `os` functions return `bytes` when given `Path`, but we add methods to `Path` for the common ones, which return `Path` instead.
- Use `Path.parse_args` instead of `argparse.ArgumentParser.parse_args` to let your tests pass in `Path` (e.g. `temp_dir()`).
- Use `Path.json_dumps` instead of `json.dumps` to transparently serialize `Path`.

### Use context managers instead of functions when appropriate

If not already familiar, learn about the virtues of [RAII](https://en.wikipedia.org/wiki/Resource_acquisition_is_initialization). In `antlir`, we manage a lot of resources -- temporary files & directories, btrfs subvolumes, subprocesses.

You will often write code that manages resources, and then does something with these resources. In these cases, it's usually best to:

- Create each resource in a separate `@contextmanager` function (very rarely, a class).
- Use chained `with` statements, or `ExitStack`, to sequentialize allocation & destruction of the resources. Specifically, do **not** try to manually clean up multiple resources with a single `try: finally`, you will get it wrong.

Things to avoid:

- Python destructors that free resources (google for why they are problematic).
- `subprocess.run()`-style APIs for managing long-running processes. Just provide a `Popen`-style `@contextmanager` right away, and perhaps add `run()`-style syntax sugar commonly needed. Refactoring out of this design mistake can be costly (look at the history of `antlir/nspawn_in_subvol`).
- `finally:` blocks that clean up more than one resources.

Advanced readers might ask "what about `async`?". The general rule is that it's acceptable at module level, but we want to avoid infecting the entire codebase with the extra conceptual complexity it entails, so please don't blindly `async`ify everything up the stack. Find a reasonable point to wait, and/or start a reasoned discussion about the large benefits that broader `async`ification would bring.

### Prefer long-form args (`--absolute-names`) over short (`-P`)

Seriously, there's not even one `P` in `--absolute-names`.

### Use the `--foo=bar` form for long-form args when possible

The only known exceptions are:

- The target command does not accept the `=` form, and requires two separate arguments.
- The option takes multiple arguments. Then, the suggested form is:

  ```
  subprocess.Popen([
      '--one-arg=foo',
      # Group to prevent `black` from putting this on 3 lines
      *('--multi-arg', 'bar', baz'),
  ])
  ```

Note that `*()` is **not** mandatory, use it only when it makes it easier to understand the grouping.

Why have a convention at all? First, we can more easily adopt syntax sugar for our convention (e.g. this motivated `Path.__format__`). Also, uniformity can help readability.

Why use one arg with `=` instead of two separate args?

- Most importantly, it is clear at a glance that an option takes an argument. Reading `['--foo', bar, baz]` leaves ambiguity, `['--foo={bar}', baz]` does not.
- Repeated arguments are cleaner, you can do `[*['--i={i}' for i in range(3)], '-b']` instead of flattening nested lists.
- With separate arguments, `black` formats them one-per-line, which often results in over-long functions for no benefit.
- Although separate args line-wrap naturally, with `=` you can easily get line-wrapping via `+`, e.g.:

  ```
  '--a-long-option-name='
      + 'its value',
  ```

### Respect private identifiers

Conventionally, private Python identifiers start with `_`, and should not be used outside the module, or its test. A module can span multiple files (like `nspawn_in_subvol`), the point is to respect abstraction boundaries and not leak implementation details.

As another example, do not use `debug_only_opts` from `nspawn_in_subvol` in any code that runs on `buck build`, `buck test`, or automation. This feature set is really just for interactive debugging by humans via the CLI. Don't use it in other code!

### Limit positional args to 2, use keyword-only args (`pos, *, kw1, kw2`)

Callsites using many positional arguments are harder to read & understand -- it is rare that an action takes more than 2 obviously ordered objects. In rare cases, 3 positional args are OK.

On the flip side, callsites with keyword arguments are much easier to maintain. You can safely change the signature in the following ways:

- Add more (defaulted) arguments
- Reorder arguments
- Remove or rename arguments, only updating the callsites that explicitly mention them (i.e. `hg grep kwarg=`)
