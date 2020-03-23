buck run //fs_image/rpm:snapshot-repos -- \
  --snapshot-dir=snapshot/fedora31 \
  --gpg-key-whitelist-dir=config/fedora31 \
  --db='{"kind": "sqlite", "db_path": "snapshot/snapshots.sql3"}' \
  --threads=16 \
  --storage='{"kind":"s3", "key": "s3", "bucket": "fs-image", "prefix": "fedora31", "region": "us-west-2"}' \
  --one-universe-for-all-repos=fedora31 \
  --dnf-conf=config/fedora31/dnf.conf \
  --yum-conf=config/fedora31/dnf.conf \
  --debug
