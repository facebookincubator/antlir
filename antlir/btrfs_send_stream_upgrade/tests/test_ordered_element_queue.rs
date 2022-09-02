/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fmt::Display;
use std::sync::Arc;
use std::thread;
use std::time;

pub use btrfs_send_stream_upgrade_lib::mp::sync::blocking_queue::BlockingQueue;
pub use btrfs_send_stream_upgrade_lib::mp::sync::ordered_element::OrderedElement;
pub use btrfs_send_stream_upgrade_lib::mp::sync::ordered_element_queue::OrderedElementQueue;
use rand::seq::SliceRandom;
use rand::thread_rng;

const FIVE_MILLISECONDS: time::Duration = time::Duration::from_millis(5);
const TWO_SECONDS: time::Duration = time::Duration::from_secs(2);
const FIVE_SECONDS: time::Duration = time::Duration::from_secs(5);
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

impl OrderedElement for NumberWrapper {
    fn get_first_id(&self) -> u64 {
        self.value as u64
    }
    fn get_last_id(&self) -> u64 {
        self.value as u64
    }
    fn is_last_id_shared(&self) -> bool {
        false
    }
}

fn producer(
    queue: &OrderedElementQueue<NumberWrapper>,
    thread_idx: u32,
    num_threads: u32,
    max_value: u32,
    gracefully_exit_on_enqueue_failure: bool,
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
        let item = items_to_enqueue
            .pop()
            .expect("Expected to have an item to pop!");
        if let Err(error) = queue.enqueue(item) {
            // Ignore wacky errors on shutdown
            // Normally we don't expect to gracefully shutdown while there
            // are elements left to enqueue
            if gracefully_exit_on_enqueue_failure {
                return;
            }
            panic!("Producer hit error on enqueue {}", error);
        }
        thread::sleep(FIVE_MILLISECONDS);
    }
}

fn consumer(
    queue: &OrderedElementQueue<NumberWrapper>,
    max_value: u32,
    gracefully_exit_on_none: bool,
) {
    for i in 0..max_value {
        let item = match queue.dequeue() {
            Ok(Some(value)) => value,
            Ok(None) => {
                if gracefully_exit_on_none {
                    return;
                }
                panic!("Consumer hit unexpected end of input on item {}", i);
            }
            Err(e) => panic!("Consumer hit error on dequeue {}", e),
        };
        if item.value != i {
            panic!("Got a value of {} but expected {}", item.value, i);
        }
    }
}

fn resequence_from_random_order_int(mode: Option<(time::Duration, bool)>) -> anyhow::Result<()> {
    let queue = match OrderedElementQueue::<NumberWrapper>::new("Test Queue") {
        Ok(queue) => Arc::new(queue),
        Err(error) => anyhow::bail!(error),
    };

    let force_graceful_shutdown = match mode {
        None => false,
        Some((_, unplanned)) => !unplanned,
    };

    // Start up the producer
    let thread_local_queue = queue.clone();
    let consumer = thread::spawn(move || {
        consumer(&thread_local_queue, MAXIMUM_NUMBER, force_graceful_shutdown)
    });

    let mut producers = Vec::<thread::JoinHandle<_>>::new();

    for i in 0..NUM_PRODUCERS {
        let thread_local_queue = queue.clone();
        let producer = thread::spawn(move || {
            producer(
                &thread_local_queue,
                i,
                NUM_PRODUCERS,
                MAXIMUM_NUMBER,
                force_graceful_shutdown,
            )
        });
        producers.push(producer);
    }

    match mode {
        None => (),
        Some((wait_time, unplanned)) => {
            // Wait before we abort the test
            thread::sleep(wait_time);
            // Halt
            queue.halt(unplanned)?;
        }
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

#[test]
fn resequence_from_random_order() -> anyhow::Result<()> {
    resequence_from_random_order_int(None)
}

#[test]
fn resequence_from_random_order_unplanned_shutdown() -> anyhow::Result<()> {
    let now = time::Instant::now();
    match resequence_from_random_order_int(Some((TWO_SECONDS, true))) {
        Ok(()) => {
            anyhow::bail!("Unplanned shutdown should have panicked consumer");
        }
        Err(_) => {
            // If we tried to kill a ~14s test after 2s, we should complete
            // within ~5s
            anyhow::ensure!(
                now.elapsed() < FIVE_SECONDS,
                "Test took longer than 5s to wrap up"
            );
        }
    }
    Ok(())
}

#[test]
fn resequence_from_random_order_planned_shutdown() -> anyhow::Result<()> {
    let now = time::Instant::now();
    match resequence_from_random_order_int(Some((TWO_SECONDS, false))) {
        Ok(()) => {
            // If we tried to kill a ~14s test after 2s, we should complete
            // within ~5s
            anyhow::ensure!(
                now.elapsed() < FIVE_SECONDS,
                "Test took longer than 5s to wrap up"
            );
        }
        Err(_) => {
            anyhow::bail!("Unplanned shutdown should have panicked consumer");
        }
    }
    Ok(())
}
