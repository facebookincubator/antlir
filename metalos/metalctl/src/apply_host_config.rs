/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::path::PathBuf;

use anyhow::{anyhow, Context, Result};
use slog::{o, Logger};
use structopt::StructOpt;
use url::Url;

use evalctx::Generator;
use host::types::HostConfig;

#[derive(StructOpt)]
pub struct Opts {
    host_config_uri: Url,
    root: PathBuf,

    #[structopt(default_value = "/usr/lib/metalos/generators")]
    generators_root: PathBuf,
}

pub async fn apply_host_config(log: Logger, opts: Opts) -> Result<()> {
    let log = log.new(o!("host-config-uri" => opts.host_config_uri.to_string(), "root" => opts.root.display().to_string()));

    let host: HostConfig = match opts.host_config_uri.scheme() {
        "http" | "https" => {
            let client = crate::http::client()?;
            client
                .get(opts.host_config_uri.clone())
                .send()
                .await
                .with_context(|| format!("while GETting {}", opts.host_config_uri))?
                .json()
                .await
                .context("while parsing host json")
        }
        "file" => {
            let f = std::fs::File::open(opts.host_config_uri.path())
                .with_context(|| format!("while opening file {}", opts.host_config_uri.path()))?;
            serde_json::from_reader(f).context("while deserializing json")
        }
        scheme => Err(anyhow!(
            "Unsupported scheme {} in {:?}",
            scheme,
            opts.host_config_uri
        )),
    }?;

    let generators = Generator::load(&opts.generators_root).context(format!(
        "failed to load generators from {:?}",
        &opts.generators_root
    ))?;
    for gen in generators {
        let output = gen.eval(&host.provisioning_config.identity)?;
        output.apply(log.clone(), &opts.root)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{apply_host_config, Opts};
    use anyhow::{Context, Result};
    use evalctx::HostIdentity;
    use tempfile::{tempdir, NamedTempFile};
    use url::Url;

    #[test]
    async fn test_generators() -> Result<()> {
        let generators_dir = tempdir()?;
        std::fs::write(
            generators_dir.path().join("test.star"),
            r#"
def generator(host: metalos.HostIdentity) -> metalos.Output.type:
    return metalos.Output(
        files=[
            metalos.file(path="test_output_file", contents="test output for " + host.hostname),
        ]
    )
"#,
        )?;

        let host_config_file = NamedTempFile::new().context("while creating tempfile")?;
        serde_json::to_writer(
            &host_config_file,
            &evalctx::host::HostConfig {
                provisioning_config: evalctx::host::ProvisioningConfig {
                    identity: HostIdentity::example_host_for_tests(),
                },
                runtime_config: Default::default(),
            },
        )?;

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
