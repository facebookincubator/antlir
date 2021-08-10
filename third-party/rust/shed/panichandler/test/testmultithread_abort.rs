/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under both the MIT license found in the
 * LICENSE-MIT file in the root directory of this source tree and the Apache
 * License, Version 2.0 found in the LICENSE-APACHE file in the root directory
 * of this source tree.
 */

use std::thread;

use panichandler::Fate;

#[cfg(unix)]
extern "C" fn sighandler(_sig: std::os::raw::c_int) {
    println!("I shouldn't have been called")
}

fn main() {
    #[cfg(unix)]
    unsafe {
        libc::signal(libc::SIGABRT, sighandler as libc::size_t);
    }

    println!("I'm on an adventure!");

    panichandler::set_panichandler(Fate::Abort);

    let t = thread::spawn(|| panic!("I paniced! {} {}", "Everything's awful!", 1234));
    let _ = t.join();

    println!("I shouldn't have returned");
}
