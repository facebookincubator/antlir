/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::net::IpAddr;

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Result;
use async_trait::async_trait;
use clap::Parser;
use reqwest::Client;
use slog::info;
use slog::Logger;
use tokio::sync::Mutex;
use url::Url;

#[cfg(test)]
#[macro_use]
extern crate metalos_macros;

/// This enum holds the different possible identifiers of who is sending this
/// request. Depending on your situation it may be easier to get one of these
/// than the others
#[derive(Debug, Clone)]
#[cfg_attr(test, derive(PartialEq))]
pub enum Source {
    AssetId(i64),
    Mac(String),
    IpAddr(IpAddr),
}

/// This is a convenience struct that you can use in your argparse if you want
/// to allow people to specify their caller source.
#[derive(Parser, Debug, Clone)]
pub struct SourceArgs {
    /// Send events with this asset_id
    /// Note: this is not the id we send the event to but rather what the source machines id is.
    #[clap(long, conflicts_with_all = &["mac-address", "ip-address"])]
    asset_id: Option<i64>,

    /// Send events with this mac address
    /// Note: this is not the mac we send the event to but rather what the source machines mac is.
    #[clap(long, conflicts_with_all = &["asset-id", "ip-address"])]
    mac_address: Option<String>,

    /// Send events with this IP Address.
    /// Note: this is not the IP we send the event to but rather what the source machines IP is.
    #[clap(long, conflicts_with_all = &["asset-id", "mac-address"])]
    ip_address: Option<IpAddr>,
}

// We provide this impl so that you can easily use it with
impl From<SourceArgs> for Option<Source> {
    fn from(opts: SourceArgs) -> Self {
        match (opts.asset_id, opts.mac_address, opts.ip_address) {
            (Some(asset_id), None, None) => Some(Source::AssetId(asset_id)),
            (None, Some(mac_address), None) => Some(Source::Mac(mac_address)),
            (None, None, Some(ip_address)) => Some(Source::IpAddr(ip_address)),
            (None, None, None) => None,
            _ => {
                panic!("Clap and code logic don't match. This is a bug")
            }
        }
    }
}

/// Identity is used to identify the source of the event
/// it include both the host it's coming from and also who
/// on the host is sending it.
#[derive(Debug, Clone)]
#[cfg_attr(test, derive(PartialEq))]
pub struct Identity {
    pub source: Source,
    // This is usually something like the script or high level
    // component that is sending this message
    pub sender: String,
}

/// A generic event type
#[derive(Debug)]
#[cfg_attr(test, derive(PartialEq))]
pub struct Event {
    pub name: String,
    pub payload: Option<serde_json::Value>,
}

pub struct EventSender<S> {
    identity: Identity,
    sink: S,
}

impl<S, ID> EventSender<S>
where
    S: EventSink<EventId = ID>,
{
    pub fn new(source: Source, sender: String, sink: S) -> Self {
        Self {
            identity: Identity { source, sender },
            sink,
        }
    }

    pub async fn send<T, E>(&self, event: T) -> Result<S::EventId>
    where
        T: TryInto<Event, Error = E>,
        E: Into<anyhow::Error> + 'static + Send + Sync,
    {
        self.sink
            .send(
                event
                    .try_into()
                    .map_err(|e| e.into())
                    .context("Trying to convert to Event")?,
                &self.identity,
            )
            .await
    }
}

#[async_trait]
pub trait EventSink {
    type EventId: std::fmt::Debug;

    async fn send(&self, event: Event, identity: &Identity) -> Result<Self::EventId>;
}

#[async_trait]
pub trait SerializedEventSink: Send + Sync {
    type EventId: std::fmt::Debug;

    async fn send(&mut self, event: Event, identity: &Identity) -> Result<Self::EventId>;
}

// We can provide an impl of serialized sink for anything that doesn't need to be exclusive.
// If we are given exclusive reference to something we can just use the non-exclusive logic
// and it will work just fine.
#[async_trait]
impl<S> SerializedEventSink for S
where
    S: EventSink + Send + Sync,
{
    type EventId = S::EventId;

    async fn send(&mut self, event: Event, identity: &Identity) -> Result<Self::EventId> {
        <Self as EventSink>::send(self, event, identity).await
    }
}

pub struct SerializedSink<'a, S: SerializedEventSink> {
    sink: Mutex<&'a mut S>,
}

impl<'a, S: SerializedEventSink> SerializedSink<'a, S> {
    pub fn new(sink: &'a mut S) -> Self {
        Self {
            sink: Mutex::new(sink),
        }
    }
}

#[async_trait]
impl<'a, S: SerializedEventSink> EventSink for SerializedSink<'a, S> {
    type EventId = S::EventId;

    async fn send(&self, event: Event, identity: &Identity) -> Result<Self::EventId> {
        let mut sink = self.sink.lock().await;
        (&mut sink).send(event, identity).await
    }
}

/// A HTTP reporting sink. It expects an endpoint supporting GET requests with the following formats:
///
///  * /sendEvent?name=<name>&sender=<text>&mac=<mac>&payload=<json_payload>
///  * /sendEvent?name=<name>&sender=<text>&ip=<ipv6 or ipv4>&payload=<payload>
///  * /sendEvent?name=<name>&sender=<text>&assetID=<assetID>&payload=<payload>
///
/// We also expect that this endpoint gives back some id that we can give to our caller
/// identifying the send event.
#[derive(Debug)]
pub struct HttpSink {
    pub target_url: Url,
}

impl HttpSink {
    pub fn new(target_url: Url) -> Self {
        Self { target_url }
    }
}

#[async_trait]
impl EventSink for HttpSink {
    type EventId = String;

    async fn send(&self, event: Event, identity: &Identity) -> Result<Self::EventId> {
        let mut url = self.target_url.clone();

        url.query_pairs_mut()
            .append_pair("name", &event.name)
            .append_pair("sender", &identity.sender);
        if let Some(p) = &event.payload {
            url.query_pairs_mut().append_pair(
                "payload",
                &serde_json::to_string(p).context("Failed to convert payload to json")?,
            );
        }

        match &identity.source {
            Source::AssetId(asset_id) => url
                .query_pairs_mut()
                .append_pair("assetID", &asset_id.to_string()),
            Source::Mac(mac_address) => url.query_pairs_mut().append_pair("mac", mac_address),
            Source::IpAddr(ip_address) => url
                .query_pairs_mut()
                .append_pair("ip", &ip_address.to_string()),
        };

        let client = Client::builder()
            .use_rustls_tls()
            .build()
            .context("building http client")?;

        let response = client
            .get(url.clone())
            .send()
            .await
            .context(format!("while sending event GET to: {:?}", url))?;

        let status = response.status();

        let body = response
            .text()
            .await
            .context("while parsing event response as text")?;

        if status.is_success() {
            Ok(body)
        } else {
            Err(anyhow!(
                "Got status {} when sending our request. Body of response: {}",
                status.as_u16(),
                body
            ))
        }
    }
}

pub struct LoggerSink {
    log: Logger,
}

impl LoggerSink {
    pub fn new(log: Logger) -> Self {
        Self { log }
    }
}

#[async_trait]
impl EventSink for LoggerSink {
    type EventId = ();

    async fn send(&self, event: Event, identity: &Identity) -> Result<Self::EventId> {
        let payload = match &event.payload {
            Some(payload) => {
                serde_json::to_string(payload).context("Failed to convert payload to json")?
            }
            None => "None".to_string(),
        };

        info!(
            self.log,
            "Event: {} sent by {} from {:?} with payload {}",
            event.name,
            identity.sender,
            identity.source,
            payload
        );

        Ok(())
    }
}

pub struct MockSink {
    pub next_error: Option<anyhow::Error>,
    pub events: Vec<(Event, Identity)>,
}

impl MockSink {
    pub fn new() -> Self {
        Self {
            next_error: None,
            events: Vec::new(),
        }
    }

    pub fn set_error(&mut self, error: anyhow::Error) {
        self.next_error = Some(error);
    }
}

#[async_trait]
impl SerializedEventSink for MockSink {
    type EventId = usize;

    async fn send(&mut self, event: Event, identity: &Identity) -> Result<Self::EventId> {
        self.events.push((event, identity.clone()));
        if self.next_error.is_some() {
            Err(self.next_error.take()).unwrap()
        } else {
            Ok(self.events.len())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use http_test::make_test_server;
    use maplit::hashmap;
    use serde_json::Value;

    #[test]
    fn test_source_args() -> Result<()> {
        assert_eq!(
            Into::<Option<Source>>::into(SourceArgs {
                asset_id: Some(123),
                mac_address: None,
                ip_address: None,
            }),
            Some(Source::AssetId(123)),
        );
        assert_eq!(
            Into::<Option<Source>>::into(SourceArgs {
                asset_id: None,
                mac_address: Some("fake_mac".to_string()),
                ip_address: None,
            }),
            Some(Source::Mac("fake_mac".to_string())),
        );
        assert_eq!(
            Into::<Option<Source>>::into(SourceArgs {
                asset_id: None,
                mac_address: None,
                ip_address: Some("127.0.0.1".parse()?),
            }),
            Some(Source::IpAddr("127.0.0.1".parse()?)),
        );
        assert_eq!(
            Into::<Option<Source>>::into(SourceArgs {
                asset_id: None,
                mac_address: None,
                ip_address: None,
            }),
            None,
        );

        assert!(is_bad_args(SourceArgs {
            asset_id: Some(123),
            mac_address: Some("fake_mac".to_string()),
            ip_address: Some("127.0.0.1".parse()?),
        }));
        assert!(is_bad_args(SourceArgs {
            asset_id: None,
            mac_address: Some("fake_mac".to_string()),
            ip_address: Some("127.0.0.1".parse()?),
        }));
        assert!(is_bad_args(SourceArgs {
            asset_id: Some(123),
            mac_address: None,
            ip_address: Some("127.0.0.1".parse()?),
        }));
        assert!(is_bad_args(SourceArgs {
            asset_id: Some(123),
            mac_address: Some("fake_mac".to_string()),
            ip_address: None,
        }));

        Ok(())
    }

    fn is_bad_args(args: SourceArgs) -> bool {
        std::panic::catch_unwind(|| {
            let _: Option<Source> = args.into();
            Result::<(), ()>::Ok(())
        })
        .is_err()
    }

    #[test]
    async fn test_event_sender() -> Result<()> {
        let mut sink = MockSink::new();
        // Because MockSink isn't thread safe we are also testing our Serialized logic
        let serialized_sink = SerializedSink::new(&mut sink);

        let sender = EventSender::new(
            Source::AssetId(123),
            "unit-test".to_string(),
            serialized_sink,
        );

        let v: Value = serde_json::from_str(
            r#"{
                "something": true,
                "number": 123,
                "null": null
            }"#,
        )?;

        let id = sender
            .send(Event {
                name: "test-event".to_string(),
                payload: Some(v.clone()),
            })
            .await
            .context("failed to send event")?;

        assert_eq!(id, 1);
        assert_eq!(
            sink.events,
            vec![(
                Event {
                    name: "test-event".to_string(),
                    payload: Some(v),
                },
                Identity {
                    sender: "unit-test".to_string(),
                    source: Source::AssetId(123),
                }
            )]
        );

        Ok(())
    }

    #[containertest]
    async fn test_http_event_sender() -> Result<()> {
        let f = std::fs::File::create("/etc/resolv.conf")
            .context("failed to create empty /etc/resolv.conf")?;
        f.sync_all().context("failed to sync file")?;

        let test_payload: Value = serde_json::from_str(
            r#"{
                "something": true,
                "number": 123,
                "null": null
            }"#,
        )?;
        let test_payload_inner = test_payload.clone();

        let (requests, test_fn_outcome) = make_test_server(
            move |addr| {
                let test_payload_inner = test_payload_inner.clone();
                async move {
                    let sink = HttpSink::new(
                        format!("http://{}/sendEvent", addr)
                            .parse()
                            .expect("Failed to parse send event uri"),
                    );
                    let sender =
                        EventSender::new(Source::AssetId(123), "unit-test".to_string(), sink);

                    let id = sender
                        .send(Event {
                            name: "test-event".to_string(),
                            payload: Some(test_payload_inner.clone()),
                        })
                        .await
                        .context("failed to send event")?;

                    if &id != "1" {
                        return Err(anyhow!("Got id {} instead of 1", id));
                    }
                    Ok(())
                }
            },
            &|_| async { "1" },
        )
        .await;

        test_fn_outcome.context("Failed to run test function")?;

        assert_eq!(requests.len(), 1);
        let request = requests.into_iter().next().unwrap();

        assert_eq!(request.path, "/sendEvent");
        let mut params = request
            .query_params
            .clone()
            .context("expected to find query params")?;

        let payload_value: Value = serde_json::from_str(
            &params
                .remove("payload")
                .context("expected to find payload key")?,
        )
        .context("failed to decode payload into json Value")?;
        assert_eq!(payload_value, test_payload);

        assert_eq!(
            params,
            hashmap! {
                "name".to_string() => "test-event".to_string(),
                "sender".to_string() => "unit-test".to_string(),
                "assetID".to_string() => "123".to_string(),
            }
        );

        assert_eq!(request.body, "");
        assert_eq!(request.method, http::method::Method::GET);

        Ok(())
    }
}
