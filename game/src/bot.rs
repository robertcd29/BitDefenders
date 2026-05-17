use std::collections::{HashMap, HashSet};

use crate::protocol::{
    GameConfig, Hero, MoveArgs, ShootArgs, StartMatchArgs, StartTurnArgs,
};
use crate::game;

pub enum Order {
    Move(MoveArgs),
    Shoot(ShootArgs),
}

pub struct Bot {
    my_player_id: i32,
    config: GameConfig,
    last_seen: HashMap<i32, (i32, i32)>,
}

impl Bot {
    pub fn new(args: StartMatchArgs) -> Self {
        Self {
            my_player_id: args.your_player_id,
            config: args.config,
            last_seen: HashMap::new(),
        }
    }

    pub fn take_turn(&mut self, args: StartTurnArgs) -> Vec<Order> {
        let state = &args.state;

        for hero in &state.heroes {
            if hero.owner_id != self.my_player_id {
                self.last_seen.insert(hero.id, (hero.x, hero.y));
            } else {
                self.last_seen.remove(&hero.id);
            }
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
                    comment: Some("Shoot".into()),
                });
            }
        }

        let destination = self.pick_destination(hero, hero_index, enemies, walls);

        if pos == destination {
            return Order::Move(MoveArgs { hero_id: hero.id, x: hero.x, y: hero.y, comment: Some("Move".into()) });
        }

        let next = game::astar_next_step_safe(
            pos,
            destination,
            self.config.width,
            self.config.height,
            walls,
        ).unwrap_or(pos);

        Order::Move(MoveArgs {
            hero_id: hero.id,
            x: next.0,
            y: next.1,
            comment: Some("Move".into())
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

        let focus = enemies.iter()
            .min_by_key(|e| e.hp)
            .map(|e| (e.x, e.y))
            .or_else(|| {
                self.last_seen.values()
                    .min_by_key(|&&p| game::manhattan(pos, p))
                    .copied()
            });

        if let Some(epos) = focus {
            if hero.cooldown > 0 {
                if game::has_clear_shot(pos, epos, walls) || game::manhattan(pos, epos) < 12 {
                    if let Some(hideout) = game::find_hideout(pos, epos, walls, self.config.width, self.config.height) {
                        return hideout;
                    }
                }
                return pos;
            } else {
                if let Some(peek) = self.find_peek_spot(pos, epos, walls) {
                    return peek;
                }
                return epos;
            }
        }

        self.explore(hero_index)
    }

    fn find_peek_spot(&self, my_pos: (i32, i32), enemy_pos: (i32, i32), walls: &HashSet<(i32, i32)>) -> Option<(i32, i32)> {
        let mut best_spot = None;
        let mut best_dist = i32::MAX;

        for dx in (-18..=18).step_by(3) {
            for dy in (-18..=18).step_by(3) {
                let nx = my_pos.0 + dx;
                let ny = my_pos.1 + dy;

                if !game::in_bounds(nx, ny, self.config.width, self.config.height) || !game::on_grid(nx, ny) || game::overlaps_wall(nx, ny, walls) {
                    continue;
                }

                if game::has_clear_shot((nx, ny), enemy_pos, walls) {
                    let dist = game::manhattan(my_pos, (nx, ny));
                    if dist < best_dist {
                        best_dist = dist;
                        best_spot = Some((nx, ny));
                    }
                }
            }
        }
        best_spot
    }

    fn explore(&self, hero_index: usize) -> (i32, i32) {
        let w = self.config.width;
        let h = self.config.height;
        let y_mid = h / 2;
        let x_target = if hero_index == 0 { w / 4 } else { w * 3 / 4 };
        game::snap_to_grid((x_target, y_mid))
    }
}