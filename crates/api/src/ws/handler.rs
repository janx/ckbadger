use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    response::Response,
};
use futures::{SinkExt, StreamExt};
use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::info;

use super::manager::{BroadcastMessage, WsMessage};
use crate::AppState;

pub async fn ws_handler(ws: WebSocketUpgrade, State(state): State<Arc<AppState>>) -> Response {
    ws.on_upgrade(|socket| handle_socket(socket, state))
}

async fn handle_socket(socket: WebSocket, state: Arc<AppState>) {
    let (mut sender, mut receiver) = socket.split();

    let ws_manager = &state.ws_manager;
    let mut block_rx: Option<broadcast::Receiver<BroadcastMessage>> = None;
    let mut tx_rx: Option<broadcast::Receiver<BroadcastMessage>> = None;
    let mut reorg_rx: Option<broadcast::Receiver<BroadcastMessage>> = None;
    let mut activity_rx: Option<broadcast::Receiver<BroadcastMessage>> = None;

    info!("WebSocket client connected");

    loop {
        tokio::select! {
            Some(msg) = receiver.next() => {
                match msg {
                    Ok(Message::Text(text)) => {
                        if let Ok(ws_msg) = serde_json::from_str::<WsMessage>(&text) {
                            match ws_msg.action.as_str() {
                                "subscribe" => {
                                    if let Some(channel) = &ws_msg.channel {
                                        match channel.as_str() {
                                            "new_block" => {
                                                block_rx = Some(ws_manager.subscribe_blocks());
                                                let _ = sender.send(Message::Text(
                                                    r#"{"status":"subscribed","channel":"new_block"}"#.into()
                                                )).await;
                                            }
                                            "new_transaction" => {
                                                tx_rx = Some(ws_manager.subscribe_transactions());
                                                let _ = sender.send(Message::Text(
                                                    r#"{"status":"subscribed","channel":"new_transaction"}"#.into()
                                                )).await;
                                            }
                                            "reorg" => {
                                                reorg_rx = Some(ws_manager.subscribe_reorgs());
                                                let _ = sender.send(Message::Text(
                                                    r#"{"status":"subscribed","channel":"reorg"}"#.into()
                                                )).await;
                                            }
                                            "latest_activity" => {
                                                activity_rx = Some(ws_manager.subscribe_activities());
                                                let _ = sender.send(Message::Text(
                                                    r#"{"status":"subscribed","channel":"latest_activity"}"#.into()
                                                )).await;
                                            }
                                            _ => {}
                                        }
                                    }
                                }
                                "unsubscribe" => {
                                    if let Some(channel) = &ws_msg.channel {
                                        match channel.as_str() {
                                            "new_block" => block_rx = None,
                                            "new_transaction" => tx_rx = None,
                                            "reorg" => reorg_rx = None,
                                            "latest_activity" => activity_rx = None,
                                            _ => {}
                                        }
                                    }
                                }
                                "ping" => {
                                    let _ = sender.send(Message::Text(r#"{"type":"pong"}"#.into())).await;
                                }
                                _ => {}
                            }
                        }
                    }
                    Ok(Message::Ping(data)) => {
                        let _ = sender.send(Message::Pong(data)).await;
                    }
                    Ok(Message::Close(_)) | Err(_) => break,
                    _ => {}
                }
            }
            msg = async {
                if let Some(ref mut rx) = block_rx {
                    rx.recv().await.ok()
                } else {
                    std::future::pending::<Option<BroadcastMessage>>().await
                }
            } => {
                if let Some(broadcast_msg) = msg {
                    if let Ok(json) = serde_json::to_string(&broadcast_msg) {
                        if sender.send(Message::Text(json.into())).await.is_err() {
                            break;
                        }
                    }
                }
            }
            msg = async {
                if let Some(ref mut rx) = tx_rx {
                    rx.recv().await.ok()
                } else {
                    std::future::pending::<Option<BroadcastMessage>>().await
                }
            } => {
                if let Some(broadcast_msg) = msg {
                    if let Ok(json) = serde_json::to_string(&broadcast_msg) {
                        if sender.send(Message::Text(json.into())).await.is_err() {
                            break;
                        }
                    }
                }
            }
            msg = async {
                if let Some(ref mut rx) = reorg_rx {
                    rx.recv().await.ok()
                } else {
                    std::future::pending::<Option<BroadcastMessage>>().await
                }
            } => {
                if let Some(broadcast_msg) = msg {
                    if let Ok(json) = serde_json::to_string(&broadcast_msg) {
                        if sender.send(Message::Text(json.into())).await.is_err() {
                            break;
                        }
                    }
                }
            }
            msg = async {
                if let Some(ref mut rx) = activity_rx {
                    rx.recv().await.ok()
                } else {
                    std::future::pending::<Option<BroadcastMessage>>().await
                }
            } => {
                if let Some(broadcast_msg) = msg {
                    if let Ok(json) = serde_json::to_string(&broadcast_msg) {
                        if sender.send(Message::Text(json.into())).await.is_err() {
                            break;
                        }
                    }
                }
            }
        }
    }

    info!("WebSocket client disconnected");
}
