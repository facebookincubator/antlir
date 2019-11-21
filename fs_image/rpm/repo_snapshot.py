#!/usr/bin/env python3
'''
If several `RepoSnapshot`s are serialized under `snapshot-dir/`, you can
list all `ReportableError`s via:

    jq '.[].error | select(. != null)' $(find snapshot-dir/ -name '*.json')
'''
import json

from typing import Mapping, NamedTuple, Union

from .common import get_file_logger, create_ro, Path

log = get_file_logger(__file__)


class ReportableError(Exception):
    '''
    Base class for errors do not abort the snapshot, but have to be reported
    as part of RepoSnapshot.
    '''
    def __init__(self, **kwargs):
        # Even though this is morally a dict, writing a tuple to `args`
        # better honors Python's interesting exception norms, and gets us
        # usable-looking backtraces for "free".
        self.args = tuple(sorted(kwargs.items()))
        log.error(f'{type(self).__name__}{self.args}')

    def to_dict(self):
        '''
        Returns a POD dictionary to be output as an `'error'` field in the
        serialized `RepoSnapshot`.
        '''
        return dict(self.args)


class FileIntegrityError(ReportableError):
    def __init__(self, *, location, failed_check, expected, actual):
        super().__init__(
            error='file_integrity',
            message='File had unexpected size or checksum',
            location=location,
            failed_check=failed_check,
            expected=str(expected),
            actual=str(actual),
        )


class HTTPError(ReportableError):
    def __init__(self, *, location, http_status):
        super().__init__(
            error='http',
            message='Failed HTTP request while downloading a repo object',
            location=location,
            http_status=http_status,
        )


class MutableRpmError(ReportableError):
    def __init__(self, *, location, storage_id, checksum, other_checksums):
        super().__init__(
            error='mutable_rpm',
            message='Found MULTIPLE canonical checksums for one RPM filename. '
                'This means that the same file exists (or had existed) with '
                'different variants of its content.',
            location=location,
            storage_id=storage_id,  # This bad RPM is still retrievable
            checksum=str(checksum),  # Canonical checksum of this `storage_id`
            # Canonical checksums with other storage IDs & the same filename:
            other_checksums=sorted(str(c) for c in other_checksums),
        )


MaybeStorageID = Union[str, ReportableError]


class RepoSnapshot(NamedTuple):
    repomd: 'RepoMetadata'
    storage_id_to_repodata: Mapping[MaybeStorageID, 'Repodata']
    storage_id_to_rpm: Mapping[MaybeStorageID, 'Rpm']

    def to_directory(self, path: Path):
        # Future: we could potentially assert that the objects written are
        # exactly:
        #  - (for repodatas) the ones listed in repomd.xml
        #  - (for RPMs) the ones listed in the primary repodata
        # This amounts to a quick re-parsing and comparison of keys, but I'm
        # not sure it's worthwhile -- good test coverage seems better.
        with create_ro(path / 'repomd.xml', 'wb') as out:
            out.write(self.repomd.xml)

        for filename, sid_to_obj in (
            ('repodata.json', self.storage_id_to_repodata),
            ('rpm.json', self.storage_id_to_rpm),
        ):
            with create_ro(path / filename, 'w') as out:
                obj_map = {
                    obj.location: {
                        'checksum': str(obj.best_checksum()),
                        'size': obj.size,
                        'build_timestamp': obj.build_timestamp,
                        **(
                            {'error': sid.to_dict()}
                                if isinstance(sid, ReportableError)
                                    else {'storage_id': sid}
                        ),
                    } for sid, obj in sid_to_obj.items()
                }
                assert len(obj_map) == len(sid_to_obj), \
                    f'location collided {filename}'
                json.dump(obj_map, out, sort_keys=True, indent=4)
        return self

    def visit(self, visitor):
        'Visits the objects in this snapshot (i.e. this shard)'
        visitor.visit_repomd(self.repomd)
        for repodata in self.storage_id_to_repodata.values():
            visitor.visit_repodata(repodata)
        for rpm in self.storage_id_to_rpm.values():
            visitor.visit_rpm(rpm)
        return self
