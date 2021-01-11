"""
This exists to build a `locale-archive` for a specific set of locales, resulting in a
far smaller and stripped down size.  Since most services do not require more than
one locale, we can save a lot of space by only building what we need.
"""

load("//antlir/bzl:image_foreign_layer.bzl", "image_foreign_layer")

def image_build_locale_archive(name, parent_layer, locales):
    """
    `parent_layer` must have both the locale desired and the
    `build-locale-archive` binary to rebuild the archive.
    """
    image_foreign_layer(
        name = name,
        cmd = [
            "bash",
            "-o",
            "pipefail",
            "-uec",
            r"""\
cp /usr/lib/locale/locale-archive /usr/lib/locale/locale-archive.tmpl
build-locale-archive --install-langs="{}"
cp /usr/lib/locale/locale-archive /
        """.format(":".join(locales)),
        ],
        parent_layer = parent_layer,
        rule_type = "image_build_locale_archive",
        user = "root",
        antlir_rule = "user-internal",
    )
