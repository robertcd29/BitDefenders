mod bot;
mod game;
mod net;
mod protocol;

use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::{connect_async, tungstenite::Message};

use bot::{Bot, Order};
use net::send;
use protocol::{
    cmd, LoginArgs, PracticeArgs,
    StartMatchArgs, StartTurnArgs, WebSocketMessage, PROTOCOL_VERSION,
};

const SERVER_URL: &str = "wss://bitdefenders.cvjd.me/ws";
const BOT_NAME: &str = "robertcd29";

#[tokio::main]
async fn main() {
    let (ws, _) = match connect_async(SERVER_URL).await {
        Ok(v) => v,
        Err(e) => { eprintln!("Failed to connect: {e}"); return; }
    };

    let (mut write, mut read) = ws.split();
    let mut bot: Option<Bot> = None;

    while let Some(msg) = read.next().await {
        let msg = match msg {
            Ok(m) => m,
            Err(e) => { eprintln!("WS error: {e}"); break; }
        };

        match msg {
            Message::Ping(p) => { let _ = write.send(Message::Pong(p)).await; }
            Message::Close(_) => { println!("Connection closed."); break; }
            Message::Text(text) => {
                let message: WebSocketMessage = match serde_json::from_str(&text) {
                    Ok(m) => m,
                    Err(e) => { eprintln!("Parse error: {e}"); continue; }
                };

                match message.command.as_str() {
                    cmd::HELLO => {
                        println!("[←] HELLO");
                        send(&mut write, WebSocketMessage::new(cmd::LOGIN, LoginArgs {
                            name: BOT_NAME.to_string(),
                            version: PROTOCOL_VERSION,
                        })).await;
                    }
                    cmd::READY => {
                        println!("[←] READY");
                        send(&mut write, WebSocketMessage::new(
                            cmd::PRACTICE, PracticeArgs { seed: None }
                        )).await;
                    }
                    cmd::START_MATCH => {
                        let args: StartMatchArgs = match serde_json::from_value(message.args) {
                            Ok(a) => a,
                            Err(e) => { eprintln!("START_MATCH parse error: {e}"); continue; }
                        };
                        println!("[←] START_MATCH match={} player={} map={}×{}",
                            args.match_id, args.your_player_id,
                            args.config.width, args.config.height);
                        bot = Some(Bot::new(args));
                    }
                    cmd::START_TURN => {
                        let args: StartTurnArgs = match serde_json::from_value(message.args) {
                            Ok(a) => a,
                            Err(e) => { eprintln!("START_TURN parse error: {e}"); continue; }
                        };
                        println!("[←] START_TURN {}", args.turn);

                        if let Some(b) = bot.as_mut() {
                            let orders = b.take_turn(args);
                            for order in orders {
                                match order {
                                    Order::Move(a) => send(&mut write, WebSocketMessage::new(cmd::MOVE, a)).await,
                                    Order::Shoot(a) => send(&mut write, WebSocketMessage::new(cmd::SHOOT, a)).await,
                                }
                            }
                        }
                    }
                    cmd::END_MATCH => {
                        if let Ok(args) = serde_json::from_value::<protocol::EndMatchArgs>(message.args) {
                            println!("[←] END_MATCH reason={} winner={:?}", args.reason, args.winner);
                        }
                        bot = None;
                    }
                    cmd::ERROR => {
                        if let Ok(args) = serde_json::from_value::<protocol::ErrorArgs>(message.args) {
                            eprintln!("[←] ERROR [{}] {} fatal={}", args.code, args.message, args.fatal);
                            if args.fatal { std::process::exit(1); }
                        }
                    }
                    other => println!("[←] Unknown: {other}"),
                }
            }
            _ => {}
        }
    }
}