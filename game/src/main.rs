use anyhow::{Context, Ok};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio_tungstenite::{connect_async, tungstenite::Message};
use std::collections::HashMap;
mod protocol;
use crate::protocol::{HelloArgs, LoginArgs, MoveArgs, StartMatchArgs, StartTurnArgs};


#[derive(Debug, Serialize, Deserialize)]
pub struct WebSocketMessage {
    command: Command,
    args: serde_json::Value,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Command {
    Hello,
    Login,
    Error,
    Ready,
    Practice,
    StartMatch,
    StartTurn,
    Move,
    EndMatch,
}



async fn send_command<
    S: SinkExt<Message, Error = tokio_tungstenite::tungstenite::Error> + Unpin,
>(
    write: &mut S,
    msg: WebSocketMessage,
) -> anyhow::Result<()> {
    let msg_deserialized = serde_json::to_string(&msg).context("serialize message")?;
    write
        .send(Message::Text(msg_deserialized.into()))
        .await
        .context("send message")?;
    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let url = "wss://bitdefenders.cvjd.me/ws";
    let (ws, _) = connect_async(url).await.unwrap();
    let (mut write, mut read) = ws.split();
    let mut myPlayerId: i32 = -1;
    let mut current_turn: StartTurnArgs;
    println!("connected");

    

    while let Some(msg) = read.next().await {
        let msg = msg.unwrap();

        let text = match msg {
            Message::Text(text) => text,
            Message::Ping(payload) => {
                write.send(Message::Pong(payload)).await.unwrap();
                continue;
            }
            Message::Pong(_) => {
                println!("pong");
                continue;
            }
            Message::Binary(_) => {
                println!("binary message ignored");
                continue;
            }
            Message::Close(frame) => {
                println!("closed: {frame:?}");
                break;
            }
            Message::Frame(_) => continue,
        };
        let message: WebSocketMessage = serde_json::from_str(&text).unwrap();
        println!("{message:?}");
        match message.command {
            Command::Hello => {
                if let Err(e) = send_command(
                    &mut write,
                    WebSocketMessage {
                        command: Command::Login,
                        args: serde_json::json!({"version": 1, "name": "robertcd29"}),
                    },
                )
                .await {
                    println!("Failed to send login command: {e}");
                    break;
                }
            }
            Command::Login => {
                panic!("What are you doing here?");
            },
            Command::Error => {
                println!("Error: {message:?}");
                break;
            }
            Command::Ready => {
                if let Err(e) = send_command(
                    &mut write,
                    WebSocketMessage {
                        command: Command::Practice,
                        args: serde_json::json!({}),
                    },
                ).await {
                    println!("Failed to send practice command: {e}");
                    break;
                }
            },
            Command::Practice => {
                println!("Entered practice mode!");
            },
            Command::StartMatch => {
                let response:StartMatchArgs = serde_json::from_value(message.args).unwrap();
                myPlayerId = response.your_player_id;
                
            },
            Command::StartTurn => {
                current_turn = serde_json::from_value(message.args).unwrap();
                let moveArgs = protocol::MoveArgs {
                    hero_id: myPlayerId,
                    x: -1,
                    y: 1,
                };


                let response = send_command(
                    &mut write,
                    WebSocketMessage {
                        command: Command::Move,
                        args: serde_json::to_value(moveArgs).unwrap(),
                    }
                ).await?;

                let args2 : MoveArgs = protocol::MoveArgs {
                    hero_id: 1,
                    x: 1,
                    y: 1,
                };

                let response2 = send_command(
                    &mut write,
                    WebSocketMessage {
                        command: Command::Move,
                        args: serde_json::to_value(args2).unwrap(),
                    }
                ).await?;
            },
            Command::Move => {
                println!("Move command response: {message:?}");
            },
            Command::EndMatch => {
                println!("Match ended: {message:?}");
            }
        }
    }
    Ok(())
}

