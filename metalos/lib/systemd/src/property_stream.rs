/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::pin::Pin;

use futures::stream::Stream;
use futures::task::Context;
use futures::task::Poll;
use futures::FutureExt;
use slog::debug;
use slog::error;
use slog::Logger;
use tokio::sync::mpsc::Receiver;
use zbus::Proxy;
use zvariant::OwnedValue;

use crate::Error;
use crate::Result;

/// Implement our own version of PropertyStream to catch DBus property changes,
/// that [zbus] has some unfortunate bugs that cause it to frequently lose
/// updates when using the stream api.
pub struct PropertyStream<V>
where
    V: TryFrom<OwnedValue, Error = zvariant::Error> + Send,
{
    rx: Receiver<V>,
}

impl<V: 'static> PropertyStream<V>
where
    V: TryFrom<OwnedValue, Error = zvariant::Error> + Send,
{
    fn new(rx: Receiver<V>) -> Self {
        Self { rx }
    }

    pub(crate) async fn start<'a>(
        log: Logger,
        proxy: &Proxy<'a>,
        name: &'static str,
        initial_value: V,
    ) -> Result<PropertyStream<V>> {
        let (tx, rx) = tokio::sync::mpsc::channel(64);
        tx.send(initial_value)
            .await
            .map_err(|_| Error::PropertyStream("failed to send initial value".to_string()))?;
        proxy
            .connect_property_changed(name, move |val| {
                let tx = tx.clone();
                let log = log.clone();
                if val.is_none() {
                    return async {}.boxed();
                }
                match V::try_from(OwnedValue::from(val.unwrap())) {
                    Ok(v) => async move {
                        // The only failure mode for .send() is if the receiver is
                        // closed (if the channel is simply full, send() blocks). In
                        // this event, we really should deregister the property
                        // change handler to prevent further calls, but the zbus api
                        // makes this impossible, so just nicely log the fact that
                        // we dropped an update and move on.
                        if tx.send(v).await.is_err() {
                            debug!(log, "Got update after receiver was closed");
                        }
                    }
                    .boxed(),
                    Err(e) => {
                        error!(
                            log,
                            "Failed to convert property update into required type: {:?}", e
                        );
                        async {}.boxed()
                    }
                }
            })
            .await
            .map_err(|e| {
                Error::PropertyStream(format!("failed to attach property changed handle: {:?}", e))
            })?;
        Ok(Self::new(rx))
    }
}

impl<V> Stream for PropertyStream<V>
where
    V: TryFrom<OwnedValue, Error = zvariant::Error> + Send,
{
    type Item = V;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.get_mut().rx.poll_recv(cx)
    }
}

/// Create a PropertyStream using the specified proxy and connect_$prop_changed
/// method. For convenience, the current property value is put into the stream
/// so that caller code does not have to poll for the initial value.
/// Sure, macros are not the prettiest, but this is actually safe at compile
/// time, compared to the stringly-typed generic api that is exposed by [zbus],
/// that would not know if a property even existed at compile time, let alone
/// what type it contains.
macro_rules! property_stream {
    ($log:expr, $proxy:expr, $property:ident, $property_dbus:expr) => {{
        // Not only does sending the initial value make calling code
        // simpler, but it also has the nice side effect of ensuring that
        // the property we want is actually the right type at compile time.
        let value = $proxy.$property().await.map_err(|e| {
            crate::Error::PropertyStream(format!("failed to get initial property value: {:?}", e))
        })?;
        PropertyStream::start($log.clone(), &$proxy, $property_dbus, value)
    }};
}

pub(crate) use property_stream;
