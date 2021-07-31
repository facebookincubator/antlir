use std::path::PathBuf;

use structopt::{StructOpt, clap};


#[derive(StructOpt)]
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


fn main() {
    let options = Options::from_args();
    if options.ignored.len() > 0 {
        println!("Warning: Unimplemented options were ignored {:?}", options.ignored);
    }

    println!("{:?}", options.spec);
}
