/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use reqwest::Client;
use reqwest::Url;

/// This test verifies:
/// 1. vmtest-host is specified in /etc/hosts for the VM to point to the right
///    IP inside the container.
/// 2. sidecar is running and working
#[tokio::test]
async fn test_sidecar() {
    let client = Client::builder()
        .build()
        .expect("Failed to get http client");
    let url = Url::parse("http://vmtest-host:8000/hello").expect("URL should be valid");
    let response = client
        .get(url)
        .send()
        .await
        .expect("Failed to send request");
    assert!(response.status().is_success());
    let body = response.text().await.expect("Failed to read response");
    assert_eq!(body, "world");
}
