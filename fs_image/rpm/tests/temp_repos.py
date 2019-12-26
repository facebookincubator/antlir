#!/usr/bin/env python3
'See `temp_repo_steps` and `SAMPLE_STEPS` for documentation.'
import os
import shlex
import shutil
import subprocess
import tempfile
import textwrap

from configparser import ConfigParser
from contextlib import contextmanager
from typing import Dict, List, NamedTuple, Optional

from ..common import Path, temp_dir
from fs_image.common import load_location


def rpmbuild_path() -> str:
    # Using the system `rpmbuild` is non-hermetic and may break in many ways.
    # Future: try using a build appliance here to mitigate this.
    return '/usr/bin/rpmbuild'


class Rpm(NamedTuple):
    name: str
    version: str
    release: str
    override_contents: Optional[str] = None
    # Be careful with enabling this broadly since it make the RPM
    # dramatically bigger and likely makes the test slower.
    test_post_install: bool = False

    def spec(self):
        format_kwargs = {
            **self._asdict(),
            'quoted_contents': shlex.quote(
                f'{self.name} {self.version} {self.release}'
                    if self.override_contents is None
                    else self.override_contents
            ),
            'quoted_busybox_path': shlex.quote(
                load_location('rpm', 'busybox-path')
            )
        }

        common_spec = textwrap.dedent('''\
        Summary: The "{name}" package.
        Name: rpm-test-{name}
        Version: {version}
        Release: {release}
        License: BSD
        Group: Facebook/Script
        Vendor: Facebook, Inc.
        Packager: somebody@example.com

        %description
        %install
        mkdir -p "$RPM_BUILD_ROOT"/usr/share/rpm_test
        echo {quoted_contents} > "$RPM_BUILD_ROOT"/usr/share/rpm_test/{name}.txt
        mkdir -p "$RPM_BUILD_ROOT"/bin
        ''').format(**format_kwargs)

        return common_spec + textwrap.dedent((
            '''\
            %files
            /usr/share/rpm_test/{name}.txt
            '''
        ) if not self.test_post_install else (
            '''\
            cp {quoted_busybox_path} "$RPM_BUILD_ROOT"/bin/sh
            %post
            '''
            # yum-from-snapshot prepares /dev in a subtle way to protect host
            # system from side-effects of rpm post-install scripts. The command
            # below lets us test that /dev/null is prepared properly: if
            # "echo > /dev/null" fails, tests will catch the absence of post.txt
            '''\
            echo > /dev/null && echo 'stuff' > \
              "$RPM_BUILD_ROOT"/usr/share/rpm_test/post.txt
            %files
            /bin/sh
            /usr/share/rpm_test/{name}.txt
            '''
        )).format(**format_kwargs)


class Repo(NamedTuple):
    rpms: List[Rpm]

    # Future: Reconsider having repos not know their names, and being
    # represented as dicts.  Lists of name-aware repos may be cleaner.
    # Similarly, `arch` should probably be in `Repo`, and defaulted there.
    def locations(self, repo_name, arch: str = 'x86_64'):
        return [
            (
                f'{repo_name}-pkgs/rpm-test-'
                f'{r.name}-{r.version}-{r.release}.{arch}.rpm'
            ) for r in self.rpms
        ]


# The `rpm` module is concerned with recording the evolution of a set of
# RPM repos over time. Therefore, a generic input to achieve good test
# coverage should:
#  - provide a repo history with several time-steps,
#  - contain potentially related RPM repos that change between time-steps,
#  - contain different packages, and varying versions of the same package,
#  - have packages that occur in the same or different versions across repos.
#
# This `SAMPLE_STEPS` attempts to provide such a history without making
# tests excessively slow.  Feel free to revise it, as long as tests pass.
# Most tests should NOT depend on the specifics of the repo contents -- look
# for `test_post_install` for the sole exception.
#
# Below, the array index is the step number, modeling the passage of time.
#
#  - If a repo has a value of `None`, we will delete this repo, asserting
#    that it existed in the prior timestamp.
#  - If a repo value is a string, it is an alias to another existing repo,
#    which is a symlink to the original, to minimize the performance
#    overhead.  If you MUST commit a temp repo to a source control repo
#    using Buck, you may wish to set `avoid_symlinks`, since the Buck team
#    really dislikes symlinks in repos (i.e. something may break).
SAMPLE_STEPS = [
    {
        'bunny': Repo([Rpm('carrot', '2', 'rc0')]),
        'cat': Repo([
            Rpm(
                'milk', '2.71', '8',  # Newer version than in `dog`
                # Installing at least one such package is important for
                # validating the `yum-from-snapshot` runtime.
                test_post_install=True,
            ),
            Rpm('mice', '0.1', 'a'),
            # Since this is older than version `2-rc0` it needs versionlock.
            Rpm('carrot', '1', 'lockme'),
        ]),
        'dog': Repo([
            Rpm('milk', '1.41', '42'),
            Rpm('mice', '0.1', 'a'),
            Rpm('carrot', '2', 'rc0'),  # Same version as in `bunny`
        ]),
        'puppy': 'dog',
    },
    {
        'bunny': None,
        'cat': Repo([Rpm('milk', '3.14', '15')]),  # New version
        'dog': Repo([Rpm('bone', '5i', 'beef'), Rpm('carrot', '2', 'rc0')]),
        'kitty': 'cat',
    },
]


def build_rpm(package_dir: Path, arch: str, rpm: Rpm) -> bytes:
    'Returns the filename of the built RPM.'
    with temp_dir(dir=package_dir) as td, tempfile.NamedTemporaryFile() as tf:
        tf.write(rpm.spec().encode())
        tf.flush()
        subprocess.run(
            [
                rpmbuild_path(), '-bb', '--target', arch,
                '--buildroot', td / 'build', tf.name,
            ],
            env={'HOME': td / 'home'},
            check=True,
        )
        # `rpmbuild` has a non-configurable output layout, so
        # we'll move the resulting rpm into our package dir.
        rpms_dir = td / 'home/rpmbuild/RPMS' / arch
        rpm_name, = os.listdir(rpms_dir)
        os.rename(rpms_dir / rpm_name, package_dir / rpm_name)
        return rpm_name


def make_repo_steps(
    out_dir: Path, repo_change_steps: List[Dict[str, Repo]], arch: str,
    avoid_symlinks: bool = False,
):
    # When an RPM occurs in two different repos, we want it to be
    # bit-identical (otherwise, the snapshot would see a `mutable_rpm`
    # error).  This means never rebuilding an RPM that was previously seen.
    # The paths are relative to `out_dir`.
    rpm_to_path = {}
    # The repos that exist at the current step.
    repos = {}
    for step, repo_changes in enumerate(repo_change_steps):
        step = Path(str(step))
        for repo_name, repo in repo_changes.items():
            if repo is None:
                del repos[repo_name]
            else:
                repos[repo_name] = repo
        step_dir = out_dir / step
        os.makedirs(step_dir)
        yum_dnf_conf = ConfigParser()
        yum_dnf_conf['main'] = {}
        for repo_name, repo in repos.items():
            repo_dir = step_dir / repo_name
            yum_dnf_conf[repo_name] = {'baseurl': repo_dir.file_url()}
            if isinstance(repo, str):  # Alias of another repo
                assert repo in repos
                if avoid_symlinks:
                    shutil.copytree(step_dir / repo, repo_dir)
                else:
                    os.symlink(repo, repo_dir)
                continue
            # Each repo's package dir is different to exercise the fact
            # that the same file's location may differ across repos.
            package_dir = repo_dir / f'{repo_name}-pkgs'
            os.makedirs(package_dir)
            for rpm in repo.rpms:
                prev_path = rpm_to_path.get(rpm)
                if prev_path and avoid_symlinks:
                    shutil.copy(
                        out_dir / prev_path,
                        package_dir / prev_path.basename(),
                    )
                elif prev_path:
                    os.symlink(
                        '../../..' / prev_path,
                        package_dir / prev_path.basename(),
                    )
                else:
                    rpm_to_path[rpm] = (
                        step / repo_name / package_dir.basename() /
                        build_rpm(package_dir, arch, rpm)
                    )
            # Now that all RPMs were built, we can generate the Yum metadata
            subprocess.run(['createrepo_c', repo_dir], check=True)
        for prog_name in ['dnf', 'yum']:
            with open(step_dir / f'{prog_name}.conf', 'w') as out_f:
                yum_dnf_conf.write(out_f)

@contextmanager
def temp_repos_steps(base_dir=None, arch: str = 'x86_64', *args, **kwargs):
    '''
    Given a history of changes to a set of RPM repos (as in `SAMPLE_STEPS`),
    generates a collection of RPM repos on disk by running:
      - `rpmbuild` to build the RPM files
      - `createrepo` to build the repo metadata

    Returns a temporary path, cleaned up once the context exits, containing
    a directory per time step (named 0, 1, 2, etc).  Each timestep directory
    contains a directory per repo, and each repo has this layout:
        repodata/{repomd.xml,other-repodata.{xml,sqlite}.bz2}
        reponame-pkgs/rpm-test-<name>-<version>-<release>.<arch>.rpm
    '''
    td = Path(tempfile.mkdtemp(dir=base_dir))
    try:
        make_repo_steps(out_dir=td, arch=arch, *args, **kwargs)
        yield td
    except BaseException:  # Clean up even on Ctrl-C
        shutil.rmtree(td)
        raise
