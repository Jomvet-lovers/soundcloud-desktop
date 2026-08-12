//! WS /events: мультиплекс событийных каналов ядра. Протокол (контракт
//! Core/data/src/events.ts): клиент шлёт {"op":"sub"|"unsub","channel":...},
//! сервер — {"channel":..., "payload":...}. Каналы поднимаются лениво —
//! спектр не заводит FFT, пока на него никто не подписан.

use std::collections::HashMap;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use futures_util::{SinkExt, StreamExt};
use sc_core::ScRuntime;
use serde::Deserialize;
use serde_json::json;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::http::AppState;

#[derive(Deserialize)]
pub struct WsAuth {
    t: String,
}

#[derive(Deserialize)]
struct Op {
    op: String,
    channel: String,
}

pub async fn ws(
    State(state): State<AppState>,
    Query(auth): Query<WsAuth>,
    upgrade: WebSocketUpgrade,
) -> Response {
    // браузерный WebSocket не умеет заголовки — токен идёт query-параметром
    if auth.t != state.token {
        return (StatusCode::UNAUTHORIZED, "bad token").into_response();
    }
    upgrade.on_upgrade(move |socket| serve(socket, state.rt)).into_response()
}

async fn serve(socket: WebSocket, rt: ScRuntime) {
    let (mut sink, mut stream) = socket.split();
    let (out, mut out_rx) = mpsc::channel::<String>(256);

    let writer: JoinHandle<()> = tokio::spawn(async move {
        while let Some(msg) = out_rx.recv().await {
            if sink.send(Message::Text(msg.into())).await.is_err() {
                break;
            }
        }
    });

    let mut subs: HashMap<String, JoinHandle<()>> = HashMap::new();
    while let Some(Ok(msg)) = stream.next().await {
        let Message::Text(text) = msg else {
            if matches!(msg, Message::Close(_)) {
                break;
            }
            continue;
        };
        let Ok(Op { op, channel }) = serde_json::from_str::<Op>(&text) else {
            continue;
        };
        match op.as_str() {
            "sub" => {
                if !subs.contains_key(&channel) {
                    if let Some(task) = forwarder(&rt, &channel, out.clone()) {
                        subs.insert(channel, task);
                    }
                }
            }
            "unsub" => {
                if let Some(task) = subs.remove(&channel) {
                    task.abort();
                }
            }
            _ => {}
        }
    }

    for task in subs.into_values() {
        task.abort();
    }
    writer.abort();
}

fn emit(out: &mpsc::Sender<String>, channel: &str, payload: impl serde::Serialize) -> bool {
    match serde_json::to_value(payload) {
        Ok(value) => out
            .try_send(json!({ "channel": channel, "payload": value }).to_string())
            .is_ok(),
        Err(_) => true,
    }
}

fn forwarder(rt: &ScRuntime, channel: &str, out: mpsc::Sender<String>) -> Option<JoinHandle<()>> {
    let task = match channel {
        "position" => {
            let mut rx = rt.position_watch();
            tokio::spawn(async move {
                while rx.changed().await.is_ok() {
                    let v = *rx.borrow();
                    if !emit(&out, "position", v) {
                        break;
                    }
                }
            })
        }
        "playback" => {
            let mut rx = rt.events();
            tokio::spawn(async move {
                loop {
                    match rx.recv().await {
                        Ok(ev) => {
                            if !emit(&out, "playback", ev) {
                                break;
                            }
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                        Err(_) => break,
                    }
                }
            })
        }
        "download_progress" => {
            let mut rx = rt.download_progress();
            tokio::spawn(async move {
                loop {
                    match rx.recv().await {
                        Ok(p) => {
                            if !emit(&out, "download_progress", p) {
                                break;
                            }
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                        Err(_) => break,
                    }
                }
            })
        }
        "likes_progress" => {
            let mut rx = rt.likes_progress();
            tokio::spawn(async move {
                loop {
                    match rx.recv().await {
                        Ok(p) => {
                            if !emit(&out, "likes_progress", p) {
                                break;
                            }
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                        Err(_) => break,
                    }
                }
            })
        }
        "host_status" => {
            let mut rx = rt.host_status_watch();
            tokio::spawn(async move {
                // контракт: текущее значение приходит сразу при подписке
                let current = *rx.borrow();
                if !emit(&out, "host_status", current) {
                    return;
                }
                while rx.changed().await.is_ok() {
                    let v = *rx.borrow();
                    if !emit(&out, "host_status", v) {
                        break;
                    }
                }
            })
        }
        "spectrum" => {
            let mut rx = rt.spectrum();
            tokio::spawn(async move {
                loop {
                    match rx.recv().await {
                        Ok(bands) => {
                            if !emit(&out, "spectrum", bands) {
                                break;
                            }
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                        Err(_) => break,
                    }
                }
            })
        }
        _ => return None,
    };
    Some(task)
}
