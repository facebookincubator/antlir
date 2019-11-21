#!/usr/bin/env python3
from configparser import ConfigParser
from typing import Iterable, Iterator, NamedTuple, Tuple

# NB: The 'main' section in `yum.conf` acts similarly to ConfigParser's
# magic 'DEFAULT', in that it provides default values for some of the repo
# options.  I did not investigate this in enough detail to say that setting
# `default_section='main'` would be appropriate.  Since this code currently
# only cares about `baseurl`, this is good enough.
_NON_REPO_SECTIONS = ['DEFAULT', 'main']


class YumConfRepo(NamedTuple):
    name: str
    base_url: str
    gpg_key_urls: Tuple[str]

    @classmethod
    def from_config_section(cls, name, cfg_sec):
        assert '/' not in name and '\0' not in name, f'Bad repo name {name}'
        return YumConfRepo(
            name=name,
            base_url=cfg_sec['baseurl'],
            gpg_key_urls=tuple(cfg_sec['gpgkey'].split('\n'))
                if 'gpgkey' in cfg_sec else (),
        )


class YumConfIsolator:
    '''
    The functions in this class ATTEMPT to edit `yum.conf` in such a way
    that it will:
      - never interact with state, caches, or configuration from the host
        filesystem,
      - never interact with servers outside of the ones we specify.

    IMPORTANT: With `yum`, it is actually impossible to configure it such
    that it does not touch the host filesystem.  A couple of specific
    examples:

    (1) Regardless of the configuration, `yum` will look in
        `$host_root/$cachedir` BEFORE `$installroot/$cachedir`, which is
        breaks isolation of RPM content and repodata.

    (2) Regardless of the configuration, `yum` will attempt to read
        `/etc/yum/vars` from the host, breaking isolation of configuration.

    There are other examples. To see the bind-mount protections we use to
    avoid leakage from the host, read `_isolate_yum_and_wait_until_ready` --
    and of course, the larger purpose of `yum-from-snapshot` is to run its
    `yum` inside a private network namespace to guarantee no off-host repo
    accesses.
    '''

    def __init__(self, cp: ConfigParser):
        self._cp = ConfigParser()
        self._cp.read_dict(cp)  # Make a copy
        self._isolated_main = False
        self._isolated_repos = False

    def isolate_repos(self, repos: Iterable[YumConfRepo]) -> 'YumConfIsolator':
        '''
        Asserts that the passed repos are exactly those defined in the
        config file. This ensures that we leave no repo unisolated.

        For each specified repo, sets the config values specified in its
        `YumConfRepo`, and clears `proxy`.  Other config keys are left
        unchanged -- but seeing some "known bad" configs in the config file
        will cause an assertion error.

        IMPORTANT: See the class docblock, this is not **ENOUGH**.
        '''
        unchanged_repos = {r for r in self._cp if r not in _NON_REPO_SECTIONS}
        for repo in repos:
            unchanged_repos.remove(repo.name)
            assert repo.name not in _NON_REPO_SECTIONS
            repo_sec = self._cp[repo.name]
            repo_sec['baseurl'] = repo.base_url
            repo_sec['gpgkey'] = '\n'.join(repo.gpg_key_urls)
            repo_sec.pop('proxy', None)  # We talk only to a local reposerver.
            # These are not handled for now, but could be supported. The
            # goal of asserting their absence is to avoid accidentally
            # having non-isolated URLs in the config.
            for unsupported_key in [
                'include', 'metalink', 'mirrorlist', 'gpgcakey'
            ]:
                assert unsupported_key not in repo_sec, (unsupported_key, repo)
            # NB: As with [main], we let the SSL-related options come
            # from the host: `sslcacert`, `sslclientcert`, and `sslclientkey`
        assert not unchanged_repos, f'Failed to isolate {unchanged_repos}'
        self._isolated_repos = True
        return self

    def isolate_main(self, *, install_root: str, config_path: str) \
            -> 'YumConfIsolator':
        '''
        Set keys that could cause `yum` to interact with the host filesystem.
        IMPORTANT: See the class docblock, this is not **ENOUGH**.
        '''
        main_sec = self._cp['main']
        assert (
            'include' not in main_sec and 'include' not in self._cp['DEFAULT']
        ), 'Includes are not supported'
        # This list was obtained by scrolling through `man yum.conf`.
        # Future: to be really thorough, we'd also remove glob filesystem
        # dependencies from options like `exclude`, `includepkgs`,
        # `protected_packages`, `exactarchlist`, etc -- but then, I'd rather
        # nspawn a `yum` appliance -- The Right Way (TM) for isolation.
        #
        # `cachedir` and `persistdir` are under `--installroot`, so no
        # isolation needed.  However, ensuring defaults makes later
        # container customization (e.g.  cleanup) easier.  These can be
        # optionalized later if a good reason arises.
        main_sec['cachedir'] = '/var/cache/yum'  # default
        main_sec['persistdir'] = '/var/lib/yum'  # default
        # Shouldn't make a difference for as-root runs, but it's good hygiene
        main_sec['usercache'] = '0'
        # Specify repos only via this `yum.conf` -- that eases isolating them.
        main_sec['reposdir'] = '/dev/null'
        # See the note about `cachedir` -- the same logic applies.
        main_sec['logfile'] = '/var/log/yum.log'  # default
        main_sec['installroot'] = install_root
        main_sec['config_file_path'] = config_path
        # NB: `sslcacert`, `sslclientcert`, and `sslclientkey` are left
        # as-is, though these read from the host filesystem.
        main_sec['syslog_device'] = ''  # We'll just use `logfile`.
        assert not main_sec.get('commands')  # This option seems dodgy.
        main_sec.pop('proxy', None)  # We talk only to a local reposerver.
        # Allowing plugins seems likely to break isolation.
        main_sec['plugins'] = '0'
        main_sec['pluginpath'] = '/dev/null'
        main_sec['pluginconfpath'] = '/dev/null'
        main_sec['bugtracker_url'] = ''  # Yum is unmaintained anyway.
        main_sec['fssnap_devices'] = '!*'  # Snapshots don't make sense.
        self._isolated_main = True
        return self

    def write(self, out: 'TextIO'):
        'Outputs a `yum.conf` file with the changed configuration.'
        assert self._isolated_main and self._isolated_repos
        self._cp.write(out)


class YumConfParser:

    def __init__(self, yum_conf: 'TextIO'):
        self._cp = ConfigParser()
        self._cp.read_file(yum_conf)

    def gen_repos(self) -> Iterator[YumConfRepo]:
        'Raises if repo names cannot be used as directory names.'
        for repo, cfg in self._cp.items():
            if repo not in _NON_REPO_SECTIONS:
                yield YumConfRepo.from_config_section(repo, cfg)

    def isolate(self) -> YumConfIsolator:
        return YumConfIsolator(self._cp)
