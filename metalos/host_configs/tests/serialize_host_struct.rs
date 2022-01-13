use anyhow::{Context, Result};
use host::Host;

fn main() -> Result<()> {
    let h: Host = serde_json::from_reader(std::io::stdin()).context("while parsing host json")?;
    serde_json::to_writer_pretty(std::io::stdout(), &h).context("while serializing host json")?;
    Ok(())
}
