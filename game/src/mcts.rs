use std::collections::HashSet;
use std::time::Instant;
use rand::Rng;
use rand::RngExt;

use crate::protocol::{GameConfig, GameState, Hero, MoveArgs, ShootArgs};
use crate::bot::Order;
use crate::game;

const SIMULATION_DEPTH: i32 = 5; // Câte ture simulăm în viitor
const TIME_LIMIT_MS: u128 = 100;

pub struct MctsSolver {
    config: GameConfig,
    my_player_id: i32,
    damage: i32,
    cooldown_time: i32,
}

impl MctsSolver {
    pub fn new(config: GameConfig, my_player_id: i32) -> Self {
        let damage = config.hero_types.get("sniper").map(|s| s.projectile_damage).unwrap_or(1000);
        let cooldown_time = config.hero_types.get("sniper").map(|s| s.shoot_cooldown).unwrap_or(4);
        
        Self { config, my_player_id, damage, cooldown_time }
    }

    pub fn search(&self, state: &GameState, my_heroes: &[&Hero], enemies: &[&Hero], walls: &HashSet<(i32, i32)>) -> Vec<Order> {
        let start_time = Instant::now();
        let mut best_orders = Vec::new();
        let mut rng = rand::rng();

        for hero in my_heroes {
            let possible_actions = self.generate_actions(hero, enemies, walls);
            if possible_actions.is_empty() {
                continue;
            }

            let mut action_scores: Vec<(Order, i32, i32)> = possible_actions.into_iter()
                .map(|order| (order, 0, 0))
                .collect();

            // Simulăm cât ne permite timpul per erou
            let time_per_hero = TIME_LIMIT_MS / my_heroes.len() as u128;
            while start_time.elapsed().as_millis() < time_per_hero {
                let action_idx = rng.random_range(0..action_scores.len());
                let test_action = action_scores[action_idx].0.clone();

                let mut sim_state = state.clone();
                let score = self.playout(&mut sim_state, test_action, hero.id, walls, &mut rng);

                action_scores[action_idx].1 += score;
                action_scores[action_idx].2 += 1;
            }

            let best_action = action_scores.into_iter()
                .max_by(|a, b| {
                    let avg_a = if a.2 > 0 { a.1 / a.2 } else { i32::MIN };
                    let avg_b = if b.2 > 0 { b.1 / b.2 } else { i32::MIN };
                    avg_a.cmp(&avg_b)
                });

            if let Some((order, _, _)) = best_action {
                best_orders.push(order);
            }
        }

        best_orders
    }

    fn generate_actions(&self, hero: &Hero, enemies: &[&Hero], walls: &HashSet<(i32, i32)>) -> Vec<Order> {
        let mut actions = Vec::new();
        let pos = (hero.x, hero.y);

        if hero.cooldown == 0 {
            for enemy in enemies {
                if game::has_clear_shot(pos, (enemy.x, enemy.y), walls) {
                    actions.push(Order::Shoot(ShootArgs {
                        hero_id: hero.id,
                        x: enemy.x,
                        y: enemy.y,
                        comment: Some("Shoot".to_string()),
                    }));
                }
            }
        }

        if let Some(epos) = enemies.first().map(|e| (e.x, e.y)) {
            if let Some(hideout) = game::find_hideout(pos, epos, walls, self.config.width, self.config.height) {
                if let Some(next_step) = game::astar_next_step_safe(pos, hideout, self.config.width, self.config.height, walls) {
                    actions.push(Order::Move(MoveArgs {
                        hero_id: hero.id,
                        x: next_step.0,
                        y: next_step.1,
                        comment: Some("Hide".to_string()),
                    }));
                }
            }
        }

        let dirs = [(3, 0), (-3, 0), (0, 3), (0, -3), (3, 3), (-3, -3), (3, -3), (-3, 3)];
        for (dx, dy) in dirs.iter() {
            let nx = pos.0 + dx;
            let ny = pos.1 + dy;
            if game::in_bounds(nx, ny, self.config.width, self.config.height) 
                && game::on_grid(nx, ny) 
                && !game::overlaps_wall(nx, ny, walls) 
            {
                actions.push(Order::Move(MoveArgs {
                    hero_id: hero.id,
                    x: nx,
                    y: ny,
                    comment: Some("Move".to_string()),
                }));
            }
        }

        actions
    }

    // --- FORWARD MODEL: Fizica Jocului ---

    fn playout<R: Rng>(&self, state: &mut GameState, initial_action: Order, active_hero_id: i32, walls: &HashSet<(i32, i32)>, rng: &mut R) -> i32 {
        // 1. Aplicăm mutarea pe care vrem să o testăm
        self.apply_action(state, &initial_action, active_hero_id, walls);

        // 2. Simulăm următoarele N ture în viitor
        for _ in 0..SIMULATION_DEPTH {
            let mut actions_to_apply = Vec::new();

            // Generăm o acțiune aleatoare pentru fiecare erou rămas în viață
            for hero in &state.heroes {
                if let Some(action) = self.get_random_sim_action(hero, state, walls, rng) {
                    actions_to_apply.push((hero.id, action));
                }
            }

            // Aplicăm acțiunile simultan
            for (h_id, action) in actions_to_apply {
                self.apply_action(state, &action, h_id, walls);
            }

            // Scădem cooldown-ul la final de tură simulată
            for hero in &mut state.heroes {
                if hero.cooldown > 0 {
                    hero.cooldown -= 1;
                }
            }

            // Curățăm morții
            state.heroes.retain(|h| h.hp > 0);

            // Verificăm dacă meciul s-a terminat prematur
            let my_alive = state.heroes.iter().any(|h| h.owner_id == self.my_player_id);
            let enemy_alive = state.heroes.iter().any(|h| h.owner_id != self.my_player_id);
            if !my_alive || !enemy_alive {
                break;
            }
        }

        // 3. Evaluăm scorul stării finale
        let my_hp: i32 = state.heroes.iter().filter(|h| h.owner_id == self.my_player_id).map(|h| h.hp).sum();
        let enemy_hp: i32 = state.heroes.iter().filter(|h| h.owner_id != self.my_player_id).map(|h| h.hp).sum();

        // Diferența de HP. Dacă inamicul a murit, scorul va fi uriaș pozitiv.
        my_hp - enemy_hp
    }

    fn get_random_sim_action<R: Rng>(&self, hero: &Hero, state: &GameState, walls: &HashSet<(i32, i32)>, rng: &mut R) -> Option<Order> {
        let mut valid_actions = Vec::new();
        let pos = (hero.x, hero.y);

        // Dacă poate trage, adaugă tragerile în lista de opțiuni
        if hero.cooldown == 0 {
            for other in &state.heroes {
                if other.owner_id != hero.owner_id && other.hp > 0 {
                    if game::has_clear_shot(pos, (other.x, other.y), walls) {
                        valid_actions.push(Order::Shoot(ShootArgs { hero_id: hero.id, x: other.x, y: other.y, comment: None }));
                    }
                }
            }
        }

        // Adaugă mutările posibile
        let dirs = [(3, 0), (-3, 0), (0, 3), (0, -3), (3, 3), (-3, -3), (3, -3), (-3, 3)];
        for (dx, dy) in dirs.iter() {
            let nx = pos.0 + dx;
            let ny = pos.1 + dy;
            if game::in_bounds(nx, ny, self.config.width, self.config.height) && game::on_grid(nx, ny) && !game::overlaps_wall(nx, ny, walls) {
                valid_actions.push(Order::Move(MoveArgs { hero_id: hero.id, x: nx, y: ny, comment: None }));
            }
        }

        if valid_actions.is_empty() {
            None
        } else {
            let idx = rng.random_range(0..valid_actions.len());
            Some(valid_actions[idx].clone())
        }
    }

    fn apply_action(&self, state: &mut GameState, action: &Order, hero_id: i32, _walls: &HashSet<(i32, i32)>) {
        match action {
            Order::Move(m) => {
                if let Some(h) = state.heroes.iter_mut().find(|x| x.id == hero_id) {
                    h.x = m.x;
                    h.y = m.y;
                }
            }
            Order::Shoot(s) => {
                let mut target_hit_id = None;
                
                // Verificăm dacă eroul există și poate trage
                if let Some(h) = state.heroes.iter().find(|x| x.id == hero_id) {
                    if h.cooldown == 0 {
                        // Găsim cine e la coordonatele s.x, s.y
                        if let Some(target) = state.heroes.iter().find(|x| x.x == s.x && x.y == s.y) {
                            target_hit_id = Some(target.id);
                        }
                    }
                }

                if let Some(tid) = target_hit_id {
                    // Aplicăm damage țintei
                    if let Some(t) = state.heroes.iter_mut().find(|x| x.id == tid) {
                        t.hp -= self.damage;
                    }
                    // Resetăm cooldown-ul atacatorului
                    if let Some(h) = state.heroes.iter_mut().find(|x| x.id == hero_id) {
                        h.cooldown = self.cooldown_time;
                    }
                }
            }
        }
    }
}