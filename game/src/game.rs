use std::collections::{HashMap, HashSet, VecDeque};

pub fn build_wall_set(walls: &[crate::protocol::Wall]) -> HashSet<(i32, i32)> {
    walls.iter().map(|w| (w.x, w.y)).collect()
}

pub fn overlaps_wall(cx: i32, cy: i32, walls: &HashSet<(i32, i32)>) -> bool {
    walls.iter().any(|(wx, wy)| (cx - wx).abs() <= 2 && (cy - wy).abs() <= 2)
}

pub fn bfs_next_step(
    start: (i32, i32),
    goal: (i32, i32),
    width: i32,
    height: i32,
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

    while let Some((x, y)) = queue.pop_front() {
        if (x - goal.0).abs() <= 3 && (y - goal.1).abs() <= 3 {
            let mut cur = (x, y);
            loop {
                let prev = came_from[&cur];
                if prev == start {
                    return Some(cur);
                }
                cur = prev;
            }
        }

        for (dx, dy) in &DIRS {
            let nx = x + dx;
            let ny = y + dy;

            if nx < 1 || ny < 1 || nx >= width - 1 || ny >= height - 1 { continue; }
            if nx % 3 != 1 || ny % 3 != 1 { continue; }

            let next = (nx, ny);
            if !overlaps_wall(nx, ny, walls) && !came_from.contains_key(&next) {
                came_from.insert(next, (x, y));
                queue.push_back(next);
            }
        }
    }

    None
}

pub fn manhattan(a: (i32, i32), b: (i32, i32)) -> i32 {
    (a.0 - b.0).abs() + (a.1 - b.1).abs()
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
    for &(tx, ty) in line.iter().skip(1).rev().skip(1).rev() {
        for (wx, wy) in walls {
            if (tx - wx).abs() <= 1 && (ty - wy).abs() <= 1 {
                return false;
            }
        }
    }
    true
}

pub fn chebyshev(a: (i32, i32), b: (i32, i32)) -> i32 {
    (a.0 - b.0).abs().max((a.1 - b.1).abs())
}