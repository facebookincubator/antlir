#!/usr/bin/env python3
'''
The classes produced by ImageItem are the various types of items that can be
installed into an image.  The compiler will verify that the specified items
have all of their requirements satisfied, and will then apply them in
dependency order.

To understand how the methods `provides()` and `requires()` affect
dependency resolution / installation order, start with the docblock at the
top of `provides.py`.
'''
import enum
import hashlib
import os
import subprocess
import tempfile

from typing import AnyStr, List, Mapping, NamedTuple, Optional, Set

from compiler import procfs_serde
from compiler.enriched_namedtuple import metaclass_new_enriched_namedtuple
from fs_image.fs_utils import Path
from subvol_utils import Subvol

from .mount_utils import mountpoints_from_subvol_meta

# This path is off-limits to regular image operations, it exists only to
# record image metadata and configuration.  This is at the root, instead of
# in `etc` because that means that `FilesystemRoot` does not have to provide
# `etc` and determine its permissions.  In other words, a top-level "meta"
# directory makes the compiler less opinionated about other image content.
#
# NB: The trailing slash is significant, making this a protected directory,
# not a protected file.
META_DIR = 'meta/'


@enum.unique
class PhaseOrder(enum.Enum):
    '''
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
           avoid tripping "assert_exists" unnecessarily.

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
    paths (as is done today in `yum-from-snapshot`), then it would not be
    necessary for the compiler to know about them.
    '''
    # This phase creates the subvolume, so it must precede all others.
    # There can only ever be one item in this phase.
    MAKE_SUBVOL = enum.auto()
    # Precedes REMOVE_PATHS because RPM removes **might** be conditional on
    # the presence or absence of files, and we don't want that extra entropy
    # -- whereas file removes fail or succeed predictably.  Precedes
    # RPM_INSTALL somewhat arbitrarily, since _gen_multi_rpm_actions
    # prevents install-remove conflicts between features.
    RPM_REMOVE = enum.auto()
    RPM_INSTALL = enum.auto()
    # Building RPMs should be close to last as it expects all other actions to
    # set up for the RPM build.
    RPM_BUILD = enum.auto()
    # This MUST be a separate phase that comes after all the regular items
    # because the dependency sorter has no provisions for eliminating
    # something that another item `provides()`.
    #
    # By having this phase be last, we also allow removing files added by
    # RPM_INSTALL.  The downside is that this is a footgun.  The upside is
    # that e.g.  cleaning up yum log & caches can be done as an
    # `image_feature` instead of being code.  We might also use this to
    # remove e.g.  unnecessary parts of excessively monolithic RPMs.
    REMOVE_PATHS = enum.auto()


class LayerOpts(NamedTuple):
    artifacts_may_require_repo: bool
    build_appliance: str
    layer_target: str
    yum_from_snapshot: str
    target_to_path: Mapping[str, str]
    subvolumes_dir: str


class ImageItem(type):
    'A metaclass for the types of items that can be installed into images.'
    def __new__(metacls, classname, bases, dct):

        # Future: `deepfrozen` has a less clunky way of doing this
        def customize_fields(kwargs):
            fn = dct.get('customize_fields')
            if fn:
                fn(kwargs)
            return kwargs

        # Some items, like RPM actions, are not sorted by dependencies, but
        # get a fixed installation order.  The absence of a phase means the
        # item is ordered via the topo-sort in `dep_graph.py`.
        class PhaseOrderBase:
            __slots__ = ()

            def phase_order(self):
                return None

        return metaclass_new_enriched_namedtuple(
            __class__,
            ['from_target'],
            metacls, classname, (PhaseOrderBase,) + bases, dct,
            customize_fields
        )


META_ARTIFACTS_REQUIRE_REPO = os.path.join(
    META_DIR, 'private/opts/artifacts_may_require_repo',
)


def _validate_artifacts_require_repo(
    dependency: Subvol, layer_opts: LayerOpts, message: str,
):
    dep_arr = procfs_serde.deserialize_int(
        dependency, META_ARTIFACTS_REQUIRE_REPO,
    )
    # The check is <= because we should permit building @mode/dev layers
    # that depend on published @mode/opt images.  The CLI arg is bool.
    assert dep_arr <= int(layer_opts.artifacts_may_require_repo), (
        f'is trying to build a self-contained layer (layer_opts.'
        f'artifacts_may_require_repo) with a dependency {dependency.path()} '
        f'({message}) that was marked as requiring the repo to run ({dep_arr})'
    )


def make_path_normal_relative(orig_d: str) -> str:
    '''
    In image-building, we want relative paths that do not start with `..`,
    so that the effects of ImageItems are confined to their destination
    paths. For convenience, we accept absolute paths, too.
    '''
    # lstrip so we treat absolute paths as image-relative
    d = os.path.normpath(orig_d).lstrip('/')
    if d == '..' or d.startswith('../'):
        raise AssertionError(f'path {orig_d} cannot start with ../')
    # This is a directory reserved for image build metadata, so we prevent
    # regular items from writing to it. `d` is never absolute here.
    # NB: This check is redundant with `ProvidesDoNotAccess(path=META_DIR)`,
    # this is just here as a fail-fast backup.
    if (d + '/').startswith(META_DIR):
        raise AssertionError(f'path {orig_d} cannot start with {META_DIR}')
    return d


def coerce_path_field_normal_relative(kwargs, field: str):
    d = kwargs.get(field)
    if d is not None:
        kwargs[field] = make_path_normal_relative(d)


def protected_path_set(subvol: Optional[Subvol]) -> Set[str]:
    '''
    Identifies the protected paths in a subvolume.  Pass `subvol=None` if
    the subvolume doesn't yet exist (for FilesystemRoot).

    All paths will be relative to the image root, no leading /.  If a path
    has a trailing /, it is a protected directory, otherwise it is a
    protected file.

    Future: The trailing / convention could be eliminated, since any place
    actually manipulating these paths can inspect what's on disk, and act
    appropriately.  If the convention proves burdensome, this is an easy
    change -- mostly affecting this file, and `yum_from_snapshot.py`.
    '''
    paths = {META_DIR}
    if subvol is not None:
        # NB: The returned paths here already follow the trailing / rule.
        for mountpoint in mountpoints_from_subvol_meta(subvol):
            paths.add(mountpoint.lstrip('/'))
    # Never absolute: yum-from-snapshot interprets absolute paths as host paths
    assert not any(p.startswith('/') for p in paths), paths
    return paths


def is_path_protected(path: str, protected_paths: Set[str]) -> bool:
    # NB: The O-complexity could obviously be lots better, if needed.
    for prot_path in protected_paths:
        # Handle both protected files and directories.  This test is written
        # to return True even if `prot_path` is `/path/to/file` while `path`
        # is `/path/to/file/oops`.
        if (path + '/').startswith(
            prot_path + ('' if prot_path.endswith('/') else '/')
        ):
            return True
    return False


def ensure_meta_dir_exists(subvol: Subvol, layer_opts: LayerOpts):
    subvol.run_as_root([
        'mkdir', '--mode=0755', '--parents', subvol.path(META_DIR),
    ])
    # One might ask: why are we serializing this into the image instead
    # of just putting a condition on `built_artifacts_require_repo`
    # into our Buck macros? Two reasons:
    #   - In the case of build appliance images, it is possible for a
    #     @mode/dev (in-place) build to use **either** a @mode/dev, or a
    #     @mode/opt (standalone) build appliance. The only way to know
    #     to know if the appliance needs a repo mount is to have a marker
    #     in the image.
    #   - By marking the images, we avoid having to conditionally add
    #     `--bind-repo-ro` flags in a bunch of places in our codebase.  The
    #     in-image marker enables `nspawn_in_subvol` to decide.
    if os.path.exists(subvol.path(META_ARTIFACTS_REQUIRE_REPO)):
        _validate_artifacts_require_repo(subvol, layer_opts, 'parent layer')
        # I looked into adding an `allow_overwrite` flag to `serialize`, but
        # it was too much hassle to do it right.
        subvol.run_as_root(['rm', subvol.path(META_ARTIFACTS_REQUIRE_REPO)])
    procfs_serde.serialize(
        layer_opts.artifacts_may_require_repo,
        subvol,
        META_ARTIFACTS_REQUIRE_REPO,
    )


def _hash_path(path: str, algorithm: str) -> str:
    'Returns the hex digest'
    algo = hashlib.new(algorithm)
    with open(path, 'rb') as f:
        for chunk in iter(lambda: f.read(4096), b''):
            algo.update(chunk)
    return algo.hexdigest()


def _generate_file(
    temp_dir: str, generator: bytes, generator_args: List[str],
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
    output_filename = subprocess.check_output([
        generator, *generator_args, temp_dir,
    ]).decode()
    assert output_filename.endswith('\n'), (generator, output_filename)
    output_filename = os.path.normpath(output_filename[:-1])
    assert (
        not output_filename.startswith('/')
        and not output_filename.startswith('../')
    ), output_filename
    return os.path.join(temp_dir, output_filename)


def _image_source_path(
    layer_opts: LayerOpts, *,
    source: AnyStr = None, layer: Subvol = None, path: AnyStr = None,
) -> Path:
    assert (source is None) ^ (layer is None), (source, layer, path)
    source = Path.or_none(source)
    # Absolute `path` is still relative to `source` or `layer`
    path = Path((path and path.lstrip('/')) or '.')

    if source:
        return (source / path).normpath()

    if os.path.exists(layer.path(META_ARTIFACTS_REQUIRE_REPO)):
        _validate_artifacts_require_repo(layer, layer_opts, 'image.source')
    return Path(layer.path(path))


def _make_image_source_item(
    item_cls, exit_stack, layer_opts: LayerOpts, *,
    source: Optional[Mapping[str, str]], **kwargs,
):
    if source is None:
        return item_cls(**kwargs, source=None)

    assert 1 == (
        bool(source.get('generator')) + bool(source.get('source')) +
        bool(source.get('layer'))
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
    generator = source.pop('generator', None)
    generator_args = source.pop('generator_args', None)
    generator_args = list(generator_args) if generator_args is not None else []
    if generator or generator_args:
        source['source'] = _generate_file(
            exit_stack.enter_context(tempfile.TemporaryDirectory()),
            generator,
            generator_args,
        )

    algo_and_hash = source.pop('content_hash', None)
    source_path = _image_source_path(layer_opts, **source)
    if algo_and_hash:
        algorithm, expected_hash = algo_and_hash.split(':')
        actual_hash = _hash_path(source_path, algorithm)
        if actual_hash != expected_hash:
            raise AssertionError(
                f'{item_cls} {kwargs} failed hash validation, got {actual_hash}'
            )

    return item_cls(**kwargs, source=source_path)


def image_source_item(item_cls, exit_stack, layer_opts: LayerOpts):
    return lambda **kwargs: _make_image_source_item(
        item_cls, exit_stack, layer_opts, **kwargs,
    )
