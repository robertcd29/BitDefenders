use futures_util::SinkExt;
use tokio_tungstenite::tungstenite::Message;
use crate::protocol::WebSocketMessage;

pub async fn send<S>(write: &mut S, msg: WebSocketMessage)
where
    S: SinkExt<Message, Error = tokio_tungstenite::tungstenite::Error> + Unpin,
{
    match serde_json::to_string(&msg) {
        Ok(text) => {
            if let Err(e) = write.send(Message::Text(text.into())).await {
                eprintln!("[net] send error: {e}");
            }
        }
        Err(e) => eprintln!("[net] serialize error: {e}"),
    }
}