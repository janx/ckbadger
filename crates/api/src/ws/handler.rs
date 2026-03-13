use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    response::Response,
};
use futures::{stream::SplitSink, SinkExt, StreamExt};
use std::ops::ControlFlow;
use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::{info, warn};

use super::manager::{BroadcastMessage, WsMessage};
use crate::AppState;

pub async fn ws_handler(ws: WebSocketUpgrade, State(state): State<Arc<AppState>>) -> Response {
    ws.on_upgrade(|socket| handle_socket(socket, state))
}

/// Handle a broadcast channel recv result: serialize and send to the WebSocket,
/// returning `ControlFlow::Break` if the WebSocket is closed, or
/// `ControlFlow::Continue` otherwise.
async fn forward_broadcast(
    result: Result<BroadcastMessage, broadcast::error::RecvError>,
    sender: &mut SplitSink<WebSocket, Message>,
    channel_name: &str,
) -> ControlFlow<()> {
    match result {
        Ok(msg) => {
            if let Ok(json) = serde_json::to_string(&msg) {
                if sender.send(Message::Text(json.into())).await.is_err() {
                    return ControlFlow::Break(());
                }
            }
        }
        Err(broadcast::error::RecvError::Lagged(n)) => {
            warn!("ws channel '{}' lagged by {} messages", channel_name, n);
        }
        Err(broadcast::error::RecvError::Closed) => {
            return ControlFlow::Break(());
        }
    }
    ControlFlow::Continue(())
}

/// Await an optional broadcast receiver. If `None`, pend forever.
async fn recv_optional(
    rx: &mut Option<broadcast::Receiver<BroadcastMessage>>,
) -> Result<BroadcastMessage, broadcast::error::RecvError> {
    match rx {
        Some(rx) => rx.recv().await,
        None => std::future::pending().await,
    }
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
            result = recv_optional(&mut block_rx) => {
                if forward_broadcast(result, &mut sender, "new_block").await.is_break() {
                    break;
                }
            }
            result = recv_optional(&mut tx_rx) => {
                if forward_broadcast(result, &mut sender, "new_transaction").await.is_break() {
                    break;
                }
            }
            result = recv_optional(&mut reorg_rx) => {
                if forward_broadcast(result, &mut sender, "reorg").await.is_break() {
                    break;
                }
            }
            result = recv_optional(&mut activity_rx) => {
                if forward_broadcast(result, &mut sender, "latest_activity").await.is_break() {
                    break;
                }
            }
        }
    }

    info!("WebSocket client disconnected");
}
