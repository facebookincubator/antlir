---
id: helper-buck-targets
title: Helper Buck Targets
---

## Helper Buck Targets

Antlir exposes many different `buck run` helper targets that are invoked when appending a suffix to the target name. For example, an `image.layer` target named `foo` with helper target `=container` can be run with:

```
buck run foo=container
```

These interactive helper targets may change and should not be used in automation without asking the Antlir team. In addition, any suffix that begins with a double underscore, such as `__test_layer`, is not to be relied upon at all and is strictly for implementation purposes.

### `image.layer`

- `=container` will launch the user into an interactive shell inside the image.
- `=systemd` will boot `systemd` inside the image.

### `image.*_unittest`

- `=container` will launch the user into an interactive shell inside the image.

### `image_chef_solo`

- `=debug-chef` will launch the user into an interactive shell inside the image.
- `=update-fbpkg-db` will build chef_solo image, discover fbpkg installed using fbpkg_proxy, and add missing packages into Antlir fbpkg DB.

### `vm.*_unittest`

- `=vmtest` will run the test binary located at `/vmtest/test` inside a vm with the latest stable release kernel inside the image and launch the user into an interactive shell via ssh.
