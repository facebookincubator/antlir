/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

use tracing::debug;

/// Format the Command for printing
pub(crate) fn format_command(command: &Command) -> String {
    let program = command.get_program().to_string_lossy().to_string();
    let args: Vec<_> = command
        .get_args()
        .map(|x| x.to_string_lossy().to_string())
        .collect();
    format!("Program: `{}`. Args: `{:?}`", program, args)
}

/// Log the command being executed, unless it can't be decoded.
pub(crate) fn log_command(command: &mut Command) -> &mut Command {
    debug!("Executing command: {}", format_command(command));
    command
}

/// A lot of qemu arguments take a node_name. The main requirement of that is to be
/// unique. Add a helper to generate such names.
#[derive(Debug, Default)]
pub(crate) struct NodeNameCounter {
    prefix: String,
    count: u32,
}

impl NodeNameCounter {
    pub fn new(prefix: &str) -> Self {
        Self {
            prefix: prefix.to_string(),
            count: 0,
        }
    }

    pub fn next(&mut self) -> String {
        let count = self.count;
        self.count += 1;
        format!("{}{}", self.prefix, count)
    }
}

/// Return a path to record VM console output. When invoked under tpx, this
/// will be uploaded as an artifact.
pub(crate) fn console_output_path_for_tpx() -> Result<Option<PathBuf>, std::io::Error> {
    // If tpx has provided this artifacts dir, put the logs there so they get
    // uploaded along with the test results
    if let Some(artifacts_dir) = std::env::var_os("TEST_RESULT_ARTIFACTS_DIR") {
        fs::create_dir_all(&artifacts_dir)?;
        let dst = Path::new(&artifacts_dir).join("console.txt");
        // The artifact metadata is set up before running the test so that it
        // still gets uploaded even in case of a timeout
        if let Some(annotations_dir) = std::env::var_os("TEST_RESULT_ARTIFACT_ANNOTATIONS_DIR") {
            fs::create_dir_all(&annotations_dir)?;
            fs::write(
                Path::new(&annotations_dir).join("console.txt.annotation"),
                r#"{"type": {"generic_text_log": {}}, "description": "console logs"}"#,
            )?;
        }
        Ok(Some(dst))
    } else {
        Ok(None)
    }
}

#[cfg(test)]
/// Helper function for converting qemu args to a single string for asserting in tests.
/// This is usually only needed for string only functions like `contains`.
pub(crate) fn qemu_args_to_string(args: &[std::ffi::OsString]) -> String {
    args.join(std::ffi::OsStr::new(" "))
        .to_str()
        .expect("Invalid unicode")
        .to_string()
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_node_name_counter() {
        let mut test = NodeNameCounter::new("vd");
        assert_eq!(test.next(), "vd0");
        assert_eq!(test.next(), "vd1");
        assert_eq!(test.next(), "vd2");
    }

    #[test]
    fn test_format_command() {
        assert_eq!(
            format_command(&Command::new("hello")),
            "Program: `hello`. Args: `[]`",
        );
        assert_eq!(
            format_command(Command::new("hello").arg("world")),
            format!("Program: `hello`. Args: `{:?}`", vec!["world"]),
        );
    }
}
