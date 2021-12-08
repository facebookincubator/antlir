/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use anyhow::Result;
use slog::{debug, info, Logger};
use std::fmt;
use structopt::StructOpt;

use netlink::{NlRoutingSocket, RtnlCachedLink, RtnlCachedLinkTrait, RtnlLinkCache};

#[derive(StructOpt)]
pub struct Opts {}

fn link_cleanup<T: RtnlCachedLinkTrait + fmt::Display>(
    log: &Logger,
    rsock: &NlRoutingSocket,
    rlc: &[T],
) -> Result<()> {
    for link in &mut rlc.iter() {
        debug!(log, "Inspecting link: {}", link);

        // Look at up links named "eth*".
        if !link.is_up()
            || !link
                .name()
                .unwrap_or_else(|| "".to_string())
                .starts_with("eth")
        {
            continue;
        }

        info!(log, "Resetting link: {}", link);
        link.set_down(rsock)?;
        link.set_up(rsock)?;
    }
    Ok(())
}

pub fn network_cleanup(log: Logger, _opts: Opts) -> Result<()> {
    debug!(log, "Starting network_cleanup()");

    let rsock = NlRoutingSocket::new()?;
    let rlc = RtnlLinkCache::new(&rsock)?;
    link_cleanup::<RtnlCachedLink>(&log, &rsock, rlc.links())?;

    debug!(log, "Finished network_cleanup()");
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::network_cleanup::*;
    use anyhow::Result;
    use netlink::RtnlLinkCommon;
    use std::cell::RefCell;
    use std::fmt;

    struct RtnlLinkTest {
        index: i32,
        name: &'static str,
        up: bool,
        expected_actions: Vec<&'static str>,
        // We wrap actions in a RefCell because to update them we need
        // a mutable reference, but we need that reference from a callback
        // (set{up|down}) which has an immutable RtnlLinkTest reference.
        actions: RefCell<Vec<&'static str>>,
    }

    impl RtnlLinkTest {
        fn new(
            i: &mut i32,
            name: &'static str,
            up: bool,
            expected_actions: Vec<&'static str>,
        ) -> Self {
            let rv = Self {
                index: *i,
                name,
                up,
                expected_actions,
                actions: RefCell::new(vec![]),
            };
            *i += 1;
            rv
        }
    }

    impl RtnlLinkCommon for RtnlLinkTest {
        fn index(&self) -> i32 {
            self.index
        }
        fn name(&self) -> Option<String> {
            Some(self.name.to_string())
        }
        fn is_up(&self) -> bool {
            self.up
        }
    }

    impl RtnlCachedLinkTrait for RtnlLinkTest {
        fn set_up(&self, _sock: &NlRoutingSocket) -> Result<()> {
            self.actions.borrow_mut().push("up");
            Ok(())
        }
        fn set_down(&self, _sock: &NlRoutingSocket) -> Result<()> {
            self.actions.borrow_mut().push("down");
            Ok(())
        }
    }

    impl fmt::Display for RtnlLinkTest {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            self.display(f)
        }
    }

    #[test]
    fn test_link_cleanup() -> Result<()> {
        let mut i: i32 = 0;
        let links: Vec<RtnlLinkTest> = vec![
            RtnlLinkTest::new(&mut i, "down", false, vec![]),
            RtnlLinkTest::new(&mut i, "up", true, vec![]),
            RtnlLinkTest::new(&mut i, "eth_down", false, vec![]),
            RtnlLinkTest::new(&mut i, "eth_up1", true, vec!["down", "up"]),
            RtnlLinkTest::new(&mut i, "eth_up2", true, vec!["down", "up"]),
        ];

        let log = slog::Logger::root(slog_glog_fmt::default_drain(), slog::o!());
        let rsock = NlRoutingSocket::new()?;
        link_cleanup::<RtnlLinkTest>(&log, &rsock, &links)?;

        for link in links {
            assert!(*link.actions.borrow() == link.expected_actions);
        }

        Ok(())
    }
}
