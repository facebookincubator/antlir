---
id: faq
title: FAQ
---

# General


### How do I inspect the contents of an `image.layer`?

For a real OS image, just run `buck run :LAYER-NAME-container`.

For an image that lacks a shell, you can do something like this:

```py
load("//antlir/bzl:constants.bzl", "REPO_CFG")
load("//antlir/bzl:image.bzl", "image")

image.layer(name = "my-image", ...)

image.layer(
  name = "inspect-my-image",
  parent_layer = REPO_CFG.build_appliance_default,
  features = [image.layer_mount(":my-image", "/my")]
)
```

And then `buck run :inspect-my-image-container`.

**DO NOT RELY ON THIS, this is subject to change without warning:** In the
current implementation, you can find the layer's btrfs subvolume (but not
its mounts) under `buck-image-out/volume/targets/<layername>:NONCE/*/`.
The second wildcard is the subvolume name, which defaults to `volume`.
You can find the full path by running:

```bash
buck run //antlir:find-built-subvol -- "$(
  buck targets --show-full-output :your-layer | cut -d\  -f2-
)"
```

### How do I inspect a packaged image?

  - For tarballs, use `tar xf`.
  - For Squashfs, either mount it, or `unsquashfs`.
  - For btrfs loopbacks, either mount it, or `btrfs restore`.
  - For btrfs sendstreams, here's how to receive and mount
    `image.sendstream.zst`:
    ```bash
    truncate -s 100G image.btrfs
    mkfs.btrfs image.btrfs
    sudo unshare -m
    mkdir image
    mount image.btrfs image
    cd image
    zstd -cd ../image.sendstream.zst | btrfs receive .
    ls  # To find the resulting subvolume directory
    ```

## RPMs


### My RPM exists in the repos, but `image.rpms_install` fails to install it

Troubleshooting steps:

  - Did you specify your RPM name correctly? Remember that `foo-project` is OK,
    but `foo-project-1.12` is not supported. You can specify the version of RPM
    via `rpm_version_set_overrides` argument of `image.opts`. See docs for
    `rpms_install` in the [API page](/docs/api/image).

  - [Inspect the repo snapshot](#how-do-i-inspect-the-rpm-snapshot-db) and run
    this SQL statement, replacing `pv` with your RPM name:

    ```sql
    SELECT "repo", "path", "error", "error_json" FROM "rpm" WHERE "name" = "pv"
    ```

    If you get no rows, this means the RPM isn't actually in the snapshot.
    Or, you may see a non-empty error column, giving you a breadcrumb to
    debug the snapshot itself.


### How do I inspect the RPM snapshot DB?

**(advanced)** To look at the internals of the RPM snapshot DB, first find
the `rpm_repo_snapshot` target for your snapshot. If it is at `//RPM:SNAP`:

```bash
sqlite3 file:"$(readlink -f "$(
   buck build //RPM:SNAP --show-full-output | cut -f 2 -d ' '
)")"/snapshot/snapshot.sql3?mode=ro``
```

From there, one can get stats on RPM errors via:

```sql
SELECT "error", COUNT(1) FROM "rpm" WHERE "error" IS NOT NULL GROUP BY "error";
```

One could also see which versions of e.g. the "netperf" RPM are available with:

```sql
SELECT * from "rpm" WHERE "name" IS "netperf";
```

### How do I download RPMs from a particular snapshot?

First, you need a build appliance target path.  Grep the code for
`build_appliance_default.*//` to find the default one.  If its target path
is `//BUILD:APPLIANCE`, and it uses `dnf`, then the following code will
put any RPMs matching `jq` into your current directory:

```bash
(set -o pipefail && buck run //BUILD:APPLIANCE-container \
  -- --user=root -- /bin/bash -uexc '
    cd $(mktemp -d)
    dnf download jq >&2
    tar cf - .
  ' | tar xf -)
```

For `yum`, this is a bit harder, since Antlir does not yet wrap `yumdownloader`.

```bash
(set -o pipefail && buck run //BUILD:APPLIANCE-container \
  -- --user=root -- /bin/bash -uexc '
    cd $(mktemp -d)
    yumdownloader \
      --config \
      /__antlir__/rpm/default-snapshot-for-installer/yum/yum/etc/yum/yum.conf \
      jq >&2
    tar cf - .
  ' | tar xf -)
```

NB: We could get the easy `dnf`-like behavior by aliasing `yum download` to
`yumdownloader` in our wrapper, if this proves to be a common use-case.
