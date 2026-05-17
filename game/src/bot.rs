use std::collections::{HashMap, HashSet};

use crate::protocol::{GameConfig, Hero, MoveArgs, ShootArgs, StartMatchArgs, StartTurnArgs};
use crate::game;

pub enum Order {
    Move(MoveArgs),
    Shoot(ShootArgs),
}

pub struct Bot {
    my_player_id: i32,
    config: GameConfig,
    last_seen: HashMap<i32, (i32, i32)>,
    turn: i32,
}

impl Bot {
    pub fn new(args: StartMatchArgs) -> Self {
        Self {
            my_player_id: args.your_player_id,
            config: args.config,
            last_seen: HashMap::new(),
            turn: 0,
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

        my_heroes.iter().enumerate().map(|(i, hero)| {
            self.decide(hero, i, &enemies, &walls)
        }).collect()
    }

    fn decide(
        &self,
        hero: &Hero,
        hero_index: usize,
        enemies: &[&Hero],
        walls: &HashSet<(i32, i32)>,
    ) -> Order {
        let pos = (hero.x, hero.y);

        if hero.cooldown == 0 {
            let target = enemies.iter()
                .filter(|e| game::has_clear_shot(pos, (e.x, e.y), walls))
                .min_by_key(|e| e.hp);

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
            comment: "Moving".to_string(),
        })
    }

    fn pick_destination(
        &self,
        hero: &Hero,
        hero_index: usize,
        enemies: &[&Hero],
        walls: &HashSet<(i32, i32)>,
    ) -> (i32, i32) {
        let pos = (hero.x, hero.y);

        let mut sorted_enemies: Vec<&Hero> = enemies.to_vec();
        sorted_enemies.sort_by_key(|e| e.hp);

        let epos: (i32, i32) = if !sorted_enemies.is_empty() {
            let idx = hero_index.min(sorted_enemies.len() - 1);
            (sorted_enemies[idx].x, sorted_enemies[idx].y)
        } else {
            if let Some(&lp) = self.last_seen.values()
                .min_by_key(|&&p| game::manhattan(pos, p))
            {
                lp
            } else {
                return self.explore(hero_index);
            }
        };

        let my_max_hp = self.config.hero_types.get("sniper")
            .map(|s| s.max_hp)
            .unwrap_or(3000);
        let low_hp = hero.hp <= my_max_hp / 3;

        let enemy_can_see_us = enemies.iter()
            .any(|e| game::has_clear_shot((e.x, e.y), pos, walls));

        if low_hp && enemy_can_see_us {
            if let Some(h) = game::find_hideout(pos, epos, walls, self.config.width, self.config.height) {
                return h;
            }
            return self.strafe(pos, epos, walls, false);
        }

        if hero.cooldown == 0 {
            if game::has_clear_shot(pos, epos, walls) {
                let dist = game::manhattan(pos, epos);
                if dist >= 6 && dist <= 24 {
                    return pos;
                }
                if dist < 6 {
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
                    let penalty = if dist_to_enemy < 6 { 500 } else { 0 };
                    let score = dist_to_me + penalty;
                    if best.is_none() || score < best.unwrap().0 {
                        best = Some((score, (nx, ny)));
                    }
                }
            }
        }

        best.map(|(_, p)| p)
    }

    fn explore(&self, hero_index: usize) -> (i32, i32) {
        let w = self.config.width;
        let h = self.config.height;
        let quadrant = (self.turn / 8 + hero_index as i32) % 4;
        let target = match quadrant {
            0 => (w / 4, h / 4),
            1 => (w * 3 / 4, h / 4),
            2 => (w / 4, h * 3 / 4),
            _ => (w * 3 / 4, h * 3 / 4),
        };
        game::snap_to_grid(target)
    }
}

const GRID_DIRS: [(i32, i32); 8] = [
    (-3, 0), (3, 0), (0, -3), (0, 3),
    (-3, -3), (-3, 3), (3, -3), (3, 3),
];