use std::collections::{HashMap, HashSet, BinaryHeap};
use std::cmp::Reverse;

pub fn build_wall_set(walls: &[crate::protocol::Wall]) -> HashSet<(i32, i32)> {
    walls.iter().map(|w| (w.x, w.y)).collect()
}

pub fn overlaps_wall(cx: i32, cy: i32, walls: &HashSet<(i32, i32)>) -> bool {
    walls.iter().any(|(wx, wy)| (cx - wx).abs() <= 1 && (cy - wy).abs() <= 1)
}

pub fn snap_to_grid(p: (i32, i32)) -> (i32, i32) {
    let snap = |v: i32| (v.div_euclid(3)) * 3 + 1;
    (snap(p.0), snap(p.1))
}

const DIRS: [(i32, i32); 8] = [
    (-3, 0), (3, 0), (0, -3), (0, 3),
    (-3, -3), (-3, 3), (3, -3), (3, 3),
];

#[inline]
pub fn in_bounds(nx: i32, ny: i32, width: i32, height: i32) -> bool {
    nx >= 1 && ny >= 1 && nx < width - 1 && ny < height - 1
}

#[inline]
pub fn on_grid(nx: i32, ny: i32) -> bool {
    nx % 3 == 1 && ny % 3 == 1
}

pub fn astar_next_step_safe(
    start: (i32, i32),
    goal: (i32, i32),
    width: i32,
    height: i32,
    walls: &HashSet<(i32, i32)>,
) -> Option<(i32, i32)> {
    if start == goal {
        return None;
    }

    let goal_snapped = snap_to_grid(goal);
    let mut heap: BinaryHeap<Reverse<(i32, (i32, i32))>> = BinaryHeap::new();
    let mut came_from: HashMap<(i32, i32), (i32, i32)> = HashMap::new();
    let mut g_score: HashMap<(i32, i32), i32> = HashMap::new();

    heap.push(Reverse((0, start)));
    came_from.insert(start, start);
    g_score.insert(start, 0);

    while let Some(Reverse((_, (x, y)))) = heap.pop() {
        if (x - goal_snapped.0).abs() <= 3 && (y - goal_snapped.1).abs() <= 3 {
            let mut cur = (x, y);
            loop {
                let prev = came_from[&cur];
                if prev == start {
                    return Some(cur);
                }
                cur = prev;
            }
        }

        let g = *g_score.get(&(x, y)).unwrap_or(&i32::MAX);

        for (dx, dy) in &DIRS {
            let nx = x + dx;
            let ny = y + dy;

            if !in_bounds(nx, ny, width, height) { continue; }
            if !on_grid(nx, ny) { continue; }
            if overlaps_wall(nx, ny, walls) { continue; }

            let next = (nx, ny);
            let new_g = g + 1;

            if new_g < *g_score.get(&next).unwrap_or(&i32::MAX) {
                g_score.insert(next, new_g);
                came_from.insert(next, (x, y));
                let h = chebyshev(next, goal_snapped);
                heap.push(Reverse((new_g + h, next)));
            }
        }
    }

    None
}

/// Găsește cel mai apropiat punct care RUPE linia de vizibilitate cu inamicul.
/// Perfect pentru a te ascunde cât timp ești pe cooldown.
pub fn find_hideout(
    my_pos: (i32, i32),
    enemy_pos: (i32, i32),
    walls: &HashSet<(i32, i32)>,
    width: i32,
    height: i32,
) -> Option<(i32, i32)> {
    let mut best_hideout: Option<(i32, (i32, i32))> = None;

    // Căutăm într-o rază locală pentru viteză (ex: 15 unități distanță)
    for dx in (-15..=15).step_by(3) {
        for dy in (-15..=15).step_by(3) {
            let nx = my_pos.0 + dx;
            let ny = my_pos.1 + dy;

            if !in_bounds(nx, ny, width, height) || !on_grid(nx, ny) || overlaps_wall(nx, ny, walls) {
                continue;
            }

            // Dacă inamicul NU ne poate vedea din această poziție, e un hideout valid
            if !has_clear_shot((nx, ny), enemy_pos, walls) {
                let dist = manhattan(my_pos, (nx, ny));
                if best_hideout.is_none() || dist < best_hideout.unwrap().0 {
                    best_hideout = Some((dist, (nx, ny)));
                }
            }
        }
    }

    best_hideout.map(|(_, p)| p)
}

pub fn manhattan(a: (i32, i32), b: (i32, i32)) -> i32 {
    (a.0 - b.0).abs() + (a.1 - b.1).abs()
}

pub fn chebyshev(a: (i32, i32), b: (i32, i32)) -> i32 {
    (a.0 - b.0).abs().max((a.1 - b.1).abs())
}

pub fn bresenham(start: (i32, i32), end: (i32, i32)) -> Vec<(i32, i32)> {
    let (mut x0, mut y0) = start;
    let (x1, y1) = end;
    let dx = (x1 - x0).abs();
    let dy = (y1 - y0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx - dy;
    let mut points = Vec::new();

    loop {
        points.push((x0, y0));
        if x0 == x1 && y0 == y1 { break; }
        let e2 = 2 * err;
        if e2 > -dy { err -= dy; x0 += sx; }
        if e2 < dx  { err += dx; y0 += sy; }
    }

    points
}

pub fn has_clear_shot(from: (i32, i32), to: (i32, i32), walls: &HashSet<(i32, i32)>) -> bool {
    let line = bresenham(from, to);
    let len = line.len();
    if len <= 2 {
        return true;
    }
    let interior = &line[1..len - 1];
    for &(tx, ty) in interior {
        for (wx, wy) in walls {
            if (tx - wx).abs() <= 1 && (ty - wy).abs() <= 1 {
                return false;
            }
        }
    }
    true
}