def fedora_storage_config(release):
    return {
        "bucket": "antlir",
        "key": "s3",
        "kind": "s3",
        "prefix": "snapshots/fedora/{}".format(release),
        "region": "us-east-2",
    }
