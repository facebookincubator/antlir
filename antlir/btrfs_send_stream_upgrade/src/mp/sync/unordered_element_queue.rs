/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::VecDeque;
use std::fmt::Debug;
use std::fmt::Display;
use std::sync::Condvar;
use std::sync::Mutex;

use crate::mp::sync::blocking_queue::BlockingQueue;
use crate::mp::sync::blocking_sync_primitive::BlockingSyncPrimitive;
use crate::mp::sync::blocking_sync_primitive::PrimitiveState;
use crate::mp::sync::unordered_element::UnorderedElement;

struct UnorderedElementQueueInternal<T: UnorderedElement> {
    /// The queue is just implemented as a hashmap
    uoeqi_queue: VecDeque<T>,
    /// The state of the queue
    uoeqi_state: PrimitiveState,
}

impl<T: UnorderedElement> Debug for UnorderedElementQueue<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.fmt_internal(f)
    }
}

impl<T: UnorderedElement> Display for UnorderedElementQueue<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.fmt_internal(f)
    }
}

pub struct UnorderedElementQueue<T: UnorderedElement> {
    /// A mutex to protect the internal queue
    uoeq_mutex: Mutex<UnorderedElementQueueInternal<T>>,
    /// A condvar to block consumers when the top of the queue isn't available
    uoeq_cv: Condvar,
    /// The name of the queue
    uoeq_name: String,
}

impl<T: UnorderedElement> BlockingQueue<T> for UnorderedElementQueue<T> {
    fn new(name: &'static str) -> anyhow::Result<Self> {
        Ok(Self {
            uoeq_mutex: Mutex::new(UnorderedElementQueueInternal {
                uoeqi_queue: VecDeque::new(),
                uoeqi_state: PrimitiveState::Running,
            }),
            uoeq_cv: Condvar::new(),
            uoeq_name: name.to_string(),
        })
    }
    fn get_name(&self) -> &String {
        &self.uoeq_name
    }
    fn enqueue(&self, item: T) -> anyhow::Result<()> {
        // First lock to get the queue
        let mut queue = match self.uoeq_mutex.lock() {
            Ok(internal_queue) => internal_queue,
            // This should only happen if another thread panicked because of
            // a recursive lock
            Err(error) => anyhow::bail!("Failed to acquire lock on enqueue with error {}", error),
        };
        match (*queue).uoeqi_state {
            PrimitiveState::Running => (),
            PrimitiveState::Aborted => {
                anyhow::bail!("Enqueue failed because of early abort");
            }
            PrimitiveState::Done => {
                // We should never be done while enqueueing...
                anyhow::bail!("Enqueueing while done");
            }
        }
        (*queue).uoeqi_queue.push_back(item);
        // Wake the consumer
        self.uoeq_cv.notify_one();
        Ok(())
    }
    fn dequeue(&self) -> anyhow::Result<Option<T>> {
        // First lock to get the queue
        let queue = match self.uoeq_mutex.lock() {
            Ok(internal_queue) => internal_queue,
            // This should only happen if another thread panicked because of
            // a recursive lock
            Err(error) => anyhow::bail!("Failed to acquire lock on dequeue with error {}", error),
        };
        // Wait until we're certain that the top of the list has been inserted
        let mut queue = match self.uoeq_cv.wait_while(queue, |queue| {
            (*queue).uoeqi_queue.is_empty() && (*queue).uoeqi_state == PrimitiveState::Running
        }) {
            Ok(internal_queue) => internal_queue,
            // This should only happen if another thread panicked because of
            // a recursive lock
            Err(error) => anyhow::bail!("Failed to wait with error {}", error),
        };
        match (*queue).uoeqi_state {
            PrimitiveState::Running => (),
            PrimitiveState::Aborted => {
                anyhow::bail!("Enqueue failed because of early abort");
            }
            PrimitiveState::Done => {
                return Ok(None);
            }
        }
        let item = match (*queue).uoeqi_queue.pop_front() {
            Some(item) => item,
            // This should only happen if another thread panicked because of
            // a recursive lock
            None => anyhow::bail!("Failed to remove contained item"),
        };
        Ok(Some(item))
    }
}

impl<T: UnorderedElement> BlockingSyncPrimitive for UnorderedElementQueue<T> {
    fn halt(&self, unplanned: bool) -> anyhow::Result<()> {
        // First lock to get the queue
        let mut queue = match self.uoeq_mutex.lock() {
            Ok(internal_queue) => internal_queue,
            // This should only happen if another thread panicked because of
            // a recursive lock
            Err(error) => anyhow::bail!("Failed to acquire lock on halt with error {}", error),
        };
        // Disallow transitions from Aborted to Done
        // We can go from Done to Aborted in the case of a later failure
        if (*queue).uoeqi_state == PrimitiveState::Aborted && !unplanned {
            anyhow::bail!("Transitioning queue from Done to Aborted");
        }
        // Update the state
        (*queue).uoeqi_state = if unplanned {
            PrimitiveState::Aborted
        } else {
            PrimitiveState::Done
        };
        // Wake everyone
        self.uoeq_cv.notify_all();

        Ok(())
    }
}

impl<T: UnorderedElement> UnorderedElementQueue<T> {
    fn fmt_internal(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "<UnorderedElementQueue Queue={:?} Wait={:?} Name={:?}/>",
            self.uoeq_mutex, self.uoeq_cv, self.uoeq_name,
        )
    }
}

impl<T: UnorderedElement> Debug for UnorderedElementQueueInternal<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "<UnorderedElementQueueInternal Queue={:?} State={:?}/>",
            self.uoeqi_queue, self.uoeqi_state,
        )
    }
}
