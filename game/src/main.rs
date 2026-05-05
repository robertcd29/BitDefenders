use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use tokio_tungstenite::{connect_async, tungstenite::Message};

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
    Challenge,
    StartMatch,
    StartTurn,
    Move,
    Shoot,
    EndMatch,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct StartMatchArgs {
    pub your_player_id: i32,
    pub config: GameConfig,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GameConfig {
    pub width: usize,
    pub height: usize,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct StartTurnArgs {
    pub turn: i32,
    pub state: GameState,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GameState {
    pub heroes: Vec<Hero>,
    pub projectiles: Vec<Projectile>,
    pub walls: Vec<Wall>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Hero {
    pub id: i32,
    pub owner_id: i32,
    pub x: i32,
    pub y: i32,
    pub hp: i32,
    pub cooldown: i32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Projectile {
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Wall {
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MoveArgs {
    pub hero_id: i32,
    pub x: i32,
    pub y: i32,
}

async fn send_command<S>(write: &mut S, msg: WebSocketMessage)
where
    S: SinkExt<Message, Error = tokio_tungstenite::tungstenite::Error> + Unpin,
{
    if let Ok(text) = serde_json::to_string(&msg) {
        let _ = write.send(Message::Text(text.into())).await;
    }
}

fn bfs(
    start: (usize, usize),
    goal: (usize, usize),
    width: usize,
    height: usize,
    walls: &HashMap<(usize, usize), bool>,
) -> Option<Vec<(usize, usize)>> {
    let mut queue = VecDeque::new();
    let mut visited: HashMap<(usize, usize), (usize, usize)> = HashMap::new();

    queue.push_back(start);
    visited.insert(start, start);

    while let Some((x, y)) = queue.pop_front() {
        if (x as i32 - goal.0 as i32).abs() <= 3 && (y as i32 - goal.1 as i32).abs() <= 3 {
            let mut path = Vec::new();
            let mut current = (x, y);
            while current != start {
                path.push(current);
                current = visited[&current];
            }
            path.reverse();
            return Some(path);
        }

        for dx in &[-3, 0, 3] {
            for dy in &[-3, 0, 3] {
                if *dx == 0 && *dy == 0 { continue; }

                let nx = x as i32 + dx;
                let ny = y as i32 + dy;

                if nx >= 0 && nx < width as i32 && ny >= 0 && ny < height as i32 {
                    let next = (nx as usize, ny as usize);
                    if !walls.contains_key(&next) && !visited.contains_key(&next) {
                        visited.insert(next, (x, y));
                        queue.push_back(next);
                    }
                }
            }
        }
    }
    None
}

#[tokio::main]
async fn main() {
    let (ws, _) = match connect_async("wss://bitdefenders.cvjd.me/ws").await {
        Ok(v) => v,
        Err(e) => {
            eprintln!("Error: {e}");
            return;
        }
    };

    let (mut write, mut read) = ws.split();
    let mut my_player_id = -1;
    let mut map_size = (51, 90);

    while let Some(msg) = read.next().await {
        let msg = match msg {
            Ok(m) => m,
            Err(_) => break,
        };

        let text = match msg {
            Message::Text(t) => t,
            Message::Ping(p) => {
                let _ = write.send(Message::Pong(p)).await;
                continue;
            }
            Message::Close(_) => break,
            _ => continue,
        };

        let message: WebSocketMessage = match serde_json::from_str(&text) {
            Ok(m) => m,
            Err(_) => continue,
        };

        match message.command {
            Command::Hello => {
                send_command(&mut write, WebSocketMessage {
                    command: Command::Login,
                    args: serde_json::json!({"version": 1, "name": "robertcd29"}),
                }).await;
            }
            Command::Ready => {
                send_command(&mut write, WebSocketMessage {
                    command: Command::Practice,
                    args: serde_json::json!({}),
                }).await;
            }
            Command::Practice => {
                println!("Entered practice mode");
            }
            Command::Challenge => {
                println!("Entered challenge mode");
            }
            Command::StartMatch => {
                if let Ok(args) = serde_json::from_value::<StartMatchArgs>(message.args) {
                    my_player_id = args.your_player_id;
                    map_size = (args.config.width, args.config.height);
                }
            }
            Command::StartTurn => {
                let turn_data = match serde_json::from_value::<StartTurnArgs>(message.args) {
                    Ok(v) => v,
                    Err(_) => continue,
                };

                let mut walls = HashMap::new();
                for w in &turn_data.state.walls {
                    walls.insert((w.x as usize, w.y as usize), true);
                }

                let my_heroes: Vec<&Hero> = turn_data.state.heroes.iter()
                    .filter(|h| h.owner_id == my_player_id).collect();
                let enemies: Vec<&Hero> = turn_data.state.heroes.iter()
                    .filter(|h| h.owner_id != my_player_id).collect();

                for hero in my_heroes {
                    let start = (hero.x as usize, hero.y as usize);
                    let target = enemies.iter().min_by_key(|e| (e.x - hero.x).abs() + (e.y - hero.y).abs());

                    if let Some(enemy) = target {
                        let goal = (enemy.x as usize, enemy.y as usize);
                        
                        if let Some(path) = bfs(start, goal, map_size.0, map_size.1, &walls) {
                            if let Some(next) = path.first() {
                                send_command(&mut write, WebSocketMessage {
                                    command: Command::Move,
                                    args: serde_json::to_value(MoveArgs {
                                        hero_id: hero.id,
                                        x: next.0 as i32,
                                        y: next.1 as i32,
                                    }).unwrap(),
                                }).await;
                            }
                        }
                    }
                }
            }
            Command::EndMatch | Command::Error => break,
            _ => {}
        }
    }
}