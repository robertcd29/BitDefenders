use std::collections::{HashMap, HashSet, VecDeque};

use crate::protocol::{
    GameConfig, Hero, MoveArgs, ShootArgs, StartMatchArgs, StartTurnArgs, Wall,
};

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
            }
        }

        let walls = build_wall_set(&state.walls);

        let my_heroes: Vec<&Hero> = state.heroes.iter()
            .filter(|h| h.owner_id == self.my_player_id)
            .collect();

        let enemies: Vec<&Hero> = state.heroes.iter()
            .filter(|h| h.owner_id != self.my_player_id)
            .collect();

        println!(
            "  [turn {}] my={} enemies_visible={} last_seen={}",
            args.turn, my_heroes.len(), enemies.len(), self.last_seen.len()
        );
        for h in &my_heroes {
            println!("    MY  id={} pos=({},{}) hp={} cd={}", h.id, h.x, h.y, h.hp, h.cooldown);
        }
        for e in &enemies {
            println!("    ENE id={} pos=({},{}) hp={}", e.id, e.x, e.y, e.hp);
        }

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

        if hero.cooldown == 0 && !enemies.is_empty() {
            let mut sorted = enemies.to_vec();
            sorted.sort_by_key(|e| (e.hp, manhattan(pos, (e.x, e.y))));

            for enemy in &sorted {
                let tp = (enemy.x, enemy.y);
                if has_clear_shot(pos, tp, walls) {
                    println!("    hero={} SHOOT → ({},{})", hero.id, tp.0, tp.1);
                    return Order::Shoot(ShootArgs {
                        hero_id: hero.id,
                        x: enemy.x,
                        y: enemy.y,
                    });
                }
            }
        }

        let destination = self.pick_destination(hero, hero_index, enemies, pos);

        if pos == destination {
            return Order::Move(MoveArgs { hero_id: hero.id, x: hero.x, y: hero.y });
        }

        let next = bfs(pos, destination, &self.config, walls)
            .unwrap_or_else(|| greedy_step(pos, destination, &self.config, walls));

        println!("    hero={} MOVE ({},{}) → ({},{})", hero.id, pos.0, pos.1, next.0, next.1);
        Order::Move(MoveArgs {
            hero_id: hero.id,
            x: next.0,
            y: next.1,
        })
    }

    fn pick_destination(
        &self,
        _hero: &Hero,
        hero_index: usize,
        enemies: &[&Hero],
        pos: (i32, i32),
    ) -> (i32, i32) {
        if !enemies.is_empty() {
            let target = enemies[hero_index % enemies.len()];
            (target.x, target.y)
        } else if !self.last_seen.is_empty() {
            *self.last_seen.values()
                .min_by_key(|&&p| manhattan(pos, p))
                .unwrap()
        } else {
            self.explore_target(hero_index)
        }
    }

    fn explore_target(&self, hero_index: usize) -> (i32, i32) {
        let w = self.config.width;
        let h = self.config.height;
        let (y_enemy_base, _y_mid) = if self.my_player_id == 0 {
            (h - 2, h * 3 / 4)
        } else {
            (1, h / 4)
        };
        let targets = [
            snap(w / 4,     y_enemy_base),
            snap(w * 3 / 4, y_enemy_base),
        ];
        targets[hero_index % targets.len()]
    }
}

fn bfs(
    start: (i32, i32),
    goal: (i32, i32),
    config: &GameConfig,
    walls: &HashSet<(i32, i32)>,
) -> Option<(i32, i32)> {
    if start == goal {
        return None;
    }

    let mut queue: VecDeque<(i32, i32)> = VecDeque::new();
    let mut came_from: HashMap<(i32, i32), (i32, i32)> = HashMap::new();

    queue.push_back(start);
    came_from.insert(start, start);

    const DIRS: [(i32, i32); 8] = [
        (-3, 0), (3, 0), (0, -3), (0, 3),
        (-3, -3), (-3, 3), (3, -3), (3, 3),
    ];

    while let Some(cur) = queue.pop_front() {
        if cur == goal || chebyshev(cur, goal) <= 3 {
            return Some(first_step(start, cur, &came_from));
        }

        for (dx, dy) in &DIRS {
            let next = (cur.0 + dx, cur.1 + dy);
            if !in_bounds(next, config) { continue; }
            if next.0 % 3 != 1 || next.1 % 3 != 1 { continue; }
            if overlaps_wall(next, walls) { continue; }
            if came_from.contains_key(&next) { continue; }
            came_from.insert(next, cur);
            queue.push_back(next);
        }
    }

    None
}

fn first_step(
    start: (i32, i32),
    end: (i32, i32),
    came_from: &HashMap<(i32, i32), (i32, i32)>,
) -> (i32, i32) {
    let mut cur = end;
    loop {
        let prev = came_from[&cur];
        if prev == start {
            return cur;
        }
        cur = prev;
    }
}

fn greedy_step(
    pos: (i32, i32),
    dest: (i32, i32),
    config: &GameConfig,
    walls: &HashSet<(i32, i32)>,
) -> (i32, i32) {
    let sx = sign(dest.0 - pos.0);
    let sy = sign(dest.1 - pos.1);

    for (dx, dy) in move_candidates(sx, sy) {
        let next = (pos.0 + dx * 3, pos.1 + dy * 3);
        if !in_bounds(next, config) { continue; }
        if next.0 % 3 != 1 || next.1 % 3 != 1 { continue; }
        if overlaps_wall(next, walls) { continue; }
        return next;
    }

    pos
}

fn bresenham(start: (i32, i32), end: (i32, i32)) -> Vec<(i32, i32)> {
    let (mut x, mut y) = start;
    let (ex, ey) = end;
    let dx = (ex - x).abs();
    let dy = (ey - y).abs();
    let sx = if x < ex { 1 } else { -1 };
    let sy = if y < ey { 1 } else { -1 };
    let mut err = dx - dy;
    let mut pts = Vec::new();

    loop {
        pts.push((x, y));
        if x == ex && y == ey { break; }
        let e2 = 2 * err;
        if e2 > -dy { err -= dy; x += sx; }
        if e2 < dx  { err += dx; y += sy; }
    }
    pts
}

fn has_clear_shot(from: (i32, i32), to: (i32, i32), walls: &HashSet<(i32, i32)>) -> bool {
    let line = bresenham(from, to);
    let middle = line.iter().skip(1).take(line.len().saturating_sub(2));
    for point in middle {
        for (wx, wy) in walls {
            if (point.0 - wx).abs() <= 1 && (point.1 - wy).abs() <= 1 {
                return false;
            }
        }
    }
    true
}

fn build_wall_set(walls: &[Wall]) -> HashSet<(i32, i32)> {
    walls.iter().map(|w| (w.x, w.y)).collect()
}

fn overlaps_wall(pos: (i32, i32), walls: &HashSet<(i32, i32)>) -> bool {
    walls.iter().any(|(wx, wy)| (pos.0 - wx).abs() <= 2 && (pos.1 - wy).abs() <= 2)
}

fn in_bounds(pos: (i32, i32), config: &GameConfig) -> bool {
    pos.0 >= 1 && pos.1 >= 1 && pos.0 < config.width - 1 && pos.1 < config.height - 1
}

fn manhattan(a: (i32, i32), b: (i32, i32)) -> i32 {
    (a.0 - b.0).abs() + (a.1 - b.1).abs()
}

fn chebyshev(a: (i32, i32), b: (i32, i32)) -> i32 {
    (a.0 - b.0).abs().max((a.1 - b.1).abs())
}

fn sign(v: i32) -> i32 {
    if v > 0 { 1 } else if v < 0 { -1 } else { 0 }
}

fn snap(x: i32, y: i32) -> (i32, i32) {
    ((x / 3) * 3 + 1, (y / 3) * 3 + 1)
}

fn move_candidates(sx: i32, sy: i32) -> Vec<(i32, i32)> {
    let mut v = vec![(sx, sy)];
    if sx != 0 && sy != 0 {
        v.push((sx, 0));
        v.push((0, sy));
    } else if sx == 0 {
        v.push((1, sy)); v.push((-1, sy));
    } else {
        v.push((sx, 1)); v.push((sx, -1));
    }
    for dx in [-1i32, 0, 1] {
        for dy in [-1i32, 0, 1] {
            if dx == 0 && dy == 0 { continue; }
            if !v.contains(&(dx, dy)) { v.push((dx, dy)); }
        }
    }
    v
}