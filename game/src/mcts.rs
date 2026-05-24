use std::collections::HashSet;
use std::time::Instant;

use crate::protocol::{GameConfig, GameState, Hero, MoveArgs, ShootArgs};
use crate::bot::Order;
use crate::game;

const TIME_LIMIT_MS: u128 = 85;
const ROLLOUT_DEPTH: i32 = 10;
const UCB_C: f64 = 1.2;
const MIN_VISITS_BEFORE_EXPAND: u32 = 2;

fn xorshift(state: &mut u64) -> u64 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    *state
}

fn rand_usize(state: &mut u64, n: usize) -> usize {
    (xorshift(state) as usize) % n
}

fn rand_bool_weighted(state: &mut u64, threshold: u64, out_of: u64) -> bool {
    xorshift(state) % out_of < threshold
}

#[derive(Clone)]
struct SimHero {
    id: i32,
    owner_id: i32,
    x: i32,
    y: i32,
    hp: i32,
    cooldown: i32,
}

impl SimHero {
    fn from(h: &Hero) -> Self {
        Self { id: h.id, owner_id: h.owner_id, x: h.x, y: h.y, hp: h.hp, cooldown: h.cooldown }
    }
}

#[derive(Clone)]
struct SimState {
    heroes: Vec<SimHero>,
}

struct MctsNode {
    actions: Option<Vec<Order>>,
    visits: u32,
    total_score: f64,
    children: Vec<MctsNode>,
    untried_actions: Vec<Vec<Order>>,
    expanded: bool,
}

impl MctsNode {
    fn new(untried: Vec<Vec<Order>>) -> Self {
        Self {
            actions: None,
            visits: 0,
            total_score: 0.0,
            children: Vec::new(),
            untried_actions: untried,
            expanded: false,
        }
    }

    fn ucb1(&self, parent_visits: u32) -> f64 {
        if self.visits == 0 {
            return f64::INFINITY;
        }
        let exploitation = self.total_score / self.visits as f64;
        let exploration = UCB_C * ((parent_visits as f64).ln() / self.visits as f64).sqrt();
        exploitation + exploration
    }

    fn best_child_idx(&self) -> usize {
        self.children
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| {
                a.ucb1(self.visits).partial_cmp(&b.ucb1(self.visits)).unwrap()
            })
            .map(|(i, _)| i)
            .unwrap_or(0)
    }

    fn best_action_idx(&self) -> usize {
        self.children
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| {
                let avg_a = if a.visits > 0 { a.total_score / a.visits as f64 } else { f64::NEG_INFINITY };
                let avg_b = if b.visits > 0 { b.total_score / b.visits as f64 } else { f64::NEG_INFINITY };
                avg_a.partial_cmp(&avg_b).unwrap()
            })
            .map(|(i, _)| i)
            .unwrap_or(0)
    }
}

pub struct MctsSolver {
    config: GameConfig,
    my_player_id: i32,
    damage: i32,
    cooldown_time: i32,
    max_hp: i32,
}

impl MctsSolver {
    pub fn new(config: GameConfig, my_player_id: i32) -> Self {
        let damage = config.hero_types.get("sniper").map(|s| s.projectile_damage).unwrap_or(1000);
        let cooldown_time = config.hero_types.get("sniper").map(|s| s.shoot_cooldown).unwrap_or(4);
        let max_hp = config.hero_types.get("sniper").map(|s| s.max_hp).unwrap_or(3000);
        Self { config, my_player_id, damage, cooldown_time, max_hp }
    }

    pub fn search(
        &self,
        state: &GameState,
        my_heroes: &[&Hero],
        enemies: &[&Hero],
        walls: &HashSet<(i32, i32)>,
    ) -> Vec<Order> {
        if my_heroes.is_empty() || enemies.is_empty() {
            return Vec::new();
        }

        let sim = self.game_state_to_sim(state);
        let initial_combos = self.generate_joint_actions(my_heroes, enemies, walls);

        if initial_combos.is_empty() {
            return Vec::new();
        }

        let mut root = MctsNode::new(initial_combos);
        let start = Instant::now();
        let mut rng_state: u64 = {
            let t = start.elapsed().subsec_nanos() as u64;
            let p = &root as *const _ as u64;
            (t ^ p ^ 0xdeadbeefcafe1337).max(1)
        };

        while start.elapsed().as_millis() < TIME_LIMIT_MS {
            let mut sim_copy = sim.clone();
            self.mcts_iteration(&mut root, &mut sim_copy, walls, 0, &mut rng_state);
        }

        if root.children.is_empty() {
            return Vec::new();
        }

        let best_idx = root.best_action_idx();
        if let Some(actions) = &root.children[best_idx].actions {
            return actions.clone();
        }

        Vec::new()
    }

    fn mcts_iteration(
        &self,
        node: &mut MctsNode,
        state: &mut SimState,
        walls: &HashSet<(i32, i32)>,
        depth: i32,
        rng: &mut u64,
    ) -> f64 {
        if depth > 6 {
            return self.evaluate(state);
        }

        if !node.untried_actions.is_empty() && (!node.expanded || node.visits < MIN_VISITS_BEFORE_EXPAND) {
            let action_idx = rand_usize(rng, node.untried_actions.len());
            let actions = node.untried_actions.remove(action_idx);

            for action in &actions {
                let hero_id = self.order_hero_id(action);
                self.apply_action_sim(state, action, hero_id, walls);
            }
            self.apply_enemy_actions_sim(state, walls, rng);
            self.tick_sim(state);

            let my_heroes_sim: Vec<_> = state.heroes.iter()
                .filter(|h| h.owner_id == self.my_player_id)
                .collect();
            let enemies_sim: Vec<_> = state.heroes.iter()
                .filter(|h| h.owner_id != self.my_player_id)
                .collect();
            let new_untried = self.generate_joint_actions_sim(&my_heroes_sim, &enemies_sim, walls);

            let score = self.rollout(state, walls, rng);

            let mut child = MctsNode::new(new_untried);
            child.actions = Some(actions);
            child.visits = 1;
            child.total_score = score;
            node.children.push(child);
            node.expanded = true;
            node.visits += 1;
            node.total_score += score;

            return score;
        }

        if node.children.is_empty() {
            let score = self.rollout(state, walls, rng);
            node.visits += 1;
            node.total_score += score;
            return score;
        }

        let best_idx = node.best_child_idx();
        let child_actions = node.children[best_idx].actions.clone();

        if let Some(actions) = &child_actions {
            for action in actions {
                let hero_id = self.order_hero_id(action);
                self.apply_action_sim(state, action, hero_id, walls);
            }
            self.apply_enemy_actions_sim(state, walls, rng);
            self.tick_sim(state);
        }

        let score = self.mcts_iteration(&mut node.children[best_idx], state, walls, depth + 1, rng);
        node.visits += 1;
        node.total_score += score;

        score
    }

    fn order_hero_id(&self, order: &Order) -> i32 {
        match order {
            Order::Move(m) => m.hero_id,
            Order::Shoot(s) => s.hero_id,
        }
    }

    fn apply_enemy_actions_sim(&self, state: &mut SimState, walls: &HashSet<(i32, i32)>, rng: &mut u64) {
        let enemy_ids: Vec<i32> = state.heroes.iter()
            .filter(|h| h.owner_id != self.my_player_id)
            .map(|h| h.id)
            .collect();

        for eid in enemy_ids {
            let action = {
                let hero = match state.heroes.iter().find(|h| h.id == eid) {
                    Some(h) => h.clone(),
                    None => continue,
                };
                let s = state.clone();
                self.guided_action_sim(&hero, &s, walls, rng)
            };
            self.apply_action_sim(state, &action, eid, walls);
        }
    }

    fn rollout(
        &self,
        state: &mut SimState,
        walls: &HashSet<(i32, i32)>,
        rng: &mut u64,
    ) -> f64 {
        let mut s = state.clone();

        for _ in 0..ROLLOUT_DEPTH {
            let my_alive = s.heroes.iter().any(|h| h.owner_id == self.my_player_id);
            let enemy_alive = s.heroes.iter().any(|h| h.owner_id != self.my_player_id);
            if !my_alive || !enemy_alive {
                break;
            }

            let hero_ids: Vec<i32> = s.heroes.iter().map(|h| h.id).collect();
            let actions: Vec<(i32, Order)> = hero_ids.iter().filter_map(|&hid| {
                let hero = s.heroes.iter().find(|h| h.id == hid)?.clone();
                let sc = s.clone();
                let action = self.guided_action_sim(&hero, &sc, walls, rng);
                Some((hid, action))
            }).collect();

            for (hid, action) in actions {
                self.apply_action_sim(&mut s, &action, hid, walls);
            }
            self.tick_sim(&mut s);
        }

        self.evaluate(&s)
    }

    fn guided_action_sim(
        &self,
        hero: &SimHero,
        state: &SimState,
        walls: &HashSet<(i32, i32)>,
        rng: &mut u64,
    ) -> Order {
        let pos = (hero.x, hero.y);
        let enemies: Vec<_> = state.heroes.iter()
            .filter(|h| h.owner_id != hero.owner_id && h.hp > 0)
            .collect();

        if enemies.is_empty() {
            return Order::Move(MoveArgs { hero_id: hero.id, x: pos.0, y: pos.1, comment: None });
        }

        let is_mine = hero.owner_id == self.my_player_id;

        let nearest_enemy = enemies.iter()
            .min_by_key(|e| game::manhattan(pos, (e.x, e.y)))
            .unwrap();

        // When far from combat, target the weakest enemy to encourage focus fire
        let dist_to_nearest = game::manhattan(pos, (nearest_enemy.x, nearest_enemy.y));
        let focus_target = if is_mine && dist_to_nearest > 24 {
            enemies.iter().min_by_key(|e| e.hp).unwrap()
        } else {
            nearest_enemy
        };
        let epos = (focus_target.x, focus_target.y);

        if hero.cooldown == 0 {
            let shootable: Vec<_> = enemies.iter()
                .filter(|e| game::has_clear_shot(pos, (e.x, e.y), walls))
                .collect();

            if !shootable.is_empty() {
                let finishing: Vec<_> = shootable.iter()
                    .filter(|e| e.hp <= self.damage)
                    .collect();
                let target = if !finishing.is_empty() {
                    finishing[0]
                } else if rand_bool_weighted(rng, 8, 10) {
                    shootable.iter().min_by_key(|e| e.hp).unwrap()
                } else {
                    let idx = rand_usize(rng, shootable.len());
                    shootable[idx]
                };
                return Order::Shoot(ShootArgs {
                    hero_id: hero.id,
                    x: target.x,
                    y: target.y,
                    comment: None,
                });
            }
        }

        let critical_hp = hero.hp <= self.max_hp / 3;
        let low_hp = hero.hp <= self.max_hp / 2;
        let enemy_sees_us = enemies.iter().any(|e| game::has_clear_shot((e.x, e.y), pos, walls));
        let focus_fired = enemies.iter().filter(|e| game::has_clear_shot((e.x, e.y), pos, walls)).count() >= 2;

        if is_mine && (critical_hp || focus_fired) {
            if let Some(h) = game::find_hideout(pos, epos, walls, self.config.width, self.config.height) {
                if let Some(next) = game::astar_next_step_safe(pos, h, self.config.width, self.config.height, walls) {
                    return Order::Move(MoveArgs { hero_id: hero.id, x: next.0, y: next.1, comment: None });
                }
            }
            return Order::Move(MoveArgs { hero_id: hero.id, x: pos.0, y: pos.1, comment: None });
        }

        if is_mine && low_hp && enemy_sees_us {
            if let Some(h) = game::find_hideout(pos, epos, walls, self.config.width, self.config.height) {
                if let Some(next) = game::astar_next_step_safe(pos, h, self.config.width, self.config.height, walls) {
                    return Order::Move(MoveArgs { hero_id: hero.id, x: next.0, y: next.1, comment: None });
                }
            }
        }

        let dist = game::manhattan(pos, epos);

        if is_mine && dist < 12 {
            let retreat = self.best_retreat_move(pos, epos, walls);
            return Order::Move(MoveArgs { hero_id: hero.id, x: retreat.0, y: retreat.1, comment: None });
        }

        if is_mine && hero.cooldown == 0 && dist > 24 {
            let best_move = self.best_approach_move(pos, epos, walls);
            return Order::Move(MoveArgs { hero_id: hero.id, x: best_move.0, y: best_move.1, comment: None });
        }

        if is_mine && !enemy_sees_us && hero.cooldown <= 2 && dist > 24 {
            let best_move = self.best_approach_move(pos, epos, walls);
            return Order::Move(MoveArgs { hero_id: hero.id, x: best_move.0, y: best_move.1, comment: None });
        }

        if enemy_sees_us && hero.cooldown > 0 {
            let retreat = self.best_retreat_move(pos, epos, walls);
            return Order::Move(MoveArgs { hero_id: hero.id, x: retreat.0, y: retreat.1, comment: None });
        }

        if is_mine && dist >= 12 && dist <= 24 {
            return Order::Move(MoveArgs { hero_id: hero.id, x: pos.0, y: pos.1, comment: None });
        }

        let best_move = self.best_approach_move(pos, epos, walls);
        Order::Move(MoveArgs { hero_id: hero.id, x: best_move.0, y: best_move.1, comment: None })
    }

    fn best_approach_move(&self, pos: (i32, i32), target: (i32, i32), walls: &HashSet<(i32, i32)>) -> (i32, i32) {
        const DIRS: [(i32, i32); 8] = [(-3,0),(3,0),(0,-3),(0,3),(-3,-3),(-3,3),(3,-3),(3,3)];
        let mut best = pos;
        let mut best_score = i32::MAX;

        for (dx, dy) in DIRS {
            let nx = pos.0 + dx;
            let ny = pos.1 + dy;
            if game::in_bounds(nx, ny, self.config.width, self.config.height)
                && game::on_grid(nx, ny)
                && !game::overlaps_wall(nx, ny, walls)
            {
                let score = game::chebyshev((nx, ny), target);
                if score < best_score {
                    best_score = score;
                    best = (nx, ny);
                }
            }
        }

        best
    }

    fn best_retreat_move(&self, pos: (i32, i32), enemy_pos: (i32, i32), walls: &HashSet<(i32, i32)>) -> (i32, i32) {
        const DIRS: [(i32, i32); 8] = [(-3,0),(3,0),(0,-3),(0,3),(-3,-3),(-3,3),(3,-3),(3,3)];
        let mut best = pos;
        let mut best_score = i32::MIN;

        for (dx, dy) in DIRS {
            let nx = pos.0 + dx;
            let ny = pos.1 + dy;
            if game::in_bounds(nx, ny, self.config.width, self.config.height)
                && game::on_grid(nx, ny)
                && !game::overlaps_wall(nx, ny, walls)
                && !game::has_clear_shot((nx, ny), enemy_pos, walls)
            {
                let score = game::manhattan((nx, ny), enemy_pos);
                if score > best_score {
                    best_score = score;
                    best = (nx, ny);
                }
            }
        }

        if best == pos {
            for (dx, dy) in DIRS {
                let nx = pos.0 + dx;
                let ny = pos.1 + dy;
                if game::in_bounds(nx, ny, self.config.width, self.config.height)
                    && game::on_grid(nx, ny)
                    && !game::overlaps_wall(nx, ny, walls)
                {
                    let score = game::manhattan((nx, ny), enemy_pos);
                    if score > best_score {
                        best_score = score;
                        best = (nx, ny);
                    }
                }
            }
        }

        best
    }

    fn evaluate(&self, state: &SimState) -> f64 {
        let my_heroes: Vec<_> = state.heroes.iter().filter(|h| h.owner_id == self.my_player_id).collect();
        let enemy_heroes: Vec<_> = state.heroes.iter().filter(|h| h.owner_id != self.my_player_id).collect();

        if my_heroes.is_empty() && enemy_heroes.is_empty() {
            return 0.0;
        }
        if my_heroes.is_empty() {
            return -1000.0;
        }
        if enemy_heroes.is_empty() {
            return 1000.0;
        }

        let my_hp: i32 = my_heroes.iter().map(|h| h.hp).sum();
        let enemy_hp: i32 = enemy_heroes.iter().map(|h| h.hp).sum();

        let hp_score = (my_hp - enemy_hp) as f64 / self.max_hp as f64;

        let my_count = my_heroes.len() as f64;
        let enemy_count = enemy_heroes.len() as f64;
        let count_score = (my_count - enemy_count) * 5.0;

        let focus_penalty: f64 = my_heroes.iter().map(|me| {
            let exposure = enemy_heroes.iter()
                .filter(|e| game::has_clear_shot((e.x, e.y), (me.x, me.y), &HashSet::new()))
                .count() as f64;
            if exposure >= 2.0 { -3.0 * (me.hp as f64 / self.max_hp as f64) } else { 0.0 }
        }).sum();

        let dist_score: f64 = my_heroes.iter().map(|me| {
            let min_dist = enemy_heroes.iter()
                .map(|e| game::manhattan((me.x, me.y), (e.x, e.y)) as f64)
                .fold(f64::INFINITY, f64::min);
            if min_dist < 12.0 {
                (min_dist - 12.0) * 0.15
            } else if min_dist > 24.0 {
                -(min_dist - 24.0) / 150.0
            } else {
                0.0
            }
        }).sum::<f64>() / my_heroes.len() as f64;

        // Penalize heroes being far apart from each other (encourages fighting together)
        let separation_penalty: f64 = if my_heroes.len() >= 2 {
            let mut total = 0.0;
            for i in 0..my_heroes.len() {
                for j in (i+1)..my_heroes.len() {
                    let sep = game::manhattan(
                        (my_heroes[i].x, my_heroes[i].y),
                        (my_heroes[j].x, my_heroes[j].y),
                    ) as f64;
                    if sep > 24.0 {
                        total -= (sep - 24.0) / 100.0;
                    }
                }
            }
            total
        } else {
            0.0
        };

        let enemy_separation: f64 = if enemy_heroes.len() >= 2 {
            let sep = game::manhattan(
                (enemy_heroes[0].x, enemy_heroes[0].y),
                (enemy_heroes[1].x, enemy_heroes[1].y),
            ) as f64;
            sep / 200.0
        } else {
            0.0
        };

        hp_score * 10.0 + count_score + focus_penalty + dist_score + separation_penalty + enemy_separation
    }

    fn generate_joint_actions(
        &self,
        my_heroes: &[&Hero],
        enemies: &[&Hero],
        walls: &HashSet<(i32, i32)>,
    ) -> Vec<Vec<Order>> {
        if my_heroes.is_empty() {
            return Vec::new();
        }

        let mut per_hero: Vec<Vec<Order>> = my_heroes.iter().map(|h| {
            self.generate_actions_for(h, enemies, walls)
        }).collect();

        for actions in &mut per_hero {
            if actions.len() > 4 {
                actions.truncate(4);
            }
        }

        if per_hero.len() == 1 {
            return per_hero.remove(0).into_iter().map(|a| vec![a]).collect();
        }

        let mut combos = Vec::new();
        for a0 in &per_hero[0] {
            for a1 in &per_hero[1] {
                combos.push(vec![a0.clone(), a1.clone()]);
            }
        }
        combos
    }

    fn generate_joint_actions_sim(
        &self,
        my_heroes: &[&SimHero],
        enemies: &[&SimHero],
        walls: &HashSet<(i32, i32)>,
    ) -> Vec<Vec<Order>> {
        if my_heroes.is_empty() {
            return Vec::new();
        }

        let mut per_hero: Vec<Vec<Order>> = my_heroes.iter().map(|h| {
            self.generate_actions_for_sim(h, enemies, walls)
        }).collect();

        for actions in &mut per_hero {
            if actions.len() > 4 {
                actions.truncate(4);
            }
        }

        if per_hero.len() == 1 {
            return per_hero.remove(0).into_iter().map(|a| vec![a]).collect();
        }

        let mut combos = Vec::new();
        for a0 in &per_hero[0] {
            for a1 in &per_hero[1] {
                combos.push(vec![a0.clone(), a1.clone()]);
            }
        }
        combos
    }

    fn generate_actions_for(
        &self,
        hero: &Hero,
        enemies: &[&Hero],
        walls: &HashSet<(i32, i32)>,
    ) -> Vec<Order> {
        let pos = (hero.x, hero.y);
        let mut actions = Vec::new();

        if hero.cooldown == 0 {
            let mut shoot_targets: Vec<_> = enemies.iter()
                .filter(|e| game::has_clear_shot(pos, (e.x, e.y), walls))
                .collect();
            shoot_targets.sort_by_key(|e| e.hp);
            for e in shoot_targets {
                actions.push(Order::Shoot(ShootArgs { hero_id: hero.id, x: e.x, y: e.y, comment: None }));
            }
        }

        const DIRS: [(i32, i32); 8] = [(-3,0),(3,0),(0,-3),(0,3),(-3,-3),(-3,3),(3,-3),(3,3)];
        let epos = enemies.first().map(|e| (e.x, e.y)).unwrap_or((0, 0));

        let mut moves: Vec<(i32, i32)> = DIRS.iter().filter_map(|&(dx, dy)| {
            let nx = pos.0 + dx;
            let ny = pos.1 + dy;
            if game::in_bounds(nx, ny, self.config.width, self.config.height)
                && game::on_grid(nx, ny)
                && !game::overlaps_wall(nx, ny, walls)
            {
                Some((nx, ny))
            } else {
                None
            }
        }).collect();

        moves.sort_by_key(|&p| game::chebyshev(p, epos));
        moves.truncate(4);

        for (nx, ny) in moves {
            actions.push(Order::Move(MoveArgs { hero_id: hero.id, x: nx, y: ny, comment: None }));
        }

        actions
    }

    fn generate_actions_for_sim(
        &self,
        hero: &SimHero,
        enemies: &[&SimHero],
        walls: &HashSet<(i32, i32)>,
    ) -> Vec<Order> {
        let pos = (hero.x, hero.y);
        let mut actions = Vec::new();

        if hero.cooldown == 0 {
            for e in enemies {
                if game::has_clear_shot(pos, (e.x, e.y), walls) {
                    actions.push(Order::Shoot(ShootArgs { hero_id: hero.id, x: e.x, y: e.y, comment: None }));
                }
            }
        }

        const DIRS: [(i32, i32); 8] = [(-3,0),(3,0),(0,-3),(0,3),(-3,-3),(-3,3),(3,-3),(3,3)];
        let epos = enemies.first().map(|e| (e.x, e.y)).unwrap_or((0, 0));

        let mut moves: Vec<(i32, i32)> = DIRS.iter().filter_map(|&(dx, dy)| {
            let nx = pos.0 + dx;
            let ny = pos.1 + dy;
            if game::in_bounds(nx, ny, self.config.width, self.config.height)
                && game::on_grid(nx, ny)
                && !game::overlaps_wall(nx, ny, walls)
            {
                Some((nx, ny))
            } else {
                None
            }
        }).collect();

        moves.sort_by_key(|&p| game::chebyshev(p, epos));
        moves.truncate(4);

        for (nx, ny) in moves {
            actions.push(Order::Move(MoveArgs { hero_id: hero.id, x: nx, y: ny, comment: None }));
        }

        actions
    }

    fn apply_action_sim(
        &self,
        state: &mut SimState,
        action: &Order,
        hero_id: i32,
        _walls: &HashSet<(i32, i32)>,
    ) {
        match action {
            Order::Move(m) => {
                if let Some(h) = state.heroes.iter_mut().find(|h| h.id == hero_id) {
                    h.x = m.x;
                    h.y = m.y;
                }
            }
            Order::Shoot(s) => {
                let can_shoot = state.heroes.iter()
                    .find(|h| h.id == hero_id)
                    .map(|h| h.cooldown == 0)
                    .unwrap_or(false);

                if !can_shoot {
                    return;
                }

                let target_id = state.heroes.iter()
                    .find(|h| h.x == s.x && h.y == s.y && h.hp > 0)
                    .map(|h| h.id);

                if let Some(tid) = target_id {
                    if let Some(t) = state.heroes.iter_mut().find(|h| h.id == tid) {
                        t.hp -= self.damage;
                    }
                    if let Some(h) = state.heroes.iter_mut().find(|h| h.id == hero_id) {
                        h.cooldown = self.cooldown_time;
                    }
                }
            }
        }
    }

    fn tick_sim(&self, state: &mut SimState) {
        for h in &mut state.heroes {
            if h.cooldown > 0 {
                h.cooldown -= 1;
            }
        }
        state.heroes.retain(|h| h.hp > 0);
    }

    fn game_state_to_sim(&self, state: &GameState) -> SimState {
        SimState {
            heroes: state.heroes.iter().map(SimHero::from).collect(),
        }
    }
}