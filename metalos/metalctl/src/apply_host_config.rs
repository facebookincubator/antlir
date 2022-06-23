/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Parser;
use slog::{o, Logger};
use url::Url;

use evalctx::{Generator, StarlarkGenerator};
use get_host_config::get_host_config;

#[derive(Parser)]
pub struct Opts {
    host_config_uri: Url,
    root: PathBuf,
    #[clap(
        default_value = "usr/lib/metalos/generators",
        help = "Root of starlark generator files. If a relative path, it will \
        be interpreted as relative to --root."
    )]
    generators_root: PathBuf,
}

pub async fn apply_host_config(log: Logger, opts: Opts) -> Result<()> {
    let log = log.new(o!("host-config-uri" => opts.host_config_uri.to_string(), "root" => opts.root.display().to_string()));

    let host = get_host_config(&opts.host_config_uri)
        .await
        .with_context(|| format!("while loading host config from {} ", opts.host_config_uri))?;

    // if --generators-root is absolute, this join will still do the right
    // thing, but otherwise makes it possible for users to pass a different
    // relative path if desired
    let generators_root = opts.root.join(opts.generators_root);

    let generators = StarlarkGenerator::load(&generators_root).context(format!(
        "failed to load generators from {:?}",
        &generators_root
    ))?;
    for gen in generators {
        let output = gen
            .eval(&host.provisioning_config)
            .context(format!("could not apply eval generator for {}", gen.name()))?;
        output.apply(log.clone(), &opts.root)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{apply_host_config, Opts};
    use anyhow::{Context, Result};
    use tempfile::{tempdir, NamedTempFile};
    use url::Url;

    #[test]
    async fn test_generators() -> Result<()> {
        let generators_dir = tempdir()?;
        std::fs::write(
            generators_dir.path().join("test.star"),
            r#"
def generator(prov: metalos.ProvisioningConfig) -> metalos.Output.type:
    return metalos.Output(
        files=[
            metalos.file(path="test_output_file", contents="test output for " + prov.identity.hostname),
        ]
    )
"#,
        )?;

        let host_config_file = NamedTempFile::new().context("while creating tempfile")?;
        let json = fbthrift::simplejson_protocol::serialize(
            &example_host_for_tests::example_host_for_tests(),
        );
        std::fs::write(host_config_file.path(), &json).context("while writing host config file")?;

        let root_dir = tempdir()?;

        let log = slog::Logger::root(slog_glog_fmt::default_drain(), slog::o!());
        let opts = Opts {
            host_config_uri: Url::from_file_path(host_config_file.path()).unwrap(),
            root: root_dir.path().to_path_buf(),
            generators_root: generators_dir.path().to_path_buf(),
        };
        apply_host_config(log, opts).await?;
        let result = std::fs::read_to_string(root_dir.path().join("test_output_file"))?;
        assert_eq!(result, "test output for host001.01.abc0.facebook.com");
        Ok(())
    }
}
