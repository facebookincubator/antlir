---
id: oss-test-runner
title: Test Runner
---

# Test Runner

Antlir ships with a [buck external test runner](https://buck.build/files-and-dirs/buckconfig.html#test.external_runner) that will soon become the default runner when `buck test` is invoked.

This offers a number of features over the internal runner, including configurable retries, automatically ignoring disabled tests and more configurable parallel execution of individual test cases.

## Implementation

The test runner is implemented with a rust binary located in `tools/testinfra/runner`. It is currently capable of running Python and Rust unit tests, and outputting a mostly-JUnit-compatible XML file reporting the results.

## CI Integration

The external runner is still under development, so it is not yet the default test runner for the repo, but instead is launched as a secondary GitHub Actions workflow until it has feature parity with the internal test runner (mainly pretty HTML reports on workflow runs)

## Future Features

- Automatic disabling of failing tests
- Pretty HTML reports
