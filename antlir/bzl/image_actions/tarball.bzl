"""
`image.tarball("files/xyz.tar", "/a/b")` extracts tarball located at `files/xyz.tar` to `/a/b` in the image --
  - `source` is one of:
    - an `image.source` (docs in `image_source.bzl`), or
    - the path of a target outputting a tarball target path,
      e.g. an `export_file` or a `genrule`
  - `dest` is the destination of the unpacked tarball in the image.
    This is an image-absolute path to a directory that must be created
    by another `image_feature` item.
"""

load("//antlir/bzl:maybe_export_file.bzl", "maybe_export_file")
load("//antlir/bzl:target_tagger.bzl", "image_source_as_target_tagged_dict", "new_target_tagger", "target_tagger_to_feature")

def image_tarball(source, dest, force_root_ownership = False):
    target_tagger = new_target_tagger()
    tarball_spec = {
        "force_root_ownership": force_root_ownership,
        "into_dir": dest,
        "source": image_source_as_target_tagged_dict(
            target_tagger,
            maybe_export_file(source),
        ),
    }
    return target_tagger_to_feature(
        target_tagger,
        items = struct(tarballs = [tarball_spec]),
        # The `fake_macro_library` docblock explains this self-dependency
        extra_deps = ["//antlir/bzl/image_actions:tarball"],
    )
