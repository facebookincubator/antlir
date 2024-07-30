---
sidebar_position: 80
---

# Image Reproducibility

:::note

`antlir2` currently only produces _equivalent_ images

When we say "reproducible", we mean that everything installed in the image
(buck-built binaries, files, upstream packages, etc) are equivalent (binaries
are built from same sources, all installed packages have same exact versions),
**not** that the final artifacts are _bitwise_ identical.

:::

## Upstream Packages

Distro package managers are not designed for reproducibility. As soon as you
`dnf update` to get a new set of metadata, any `install` commands you run can
pick up new versions of packages that you didn't know about yesterday. On
interactive systems, this is desirable - who doesn't want to run the latest and
greatest?!

For a deterministic image builder though, this is far from ideal. We want to be
able to provide our users with a guarantee that if they checkout a source rev
from a month ago, that the image is built exactly the same as it would have been
when committed. If we can provide that property, we can both install a lot more
confidence in what was running in production and enable very useful things that
engineers have come to expect, like `hg bisect`-ing a test failure.

We get the best of both worlds in `antlir2` by automatically updating the
upstream packages that we make available during image builds (see
[here](../internals/rpms) for more details on this automation).

Once we have a persistent view of upstream repositories, we can present that to
package managers as their view of the world and builds become reproducible.

:::note

`antlir2` currently only supports RPM

The same concepts would be true of any other package manager we add support for
in the future.

:::
