#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
The classes produced by ImageItem are the various types of items that can be
installed into an image.  The compiler will verify that the specified items
have all of their requirements satisfied, and will then apply them in
dependency order.

To understand how the methods `provides()` and `requires()` affect
dependency resolution / installation order, start with the docblock at the
top of `provides.py`.
"""
import dataclasses
import enum
import hashlib
import inspect
import os
import subprocess
import tempfile
from typing import AnyStr, FrozenSet, List, Mapping, NamedTuple, Optional, Set

from antlir.compiler import procfs_serde
from antlir.config import repo_config
from antlir.fs_utils import META_BUILD_DIR, META_DIR, META_FLAVOR_FILE, Path
from antlir.rpm.yum_dnf_conf import YumDnf
from antlir.subvol_utils import Subvol
from pydantic import validator

from .mount_utils import mountpoints_from_subvol_meta


@enum.unique
class PhaseOrder(enum.Enum):
    """
    With respect to ordering, there are two types of operations that the
    image compiler performs against images.

    (1) Regular additive operations are naturally ordered with respect to
        one another by filesystem dependencies.  For example: we must create
        /usr/bin **BEFORE** copying `:your-tool` there.

    (2) Everything else, including:
         - RPM installation, which has a complex internal ordering, but
           simply needs needs a definitive placement as a block of `yum`
           operations -- due to `yum`'s complexity & various scripts, it's
           not desirable to treat installs as regular additive operations.
         - Path removals.  It is simplest to perform them in bulk, without
           interleaving with other operations.  Removals have a natural
           ordering with respect to each other -- child before parent, to
           avoid tripping "must_exist" unnecessarily.

    For the operations in (2), this class sets a justifiable deteriminstic
    ordering for black-box blocks of operations, and assumes that each
    individual block's implementation will order its internals sensibly.

    Phases will be executed in the order listed here.

    The operations in (1) are validated, dependency-sorted, and built after
    all of the phases have built.

    IMPORTANT: A new phase implementation MUST:
      - handle pre-existing protected paths via `_protected_path_set`
      - emit `ProvidesDoNotAccess` if it provides new protected paths
      - ensure that `_protected_path_set` in future phases knows how to
        discover these protected paths by inspecting the filesystem.
    See `ParentLayerItem`, `RemovePathsItem`, and `MountItem` for examples.

    Future: the complexity around protected paths is a symptom of a lack of
    a strong runtime abstraction.  Specifically, if `Subvol.run_as_root`
    used mount namespaces and read-only bind mounts to enforce protected
    paths (as is done today in `yum-dnf-from-snapshot`), then it would not
    be necessary for the compiler to know about them.
    """

    # This phase creates the subvolume, so it must precede all others.
    # There can only ever be one item in this phase.
    MAKE_SUBVOL = enum.auto()
    # Genrule layers cannot be combined with any item besides a single
    # `MAKE_SUBVOL`, so the ordering with respect to other items is
    # unimportant.
    GENRULE_LAYER = enum.auto()
    # Precedes REMOVE_PATHS because RPM removes **might** be conditional on
    # the presence or absence of files, and we don't want that extra entropy
    # -- whereas file removes fail or succeed predictably.  Precedes
    # RPM_INSTALL somewhat arbitrarily, since _gen_multi_rpm_actions
    # prevents install-remove conflicts between features.
    RPM_REMOVE = enum.auto()
    RPM_INSTALL = enum.auto()
    # Phase order for Facebook-only items. In this future this should be
    # removed in favor of enforcing custom layers for those features.
    FACEBOOK = enum.auto()
    # This MUST be a separate phase that comes after all the regular items
    # because the dependency sorter has no provisions for eliminating
    # something that another item `provides()`.
    #
    # By having this phase be last, we also allow removing files added by
    # RPM_INSTALL.  The downside is that this is a footgun.  The upside is
    # that e.g.  cleaning up yum log & caches can be done as a
    # `feature` instead of being code.  We might also use this to
    # remove e.g.  unnecessary parts of excessively monolithic RPMs.
    REMOVE_PATHS = enum.auto()


class LayerOpts(NamedTuple):
    artifacts_may_require_repo: bool
    build_appliance: Optional[Subvol]
    layer_target: str
    flavor: str
    # For images installing RPMs, both are required, and set by the flavor.
    rpm_installer: Optional[YumDnf]
    rpm_repo_snapshot: Optional[Path]
    target_to_path: Mapping[str, Path]
    subvolumes_dir: Path
    version_set_override: Optional[str]
    debug: bool = False
    allowed_host_mount_targets: FrozenSet[str] = frozenset()
    unsafe_bypass_flavor_check: bool = False

    def requires_build_appliance(self) -> Subvol:
        assert self.build_appliance is not None, (
            f"`image_layer` {self.layer_target} must set " "`build_appliance`"
        )
        return self.build_appliance


@dataclasses.dataclass(init=False, frozen=True)
# pyre-fixme[13]: Attribute `from_target` is never initialized.
class ImageItem:
    "Base class for the types of items that can be installed into images."

    from_target: str

    def phase_order(self) -> PhaseOrder:
        # pyre-fixme[7]: Expected `PhaseOrder` but got `None`.
        return None

    @classmethod
    def customize_fields(cls, kwargs):
        pass

    def __init__(self, **kwargs):
        """Constructor for ImageItem subclass.

        Differently from the constructor of a Python dataclass, this allows
        pre-processing the arguments before passing them to the original
        dataclass constructor.

        Furthermore, we only accept named arguments.

        We call class method `customize_fields` to modify kwargs (in place)
        before passing them to the original constructor of the dataclass.
        """
        self.__class__.customize_fields(kwargs)

        # We reproduce the logic from the constructor created by dataclasses.
        # Since we're pulling an internal function from the dataclasses module,
        # we need to cope with the API change introduced to that function in
        # Python 3.9, adding a new `globals` argument. We use `inspect` to
        # detect that case.
        dataclasses._init_fn(
            fields=[
                f
                for f in dataclasses.fields(self)
                if f._field_type
                in (dataclasses._FIELD, dataclasses._FIELD_INITVAR)
            ],
            frozen=True,
            has_post_init=False,
            self_name="self",
            **(
                {"globals": {}}
                if "globals"
                in inspect.getfullargspec(dataclasses._init_fn).args
                else {}
            ),
        )(self, **kwargs)


META_ARTIFACTS_REQUIRE_REPO = (
    META_DIR / "private/opts/artifacts_may_require_repo"
)


def _validate_artifacts_require_repo(
    dependency: Subvol, layer_opts: LayerOpts, message: str
):
    dep_arr = procfs_serde.deserialize_int(
        dependency.path(), META_ARTIFACTS_REQUIRE_REPO.decode()
    )
    # The check is <= because we should permit building @mode/dev layers
    # that depend on published @mode/opt images.  The CLI arg is bool.
    assert dep_arr <= int(layer_opts.artifacts_may_require_repo), (
        f"is trying to build a self-contained layer (layer_opts."
        f"artifacts_may_require_repo) with a dependency {dependency.path()} "
        f"({message}) that was marked as requiring the repo to run ({dep_arr})"
    )


def make_path_normal_relative(orig_d: str, *, meta_check: bool = True) -> str:
    """
    In image-building, we want relative paths that do not start with `..`,
    so that the effects of ImageItems are confined to their destination
    paths. For convenience, we accept absolute paths, too.
    """
    # lstrip so we treat absolute paths as image-relative
    d = os.path.normpath(orig_d).lstrip("/")
    if d == ".." or d.startswith("../"):
        raise AssertionError(f"path {orig_d} cannot start with ../")
    # This is a directory reserved for image build metadata, so we prevent
    # regular items from writing to it. `d` is never absolute here.
    # NB: This check is redundant with `ProvidesDoNotAccess(path=META_DIR)`,
    # this is just here as a fail-fast backup.
    if meta_check and (d + "/").startswith(META_DIR.decode()):
        raise AssertionError(f"path {orig_d} cannot start with {META_DIR}")
    return d


def validate_path_field_normal_relative(field: str):
    return validator(field, allow_reuse=True, pre=True)(
        lambda value: make_path_normal_relative(value)
    )


def protected_path_set(subvol: Optional[Subvol]) -> Set[Path]:
    """
    Identifies the protected paths in a subvolume.  Pass `subvol=None` if
    the subvolume doesn't yet exist (for FilesystemRoot).

    All paths will be relative to the image root, no leading /.  If a path
    has a trailing /, it is a protected directory, otherwise it is a
    protected file.

    Future: The trailing / convention could be eliminated, since any place
    actually manipulating these paths can inspect what's on disk, and act
    appropriately.  If the convention proves burdensome, this is an easy
    change -- mostly affecting this file, and `yum_dnf_from_snapshot.py`.
    """
    paths = {META_DIR}
    if subvol is not None:
        # NB: The returned paths here already follow the trailing / rule.
        for mountpoint in mountpoints_from_subvol_meta(subvol):
            paths.add(mountpoint)
    # Never absolute: yum-dnf-from-snapshot interprets absolute paths as
    # host paths.
    assert not any(p.startswith(b"/") for p in paths), paths
    # Return these as strings for use in yum-dnf-from-snapshot and
    # the logic in phases_provide.py.  Those callsites don't yet
    # understand the Path type.
    return paths


def is_path_protected(path: Path, protected_paths: Set[Path]) -> bool:
    # NB: The O-complexity could obviously be lots better, if needed.
    for prot_path in protected_paths:
        # Handle both protected files and directories.  This test is written
        # to return True even if `prot_path` is `/path/to/file` while `path`
        # is `/path/to/file/oops`.
        if (path + b"/").startswith(
            prot_path + (b"" if prot_path.endswith(b"/") else b"/")
        ):
            return True
    return False


def setup_meta_dir(subvol: Subvol, layer_opts: LayerOpts):
    subvol.run_as_root(
        ["mkdir", "--mode=0755", "--parents", subvol.path(META_DIR)]
    )
    # One might ask: why are we serializing this into the image instead of
    # just putting a condition on `ARTIFACTS_REQUIRE_REPO` into our Buck
    # macros?  Two reasons:
    #   - In the case of build appliance images, it is possible for a
    #     @mode/dev (in-place) build to use **either** a @mode/dev, or a
    #     @mode/opt (standalone) build appliance. The only way to know
    #     to know if the appliance needs a repo mount is to have a marker
    #     in the image.
    #   - By marking the images, we avoid having to conditionally add
    #     `--bind-repo-ro` flags in a bunch of places in our codebase.  The
    #     in-image marker enables `nspawn_in_subvol` to decide.
    if subvol.path(META_ARTIFACTS_REQUIRE_REPO).exists():
        _validate_artifacts_require_repo(subvol, layer_opts, "parent layer")
        # I looked into adding an `allow_overwrite` flag to `serialize`, but
        # it was too much hassle to do it right.
        subvol.run_as_root(["rm", subvol.path(META_ARTIFACTS_REQUIRE_REPO)])
    procfs_serde.serialize(
        layer_opts.artifacts_may_require_repo,
        subvol,
        META_ARTIFACTS_REQUIRE_REPO.decode(),
    )

    build_appliance = layer_opts.build_appliance
    flavor = layer_opts.flavor

    # Add metadata info
    if not os.path.isdir(subvol.path(META_BUILD_DIR)):
        subvol.run_as_root(
            ["mkdir", "--mode=0755", subvol.path(META_BUILD_DIR)]
        )

    subvol.overwrite_path_as_root(
        META_BUILD_DIR / "target", f"{layer_opts.layer_target}\n"
    )

    subvol.overwrite_path_as_root(
        META_BUILD_DIR / "revision",
        f"{repo_config().vcs_revision}\n",
    )

    if layer_opts.unsafe_bypass_flavor_check:
        subvol.overwrite_path_as_root(META_FLAVOR_FILE, flavor)
        return

    # TODO: Remove the existence check once the flavor has been written
    # in all built sendstreams.
    if build_appliance and build_appliance.path(META_FLAVOR_FILE).exists():
        build_appliance_flavor = build_appliance.read_path_text(
            META_FLAVOR_FILE
        )
        assert flavor == build_appliance_flavor, (
            f"The flavor `{flavor}` given differs from "
            f"the flavor `{build_appliance_flavor}` of the "
            "build appliance`."
        )

    if subvol.path(META_FLAVOR_FILE).exists():
        subvol_flavor = subvol.read_path_text(META_FLAVOR_FILE)
        assert flavor == subvol_flavor, (
            f"The flavor `{flavor}` given differs from the "
            f"flavor `{subvol_flavor}` already written in the subvol`."
        )
    else:
        subvol.overwrite_path_as_root(META_FLAVOR_FILE, flavor)


def _hash_path(path: str, algorithm: str) -> str:
    "Returns the hex digest"
    algo = hashlib.new(algorithm)
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(4096), b""):
            algo.update(chunk)
    return algo.hexdigest()


def _generate_file(
    temp_dir: str, generator: bytes, generator_args: List[str]
) -> str:
    # API design notes:
    #
    #  1) The generator takes an output directory, not a file, because we
    #     prefer not to have to hardcode the name of the output file in the
    #     TARGETS file -- that would make it more laborious to change the
    #     compression format for tarballs, e.g.  Instead, the generator
    #     prints the name of the created file to stdout.  This does not
    #     introduce nondeterminism for current consumers of `image.source`:
    #       - A tarball name cannot affect the result of its extraction.
    #       - `yum` ignores the RPM name, and reads metadata from its content.
    #       - `install_file` specifies the destination filename in `TARGETS`.
    #
    #     Since TARGETS already hardcodes a content hash, requiring the name
    #     would not be far-fetched, this approach just seemed cleaner.
    #
    #  2) `temp_dir` is last since this allows the use of inline scripts via
    #     `generator_args` with e.g. `/bin/bash`.
    #
    # Future: it would be best to sandbox the generator to limit its
    # filesystem writes.  At the moment, we trust rule authors not to abuse
    # this feature and write stuff outside the given directory.
    output_filename = subprocess.check_output(
        [generator, *generator_args, temp_dir],
        cwd=repo_config().repo_root,
    ).decode()
    assert output_filename.endswith("\n"), (generator, output_filename)
    output_filename = os.path.normpath(output_filename[:-1])
    assert not output_filename.startswith(
        "/"
    ) and not output_filename.startswith("../"), output_filename
    return os.path.join(temp_dir, output_filename)


def _image_source_path(
    layer_opts: LayerOpts,
    *,
    # pyre-fixme[9]: source has type `AnyStr`; used as `None`.
    source: AnyStr = None,
    # pyre-fixme[9]: layer has type `Subvol`; used as `None`.
    layer: Subvol = None,
    # pyre-fixme[9]: path has type `AnyStr`; used as `None`.
    path: AnyStr = None,
) -> Path:
    assert (source is None) ^ (layer is None), (source, layer, path)
    source = Path.or_none(source)
    # Absolute `path` is still relative to `source` or `layer`
    # pyre-fixme[9]: path has type `AnyStr`; used as `Path`.
    # pyre-fixme[6]: Expected `Optional[bytes]` for 1st param but got `str`.
    path = Path((path and path.lstrip("/")) or ".")

    if source:
        return (source / path).normpath()

    if os.path.exists(layer.path(META_ARTIFACTS_REQUIRE_REPO)):
        _validate_artifacts_require_repo(layer, layer_opts, "image.source")
    return Path(layer.path(path))


def _make_image_source_item(
    item_cls,
    exit_stack,
    layer_opts: LayerOpts,
    *,
    source: Optional[Mapping[str, Path]],
    **kwargs,
):
    if source is None:
        return item_cls(**kwargs, source=None)

    assert 1 == (
        bool(source.get("generator"))
        + bool(source.get("source"))
        + bool(source.get("layer"))
    ), source

    # `generator` dynamically creates a temporary source file for the item
    # being constructed.  The file is deleted when the `exit_stack` context
    # exits.
    #
    # NB: With `generator`, identical constructor arguments to this factory
    # will create different items, so if we needed item deduplication to
    # work across inputs, this is broken.  However, I don't believe the
    # compiler relies on that.  If we need it, it should not be too hard to
    # share the same source file for all generates with the same command --
    # you'd add a global map of (generator, args) -> output, perhaps using
    # weakref hooks to refcount output files and GC them.
    # pyre-fixme[16]: `Mapping` has no attribute `pop`.
    generator = source.pop("generator", None)
    generator_args = source.pop("generator_args", None)
    generator_args = list(generator_args) if generator_args is not None else []
    if generator or generator_args:
        # pyre-fixme[16]: `Mapping` has no attribute `__setitem__`.
        source["source"] = _generate_file(
            exit_stack.enter_context(tempfile.TemporaryDirectory()),
            generator,
            generator_args,
        )

    algo_and_hash = source.pop("content_hash", None)
    # pyre-fixme[6]: Expected `Subvol` for 2nd param but got `str`.
    source_path = _image_source_path(layer_opts, **source)
    if algo_and_hash:
        algorithm, expected_hash = algo_and_hash.split(":")
        # pyre-fixme[6]: Expected `str` for 1st param but got `Path`.
        actual_hash = _hash_path(source_path, algorithm)
        if actual_hash != expected_hash:
            raise AssertionError(
                f"{item_cls} {kwargs} failed hash validation, got {actual_hash}"
            )

    return item_cls(**kwargs, source=source_path)


def image_source_item(item_cls, exit_stack, layer_opts: LayerOpts):
    return lambda **kwargs: _make_image_source_item(
        item_cls, exit_stack, layer_opts, **kwargs
    )
