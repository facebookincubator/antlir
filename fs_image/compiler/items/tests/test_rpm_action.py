#!/usr/bin/env python3
import os
import subprocess
import sys

from find_built_subvol import find_built_subvol, subvolumes_dir
from fs_image.fs_utils import Path
from rpm.rpm_metadata import RpmMetadata, compare_rpm_versions
from subvol_utils import get_subvolume_path
from tests.temp_subvolumes import TempSubvolumes

from ..rpm_action import RpmAction, RpmActionItem, RpmBuildItem

from .common import BaseItemTestCase, DUMMY_LAYER_OPTS, render_subvol
from ..common import image_source_item, PhaseOrder


class RpmActionItemTestCase(BaseItemTestCase):

    def _test_rpm_action_item(self, layer_opts, preserve_yum_cache=False):
        with TempSubvolumes(sys.argv[0]) as temp_subvolumes:
            subvol = temp_subvolumes.create('rpm_action')
            self.assertEqual(['(Dir)', {}], render_subvol(subvol))

            # The empty action is a no-op
            RpmActionItem.get_phase_builder([], layer_opts)(subvol)
            self.assertEqual(['(Dir)', {}], render_subvol(subvol))

            # `yum-from-snapshot` needs a `/meta` directory to work
            subvol.run_as_root(['mkdir', subvol.path('meta')])
            self.assertEqual(
                # No `opts/artifacts_may_require_repo` here because we directly
                # created the subvol instead of using an Item.
                ['(Dir)', {'meta': ['(Dir)', {}]}], render_subvol(subvol),
            )

            # Specifying RPM versions is prohibited
            with self.assertRaises(subprocess.CalledProcessError):
                RpmActionItem.get_phase_builder(
                    # This could have been RpmActionItem(), but I want to
                    # test `image_source_item` with `source=None`.
                    [image_source_item(
                        RpmActionItem,
                        exit_stack=None,
                        layer_opts=DUMMY_LAYER_OPTS,
                    )(
                        from_target='m',
                        name='rpm-test-milk-2.71',
                        source=None,
                        action=RpmAction.install,
                    )],
                    layer_opts,
                )(subvol)

            # Cannot pass both `name` and `source`
            with self.assertRaisesRegex(
                AssertionError,
                'Exactly one of `name` or `source` must be set .*',
            ):
                RpmActionItem.get_phase_builder(
                    [RpmActionItem(
                        from_target='m',
                        name='rpm-test-milk',
                        source='rpm-test-milk',
                        action=RpmAction.install,
                    )],
                    layer_opts,
                )(subvol)

            RpmActionItem.get_phase_builder(
                [
                    RpmActionItem(
                        from_target='t', name=n, action=RpmAction.install,
                    ) for n in [
                        # This specific RPM contains `/bin/sh` and a
                        # post-install script to test `/dev/null` isolation.
                        'rpm-test-milk',
                        'rpm-test-carrot',
                    ]
                ] + [
                    RpmActionItem(
                        from_target='t',
                        source=Path(__file__).dirname() /
                            "rpm-test-cheese-1-1.rpm",
                        action=RpmAction.install,
                    )
                ],
                layer_opts,
            )(subvol)
            # Clean up the `yum` & `rpm` litter before checking the packages.
            # Maybe fixme: As a result, we end up not asserting ownership /
            # permissions / etc on directories like /var and /dev.
            subvol.run_as_root([
                'rm', '-rf',
                # Annotate all paths since `sudo rm -rf` is scary.
                subvol.path('var/lib/rpm'),
                subvol.path('var/lib/yum'),
                subvol.path('var/log/yum.log'),
                subvol.path('usr/lib/.build-id'),
                subvol.path('bin/sh'),
            ])
            # The way that RpmActionItem invokes systemd_nspawn on
            # build_appliance must gurantee that /var/cache/yum is empty.
            # Next two lines test that the /var/cache/yum directory is empty
            # because rmdir fails if it is not.
            # It is important that the yum cache of built images be empty, to
            # avoid unnecessarily increasing the distributed image size.
            rm_cmd = ['rmdir'] if (
                layer_opts.build_appliance and not preserve_yum_cache
            ) else ['rm', '-rf']
            subvol.run_as_root(rm_cmd + [subvol.path('var/cache/yum')])
            subvol.run_as_root([
                'rmdir',
                subvol.path('dev'),  # made by yum_from_snapshot.py
                subvol.path('meta'),
                subvol.path('var/cache'),
                subvol.path('var/lib'),
                subvol.path('var/log'),
                subvol.path('var/tmp'),
                subvol.path('var'),
                subvol.path('usr/lib'),
                subvol.path('bin'),
            ])
            self.assertEqual(['(Dir)', {
                'usr': ['(Dir)', {
                    'share': ['(Dir)', {
                        'rpm_test': ['(Dir)', {
                            'carrot.txt': ['(File d13)'],
                            'cheese1.txt': ['(File d36)'],
                            'milk.txt': ['(File d12)'],
                            'post.txt': ['(File d6)'],
                        }],
                    }],
                }],
            }], render_subvol(subvol))

    def test_rpm_action_item_yum_from_snapshot(self):
        self._test_rpm_action_item(layer_opts=DUMMY_LAYER_OPTS._replace(
            # This works in @mode/opt since this binary is baked into the XAR
            yum_from_snapshot=os.path.join(
                os.path.dirname(__file__), 'yum-from-test-snapshot',
            ),
        ))

    def test_rpm_action_item_build_appliance(self):
        # We have two test build appliances: one fake one assembled from
        # host mounts, and another FB-specific one that is an actual
        # published image used for production builds.  Let's exercise both.
        for filename in [
            'fb-test-build-appliance',
            'host-test-build-appliance',
            'fb-host-test-build-appliance',
        ]:
            for preserve_yum_cache in [True, False]:
                self._test_rpm_action_item(layer_opts=DUMMY_LAYER_OPTS._replace(
                    build_appliance=get_subvolume_path(
                        os.path.join(
                            os.path.dirname(__file__),
                            filename,
                            'layer.json',
                        ),
                        subvolumes_dir(),
                    ),
                    preserve_yum_cache=preserve_yum_cache,
                ), preserve_yum_cache=preserve_yum_cache)

    def test_rpm_action_item_auto_downgrade(self):
        parent_subvol = find_built_subvol(
            (Path(__file__).dirname() / 'test-with-one-local-rpm').decode()
        )
        src_rpm = Path(__file__).dirname() / "rpm-test-cheese-1-1.rpm"

        with TempSubvolumes(sys.argv[0]) as temp_subvolumes:
            # ensure cheese2 is installed in the parent from rpm-test-cheese-2-1
            assert os.path.isfile(
                parent_subvol.path('/usr/share/rpm_test/cheese2.txt')
            )
            # make sure the RPM we are installing is older in order to
            # trigger the downgrade
            src_data = RpmMetadata.from_file(src_rpm)
            subvol_data = RpmMetadata.from_subvol(parent_subvol, src_data.name)
            assert compare_rpm_versions(src_data, subvol_data) < 0

            subvol = temp_subvolumes.snapshot(parent_subvol, 'rpm_action')
            RpmActionItem.get_phase_builder(
                [RpmActionItem(
                    from_target='t',
                    source=src_rpm,
                    action=RpmAction.install,
                )],
                DUMMY_LAYER_OPTS._replace(
                    yum_from_snapshot=Path(__file__).dirname() /
                        'yum-from-test-snapshot',
                ),
            )(subvol)
            subvol.run_as_root([
                'rm', '-rf',
                subvol.path('dev'),
                subvol.path('meta'),
                subvol.path('var'),
            ])
            self.assertEqual(['(Dir)', {
                'usr': ['(Dir)', {
                    'share': ['(Dir)', {
                        'rpm_test': ['(Dir)', {
                            'cheese1.txt': ['(File d36)'],
                        }],
                    }],
                }],
            }], render_subvol(subvol))

    def test_rpm_action_conflict(self):
        layer_opts = DUMMY_LAYER_OPTS._replace(
            yum_from_snapshot='required but ignored'
        )
        # Test both install-install, install-remove, and install-downgrade
        # conflicts.
        for rpm_actions in (
            (('cat', RpmAction.install), ('cat', RpmAction.install)),
            (
                ('dog', RpmAction.remove_if_exists),
                ('dog', RpmAction.install),
            ),
        ):
            with self.assertRaisesRegex(RuntimeError, 'RPM action conflict '):
                # Note that we don't need to run the builder to hit the error
                RpmActionItem.get_phase_builder(
                    [
                        RpmActionItem(from_target='t', name=r, action=a)
                            for r, a in rpm_actions
                    ],
                    layer_opts,
                )

        with self.assertRaisesRegex(RuntimeError, 'RPM action conflict '):
            # An extra test case for local RPM name conflicts (filenames are
            # different but RPM names are the same)
            RpmActionItem.get_phase_builder(
                [
                    RpmActionItem(
                        from_target='t',
                        source=Path(__file__).dirname() /
                            "rpm-test-cheese-2-1.rpm",
                        action=RpmAction.install,
                    ),
                    RpmActionItem(
                        from_target='t',
                        source=Path(__file__).dirname() /
                            "rpm-test-cheese-1-1.rpm",
                        action=RpmAction.remove_if_exists,
                    ),
                ],
                layer_opts,
            )

    def test_rpm_action_no_passing_downgrade(self):
        with self.assertRaisesRegex(
            AssertionError, '\'downgrade\' cannot be passed'
        ):
            RpmActionItem(
                from_target='t',
                name='derp',
                action=RpmAction.downgrade
            )

    def test_rpm_build_item(self):
        parent_subvol = find_built_subvol(
            (Path(__file__).dirname() / 'toy-rpmbuild-setup').decode()
        )

        with TempSubvolumes(sys.argv[0]) as temp_subvolumes:
            assert os.path.isfile(
                parent_subvol.path('/rpmbuild/SOURCES/toy_src_file')
            )
            assert os.path.isfile(
                parent_subvol.path('/rpmbuild/SPECS/specfile.spec')
            )

            subvol = temp_subvolumes.snapshot(parent_subvol, 'rpm_build')
            item = RpmBuildItem(from_target='t', rpmbuild_dir='/rpmbuild')
            RpmBuildItem.get_phase_builder(
                [item],
                DUMMY_LAYER_OPTS,
            )(subvol)

            self.assertEqual(item.phase_order(), PhaseOrder.RPM_BUILD)
            assert os.path.isfile(
                subvol.path('/rpmbuild/RPMS/toy.rpm')
            )
