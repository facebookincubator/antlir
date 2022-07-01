#[cfg(test)]
#[macro_use]
extern crate metalos_macros;

use std::collections::HashMap;
use std::convert::Infallible;
use std::future::Future;
use std::net::IpAddr;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::Mutex;

use hyper::service::make_service_fn;
use hyper::service::service_fn;
use hyper::Body;
use hyper::Response;
use hyper::Server;

pub async fn make_test_server<TF, TFF, TFR, HF, HFR, HFF>(
    test_fn: TF,
    handler_fn: &'static HF,
) -> (Vec<Arc<UnpackedRequest>>, TFR)
where
    TF: Fn(SocketAddr) -> TFF,
    TFF: Future<Output = TFR>,
    HF: Fn(&UnpackedRequest) -> HFF + Send + Sync,
    HFF: Future<Output = HFR> + Send + Sync,
    hyper::Body: From<HFR>,
{
    let shared = Arc::new(Mutex::new(Vec::new()));
    let addr = SocketAddr::from((
        IpAddr::V6("::1".parse().expect("failed to make localhost address")),
        0,
    ));

    let inner = shared.clone();

    let make_svc = make_service_fn(move |_| {
        let inner = inner.clone();
        async move {
            Ok::<_, Infallible>(service_fn(move |mut request| {
                let inner = inner.clone();
                async move {
                    let request = UnpackedRequest {
                        query_params: request.uri().query().map(|query| {
                            url::form_urlencoded::parse(query.as_ref())
                                .into_owned()
                                .collect()
                        }),
                        path: request.uri().path().to_string(),
                        method: request.method().clone(),

                        body: std::str::from_utf8(
                            &hyper::body::to_bytes(request.body_mut())
                                .await
                                .expect("Failed to ready body"),
                        )
                        .expect("Invalid utf-8 body found")
                        .to_string(),
                    };
                    let response = handler_fn(&request).await;

                    inner
                        .clone()
                        .lock()
                        .expect("Failed to acquire lock")
                        .push(Arc::new(request));
                    Ok::<_, Infallible>(Response::new(Body::from(response)))
                }
            }))
        }
    });
    let server = Server::bind(&addr).serve(make_svc);
    let socket = server.local_addr();

    let handle = tokio::spawn(server);
    let outcome: TFR = test_fn(socket).await;

    // Best effort abort/shutdown
    handle.abort();

    let out = shared.lock().expect("failed to acquire lock").clone();
    (out, outcome)
}

#[derive(Debug, Clone, PartialEq)]
pub struct UnpackedRequest {
    pub path: String,
    pub query_params: Option<HashMap<String, String>>,
    pub body: String,
    pub method: http::method::Method,
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::anyhow;
    use anyhow::Context;
    use anyhow::Result;
    use maplit::hashmap;
    use reqwest::Client;
    use reqwest::Url;

    #[containertest]
    async fn test_http_test() -> Result<()> {
        let (requests, test_fn_outcome) = make_test_server(
            |addr| async move {
                let client = Client::builder().build().context("building http client")?;

                let url = Url::parse_with_params(
                    &format!("http://{}/some/path", addr),
                    vec![("lang", "rust"), ("browser", "unit-test")],
                )
                .context(format!("failed to make url from addr: {}", addr))?;

                let response = client
                    .get(url.clone())
                    .body("test_body")
                    .send()
                    .await
                    .context(format!("while sending GET to: {:?}", url))?;

                if !response.status().is_success() {
                    return Err(anyhow!("expected 200ish got {}", response.status()));
                }
                let body = response
                    .text()
                    .await
                    .context("failed to get body as text")?;
                if &body != "test_body response" {
                    return Err(anyhow!(
                        "Expected 'test_body response' as response got: {:?}",
                        body
                    ));
                }

                Ok(())
            },
            &|request| {
                let out = format!("{} response", request.body);
                async move { out }
            },
        )
        .await;

        test_fn_outcome.context("Failed to run test function")?;

        assert_eq!(requests.len(), 1);
        let request = requests.into_iter().next().unwrap();

        assert_eq!(
            *request,
            UnpackedRequest {
                path: "/some/path".to_string(),
                query_params: Some(hashmap! {
                    "lang".to_string() => "rust".to_string(),
                    "browser".to_string() => "unit-test".to_string(),
                }),
                body: "test_body".to_string(),
                method: http::method::Method::GET,
            }
        );

        Ok(())
    }
}
