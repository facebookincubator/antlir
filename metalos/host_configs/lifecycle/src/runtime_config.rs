/*
 * Copyright (c) Meta Platforms, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use metalos_host_configs::packages::generic::Package;
use metalos_host_configs::runtime_config::RuntimeConfig;

use crate::stage::StagableConfig;

impl StagableConfig for RuntimeConfig {
    #[deny(unused_variables)]
    fn packages(&self) -> Vec<Package> {
        let Self {
            #[cfg(facebook)]
                deployment_specific: _,
            services,
        } = self.clone();
        let mut packages = vec![];
        for svc in services {
            packages.push(svc.svc.into());
            if let Some(gen) = svc.config_generator {
                packages.push(gen.into());
            }
        }
        packages
    }
}
