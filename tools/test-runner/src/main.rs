use std::path::PathBuf;
use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;

use structopt::{clap, StructOpt};
use serde;
use serde_json;
use anyhow::{Context, Result};


#[derive(StructOpt, Debug)]
#[structopt(
    about = "A custom buck test runner for Antlir's CI",
    setting = clap::AppSettings::AllowLeadingHyphen, // allows ignored options
)]
struct Options {
    /// [BUCK] JSON file containing test descriptions
    #[structopt(parse(from_os_str), long = "buck-test-info")]
    spec: PathBuf,

    /// [BUCK] Maximum number of threads used
    #[structopt(long = "jobs", default_value = "1")]
    threads: u32,

    /// [BUCK] Warns on any further options for forward compatibility
    #[structopt(hidden = true)]
    ignored: Vec<String>,
}


#[derive(serde::Deserialize, Debug)]
#[serde(rename_all = "snake_case")]
enum TestType {
    Pyunit,
}

// refer to https://buck.build/files-and-dirs/buckconfig.html#test.external_runner
#[derive(serde::Deserialize, Debug)]
struct Test {
    /// The buck target URI of the test rule.
    target: PathBuf,

    /// The type of the test.
    #[serde(rename(deserialize = "type"))]
    type_: TestType,

    /// Command line that should be used to run the test.
    command: Vec<String>,

    /// Environment variables to be defined when running the test.
    env: HashMap<String, String>,

    /// Labels that are defined on the test rule.
    labels: Vec<String>,

    /// Contacts that are defined on the test rule.
    contacts: Vec<String>,
}


fn main() -> Result<()> {
    // parse command line
    let options = Options::from_args();
    if options.ignored.len() > 0 {
        println!(
            "Warning: Unimplemented options were ignored {:?}\n",
            options.ignored
        );
    }

    // collect test information
    let file = File::open(&options.spec)
        .with_context(|| format!("Failed to read test specs from {:?}", options.spec))?;
    let reader = BufReader::new(file);
    let tests: Vec<Test> = serde_json::from_reader(reader)
        .with_context(|| format!("Failed to parse JSON spec from {:?}", options.spec))?;

    println!("{:?}", tests);
    Ok(())
}
