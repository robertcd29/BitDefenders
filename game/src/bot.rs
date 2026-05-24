use std::collections::{HashMap, HashSet};

use crate::protocol::{GameConfig, Hero, MoveArgs, ShootArgs, StartMatchArgs, StartTurnArgs};
use crate::game;
use crate::mcts::MctsSolver;

#[derive(Clone)]
pub enum Order {
    Move(MoveArgs),
    Shoot(ShootArgs),
}

pub struct Bot {
    my_player_id: i32,
    config: GameConfig,
    last_seen: HashMap<i32, (i32, i32)>,
    turn: i32,
    solver: MctsSolver,
}

impl Bot {
    pub fn new(args: StartMatchArgs) -> Self {
        Self {
            my_player_id: args.your_player_id,
            config: args.config.clone(),
            last_seen: HashMap::new(),
            turn: 0,
            solver: MctsSolver::new(args.config, args.your_player_id),
        }
    }

    pub fn take_turn(&mut self, args: StartTurnArgs) -> Vec<Order> {
        self.turn = args.turn;
        let state = &args.state;

        for hero in state.heroes.iter().filter(|h| h.owner_id != self.my_player_id) {
            self.last_seen.insert(hero.id, (hero.x, hero.y));
        }

        let walls = game::build_wall_set(&state.walls);

        let mut my_heroes: Vec<&Hero> = state.heroes.iter()
            .filter(|h| h.owner_id == self.my_player_id)
            .collect();
        my_heroes.sort_by_key(|h| h.id);

        let enemies: Vec<&Hero> = state.heroes.iter()
            .filter(|h| h.owner_id != self.my_player_id)
            .collect();

        if my_heroes.is_empty() {
            return Vec::new();
        }

        let mut orders = Vec::new();

        let enemies_visible = !enemies.is_empty();
        let enemies_known = enemies_visible || !self.last_seen.is_empty();

        if !enemies_known {
            let intercept = game::snap_to_grid((self.config.width / 4, self.config.height / 2));
            for hero in &my_heroes {
                let pos = (hero.x, hero.y);
                let next = if pos != intercept {
                    game::astar_next_step_safe(pos, intercept, self.config.width, self.config.height, &walls)
                        .unwrap_or(pos)
                } else {
                    pos
                };
                orders.push(Order::Move(MoveArgs {
                    hero_id: hero.id,
                    x: next.0,
                    y: next.1,
                    comment: None,
                }));
            }
            return orders;
        }

        let use_mcts = enemies_visible && {
            let min_dist = my_heroes.iter()
                .flat_map(|me| enemies.iter().map(move |e| game::manhattan((me.x, me.y), (e.x, e.y))))
                .min()
                .unwrap_or(i32::MAX);
            min_dist <= 80
        };

        if use_mcts {
            let mcts_orders = self.solver.search(state, &my_heroes, &enemies, &walls);
            if mcts_orders.len() == my_heroes.len() {
                return mcts_orders;
            }
            for (i, hero) in my_heroes.iter().enumerate() {
                let order = mcts_orders.get(i)
                    .cloned()
                    .unwrap_or_else(|| self.decide(hero, i, &enemies, &walls));
                orders.push(order);
            }
            return orders;
        }

        for (i, hero) in my_heroes.iter().enumerate() {
            orders.push(self.decide(hero, i, &enemies, &walls));
        }

        orders
    }

    pub fn decide(
        &self,
        hero: &Hero,
        hero_index: usize,
        enemies: &[&Hero],
        walls: &HashSet<(i32, i32)>,
    ) -> Order {
        let pos = (hero.x, hero.y);

        let stagger_shoot = hero_index > 0 && (self.turn % 2 == 0);

        if hero.cooldown == 0 && !stagger_shoot {
            let target = self.pick_shoot_target(hero, enemies, walls);
            if let Some(t) = target {
                return Order::Shoot(ShootArgs {
                    hero_id: hero.id,
                    x: t.x,
                    y: t.y,
                    comment: None,
                });
            }
        }

        let dest = self.pick_destination(hero, hero_index, enemies, walls);

        let next = if dest != pos {
            game::astar_next_step_safe(pos, dest, self.config.width, self.config.height, walls)
                .unwrap_or(pos)
        } else {
            pos
        };

        Order::Move(MoveArgs {
            hero_id: hero.id,
            x: next.0,
            y: next.1,
            comment: None,
        })
    }

    fn pick_shoot_target<'a>(
        &self,
        hero: &Hero,
        enemies: &[&'a Hero],
        walls: &HashSet<(i32, i32)>,
    ) -> Option<&'a Hero> {
        let pos = (hero.x, hero.y);
        let damage = self.config.hero_types.get("sniper").map(|s| s.projectile_damage).unwrap_or(1000);

        let mut visible: Vec<&Hero> = enemies.iter()
            .copied()
            .filter(|e| game::has_clear_shot(pos, (e.x, e.y), walls))
            .collect();

        if visible.is_empty() {
            return None;
        }

        visible.sort_by_key(|e| {
            let dist = game::manhattan(pos, (e.x, e.y));
            let finishing_blow = if e.hp <= damage { -100000 } else { 0 };
            let low_hp_bonus = if e.hp <= damage * 2 { -5000 } else { 0 };
            let shots_to_kill = (e.hp + damage - 1) / damage;
            finishing_blow + low_hp_bonus + shots_to_kill * 100 + dist
        });

        visible.into_iter().next()
    }

    fn pick_destination(
        &self,
        hero: &Hero,
        hero_index: usize,
        enemies: &[&Hero],
        walls: &HashSet<(i32, i32)>,
    ) -> (i32, i32) {
        let pos = (hero.x, hero.y);

        let my_max_hp = self.config.hero_types.get("sniper")
            .map(|s| s.max_hp)
            .unwrap_or(3000);
        let low_hp = hero.hp <= my_max_hp / 2;
        let critical_hp = hero.hp <= my_max_hp / 3;

        let enemy_can_see_us = enemies.iter()
            .any(|e| game::has_clear_shot((e.x, e.y), pos, walls));

        let focus_fired = enemies.iter()
            .filter(|e| game::has_clear_shot((e.x, e.y), pos, walls))
            .count() >= 2;

        let target_enemy = if !enemies.is_empty() {
            if hero.cooldown == 0 {
                enemies.iter().copied()
                    .min_by_key(|e| {
                        let dist = game::manhattan(pos, (e.x, e.y));
                        let hp_bonus = if e.hp <= my_max_hp / 3 { -1000 } else { 0 };
                        dist + hp_bonus
                    })
            } else {
                enemies.iter().copied().min_by_key(|e| e.hp)
            }
        } else {
            None
        };

        let epos: (i32, i32) = if let Some(e) = target_enemy {
            (e.x, e.y)
        } else if let Some(&lp) = self.last_seen.values()
            .min_by_key(|&&p| game::manhattan(pos, p))
        {
            lp
        } else {
            return self.explore(hero_index);
        };

        if critical_hp || focus_fired {
            if let Some(h) = game::find_hideout(pos, epos, walls, self.config.width, self.config.height) {
                return h;
            }
            return self.strafe(pos, epos, walls, false);
        }

        if low_hp && enemy_can_see_us {
            if let Some(h) = game::find_hideout(pos, epos, walls, self.config.width, self.config.height) {
                return h;
            }
            return self.strafe(pos, epos, walls, false);
        }

        if hero.cooldown == 0 {
            if game::has_clear_shot(pos, epos, walls) {
                let dist = game::manhattan(pos, epos);
                if dist >= 12 && dist <= 24 {
                    return pos;
                }
                if dist < 12 {
                    return self.strafe(pos, epos, walls, true);
                }
                return self.approach_with_los(pos, epos, walls);
            }
            if let Some(peek) = self.find_peek(pos, epos, walls) {
                return peek;
            }
            return self.move_toward(pos, epos, walls);
        }

        if enemy_can_see_us {
            let dist = game::manhattan(pos, epos);
            if dist < 12 {
                return self.strafe(pos, epos, walls, false);
            }
            if let Some(h) = game::find_hideout(pos, epos, walls, self.config.width, self.config.height) {
                return h;
            }
            return self.strafe(pos, epos, walls, false);
        }

        if hero.cooldown <= 2 {
            if let Some(peek) = self.find_peek(pos, epos, walls) {
                return peek;
            }
        }

        self.move_toward(pos, epos, walls)
    }

    fn move_toward(&self, pos: (i32, i32), target: (i32, i32), walls: &HashSet<(i32, i32)>) -> (i32, i32) {
        let dx = (target.0 - pos.0).signum();
        let dy = (target.1 - pos.1).signum();

        let mut candidates: Vec<(i32, i32, i32)> = GRID_DIRS.iter().map(|&(ddx, ddy)| {
            let nx = pos.0 + ddx;
            let ny = pos.1 + ddy;
            let score = game::chebyshev((nx, ny), target);
            (score, nx, ny)
        }).collect();
        candidates.sort_by_key(|&(s, _, _)| s);

        for (_, nx, ny) in candidates {
            if game::in_bounds(nx, ny, self.config.width, self.config.height)
                && game::on_grid(nx, ny)
                && !game::overlaps_wall(nx, ny, walls)
            {
                return (nx, ny);
            }
        }

        let nx = pos.0 + dx * 3;
        let ny = pos.1 + dy * 3;
        if game::in_bounds(nx, ny, self.config.width, self.config.height)
            && game::on_grid(nx, ny)
            && !game::overlaps_wall(nx, ny, walls)
        {
            return (nx, ny);
        }

        pos
    }

    fn approach_with_los(&self, pos: (i32, i32), target: (i32, i32), walls: &HashSet<(i32, i32)>) -> (i32, i32) {
        let mut candidates: Vec<(i32, i32, i32)> = GRID_DIRS.iter().map(|&(ddx, ddy)| {
            let nx = pos.0 + ddx;
            let ny = pos.1 + ddy;
            let score = game::chebyshev((nx, ny), target);
            (score, nx, ny)
        }).collect();
        candidates.sort_by_key(|&(s, _, _)| s);

        for (_, nx, ny) in &candidates {
            if game::in_bounds(*nx, *ny, self.config.width, self.config.height)
                && game::on_grid(*nx, *ny)
                && !game::overlaps_wall(*nx, *ny, walls)
                && game::has_clear_shot((*nx, *ny), target, walls)
            {
                return (*nx, *ny);
            }
        }

        self.move_toward(pos, target, walls)
    }

    fn strafe(&self, pos: (i32, i32), enemy_pos: (i32, i32), walls: &HashSet<(i32, i32)>, keep_los: bool) -> (i32, i32) {
        let ex = enemy_pos.0 - pos.0;
        let ey = enemy_pos.1 - pos.1;

        let perp1 = (ey.signum() * 3, -ex.signum() * 3);
        let perp2 = (-ey.signum() * 3, ex.signum() * 3);
        let back = (-ex.signum() * 3, -ey.signum() * 3);

        let candidates = [perp1, perp2, back,
            (perp1.0 + back.0, perp1.1 + back.1),
            (perp2.0 + back.0, perp2.1 + back.1),
        ];

        let mut best: Option<(i32, i32)> = None;
        let mut best_dist = i32::MIN;

        for (ddx, ddy) in candidates {
            if ddx == 0 && ddy == 0 { continue; }
            let nx = pos.0 + ddx;
            let ny = pos.1 + ddy;

            let nx = ((nx).div_euclid(3)) * 3 + 1;
            let ny = ((ny).div_euclid(3)) * 3 + 1;

            if !game::in_bounds(nx, ny, self.config.width, self.config.height)
                || !game::on_grid(nx, ny)
                || game::overlaps_wall(nx, ny, walls)
            {
                continue;
            }

            let has_los = game::has_clear_shot((nx, ny), enemy_pos, walls);
            if keep_los && !has_los { continue; }
            if !keep_los && has_los { continue; }

            let dist_from_enemy = game::manhattan((nx, ny), enemy_pos);
            if dist_from_enemy > best_dist {
                best_dist = dist_from_enemy;
                best = Some((nx, ny));
            }
        }

        if let Some(p) = best {
            return p;
        }

        let mut fallback_best: Option<(i32, (i32, i32))> = None;
        for &(ddx, ddy) in &GRID_DIRS {
            let nx = pos.0 + ddx;
            let ny = pos.1 + ddy;
            if game::in_bounds(nx, ny, self.config.width, self.config.height)
                && game::on_grid(nx, ny)
                && !game::overlaps_wall(nx, ny, walls)
            {
                let d = game::manhattan((nx, ny), enemy_pos);
                if fallback_best.is_none() || d > fallback_best.unwrap().0 {
                    fallback_best = Some((d, (nx, ny)));
                }
            }
        }

        fallback_best.map(|(_, p)| p).unwrap_or(pos)
    }

    fn find_peek(&self, my_pos: (i32, i32), enemy_pos: (i32, i32), walls: &HashSet<(i32, i32)>) -> Option<(i32, i32)> {
        let mut best: Option<(i32, (i32, i32))> = None;

        for dx in (-24i32..=24).step_by(3) {
            for dy in (-24i32..=24).step_by(3) {
                if dx == 0 && dy == 0 { continue; }
                let nx = my_pos.0 + dx;
                let ny = my_pos.1 + dy;

                if !game::in_bounds(nx, ny, self.config.width, self.config.height)
                    || !game::on_grid(nx, ny)
                    || game::overlaps_wall(nx, ny, walls)
                {
                    continue;
                }

                if game::has_clear_shot((nx, ny), enemy_pos, walls) {
                    let dist_to_me = game::manhattan(my_pos, (nx, ny));
                    let dist_to_enemy = game::manhattan((nx, ny), enemy_pos);
                    let too_close_penalty = if dist_to_enemy < 6 { 500 } else { 0 };
                    let too_far_penalty = if dist_to_enemy > 30 { (dist_to_enemy - 30) * 2 } else { 0 };
                    let score = dist_to_me + too_close_penalty + too_far_penalty;
                    if best.is_none() || score < best.unwrap().0 {
                        best = Some((score, (nx, ny)));
                    }
                }
            }
        }

        best.map(|(_, p)| p)
    }

    fn explore(&self, _hero_index: usize) -> (i32, i32) {
        game::snap_to_grid((self.config.width / 2, self.config.height / 2))
    }
}

const GRID_DIRS: [(i32, i32); 8] = [
    (-3, 0), (3, 0), (0, -3), (0, 3),
    (-3, -3), (-3, 3), (3, -3), (3, 3),
];