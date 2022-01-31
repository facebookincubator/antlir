/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use anyhow::{Context, Result};
use serde_json;
use slog::{info, Logger};
use structopt::StructOpt;

use crate::net_utils::get_mac;
use send_events::{Event, EventSender, EventSink, HttpSink, Source, SourceArgs};

#[derive(StructOpt, Debug, Clone)]
pub struct Opts {
    // struct fields have public visibility so that the struct can used by
    // code outside this crate that want to reuse the send_event function.
    pub event_name: String,
    pub sender: String,

    #[structopt(long, parse(try_from_str = serde_json::from_str))]
    pub payload: Option<serde_json::Value>,

    #[structopt(flatten)]
    pub source_args: SourceArgs,
}

pub(crate) fn default_sink(config: &crate::Config) -> impl EventSink {
    HttpSink::new(config.event_backend.event_backend_base_uri().clone())
}

/// Send an event to the https endpoint configured in the metalctl.toml config file.
/// This subcommand can be used in scripts, systemd unit files and so on.
pub(super) async fn send_event(log: Logger, config: crate::Config, opts: Opts) -> Result<()> {
    let sink = default_sink(&config);
    let event_sender = EventSender::new(
        match opts.source_args.into() {
            Some(source) => source,
            None => Source::Mac(get_mac().context("Failed to find mac address")?),
        },
        opts.sender,
        sink,
    );

    let event_id = event_sender
        .send(Event {
            name: opts.event_name,
            payload: opts.payload,
        })
        .await
        .context("failed to send event")?;

    info!(log, "Event unique identifier: {:?}", event_id);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, EventBackend};
    use anyhow::Result;
    use http_test::make_test_server;
    use maplit::hashmap;
    use serde_json::Value;
    use slog::o;

    #[containertest]
    async fn test_send_event() -> Result<()> {
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
                    let log = slog::Logger::root(slog_glog_fmt::default_drain(), o!());
                    send_event(
                        log,
                        Config {
                            download: Default::default(),
                            event_backend: EventBackend {
                                event_backend_base_uri: format!("http://{}/sendEvent", addr)
                                    .parse()
                                    .context("failed to build URL")?,
                            },
                        },
                        Opts::from_iter_safe(&[
                            "send-event",
                            "test-event",
                            "unit-test",
                            "--asset-id=123",
                            "--payload",
                            &serde_json::to_string(&test_payload_inner)
                                .context("failed to convert payload to json")?,
                        ])
                        .context("failed to parse args")?,
                    )
                    .await
                    .context("failed to run send event cmd")?;

                    anyhow::Ok(())
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
