#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
NB: Surprisingly, we don't need any special cleanup for the `mount` operations
    performed by `build` and `clone_mounts` -- it appears that subvolume
    deletion, as performed by `subvolume_garbage_collector.py`, implicitly
    lazy-unmounts any mounts therein.
"""
import json
import os
from dataclasses import dataclass
from typing import Iterator, Mapping, NamedTuple, Optional, Union

from antlir.compiler import procfs_serde
from antlir.compiler.items.common import (
    assert_running_inside_ba,
    ImageItem,
    LayerOpts,
    make_path_normal_relative,
)
from antlir.compiler.items.mount_utils import META_MOUNTS_DIR, MOUNT_MARKER
from antlir.compiler.requires_provides import ProvidesDoNotAccess, RequireDirectory
from antlir.config import antlir_dep
from antlir.find_built_subvol import find_built_subvol
from antlir.fs_utils import Path
from antlir.subvol_utils import Subvol


class BuildSource(NamedTuple):
    type: str
    source: Union[Path, str]

    def to_path(
        self, *, target_to_path: Mapping[str, Path], subvolumes_dir: Path
    ) -> Path:
        if self.type == "layer":
            out_path = target_to_path.get(str(self.source))
            if out_path is None:
                raise AssertionError(f"MountItem could not resolve {self.source}")
            return find_built_subvol(out_path, subvolumes_dir=subvolumes_dir).path()
        elif self.type == "host":
            return Path(self.source)
        else:  # pragma: no cover
            raise AssertionError(f'Bad mount source "{self.type}" for {self.source}')


@dataclass(frozen=True)
class RuntimeSource:
    type: str
    # Note: these are specific to the FB runtime
    package: Optional[str] = None
    tag: Optional[str] = None
    uuid: Optional[str] = None


@dataclass(frozen=True)
class LayerPublisher:
    package: str
    # JSON contents of a shape target which can then be parsed
    shape_target_contents: str


@dataclass(frozen=True)
class Mount:
    mountpoint: str
    build_source: BuildSource
    is_directory: bool
    runtime_source: Optional[RuntimeSource] = None
    layer_publisher: Optional[LayerPublisher] = None


@dataclass(init=False, frozen=True)
# pyre-fixme[13]: Attribute `build_source` is never initialized.
# pyre-fixme[13]: Attribute `is_directory` is never initialized.
# pyre-fixme[13]: Attribute `layer_publisher` is never initialized.
# pyre-fixme[13]: Attribute `mountpoint` is never initialized.
# pyre-fixme[13]: Attribute `runtime_source` is never initialized.
class MountItem(ImageItem):
    mountpoint: str
    build_source: BuildSource
    runtime_source: str
    is_directory: bool
    layer_publisher: str

    @classmethod
    def customize_fields(cls, kwargs) -> None:
        layer_opts = kwargs.pop("layer_opts", None)
        target = kwargs.pop("target")
        cfg = kwargs.pop("mount_config")
        assert (target is None) ^ (
            cfg is None
        ), f"Exactly one of `target` or `mount_config` must be set in {kwargs}"
        if cfg is not None:
            cfg = cfg.copy()  # We must not mutate our input!
        else:
            with open(Path(target) / "mountconfig.json") as f:
                cfg = json.load(f)

        default_mountpoint = cfg.pop("default_mountpoint", None)
        if kwargs.get("mountpoint") is None:  # Missing or None => use default
            kwargs["mountpoint"] = default_mountpoint
            if kwargs["mountpoint"] is None:
                raise AssertionError(f"MountItem {kwargs} lacks mountpoint")
        coerce_path_field_normal_relative(kwargs, "mountpoint")

        kwargs["is_directory"] = cfg.pop("is_directory")

        kwargs["build_source"] = BuildSource(**cfg.pop("build_source"))
        if kwargs["build_source"].type == "host" and not (
            kwargs["from_target"] in layer_opts.allowed_host_mount_targets
            or kwargs["from_target"].startswith(antlir_dep("compiler/test"))
        ):
            raise AssertionError(
                "Host mounts cause containers to be non-hermetic and "
                "fragile, so they must be located under one of "
                f"{layer_opts.allowed_host_mount_targets} "
                "to enable close review by the owners of `antlir`."
            )

        # This is supposed to be the run-time equivalent of `build_source`,
        # but for us it's just an opaque JSON blob that the runtime wants.
        # Hack: We serialize this back to JSON since the compiler expects
        # items to be hashable, and the source WILL contain dicts.
        runtime_source = cfg.pop("runtime_source", None)
        # Future: once runtime_source grows a schema, use it here?
        if runtime_source and runtime_source.get("type") == "host":
            raise AssertionError(
                f"Only `build_source` may specify host mounts: {kwargs}"
            )
        kwargs["runtime_source"] = json.dumps(runtime_source, sort_keys=True)

        kwargs["layer_publisher"] = json.dumps(
            cfg.pop("layer_publisher", None), sort_keys=True
        )

        assert cfg == {}, f"Unparsed fields in {kwargs} mount_config: {cfg}"

    def provides(self):
        # For now, nesting of mounts is not supported, and we certainly
        # cannot allow regular items to write inside a mount.
        yield ProvidesDoNotAccess(path=Path(self.mountpoint))

    def requires(self):
        # We don't require the mountpoint itself since it will be shadowed,
        # so this item just makes it with default permissions.
        yield RequireDirectory(path=Path(self.mountpoint).dirname())

    def build(self, subvol: Subvol, layer_opts: LayerOpts) -> None:
        mount_dir = META_MOUNTS_DIR / self.mountpoint / MOUNT_MARKER
        for name, data in (
            # NB: Not exporting self.mountpoint since it's implicit in the path.
            ("is_directory", self.is_directory),
            ("build_source", self.build_source._asdict()),
            ("runtime_source", json.loads(self.runtime_source)),
            ("layer_publisher", json.loads(self.layer_publisher)),
        ):
            procfs_serde.serialize(data, subvol, Path(mount_dir / name).decode())

        source_path = self.build_source.to_path(
            target_to_path=layer_opts.target_to_path,
            subvolumes_dir=layer_opts.subvolumes_dir,
        )
        # Support mounting directories and non-directories...  This check
        # follows symlinks for the mount source, which seems correct.
        is_dir = os.path.isdir(source_path)
        assert is_dir == self.is_directory, self
        assert_running_inside_ba()
        if is_dir:
            os.makedirs(
                subvol.path(self.mountpoint),
                mode=0o755,
                exist_ok=False,  # be explicit
            )
        else:  # Regular files, device nodes, FIFOs, you name it.
            # The mode of this mountpoint will be shadowed,
            # so it doesn't matter waht the mode is.
            subvol.path(self.mountpoint).touch()


# Not covering, since this would require META_MOUNTS_DIR to be unreadable.
def _raise(ex):  # pragma: no cover
    raise ex


def mounts_from_meta(volume_path: Path) -> Iterator[Mount]:
    """
    Returns a list of constructed `MountItem`s built from the a .meta/ dir
    directly under the provided path.
    """
    mounts_path = volume_path / META_MOUNTS_DIR
    if not mounts_path.exists():
        return

    for path, _next_dirs, _files in os.walk(
        # We are not `chroot`ed, so following links could access outside the
        # image; `followlinks=False` is the default -- explicit for safety.
        mounts_path,
        onerror=_raise,
        followlinks=False,
    ):
        relpath = Path(path).relpath(mounts_path)
        if relpath.basename() == MOUNT_MARKER:
            mountpoint = relpath.dirname()
            assert not mountpoint.endswith(b"/"), mountpoint

            # Deserialize the mount madness
            cfg = procfs_serde.deserialize_untyped(
                volume_path, Path(META_MOUNTS_DIR / relpath).decode()
            )

            # Convert config info proper types and create a Mount
            mount = Mount(
                mountpoint=mountpoint.decode(),
                build_source=BuildSource(**cfg.pop("build_source")),
                is_directory={"0": False, "1": True}[cfg.pop("is_directory")],
                runtime_source=(
                    RuntimeSource(**cfg.pop("runtime_source"))
                    if "runtime_source" in cfg
                    else None
                ),
                layer_publisher=(
                    LayerPublisher(**cfg.pop("layer_publisher"))
                    if "layer_publisher" in cfg
                    else None
                ),
            )

            assert not cfg, cfg
            yield mount


def coerce_path_field_normal_relative(kwargs, field: str) -> None:
    d = kwargs.get(field)
    if d is not None:
        kwargs[field] = make_path_normal_relative(d)
