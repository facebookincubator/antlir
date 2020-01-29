#!/usr/bin/env python3
import os

from .cli_object_storage import CLIObjectStorage


class S3Storage(CLIObjectStorage, plugin_kind='s3'):

    AWS_S3_BUCKET = 's3://AWS_S3_BUCKET'
    AWS_ACCESS_KEY_ID = 'AWS_SECRET_ACCESS_KEY'
    AWS_SECRET_ACCESS_KEY = 'AWS_SECRET_ACCESS_KEY'

    def __init__(self, *, key: str, timeout_seconds: float=400):
        self.key = key
        self.timeout_seconds = timeout_seconds

    def _path_for_storage_id(self, sid: str) -> str:
        return os.path.join(self.AWS_S3_BUCKET, 'flat', sid)

    def _cmd(self, *args, path: str, operation: str):
        cmd = [
            'aws', 's3',
            '--cli-read-timeout', str(self.timeout_seconds),
            '--cli-connect-timeout', str(self.timeout_seconds),
            *args,
        ]
        if operation == 'read':
            # `-` implies local file stream (stdout)
            cmd += ['cp', path, '-']
        elif operation == 'write':
            # `-` implies local file stream (stdin)
            cmd += ['cp', '-', path]
        elif operation == 'remove':
            cmd += ['rm', path]
        elif operation == 'exists':
            cmd += ['ls', path]
        else:
            raise NotImplementedError  # pragma: no cover

        return cmd

    def _configured_env(self):
        # Configure env with AWS credentials
        env = os.environ.copy()
        env['AWS_ACCESS_KEY_ID'] = self.AWS_ACCESS_KEY_ID
        env['AWS_SECRET_ACCESS_KEY'] = self.AWS_SECRET_ACCESS_KEY
        return env
