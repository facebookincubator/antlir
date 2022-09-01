---
id: native-services
title: Native Services
---

# What is a Native Service?

A Native Service is a service that is built for MetalOS. MetalOS will manage its
lifecycle as long as the service meets a few simple requirements.

A simple demo of a native service image may be found at
`//metalos/lib/service/tests/demo_service`

There are three types of Native Services:
- `NATIVE`: this is the preferred type, and you should prefer this as much as
  possible, services of this type will run in a sandboxed environment, their rootfs
  will be read only, only a few directories will be writeable, some common directories
  from the OS layer will be bind mounted (read only)
- `OS`: this type is for services that (for various reasons) must not run in a
  sandboxed and immutable environment, try to avoid this if possible.
  If you use this type you will lose all the benefits like:
    - predictability/reproducibility
    - easy rollbacks
    - safer hotfixes
    - better auditing
    - easier comparisons
- `CONTAINER` (coming soon): this type of service is used to run a command or OS in a
  light-weight namespace container. It makes use of [systemd-nspawn](https://www.freedesktop.org/software/systemd/man/systemd-nspawn.html).
  Use this for more complex cases that do not fit the two types above.
  Some examples are:
    - "toolbox" layers containing debugging tools, operational CLIs, and so on
    - Complex native services that consist of multiple daemons that need to
      run together

Current supported service types are defined in the `//metalos/host_configs/runtime_config.thrift`.

# Requirements

While it is possible to create a native service image entirely manually, this is
a bad idea and you should instead use the `native_service` macro defined in
`//metalos/bzl:service.bzl` that will ensure your image is constructed properly.
The image format produced by this macro is as described below.

## Image
Native services are defined in (almost entirely) standalone images.
This image must include all binaries that a service needs to run, as well as any
other kinds of dependencies.

In the early stages of the MVP, native service images will be used as the
`RootDirectory` of the service, so it should have some resemblance to a full OS
tree. In the future this may change to thinner images as a size optimization.

### MetalOS directory
Each native service image must contain a top-level directory `/metalos` under
which all MetalOS-specific information must be stored. Typically this includes the binary
that makes up the service and other support files like static configuration files.

## Service definition
Native services must provide a `service_t` instance (defined in
`//metalos/bzl:service.shape.bzl`) that is thrift-serialized and stored as
`/metalos/service.shape` in the service image.

On-host, this will expand to a single systemd service unit.

## Generators
Native services may define a Service Config Generator that MetalOS will run
before the unit starts.

This generator is sandboxed and only receives an `Input` thrift structure in its
stdin, as serialised thrift.

The `Input` thrift structure contains:
* the `HostIdentity` config for the host
* and the `DeploymentRuntimeConfig` config for the host

`//metalos/lib/service/service_config_generator.thrift` contains the input/output
API for generators.

Generators are allowed to produce a serialised `Output` thrift object.
This object contains a `Dropin` structure, which contains a set of systemd
drop-in settings for the service.

The generator binary must be contained in a separate image.
The `native_service` macro will take a buck target and automatically build an fbpkg
with it.

### Generator Lifecycle
The generator will be invoked every time the service (re)starts.
The generator will also be invoked every time the generator package itself
changes, which will also trigger a service restart if the generator's output
changes.

## Lifecycle
MetalOS will only update native service images at well-defined points, but the
underlying service must be able to be started/stopped/reloaded at any time. In
other words, service restarts should not take an inordinate amount of time or
have hard dependencies on external services that would block the service from
starting.

Units in a service image must not have external dependencies beyond basic system
features (such as networking being up) or other native services.

## Filesystem access
Native services have a read-only view of the entire filesystem, and may only
write to certain directories set in environment variables:

- `RUNTIME_DIRECTORY` is volatile and will be dropped after a service is stopped
- `STATE_DIRECTORY` is persistent across all invocations of a service
- `CACHE_DIRECTORY` is persistent across all invocations of a service, but is
  only kept on a best-effort basis. MetalOS may arbitrarily purge cache
  directories (but only if a service is stopped)
- `LOGS_DIRECTORY` has similar semantics to `CACHE_DIRECTORY`. Where possible,
  usage of journald is highly preferred over writing text logs to `LOGS_DIRECTORY`

Service units may also add extra writable paths via `BindPaths`, but this should
be used sparingly in favor of `metalctl` natively managing contents of the
rootfs where possible.

- `METALOS_SERVICE_IMAGE_ROOT` points to the root of the service image, and can
  be used in the service definition to correctly locate the binary of the application.

<InternalOnly>

## Service Certificates
MetalOS natively manages service certificate generation and renewal for services
that require certs, all you have to do is set the appropriate `service_name` in
the `certificates` section of `service_t`.

</InternalOnly>

# Implementation details

This section will be more detailed with follow-up diffs as more is implemented,
but the high level idea is as follows:

1. image downloaded (this is done ahead-of-time via `metalctl runtime-config stage`)
2. service config generator is evaluated, if present
3. service unit is generated from the `service_t` in the image
  - linked into `/run/systemd/system`
4. MetalOS drop-ins written to `/run/systemd/system/`
  - `RootDirectory` is set to a RW snapshot of the service image, if the service is of
    ServiceType::NATIVE`, otherwise it won't be present
  - `{RUNTIME,STATE,CACHE,LOGS}_DIRECTORY` environment variables are set for
     the service unit
  - `METALOS_SERVICE_IMAGE_ROOT` will be present for `OS` type services

## Generate service image

A convenience buck function at `//metalos/bzl/service/service.bzl` is provided to help
the user create a service image.
Example usage can be found in `//metalos/lib/service/tests/demo_service/`.

Example generator can be found in `//metalos/lib/service/tests/demo_service/generator/`.

Example definition of a `NATIVE` type service definition can be found in `//metalos/lib/service/tests/demo_service/TARGETS` (look for `metalos.service.demo.native`)

Example definition of a `OS` type service definition can be found in `//metalos/lib/service/tests/demo_service/TARGETS` (look for `metalos.service.demo.os`)
