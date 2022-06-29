use anyhow::{Context, Result};
use fbthrift::binary_protocol::serialize;
use service::service_t;
use std::io::Write;

fn main() -> Result<()> {
    let svc: service_t =
        serde_json::from_reader(std::io::stdin()).context("while parsing service_t")?;
    let bin = serialize(&svc);
    std::io::stdout()
        .write_all(&bin)
        .context("while dumping binary thrift to stdout")?;
    Ok(())
}
