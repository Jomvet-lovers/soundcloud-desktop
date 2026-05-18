use bytes::Bytes;
use futures::future::BoxFuture;
use reqwest::Client;
use std::collections::HashMap;

use super::proxy::{fetch_get_bytes, fetch_post_bytes};

pub(crate) struct ProxyFetcher {
    pub client: Client,
    pub proxy_url: String,
}

fn map(headers: Vec<(String, String)>) -> HashMap<String, String> {
    headers.into_iter().collect()
}

impl decrypt::Fetcher for ProxyFetcher {
    fn get(
        &self,
        url: String,
        headers: Vec<(String, String)>,
    ) -> BoxFuture<'static, Result<Bytes, decrypt::Error>> {
        let client = self.client.clone();
        let proxy = self.proxy_url.clone();
        Box::pin(async move {
            fetch_get_bytes(&client, &proxy, &url, map(headers), false)
                .await
                .map(|(b, _)| b)
                .map_err(|e| decrypt::Error::Fetch(e.to_string()))
        })
    }

    fn post(
        &self,
        url: String,
        headers: Vec<(String, String)>,
        body: Vec<u8>,
    ) -> BoxFuture<'static, Result<Bytes, decrypt::Error>> {
        let client = self.client.clone();
        let proxy = self.proxy_url.clone();
        Box::pin(async move {
            fetch_post_bytes(&client, &proxy, &url, map(headers), body)
                .await
                .map(|(b, _)| b)
                .map_err(|e| decrypt::Error::Fetch(e.to_string()))
        })
    }
}
