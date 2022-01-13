/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::net::IpAddr;

use anyhow::{bail, Context, Result};
use reqwest::Url;
use serde_json;
use slog::{info, Logger};
use structopt::StructOpt;

use crate::net_utils::get_mac;

#[derive(StructOpt, Debug, Clone)]
pub struct Opts {
    // struct fields have public visibility so that the struct can used by
    // code outside this crate that want to reuse the send_event function.
    pub event_name: String,
    pub sender: String,

    #[structopt(long, conflicts_with_all = &["mac_address", "ip_address"])]
    pub asset_id: Option<i64>,

    #[structopt(long, conflicts_with_all = &["asset_id", "ip_address"])]
    pub mac_address: Option<String>,

    #[structopt(long, conflicts_with_all = &["asset_id", "mac_address"])]
    pub ip_address: Option<String>,

    #[structopt(long, parse(try_from_str = serde_json::from_str))]
    pub payload: Option<serde_json::Value>,
}

pub fn get_uri(log: Logger, config: crate::Config, opts: Opts) -> Result<Url> {
    let mut url = config.event_backend.event_backend_base_uri().clone();
    url.query_pairs_mut()
        .append_pair("name", &opts.event_name)
        .append_pair("sender", &opts.sender);

    if let Some(p) = opts.payload {
        url.query_pairs_mut()
            .append_pair("payload", &serde_json::to_string(&p).unwrap());
    }
    match (opts.asset_id, opts.mac_address, opts.ip_address) {
        (Some(asset_id), None, None) => {
            url.query_pairs_mut()
                .append_pair("assetID", &asset_id.to_string());
        }
        (None, Some(mac_address), None) => {
            url.query_pairs_mut().append_pair("mac", &mac_address);
        }
        (None, None, Some(ip_address)) => {
            // validate that ip_address is a valid IPv4 or IPv6 address.
            ip_address.parse::<IpAddr>()?;
            url.query_pairs_mut().append_pair("ip", &ip_address);
        }
        (None, None, None) => {
            let mac_address = get_mac()?;
            info!(
                log,
                "did not provide any mac_address, asset_id or ip_address, automatically inferred mac address {}",
                mac_address,
            );
            url.query_pairs_mut().append_pair("mac", &mac_address);
        }
        _ => {
            bail!("only one of mac_address, ip_address or asset_id can be provided")
        }
    }
    Ok(url)
}

/// Send an event to the https endpoint configured in the metalctl.toml config file.
/// Endpoing support GET requests wit hthe following formats:
///
///  * /sendEvent?name=<name>&sender=<text>&mac=<mac>&payload=<json_payload>
///  * /sendEvent?name=<name>&sender=<text>&ip=<ipv6 or ipv4>&payload=<payload>
///  * /sendEvent?name=<name>&sender=<text>&assetID=<assetID>&payload=<payload>
///
/// This subcommand can be used in scripts, systemd unit files and so on.
pub(crate) async fn send_event(log: Logger, config: crate::Config, opts: Opts) -> Result<()> {
    let uri = get_uri(log.clone(), config, opts)?;
    let client = crate::http::client()?;
    let event_id = client
        .get(uri)
        .send()
        .await
        .context("while sending event GET")?
        .text()
        .await
        .context("while parsing event response as text")?;
    info!(log, "Event unique identifier: {}", event_id);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{get_uri, Opts};
    use crate::config::Config;
    use anyhow::Result;
    use slog::o;
    use url::Url;

    #[test]
    fn test_get_uri() -> Result<()> {
        let log = slog::Logger::root(slog_glog_fmt::default_drain(), o!());
        let config = Config::default();
        let base_opts = Opts {
            asset_id: None,
            event_name: "EVENT_NAME".to_string(),
            sender: "metalctl-test".to_string(),
            ip_address: None,
            mac_address: None,
            payload: Some(serde_json::to_value("foopayload").unwrap()),
        };

        // asset ID test
        let mut asset_id_opts = base_opts.clone();
        asset_id_opts.asset_id = Some(1234);
        assert_eq!(
            "https://metalos/sendEvent?name=EVENT_NAME&sender=metalctl-test&payload=%22foopayload%22&assetID=1234",
            get_uri(log.clone(), config.clone(), asset_id_opts)?.to_string(),
        );

        // mac address test
        let mut mac_opts = base_opts.clone();
        mac_opts.mac_address = Some("11:22:33:44:55:66".to_string());
        assert_eq!(
            "https://metalos/sendEvent?name=EVENT_NAME&sender=metalctl-test&payload=%22foopayload%22&mac=11%3A22%3A33%3A44%3A55%3A66",
            get_uri(log.clone(), config.clone(), mac_opts)?.to_string(),
        );

        // ip address test
        let mut ip_opts = base_opts;
        ip_opts.ip_address = Some("1.2.3.4".to_string());
        assert_eq!(
            "https://metalos/sendEvent?name=EVENT_NAME&sender=metalctl-test&payload=%22foopayload%22&ip=1.2.3.4",
            get_uri(log.clone(), config, ip_opts)?.to_string(),
        );

        Ok(())
    }

    #[test]
    #[should_panic(expected = "only one of mac_address, ip_address or asset_id can be provided")]
    fn test_get_uri_error_mutual_exclusion() {
        let log = slog::Logger::root(slog_glog_fmt::default_drain(), o!());
        let config = Config::default();
        let opts = Opts {
            asset_id: Some(1234),
            event_name: "EVENT_NAME".to_string(),
            sender: "metalctl-test".to_string(),
            ip_address: Some("1.2.3.4".to_string()),
            mac_address: Some("11:22:33:44:55:66".to_string()),
            payload: Some(serde_json::to_value("foo_payload").unwrap()),
        };
        get_uri(log, config, opts).unwrap();
    }

    #[test]
    fn test_get_uri_json_payload() -> Result<()> {
        let log = slog::Logger::root(slog_glog_fmt::default_drain(), o!());
        let config = Config::default();


        let json_payload = serde_json::json!(r#"{"chef.run_exception": "LGTM"}"#);
        let opts = Opts {
            asset_id: Some(1234),
            event_name: "EVENT_NAME".to_string(),
            sender: "metalctl-test".to_string(),
            ip_address: None,
            mac_address: None,
            payload: Some(serde_json::to_value(&json_payload).unwrap()),
        };

        let mut expected =
            Url::parse("https://metalos/sendEvent?name=EVENT_NAME&sender=metalctl-test")?;
        expected
            .query_pairs_mut()
            .append_pair("payload", &serde_json::to_string(&json_payload).unwrap())
            .append_pair("assetID", "1234");

        assert_eq!(
            expected.as_str(),
            get_uri(log.clone(), config, opts)?.to_string()
        );
        Ok(())
    }
}
