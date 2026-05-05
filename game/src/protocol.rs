use std::collections::HashMap;
use serde::{Deserialize, Serialize};

pub const PROTOCOL_VERSION: i32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebSocketMessage {
    pub command: String,
    #[serde(default)]
    pub args: serde_json::Value,
}

impl WebSocketMessage {
    pub fn new(command: impl Into<String>, args: impl Serialize) -> Self {
        Self {
            command: command.into(),
            args: serde_json::to_value(args)
                .unwrap_or(serde_json::Value::Object(Default::default())),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoginArgs {
    pub name: String,
    pub version: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PracticeArgs {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seed: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ChallengeArgs {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seed: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeroTypeConfig {
    pub shoot_cooldown: i32,
    pub projectile_ttl: i32,
    pub projectile_speed: i32,
    pub max_hp: i32,
    pub projectile_damage: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerHeroSpawn {
    pub id: i32,
    pub x: i32,
    pub y: i32,
    #[serde(rename = "type")]
    pub type_: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Player {
    pub id: i32,
    pub name: String,
    pub heroes: Vec<PlayerHeroSpawn>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameConfig {
    pub width: i32,
    pub height: i32,
    pub turns: i32,
    pub vision_range: i32,
    pub seed: u32,
    pub players: Vec<Player>,
    pub hero_types: HashMap<String, HeroTypeConfig>,
}

impl GameConfig {
    pub fn sniper(&self) -> Option<&HeroTypeConfig> {
        self.hero_types.get("sniper")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hero {
    pub id: i32,
    pub owner_id: i32,
    #[serde(rename = "type")]
    pub type_: String,
    pub x: i32,
    pub y: i32,
    pub hp: i32,
    pub cooldown: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Projectile {
    pub owner_id: i32,
    #[serde(rename = "type")]
    pub type_: String,
    pub origin_x: i32,
    pub origin_y: i32,
    pub x: i32,
    pub y: i32,
    pub ttl: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Wall {
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameState {
    pub heroes: Vec<Hero>,
    pub projectiles: Vec<Projectile>,
    pub walls: Vec<Wall>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StartMatchArgs {
    pub match_id: String,
    pub your_player_id: i32,
    pub config: GameConfig,
    pub state: GameState,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StartTurnArgs {
    pub turn: i32,
    pub state: GameState,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EndMatchArgs {
    pub reason: String,
    #[serde(default)]
    pub winner: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorArgs {
    pub code: String,
    pub message: String,
    #[serde(default)]
    pub fatal: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MoveArgs {
    pub hero_id: i32,
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShootArgs {
    pub hero_id: i32,
    pub x: i32,
    pub y: i32,
}

pub mod cmd {
    pub const HELLO: &str = "HELLO";
    pub const LOGIN: &str = "LOGIN";
    pub const READY: &str = "READY";
    pub const PRACTICE: &str = "PRACTICE";
    pub const CHALLENGE: &str = "CHALLENGE";
    pub const START_MATCH: &str = "START_MATCH";
    pub const START_TURN: &str = "START_TURN";
    pub const MOVE: &str = "MOVE";
    pub const SHOOT: &str = "SHOOT";
    pub const END_MATCH: &str = "END_MATCH";
    pub const ERROR: &str = "ERROR";
}