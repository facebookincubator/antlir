#!/usr/bin/env python3

from antlir.config import repo_config

cfg = repo_config()

if cfg.artifacts_require_repo:
    print("[Service]")
    print(f"BindReadOnlyPaths={cfg.repo_root}")
    for mount in cfg.host_mounts_for_repo_artifacts:
        print(f"BindReadOnlyPaths={mount}")
