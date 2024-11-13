/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use anyhow::Result;
use clap::Parser;

#[derive(Parser, Debug)]
pub(crate) struct Args {}

const MESSAGE: &str = r#"
This is an antlir2 booted image test.

Press ^] three times within 1s to kill the container.

You have been auto-logged in to a root console. Feel free to mess around here,
any changes you make will be thrown away when the container exits.

Soon, this will be populated with a copy-pasteable command, but for now...

To run your test interactively:

In another shell on your host:

'buck2 build --show-full-output $test[inner_test]'

In this shell:

Run the binary at that path printed above
EOF
"#;

const WIDTH: usize = 80;
const PADDING: usize = 2;

const MOOSE: &str = r#"  ╲
   ╲   \_\_    _/_/
    ╲      \__/
           (oo)\_______
           (__)\       )\/\
               ||----w |
               ||     ||
"#;

impl Args {
    pub(crate) fn run(self) -> Result<()> {
        let wrapped = textwrap::wrap(MESSAGE, WIDTH);
        let mut buf = String::with_capacity(MESSAGE.len() + (4 * WIDTH * wrapped.len()));
        buf.push('┏');
        for _ in 0..(WIDTH + PADDING * 2) {
            buf.push('━');
        }
        buf.push('┓');
        buf.push('\n');
        for line in wrapped {
            buf.push('┃');
            for _ in 0..PADDING {
                buf.push(' ');
            }
            buf.push_str(&line);
            let padding = WIDTH - line.len() + PADDING;
            for _ in 0..padding {
                buf.push(' ');
            }
            buf.push('┃');
            buf.push('\n');
        }
        buf.push('┗');
        for _ in 0..(WIDTH + PADDING * 2) {
            buf.push('━');
        }
        buf.push('┛');
        buf.push('\n');
        buf.push_str(MOOSE);
        println!("{}", buf);
        Ok(())
    }
}
