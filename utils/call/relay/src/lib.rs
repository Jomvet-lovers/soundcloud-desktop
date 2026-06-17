use std::collections::HashMap;

use bytes::Bytes;

pub mod tiers {
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Tier {
        Direct,
        Client,
        Proxy,
    }

    #[derive(Clone, Debug)]
    pub struct Policy {
        pub order: Vec<Tier>,
        pub timeout_ms: u32,
        pub fallback_on_status_5xx: bool,
    }

    impl Default for Policy {
        fn default() -> Self {
            Self {
                order: vec![Tier::Direct, Tier::Client, Tier::Proxy],
                timeout_ms: 15_000,
                fallback_on_status_5xx: true,
            }
        }
    }
}

pub use tiers::{Policy, Tier};

#[derive(Clone, Debug)]
pub struct Config {
    pub control_endpoint: Option<String>,
    pub upstream_proxy: Option<String>,
    pub instance_id: String,
    pub app_version: String,
    pub relay_secret: String,
    pub policy: Policy,
}

#[derive(Clone, Debug)]
pub struct Request {
    pub url: String,
    pub method: String,
    pub headers: HashMap<String, String>,
    pub body: Bytes,
}

#[derive(Clone, Debug)]
pub struct Response {
    pub status: u16,
    pub headers: HashMap<String, String>,
    pub body: Bytes,
    pub source_tier: Tier,
    pub client_id: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("call-relay disabled")]
    Disabled,
}

impl Error {
    pub fn is_disabled(&self) -> bool {
        matches!(self, Error::Disabled)
    }
}

pub struct Client;

impl Client {
    pub async fn connect(_cfg: Config) -> Result<Self, Error> {
        Err(Error::Disabled)
    }

    pub async fn fetch(&self, _req: &Request) -> Result<Response, Error> {
        Err(Error::Disabled)
    }

    pub async fn call_method(
        &self,
        _method_id: &str,
        _script: &str,
        _inputs: Bytes,
    ) -> Result<Bytes, Error> {
        Err(Error::Disabled)
    }

    pub async fn call_method_rotated(
        &self,
        _method_id: &str,
        _script: &str,
        _inputs: Bytes,
        _region_rotation: i32,
    ) -> Result<Bytes, Error> {
        Err(Error::Disabled)
    }
}
