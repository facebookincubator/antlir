/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

/*
 * With rust 1.62, it appears that the thread sanitizer has been completely
 * broken for Linux because of the move of Mutex and other synchronization
 * primitives to futexes.
 *
 * Apparently, the address sanitizer cannot grok futexes, so it is not able
 * to understand when an access is safe.
 *
 * More details on the subject can be found here:
 * https://fburl.com/tsanwithrustfutexes
 *
 * As such, the cfg_sanitize flag will be used to conditionally disable
 * mp behavior for now.
 */
#![feature(cfg_sanitize)]

// TODO: Figure out how to hide these modules from non-test libraries
// These are only being exported as pub for now to support testing
pub mod mp;
pub mod send_elements;

pub mod upgrade;

extern crate crc32c_hw;
#[macro_use]
extern crate maplit;
extern crate num;
#[macro_use]
extern crate num_derive;
