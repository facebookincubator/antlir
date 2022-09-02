/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use crate::mp::sync::blocking_sync_primitive::BlockingSyncPrimitive;

pub trait BlockingQueue<T>: BlockingSyncPrimitive + Sized {
    /// Creates a new queue
    fn new(name: &'static str) -> anyhow::Result<Self>;
    /// Retrives the name of the queue
    fn get_name(&self) -> &String;
    /// Enqueues a new element
    fn enqueue(&self, item: T) -> anyhow::Result<()>;
    /// Dequeues an element
    fn dequeue(&self) -> anyhow::Result<Option<T>>;
}
