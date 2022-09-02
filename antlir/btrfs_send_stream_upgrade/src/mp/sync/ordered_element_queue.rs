/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::HashMap;
use std::sync::Condvar;
use std::sync::Mutex;

use crate::mp::sync::blocking_queue::BlockingQueue;
use crate::mp::sync::blocking_sync_primitive::BlockingSyncPrimitive;
use crate::mp::sync::blocking_sync_primitive::PrimitiveState;
use crate::mp::sync::ordered_element::OrderedElement;

struct OrderedElementQueueInternal<T: OrderedElement> {
    /// The queue is just implemented as a hashmap
    oeqi_map: HashMap<u64, T>,
    /// The index of the element at the front
    oeqi_first_id: u64,
    /// The state of the queue
    oeqi_state: PrimitiveState,
}

///
/// An Ordered Element Queue
///
/// This is a data structure that is similar to a FIFO that is shared by a
/// set of producers and consumers, except it also requires the consumers to
/// consume queue items in a particular order.
///
/// All items are required to provide a range of numbers to indicate their
/// position relative to other items. To prevent deadlocks, each number starting
/// with 0 must be guaranteed to be delivered exactly once to the queue. The
/// only exception for this rule that the last number in an item's range can be
/// shared with the first number in another item's range; in this case, the
/// earlier item must indicate that its last number is shared.
///
/// For example, this is a good set of items:
///
/// 0 - 10 (not shared) -> Item 1
///        11 - 11 (shared) -> Item 2
///                11 - 15 (not shared) -> Item 3
/// * All items from 0 to 15 are present exactly once except for 11, which has
/// been marked as shared.
///
/// These are bad sets of items that will result in deadlocks:
///
/// 1 - 10 (not shared) -> Item 1
///        11 - 11 (shared) -> Item 2
///                11 - 15 (not shared) -> Item 3
/// * Number 0 is missing
///
/// 0 - 10 (not shared) -> Item 1
///        11 - 11 (not shared) -> Item 2
///                11 - 15 (not shared) -> Item 3
/// * Number 11 is not marked as shared between items 2 and 3
///
/// 0 - 9 (not shared) -> Item 1
///        11 - 11 (shared) -> Item 2
///                11 - 15 (not shared) -> Item 3
/// * Number 10 is missing
///
/// Items might arrive at the queue out-of-order; the queue will block all
/// consumers until the "next" item has been successfully enqueued.
///
pub struct OrderedElementQueue<T: OrderedElement> {
    /// A mutex to protect the internal queue
    oeq_mutex: Mutex<OrderedElementQueueInternal<T>>,
    /// A condvar to block consumers when the top of the queue isn't available
    oeq_cv: Condvar,
    /// The name of the queue
    oeq_name: String,
}

impl<T: OrderedElement> BlockingQueue<T> for OrderedElementQueue<T> {
    fn new(name: &'static str) -> anyhow::Result<Self> {
        Ok(Self {
            oeq_mutex: Mutex::new(OrderedElementQueueInternal {
                oeqi_map: HashMap::new(),
                oeqi_first_id: 0,
                oeqi_state: PrimitiveState::Running,
            }),
            oeq_cv: Condvar::new(),
            oeq_name: name.to_string(),
        })
    }
    fn get_name(&self) -> &String {
        &self.oeq_name
    }
    fn enqueue(&self, item: T) -> anyhow::Result<()> {
        // First lock to get the queue
        let mut queue = match self.oeq_mutex.lock() {
            Ok(internal_queue) => internal_queue,
            // This should only happen if another thread panicked because of
            // a recursive lock
            Err(error) => anyhow::bail!("Failed to acquire lock on enqueue with error {}", error),
        };
        match (*queue).oeqi_state {
            PrimitiveState::Running => (),
            PrimitiveState::Aborted => {
                anyhow::bail!("Enqueue failed because of early abort");
            }
            PrimitiveState::Done => {
                // We should never be done while enqueueing...
                anyhow::bail!("Enqueueing while done");
            }
        }
        // Add the item to the queue in question
        let key = item.get_first_id();
        match (*queue).oeqi_map.insert(key, item) {
            Some(old_item) => {
                anyhow::bail!(
                    "Inserting duplicate entry in ordered queue old entry {}",
                    old_item
                );
            }
            None => (),
        }
        // If we just added the first element in line, then wake the consumer
        if (*queue).oeqi_first_id == key {
            self.oeq_cv.notify_one();
        }
        Ok(())
    }
    fn dequeue(&self) -> anyhow::Result<Option<T>> {
        // First lock to get the queue
        let queue = match self.oeq_mutex.lock() {
            Ok(internal_queue) => internal_queue,
            // This should only happen if another thread panicked because of
            // a recursive lock
            Err(error) => anyhow::bail!("Failed to acquire lock on dequeue with error {}", error),
        };
        // Wait until we're certain that the top of the list has been inserted
        let mut queue = match self.oeq_cv.wait_while(queue, |queue| {
            !(*queue).oeqi_map.contains_key(&(*queue).oeqi_first_id)
                && (*queue).oeqi_state == PrimitiveState::Running
        }) {
            Ok(internal_queue) => internal_queue,
            // This should only happen if another thread panicked because of
            // a recursive lock
            Err(error) => anyhow::bail!("Failed to wait with error {}", error),
        };
        match (*queue).oeqi_state {
            PrimitiveState::Running => (),
            PrimitiveState::Aborted => {
                anyhow::bail!("Enqueue failed because of early abort");
            }
            PrimitiveState::Done => {
                return Ok(None);
            }
        }
        let current_key = (*queue).oeqi_first_id;
        let item = match (*queue).oeqi_map.remove(&current_key) {
            Some(item) => item,
            // This should never happen
            None => anyhow::bail!("Failed to remove contained item"),
        };
        // Update the new top element
        // If the last id was shared, ensure that we drop it
        (*queue).oeqi_first_id = item.get_last_id() + if item.is_last_id_shared() { 0 } else { 1 };
        // In case someone has a multi-consumer case in the future avoid a
        // deadlock by notifying the next consumer
        if (*queue).oeqi_map.contains_key(&(*queue).oeqi_first_id) {
            self.oeq_cv.notify_one();
        }
        // Drop the lock before doing any checks
        Mutex::unlock(queue);
        // Check that the item has the expected correct last id
        anyhow::ensure!(
            item.get_first_id() == current_key,
            "Expected {} got item {}",
            current_key,
            item
        );
        Ok(Some(item))
    }
}

impl<T: OrderedElement> BlockingSyncPrimitive for OrderedElementQueue<T> {
    fn halt(&self, unplanned: bool) -> anyhow::Result<()> {
        // First lock to get the queue
        let mut queue = match self.oeq_mutex.lock() {
            Ok(internal_queue) => internal_queue,
            // This should only happen if another thread panicked because of
            // a recursive lock
            Err(error) => anyhow::bail!("Failed to acquire lock on halt with error {}", error),
        };
        // Abort
        if (*queue).oeqi_state != PrimitiveState::Running {
            anyhow::bail!("Double halt on queue");
        }
        // Update the state
        (*queue).oeqi_state = if unplanned {
            PrimitiveState::Aborted
        } else {
            PrimitiveState::Done
        };
        // Wake everyone
        self.oeq_cv.notify_all();

        Ok(())
    }
}
