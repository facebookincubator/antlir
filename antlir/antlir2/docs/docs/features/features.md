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

## Self-explanatory features

:::note

In the near future, this section will be replaced with an auto-generated section
with the actual buck api docs.

:::

### `feature.install`

Install a source file/directory (or output of a buck rule) into the image

### `feature.ensure_dirs_exist`

Create an entire directory chain owned by the same `user:group` and the same
mode bits

### `feature.ensure_subdirs_exist`

Create some directories under an existing directory, where only the `user:group`
and mode bits of the `subdirs_to_create` matter, as long as the parent
directories exist.

### `feature.genrule`

Run an arbitrary command inside the image as any user that exists in the image.
This cannot provide you any of the safety that `antlir2` normally provides, so
should be used as a last resort.

### `feature.remove`

Delete a file/directory.

### `feature.ensure_(file|dir)_symlink`

Create a symlink to a file or directory.

### `feature.rpms_install`

Install RPMs by name, nevra or `.rpm` artifact.

### `feature.rpms_upgrade`

Upgrade RPMs to the newest allowed version.

### `feature.rpms_remove_if_exists`

Remove RPMs by name or nevra if they are installed.

### `feature.(user|group)_add`

Add a new user/group to `/etc/passwd` and friends.

<FbInternalOnly>
There are also some <a href="./fb">Meta-only internal features</a>
</FbInternalOnly>
