use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use async_nats::jetstream::consumer::{pull, AckPolicy};
use async_nats::jetstream::stream::{RetentionPolicy, StorageType};
use async_nats::{Client, ConnectOptions, HeaderMap};
use bytes::Bytes;
use futures::StreamExt;
use serde::de::DeserializeOwned;
use serde::Serialize;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};

use crate::bus::subjects::{streams, StreamCfg};
use crate::error::{AppError, AppResult};

const REPLY_HEADER: &str = "X-Reply-To";

#[derive(Clone)]
pub struct NatsService {
    nc: Client,
    js: async_nats::jetstream::Context,
    shutdown: CancellationToken,
}

#[derive(Debug)]
struct RpcReply<T> {
    ok: bool,
    data: Option<T>,
    error: Option<String>,
}

impl NatsService {
    pub async fn connect(url: &str, shutdown: CancellationToken) -> AppResult<Arc<Self>> {
        let parsed = url::Url::parse(url)
            .map_err(|e| AppError::internal(format!("invalid NATS_URL: {e}")))?;
        let user = parsed.username();
        let pass = parsed.password().unwrap_or("");
        let clean = format!(
            "{}://{}{}",
            parsed.scheme(),
            parsed.host_str().unwrap_or("localhost"),
            parsed.port().map(|p| format!(":{p}")).unwrap_or_default()
        );

        let mut opts = ConnectOptions::new()
            .name("backend")
            .max_reconnects(None)
            .retry_on_initial_connect();
        if !user.is_empty() {
            let user_dec = urlencoding::decode(user)
                .map_err(|e| AppError::internal(format!("nats user decode: {e}")))?
                .into_owned();
            let pass_dec = urlencoding::decode(pass)
                .map_err(|e| AppError::internal(format!("nats pass decode: {e}")))?
                .into_owned();
            opts = opts.user_and_password(user_dec, pass_dec);
        }

        let nc: Client = opts
            .connect(clean.as_str())
            .await
            .map_err(|e| AppError::internal(format!("NATS connect failed: {e}")))?;
        info!(url = %clean, "NATS connected");

        let js = async_nats::jetstream::new(nc.clone());

        let svc = Arc::new(Self { nc, js, shutdown });
        svc.ensure_stream(&streams::AI_RPC, true, Some(120)).await?;
        svc.ensure_stream(&streams::INDEX_AUDIO, true, None).await?;
        svc.ensure_stream(&streams::EMBED_LYRICS, true, None)
            .await?;
        svc.ensure_stream(&streams::TRAIN_COLLAB, true, Some(6 * 60 * 60))
            .await?;
        svc.ensure_stream(&streams::TRAIN_LTR, true, Some(24 * 60 * 60))
            .await?;
        svc.ensure_stream(&streams::DONE, false, None).await?;
        svc.ensure_stream(&streams::STORAGE_EVENTS, false, None)
            .await?;
        Ok(svc)
    }

    async fn ensure_stream(
        &self,
        cfg: &StreamCfg,
        work_queue: bool,
        max_age_seconds: Option<u64>,
    ) -> AppResult<()> {
        let default_age = if work_queue { 24 * 60 * 60 } else { 60 * 60 };
        let age = Duration::from_secs(max_age_seconds.unwrap_or(default_age));
        let retention = if work_queue {
            RetentionPolicy::WorkQueue
        } else {
            RetentionPolicy::Limits
        };
        let stream_cfg = async_nats::jetstream::stream::Config {
            name: cfg.name.to_string(),
            subjects: cfg.subjects.iter().map(|s| s.to_string()).collect(),
            retention,
            storage: StorageType::File,
            max_age: age,
            ..Default::default()
        };
        match self.js.create_stream(stream_cfg.clone()).await {
            Ok(_) => {
                info!(stream = cfg.name, subjects = ?cfg.subjects, "JetStream created");
                Ok(())
            }
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("already in use") || msg.contains("already exists") {
                    self.js.update_stream(&stream_cfg).await.map_err(|e| {
                        AppError::internal(format!("JetStream update {}: {e}", cfg.name))
                    })?;
                    Ok(())
                } else {
                    Err(AppError::internal(format!(
                        "JetStream create {}: {e}",
                        cfg.name
                    )))
                }
            }
        }
    }

    pub async fn request<P, T>(
        &self,
        subject: &str,
        payload: &P,
        timeout: Duration,
        throw_on_error: bool,
    ) -> AppResult<Option<T>>
    where
        P: Serialize,
        T: DeserializeOwned,
    {
        let inbox = self.nc.new_inbox();
        let mut sub = self
            .nc
            .subscribe(inbox.clone())
            .await
            .map_err(|e| AppError::internal(format!("nats subscribe inbox failed: {e}")))?;
        sub.unsubscribe_after(1).await.ok();

        let mut headers = HeaderMap::new();
        headers.insert(REPLY_HEADER, inbox.as_str());

        let body = Bytes::from(
            serde_json::to_vec(payload)
                .map_err(|e| AppError::internal(format!("rpc payload encode: {e}")))?,
        );

        let publish = self
            .js
            .publish_with_headers(subject.to_string(), headers, body)
            .await
            .map_err(|e| AppError::internal(format!("jetstream publish {subject}: {e}")))?;
        publish
            .await
            .map_err(|e| AppError::internal(format!("jetstream ack {subject}: {e}")))?;

        let msg = match tokio::time::timeout(timeout, sub.next()).await {
            Ok(Some(m)) => m,
            Ok(None) | Err(_) => {
                debug!(subject, "request timeout / no reply");
                if throw_on_error {
                    return Err(AppError::internal(format!(
                        "{subject} timeout after {}ms",
                        timeout.as_millis()
                    )));
                }
                return Ok(None);
            }
        };

        if msg.payload.is_empty() {
            return Ok(None);
        }

        let parsed: serde_json::Value = serde_json::from_slice(&msg.payload)
            .map_err(|e| AppError::internal(format!("rpc reply decode {subject}: {e}")))?;
        let reply = parse_rpc_reply::<T>(parsed)
            .map_err(|e| AppError::internal(format!("rpc reply structure {subject}: {e}")))?;

        if !reply.ok {
            let msg = reply.error.unwrap_or_else(|| format!("{subject} failed"));
            debug!(subject, error = %msg, "rpc returned error");
            if throw_on_error {
                return Err(AppError::internal(msg));
            }
            return Ok(None);
        }

        Ok(reply.data)
    }

    pub async fn publish<P>(&self, subject: &str, payload: &P) -> AppResult<()>
    where
        P: Serialize,
    {
        let body = Bytes::from(
            serde_json::to_vec(payload)
                .map_err(|e| AppError::internal(format!("publish encode: {e}")))?,
        );
        let ack = self
            .js
            .publish(subject.to_string(), body)
            .await
            .map_err(|e| AppError::internal(format!("jetstream publish {subject}: {e}")))?;
        ack.await
            .map_err(|e| AppError::internal(format!("jetstream ack {subject}: {e}")))?;
        Ok(())
    }

    pub fn consume<F, Fut>(
        &self,
        stream: &'static str,
        durable: &'static str,
        filter_subject: Option<&'static str>,
        handler: F,
    ) where
        F: Fn(serde_json::Value) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = AppResult<()>> + Send + 'static,
    {
        let js = self.js.clone();
        let token = self.shutdown.clone();
        let handler = Arc::new(handler);

        tokio::spawn(async move {
            loop {
                if token.is_cancelled() {
                    return;
                }

                let stream_handle = match js.get_stream(stream).await {
                    Ok(s) => s,
                    Err(e) => {
                        warn!(stream, error = %e, "consume: get_stream failed, retry in 2s");
                        tokio::select! {
                            _ = token.cancelled() => return,
                            _ = tokio::time::sleep(Duration::from_secs(2)) => continue,
                        }
                    }
                };

                let mut config = pull::Config {
                    durable_name: Some(durable.to_string()),
                    ack_policy: AckPolicy::Explicit,
                    ack_wait: Duration::from_secs(30),
                    max_deliver: 5,
                    ..Default::default()
                };
                if let Some(filter) = filter_subject {
                    config.filter_subject = filter.to_string();
                }

                if let Err(e) = stream_handle
                    .get_or_create_consumer(durable, config.clone())
                    .await
                {
                    warn!(stream, durable, error = %e, "consume: get_or_create failed, retry in 2s");
                    tokio::select! {
                        _ = token.cancelled() => return,
                        _ = tokio::time::sleep(Duration::from_secs(2)) => continue,
                    }
                }

                let consumer = match stream_handle.get_consumer::<pull::Config>(durable).await {
                    Ok(c) => c,
                    Err(e) => {
                        warn!(stream, durable, error = %e, "consume: get_consumer failed, retry in 2s");
                        tokio::select! {
                            _ = token.cancelled() => return,
                            _ = tokio::time::sleep(Duration::from_secs(2)) => continue,
                        }
                    }
                };

                let mut messages = match consumer.messages().await {
                    Ok(m) => m,
                    Err(e) => {
                        warn!(stream, durable, error = %e, "consume: messages() failed, retry in 2s");
                        tokio::select! {
                            _ = token.cancelled() => return,
                            _ = tokio::time::sleep(Duration::from_secs(2)) => continue,
                        }
                    }
                };

                loop {
                    let next = tokio::select! {
                        _ = token.cancelled() => return,
                        m = messages.next() => m,
                    };
                    let msg = match next {
                        Some(Ok(m)) => m,
                        Some(Err(e)) => {
                            warn!(stream, durable, error = %e, "consume: stream error, reset");
                            break;
                        }
                        None => {
                            debug!(stream, durable, "consume: stream ended, reset");
                            break;
                        }
                    };

                    match serde_json::from_slice::<serde_json::Value>(&msg.payload) {
                        Ok(data) => match handler(data).await {
                            Ok(()) => {
                                if let Err(e) = msg.ack().await {
                                    warn!(stream, durable, error = %e, "consume: ack failed");
                                }
                            }
                            Err(e) => {
                                error!(stream, durable, error = %e, "consume: handler failed");
                                let _ = msg
                                    .ack_with(async_nats::jetstream::AckKind::Nak(Some(
                                        Duration::from_secs(5),
                                    )))
                                    .await;
                            }
                        },
                        Err(e) => {
                            error!(stream, durable, error = %e, "consume: payload decode failed");
                            let _ = msg.ack().await;
                        }
                    }
                }

                if !token.is_cancelled() {
                    tokio::time::sleep(Duration::from_secs(2)).await;
                }
            }
        });
    }
}

fn parse_rpc_reply<T: DeserializeOwned>(
    v: serde_json::Value,
) -> Result<RpcReply<T>, serde_json::Error> {
    let ok = v.get("ok").and_then(|x| x.as_bool()).unwrap_or(false);
    let error = v.get("error").and_then(|x| x.as_str()).map(String::from);
    let data = match v.get("data") {
        Some(d) if !d.is_null() => Some(serde_json::from_value::<T>(d.clone())?),
        _ => None,
    };
    Ok(RpcReply { ok, data, error })
}
