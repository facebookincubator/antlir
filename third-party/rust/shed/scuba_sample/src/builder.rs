/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under both the MIT license found in the
 * LICENSE-MIT file in the root directory of this source tree and the Apache
 * License, Version 2.0 found in the LICENSE-APACHE file in the root directory
 * of this source tree.
 */

//! See the [ScubaSampleBuilder] documentation

use fbinit::FacebookInit;
use serde_json::{Error, Value};
use std::collections::hash_map::Entry;
use std::fmt;
use std::fs::{File, OpenOptions};
use std::io::{Error as IoError, Write};
use std::num::NonZeroU64;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::sample::ScubaSample;
use crate::value::ScubaValue;
use crate::Sampling;

/// A helper builder to make it easier to create a new sample and log it into
/// the proper Scuba dataset.
#[derive(Clone)]
pub struct ScubaSampleBuilder {
    sample: ScubaSample,
    log_file: Option<Arc<Mutex<File>>>,
    sampling: Sampling,
    seq: Option<Arc<(String, AtomicU64)>>,
}

impl ScubaSampleBuilder {
    /// Create a new instance of the Builder with initially an empty sample
    /// that will preserve the sample in the provided dataset. The arguments
    /// are used only in fbcode builds.
    pub fn new<T: Into<String>>(_fb: FacebookInit, _dataset: T) -> Self {
        Self::with_discard()
    }

    /// Create a new instance of the Builder with initially an empty sample
    /// that will discard the sample instead of writing it to a Scuba dataset.
    pub fn with_discard() -> Self {
        Self {
            sample: ScubaSample::new(),
            log_file: None,
            sampling: Sampling::NoSampling,
            seq: None,
        }
    }

    /// Create a new instance of the Builder with initially an empty sample
    /// that will preserve the sample in the provided log file.
    pub fn with_log_file<L: AsRef<Path>>(mut self, log_file: L) -> Result<Self, IoError> {
        let log_file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(log_file)?;
        self.log_file = Some(Arc::new(Mutex::new(log_file)));
        Ok(self)
    }

    /// Enable log sequencing.  Each sample from this builder (or its clones)
    /// will get a monotonically incrementing sequence number logged in the
    /// named field with each log.
    pub fn with_seq(mut self, key: impl Into<String>) -> Self {
        self.seq = Some(Arc::new((key.into(), AtomicU64::new(0))));
        self
    }

    /// Return true if a client is not set for this builder. This method will
    /// return false even if a log file is provided and the sample will be
    /// preserved in it.
    pub fn is_discard(&self) -> bool {
        true
    }

    /// Call the internal sample's [super::sample::ScubaSample::add] method
    pub fn add<K: Into<String>, V: Into<ScubaValue>>(&mut self, key: K, value: V) -> &mut Self {
        self.sample.add(key, value);
        self
    }

    /// Call the internal sample's [super::sample::ScubaSample::add] method
    /// if the specified value is not `None`.
    pub fn add_opt<K: Into<String>, V: Into<ScubaValue>>(
        &mut self,
        key: K,
        value: Option<V>,
    ) -> &mut Self {
        if let Some(value) = value {
            self.sample.add(key, value);
        }
        self
    }

    /// Call the internal sample's [super::sample::ScubaSample::remove] method
    pub fn remove<K: Into<String>>(&mut self, key: K) -> &mut Self {
        self.sample.remove(key);
        self
    }

    /// Call the internal sample's [super::sample::ScubaSample::get] method
    pub fn get<K: Into<String>>(&self, key: K) -> Option<&ScubaValue> {
        self.sample.get(key)
    }

    /// Call the internal sample's [super::sample::ScubaSample::entry] method
    pub fn entry<K: Into<String>>(&mut self, key: K) -> Entry<String, ScubaValue> {
        self.sample.entry(key)
    }

    /// Only log one in sample_rate samples. The decision is made at the point where sampled() is
    /// called. Multiple calls to sampled() further reduce the logging probability.
    pub fn sampled(&mut self, sample_rate: NonZeroU64) -> &mut Self {
        self.sampling = self.sampling.sample(&mut rand::thread_rng(), sample_rate);
        self
    }

    /// Revert sampling.
    pub fn unsampled(&mut self) -> &mut Self {
        self.sampling = Sampling::NoSampling;
        self
    }

    /// Access this builder's underlying [Sampling].
    pub fn sampling(&self) -> &Sampling {
        &self.sampling
    }

    /// Get a reference to the internally built sample.
    pub fn get_sample(&self) -> &ScubaSample {
        &self.sample
    }

    /// Get a mutable reference to the internally built sample.
    pub fn get_sample_mut(&mut self) -> &mut ScubaSample {
        &mut self.sample
    }

    /// Set the [subset] of this sample.
    ///
    /// [subset]: https://fburl.com/qa/xqm9hsxx
    pub fn set_subset<S: Into<String>>(&mut self, subset: S) -> &mut Self {
        self.sample.set_subset(subset);
        self
    }

    /// Clear the [subset] of this sample.
    ///
    /// [subset]: https://fburl.com/qa/xqm9hsxx
    pub fn clear_subset(&mut self) -> &mut Self {
        self.sample.clear_subset();
        self
    }

    /// Update the sequence number in preparation for a new log operation.
    fn next_seq(&mut self) {
        if let Some((key, seq)) = self.seq.as_deref() {
            let next_seq = seq.fetch_add(1, Ordering::Relaxed);
            self.sample.add(key, next_seq);
        }
    }

    /// Log the internally built sample to the previously configured log file while overriding its
    /// timestamp to the current time. Returns whether the sample passed sampling.
    pub fn log(&mut self) -> bool {
        self.sample.set_time_now();
        self.next_seq();

        if !self.sampling.apply(&mut self.sample) {
            return false;
        }

        if let Some(ref log_file) = self.log_file {
            if let Ok(sample) = self.to_json() {
                let mut log_file = log_file.lock().expect("Poisoned lock");
                let _ = log_file.write_all(sample.to_string().as_bytes());
                let _ = log_file.write_all(b"\n");
            }
        }

        true
    }

    /// Log the internally built sample to the previously configured log file while overriding its
    /// timestamp to the provided time. Returns whether the sample passed sampling.
    pub fn log_with_time(&mut self, time: u64) -> bool {
        self.sample.set_time(time);
        self.next_seq();

        if !self.sampling.apply(&mut self.sample) {
            return true;
        }

        if let Some(ref log_file) = self.log_file {
            if let Ok(sample) = self.sample.to_json() {
                let mut log_file = log_file.lock().expect("Poisoned lock");
                let _ = log_file.write_all(sample.to_string().as_bytes());
                let _ = log_file.write_all(b"\n");
            }
        }

        true
    }

    /// Either flush the configured client with the provided timeout or flush
    /// the configured log file making sure all the logged samples have been
    /// written to it. The timeout is used only in fbcode builds.
    pub fn flush(&self, _timeout: Duration) {
        if let Some(ref log_file) = self.log_file {
            let mut log_file = log_file.lock().expect("Poisoned lock");
            let _ = log_file.flush();
        }
    }

    /// Return a json serialized sample
    pub fn to_json(&self) -> Result<Value, Error> {
        self.sample.to_json()
    }

    /// Add values to the sample that are widely used in Facebook services. For
    /// non-fbcode-builds it does nothing. The provided mapper function is used
    /// to transform the valuse before they are written to the sample.
    pub fn add_mapped_common_server_data<F>(&mut self, _mapper: F) -> &mut Self
    where
        F: Fn(ServerData) -> &'static str,
    {
        self
    }

    /// Add values to the sample that are widely used in Facebook services. For
    /// non-fbcode-builds it does nothing.
    pub fn add_common_server_data(&mut self) -> &mut Self {
        self.add_mapped_common_server_data(|data| data.default_key())
    }

    /// Call the internal sample's [super::sample::ScubaSample::join_values] method
    pub fn join_values(&mut self, sample: &ScubaSample) {
        self.sample.join_values(sample)
    }
}

impl fmt::Debug for ScubaSampleBuilder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ScubaSampleBuilder {{ sample: {:?} }}", self.sample)
    }
}

/// Enum representing commonly used server data written to the Scuba sample.
pub enum ServerData {
    /// Hostname of the server
    Hostname,
    /// Tier of the service
    Tier,
    /// Tupperware TaskId of the service
    TaskId,
    /// Tupperware CanaryId of the service
    CanaryId,
    /// Tupperware JobHandle of the service
    JobHandle,
    /// Build revision of the current binary
    BuildRevision,
    /// Build rule of the current binary
    BuildRule,
}

impl ServerData {
    /// Return a unique key for the server data under which the value will be
    /// stored in the sample. Pay attention not to use the same keys if you don't
    /// wish to override those values.
    pub fn default_key(&self) -> &'static str {
        match self {
            ServerData::Hostname => "server_hostname",
            ServerData::Tier => "server_tier",
            ServerData::TaskId => "tw_task_id",
            ServerData::CanaryId => "tw_canary_id",
            ServerData::JobHandle => "tw_handle",
            ServerData::BuildRevision => "build_revision",
            ServerData::BuildRule => "build_rule",
        }
    }
}
