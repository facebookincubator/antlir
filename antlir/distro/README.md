While almost everything internal is built to target an OS-version-agnostic
platform (aka `fbcode` platform), it is sometimes necessary to build code
targeting a system platform for a specific OS version, using toolchains
distributed by that OS distro.

This directory contains a set of rules that make that possible.
