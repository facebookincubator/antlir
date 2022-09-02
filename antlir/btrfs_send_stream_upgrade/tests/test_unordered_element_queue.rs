/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::HashSet;
use std::fmt::Display;
use std::sync::Arc;
use std::thread;
use std::time;

pub use btrfs_send_stream_upgrade_lib::mp::sync::blocking_queue::BlockingQueue;
pub use btrfs_send_stream_upgrade_lib::mp::sync::blocking_sync_primitive::BlockingSyncPrimitive;
pub use btrfs_send_stream_upgrade_lib::mp::sync::unordered_element::UnorderedElement;
pub use btrfs_send_stream_upgrade_lib::mp::sync::unordered_element_queue::UnorderedElementQueue;
use rand::seq::SliceRandom;
use rand::thread_rng;

const FIVE_MILLISECONDS: time::Duration = time::Duration::from_millis(5);
const NUM_PRODUCERS: u32 = 17;
const MAXIMUM_NUMBER: u32 = 48611;

// For testing, let's just use a simple wrapper around a number
struct NumberWrapper {
    value: u32,
}

unsafe impl Send for NumberWrapper {}

impl Display for NumberWrapper {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.value)
    }
}

impl UnorderedElement for NumberWrapper {}

fn producer(
    queue: &UnorderedElementQueue<NumberWrapper>,
    thread_idx: u32,
    num_threads: u32,
    max_value: u32,
) {
    let mut items_to_enqueue = Vec::<NumberWrapper>::new();
    let mut val = thread_idx;
    // Generate values in the specified range
    while val < max_value {
        items_to_enqueue.push(NumberWrapper { value: val });
        val += num_threads;
    }
    // Shuffle the values so that the consumer gets them out-of-order
    let mut rng = thread_rng();
    items_to_enqueue.shuffle(&mut rng);
    // Push everything we have to the consumer; we'll wait a bit between
    // pushes
    while !items_to_enqueue.is_empty() {
        let item = match items_to_enqueue.pop() {
            Some(value) => value,
            None => panic!("Expected to have an item to pop!"),
        };
        if let Err(error) = queue.enqueue(item) {
            panic!("Producer hit error on enqueue {}", error);
        }
        thread::sleep(FIVE_MILLISECONDS);
    }
}

fn consumer(queue: &UnorderedElementQueue<NumberWrapper>, max_value: u32) {
    let mut set: HashSet<u32> = HashSet::new();
    for i in 0..max_value {
        let item = match queue.dequeue() {
            Ok(Some(value)) => value,
            Ok(None) => {
                panic!("Consumer hit unexpected end of input on item {}", i);
            }
            Err(e) => panic!("Consumer hit error on dequeue {}", e),
        };
        let value = item.value;
        if set.contains(&value) || value >= max_value {
            panic!(
                "Got an invalid value of {} with max of {}",
                value, max_value
            );
        }
        set.insert(value);
    }
}

#[test]
fn ensure_all_numbers_are_seen() -> anyhow::Result<()> {
    let queue = match UnorderedElementQueue::<NumberWrapper>::new("Test Queue") {
        Ok(queue) => Arc::new(queue),
        Err(error) => anyhow::bail!(error),
    };

    // Start up the producer
    let thread_local_queue = queue.clone();
    let consumer = thread::spawn(move || consumer(&thread_local_queue, MAXIMUM_NUMBER));

    let mut producers = Vec::<thread::JoinHandle<_>>::new();

    for i in 0..NUM_PRODUCERS {
        let thread_local_queue = queue.clone();
        let producer =
            thread::spawn(move || producer(&thread_local_queue, i, NUM_PRODUCERS, MAXIMUM_NUMBER));
        producers.push(producer);
    }

    // Check to see if the consumer had any errors
    // Check this first to handle the shutdown cases -- we expect
    // the consumers to die gracefully
    if let Err(error) = consumer.join() {
        anyhow::bail!("Consumer failed with error {:#?}", error);
    }

    // Check to see if any producer had errors
    for i in 0..NUM_PRODUCERS {
        let producer = match producers.pop() {
            Some(join_handle) => join_handle,
            None => anyhow::bail!("Failed to retrieve join handle for producer {}", i),
        };
        if let Err(error) = producer.join() {
            anyhow::bail!("Producer failed with error {:#?}", error);
        }
    }

    Ok(())
}
