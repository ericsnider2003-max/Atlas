//! 3-D models from files: OBJ (with its MTL colours), STL, and glTF 2.0
//! (`.gltf` with its buffers, or a single `.glb`) — read in house, and made
//! fast to hit with a bounding volume hierarchy.
//!
//! **Sources:**
//! - Wavefront OBJ and MTL (Library of Congress format descriptions; Paul
//!   Bourke's copy of the Wavefront spec): `v`, `vn`, `f` with `v/vt/vn` and
//!   negative indices, polygons fanned into triangles, `mtllib`/`usemtl`/`Kd`.
//! - STL: 3D Systems' format — binary (80-byte header, count, 50 bytes a
//!   facet) or ASCII (`facet … vertex`).
//! - glTF 2.0 (Khronos): scenes → nodes (matrix or translation/rotation/scale,
//!   children) → meshes → primitives (mode 4, triangles) → accessors →
//!   buffer views → buffers (a file, a base64 `data:` URI, or the GLB's BIN
//!   chunk); `pbrMetallicRoughness.baseColorFactor` for colour.
//! - Möller & Trumbore (1997) for ray–triangle intersection; a BVH split by
//!   the surface area heuristic over 12 bins (Wald 2007; *PBRT* 4ed §7.3).
//!
//! Nothing here draws: `scene3d` places a [`Mesh`] like any other shape.

use std::path::{Path, PathBuf};

pub type V = [f64; 3];

fn sub(a: V, b: V) -> V {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn add(a: V, b: V) -> V {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn mul(a: V, k: f64) -> V {
    [a[0] * k, a[1] * k, a[2] * k]
}
fn dot(a: V, b: V) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: V, b: V) -> V {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn unit(a: V) -> V {
    let l = dot(a, a).sqrt();
    if l == 0.0 {
        a
    } else {
        mul(a, 1.0 / l)
    }
}

/// A triangle: corners, and a normal at each corner when the file gave them.
#[derive(Debug, Clone, PartialEq)]
pub struct Tri {
    pub p: [V; 3],
    pub n: Option<[V; 3]>,
    /// Index into [`Mesh::colours`], when the file said what it's made of.
    pub colour: Option<usize>,
}

/// A model: its triangles, the colours the file gave them (linear RGB), and a
/// BVH over them.
#[derive(Debug, Clone)]
pub struct Mesh {
    pub tris: Vec<Tri>,
    pub colours: Vec<V>,
    nodes: Vec<Node>,
    order: Vec<u32>,
    pub lo: V,
    pub hi: V,
}

#[derive(Debug, Clone, Copy)]
struct Node {
    lo: V,
    hi: V,
    /// Leaf: first index into `order` and count. Inner: left child at
    /// `start`, right child at `start + 1`… no — left is `self + 1`, right
    /// is `start`.
    start: u32,
    count: u32,
}

/// Where a ray met a model.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MeshHit {
    pub t: f64,
    pub normal: V,
    pub colour: Option<usize>,
}

impl Mesh {
    /// Build from triangles: bounds and the BVH.
    pub fn new(tris: Vec<Tri>, colours: Vec<V>) -> Result<Mesh, String> {
        let tris: Vec<Tri> = tris
            .into_iter()
            .filter(|t| t.p.iter().all(|v| v.iter().all(|c| c.is_finite())) && dot(cross(sub(t.p[1], t.p[0]), sub(t.p[2], t.p[0])), cross(sub(t.p[1], t.p[0]), sub(t.p[2], t.p[0]))) > 0.0)
            .collect();
        if tris.is_empty() {
            return Err("the model has no triangles".into());
        }
        let mut lo = [f64::MAX; 3];
        let mut hi = [f64::MIN; 3];
        for t in &tris {
            for v in &t.p {
                for k in 0..3 {
                    lo[k] = lo[k].min(v[k]);
                    hi[k] = hi[k].max(v[k]);
                }
            }
        }
        let mut m = Mesh { tris, colours, nodes: Vec::new(), order: Vec::new(), lo, hi };
        m.build();
        Ok(m)
    }

    fn build(&mut self) {
        let n = self.tris.len();
        let boxes: Vec<(V, V, V)> = self
            .tris
            .iter()
            .map(|t| {
                let mut lo = t.p[0];
                let mut hi = t.p[0];
                for v in &t.p[1..] {
                    for k in 0..3 {
                        lo[k] = lo[k].min(v[k]);
                        hi[k] = hi[k].max(v[k]);
                    }
                }
                (lo, hi, mul(add(lo, hi), 0.5))
            })
            .collect();
        self.order = (0..n as u32).collect();
        self.nodes.clear();
        self.nodes.push(Node { lo: [0.0; 3], hi: [0.0; 3], start: 0, count: 0 });
        // (node index, range start, range end)
        let mut stack = vec![(0usize, 0usize, n)];
        while let Some((ni, a, b)) = stack.pop() {
            let mut lo = [f64::MAX; 3];
            let mut hi = [f64::MIN; 3];
            let mut clo = [f64::MAX; 3];
            let mut chi = [f64::MIN; 3];
            for &i in &self.order[a..b] {
                let (l, h, c) = boxes[i as usize];
                for k in 0..3 {
                    lo[k] = lo[k].min(l[k]);
                    hi[k] = hi[k].max(h[k]);
                    clo[k] = clo[k].min(c[k]);
                    chi[k] = chi[k].max(c[k]);
                }
            }
            let count = b - a;
            let leaf = |nodes: &mut Vec<Node>| nodes[ni] = Node { lo, hi, start: a as u32, count: count as u32 };
            if count <= 4 {
                leaf(&mut self.nodes);
                continue;
            }
            // Surface area heuristic over 12 bins on the widest centroid axis.
            let axis = (0..3).max_by(|&x, &y| (chi[x] - clo[x]).total_cmp(&(chi[y] - clo[y]))).unwrap_or(0);
            let span = chi[axis] - clo[axis];
            if span <= 1e-12 {
                leaf(&mut self.nodes);
                continue;
            }
            const BINS: usize = 12;
            let bin_of = |c: f64| (((c - clo[axis]) / span * BINS as f64) as usize).min(BINS - 1);
            let mut bcount = [0usize; BINS];
            let mut blo = [[f64::MAX; 3]; BINS];
            let mut bhi = [[f64::MIN; 3]; BINS];
            for &i in &self.order[a..b] {
                let (l, h, c) = boxes[i as usize];
                let bi = bin_of(c[axis]);
                bcount[bi] += 1;
                for k in 0..3 {
                    blo[bi][k] = blo[bi][k].min(l[k]);
                    bhi[bi][k] = bhi[bi][k].max(h[k]);
                }
            }
            let area = |l: V, h: V| {
                let d = sub(h, l);
                if d[0] < 0.0 {
                    0.0
                } else {
                    d[0] * d[1] + d[1] * d[2] + d[2] * d[0]
                }
            };
            let mut best = (f64::MAX, 0usize);
            for split in 1..BINS {
                let (mut l1, mut h1, mut n1) = ([f64::MAX; 3], [f64::MIN; 3], 0);
                let (mut l2, mut h2, mut n2) = ([f64::MAX; 3], [f64::MIN; 3], 0);
                for bi in 0..BINS {
                    if bcount[bi] == 0 {
                        continue;
                    }
                    let (l, h, nn) = if bi < split { (&mut l1, &mut h1, &mut n1) } else { (&mut l2, &mut h2, &mut n2) };
                    for k in 0..3 {
                        l[k] = l[k].min(blo[bi][k]);
                        h[k] = h[k].max(bhi[bi][k]);
                    }
                    *nn += bcount[bi];
                }
                if n1 == 0 || n2 == 0 {
                    continue;
                }
                let cost = area(l1, h1) * n1 as f64 + area(l2, h2) * n2 as f64;
                if cost < best.0 {
                    best = (cost, split);
                }
            }
            if best.0 == f64::MAX {
                leaf(&mut self.nodes);
                continue;
            }
            // Partition this range by the chosen bin.
            let slice = &mut self.order[a..b];
            let mut i = 0;
            let mut j = slice.len();
            while i < j {
                if bin_of(boxes[slice[i] as usize].2[axis]) < best.1 {
                    i += 1;
                } else {
                    j -= 1;
                    slice.swap(i, j);
                }
            }
            let mid = a + i;
            let left = self.nodes.len();
            self.nodes.push(Node { lo: [0.0; 3], hi: [0.0; 3], start: 0, count: 0 });
            let right = self.nodes.len();
            self.nodes.push(Node { lo: [0.0; 3], hi: [0.0; 3], start: 0, count: 0 });
            self.nodes[ni] = Node { lo, hi, start: left as u32, count: 0 };
            debug_assert_eq!(right, left + 1);
            stack.push((left, a, mid));
            stack.push((right, mid, b));
        }
    }

    /// The nearest hit along a ray, beyond `EPS` and before `far`.
    pub fn hit(&self, o: V, d: V, far: f64) -> Option<MeshHit> {
        const EPS: f64 = 1e-7;
        let inv = [1.0 / d[0], 1.0 / d[1], 1.0 / d[2]];
        let slab = |lo: V, hi: V, far: f64| -> Option<f64> {
            let mut t0 = 0.0f64;
            let mut t1 = far;
            for k in 0..3 {
                let (mut a, mut b) = ((lo[k] - o[k]) * inv[k], (hi[k] - o[k]) * inv[k]);
                if a > b {
                    std::mem::swap(&mut a, &mut b);
                }
                t0 = t0.max(a);
                t1 = t1.min(b);
                if t0 > t1 {
                    return None;
                }
            }
            Some(t0)
        };
        let mut best: Option<MeshHit> = None;
        let mut limit = far;
        let mut stack: Vec<u32> = Vec::with_capacity(64);
        stack.push(0);
        while let Some(ni) = stack.pop() {
            let node = self.nodes[ni as usize];
            if slab(node.lo, node.hi, limit).is_none() {
                continue;
            }
            if node.count > 0 {
                for &ti in &self.order[node.start as usize..(node.start + node.count) as usize] {
                    let t = &self.tris[ti as usize];
                    // Möller–Trumbore.
                    let e1 = sub(t.p[1], t.p[0]);
                    let e2 = sub(t.p[2], t.p[0]);
                    let pv = cross(d, e2);
                    let det = dot(e1, pv);
                    if det.abs() < 1e-14 {
                        continue;
                    }
                    let id = 1.0 / det;
                    let tv = sub(o, t.p[0]);
                    let u = dot(tv, pv) * id;
                    if !(0.0..=1.0).contains(&u) {
                        continue;
                    }
                    let qv = cross(tv, e1);
                    let v = dot(d, qv) * id;
                    if v < 0.0 || u + v > 1.0 {
                        continue;
                    }
                    let tt = dot(e2, qv) * id;
                    if tt > EPS && tt < limit {
                        limit = tt;
                        let normal = match &t.n {
                            Some(n) => unit(add(add(mul(n[0], 1.0 - u - v), mul(n[1], u)), mul(n[2], v))),
                            None => unit(cross(e1, e2)),
                        };
                        best = Some(MeshHit { t: tt, normal, colour: t.colour });
                    }
                }
            } else {
                // Visit the nearer child first.
                let (l, r) = (node.start, node.start + 1);
                let (nl, nr) = (self.nodes[l as usize], self.nodes[r as usize]);
                let (dl, dr) = (slab(nl.lo, nl.hi, limit), slab(nr.lo, nr.hi, limit));
                match (dl, dr) {
                    (Some(a), Some(b)) if a <= b => {
                        stack.push(r);
                        stack.push(l);
                    }
                    (Some(_), Some(_)) => {
                        stack.push(l);
                        stack.push(r);
                    }
                    (Some(_), None) => stack.push(l),
                    (None, Some(_)) => stack.push(r),
                    (None, None) => {}
                }
            }
        }
        best
    }
}

// --------------------------------------------------------------------- OBJ

/// Read an OBJ (and the MTL it names, from beside it).
pub fn read_obj(text: &str, dir: Option<&Path>) -> Result<Mesh, String> {
    let mut vs: Vec<V> = Vec::new();
    let mut ns: Vec<V> = Vec::new();
    let mut tris = Vec::new();
    let mut colours: Vec<V> = Vec::new();
    let mut named: Vec<(String, usize)> = Vec::new();
    let mut current: Option<usize> = None;
    let num = |s: &str| s.parse::<f64>().map_err(|_| format!("\"{s}\" isn't a number"));
    let index = |s: &str, len: usize| -> Result<usize, String> {
        let i: i64 = s.parse().map_err(|_| format!("\"{s}\" isn't an index"))?;
        let k = if i < 0 { len as i64 + i } else { i - 1 };
        if k < 0 || k as usize >= len {
            return Err(format!("index {i} points past the {len} there are"));
        }
        Ok(k as usize)
    };
    for (line_no, raw) in text.lines().enumerate() {
        let line = raw.split('#').next().unwrap_or("").trim();
        let mut parts = line.split_whitespace();
        let Some(tag) = parts.next() else { continue };
        let rest: Vec<&str> = parts.collect();
        let at = |e: String| format!("line {}: {e}", line_no + 1);
        match tag {
            "v" if rest.len() >= 3 => vs.push([num(rest[0]).map_err(at)?, num(rest[1]).map_err(at)?, num(rest[2]).map_err(at)?]),
            "vn" if rest.len() >= 3 => ns.push(unit([num(rest[0]).map_err(at)?, num(rest[1]).map_err(at)?, num(rest[2]).map_err(at)?])),
            "f" if rest.len() >= 3 => {
                let mut corners: Vec<(usize, Option<usize>)> = Vec::new();
                for c in &rest {
                    let mut bits = c.split('/');
                    let vi = index(bits.next().unwrap_or(""), vs.len()).map_err(at)?;
                    let _vt = bits.next();
                    let ni = match bits.next() {
                        Some(s) if !s.is_empty() => Some(index(s, ns.len()).map_err(at)?),
                        _ => None,
                    };
                    corners.push((vi, ni));
                }
                for k in 1..corners.len() - 1 {
                    let c = [corners[0], corners[k], corners[k + 1]];
                    let n = match (c[0].1, c[1].1, c[2].1) {
                        (Some(a), Some(b), Some(d)) => Some([ns[a], ns[b], ns[d]]),
                        _ => None,
                    };
                    tris.push(Tri { p: [vs[c[0].0], vs[c[1].0], vs[c[2].0]], n, colour: current });
                }
            }
            "mtllib" => {
                if let Some(dir) = dir {
                    let file = dir.join(rest.join(" "));
                    if let Ok(mtl) = std::fs::read_to_string(&file) {
                        let mut name: Option<String> = None;
                        for l in mtl.lines() {
                            let l = l.trim();
                            if let Some(n) = l.strip_prefix("newmtl ") {
                                name = Some(n.trim().to_string());
                            } else if let (Some(n), Some(kd)) = (&name, l.strip_prefix("Kd ")) {
                                let f: Vec<f64> = kd.split_whitespace().filter_map(|x| x.parse().ok()).collect();
                                if f.len() >= 3 {
                                    // Taken as linear, as Blender writes and reads them
                                    // (its exporter puts the base colour in Kd as it is).
                                    colours.push([f[0], f[1], f[2]]);
                                    named.push((n.clone(), colours.len() - 1));
                                }
                            }
                        }
                    }
                }
            }
            "usemtl" => {
                let n = rest.join(" ");
                current = named.iter().find(|(m, _)| *m == n).map(|(_, i)| *i);
            }
            _ => {}
        }
    }
    Mesh::new(tris, colours)
}

// --------------------------------------------------------------------- STL

/// Read an STL, binary or ASCII.
pub fn read_stl(bytes: &[u8]) -> Result<Mesh, String> {
    let le32 = |i: usize| -> Option<u32> { bytes.get(i..i + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]])) };
    let f32at = |i: usize| -> f64 { f32::from_le_bytes([bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]]) as f64 };
    if let Some(n) = le32(80) {
        if bytes.len() == 84 + 50 * n as usize && n > 0 {
            let mut tris = Vec::with_capacity(n as usize);
            for k in 0..n as usize {
                let at = 84 + 50 * k;
                let p = |j: usize| [f32at(at + 12 + 12 * j), f32at(at + 16 + 12 * j), f32at(at + 20 + 12 * j)];
                tris.push(Tri { p: [p(0), p(1), p(2)], n: None, colour: None });
            }
            return Mesh::new(tris, Vec::new());
        }
    }
    let text = std::str::from_utf8(bytes).map_err(|_| "that isn't an STL (neither binary nor text)".to_string())?;
    let mut tris = Vec::new();
    let mut corners: Vec<V> = Vec::new();
    for line in text.lines() {
        let mut w = line.split_whitespace();
        if w.next() == Some("vertex") {
            let f: Vec<f64> = w.filter_map(|x| x.parse().ok()).collect();
            if f.len() == 3 {
                corners.push([f[0], f[1], f[2]]);
            }
            if corners.len() == 3 {
                tris.push(Tri { p: [corners[0], corners[1], corners[2]], n: None, colour: None });
                corners.clear();
            }
        }
    }
    Mesh::new(tris, Vec::new())
}

// -------------------------------------------------------------------- glTF

type M4 = [[f64; 4]; 4];

fn m4_identity() -> M4 {
    [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]]
}

fn m4_mul(a: &M4, b: &M4) -> M4 {
    let mut r = [[0.0; 4]; 4];
    for i in 0..4 {
        for j in 0..4 {
            r[i][j] = (0..4).map(|k| a[i][k] * b[k][j]).sum();
        }
    }
    r
}

fn m4_point(m: &M4, p: V) -> V {
    [
        m[0][0] * p[0] + m[0][1] * p[1] + m[0][2] * p[2] + m[0][3],
        m[1][0] * p[0] + m[1][1] * p[1] + m[1][2] * p[2] + m[1][3],
        m[2][0] * p[0] + m[2][1] * p[1] + m[2][2] * p[2] + m[2][3],
    ]
}

/// Normals go through the inverse transpose; for rotation and uniform or
/// axis scale, the cofactor matrix is the same direction and needs no inverse.
fn m4_normal(m: &M4, n: V) -> V {
    let a = [[m[0][0], m[0][1], m[0][2]], [m[1][0], m[1][1], m[1][2]], [m[2][0], m[2][1], m[2][2]]];
    let c = [
        [a[1][1] * a[2][2] - a[1][2] * a[2][1], a[1][2] * a[2][0] - a[1][0] * a[2][2], a[1][0] * a[2][1] - a[1][1] * a[2][0]],
        [a[0][2] * a[2][1] - a[0][1] * a[2][2], a[0][0] * a[2][2] - a[0][2] * a[2][0], a[0][1] * a[2][0] - a[0][0] * a[2][1]],
        [a[0][1] * a[1][2] - a[0][2] * a[1][1], a[0][2] * a[1][0] - a[0][0] * a[1][2], a[0][0] * a[1][1] - a[0][1] * a[1][0]],
    ];
    unit([dot(c[0], n), dot(c[1], n), dot(c[2], n)])
}

/// A glTF node's own transform: `matrix` (column-major) or T·R·S.
fn node_matrix(node: &serde_json::Value) -> M4 {
    if let Some(m) = node.get("matrix").and_then(|m| m.as_array()) {
        let f: Vec<f64> = m.iter().filter_map(|x| x.as_f64()).collect();
        if f.len() == 16 {
            let mut r = [[0.0; 4]; 4];
            for c in 0..4 {
                for row in 0..4 {
                    r[row][c] = f[c * 4 + row];
                }
            }
            return r;
        }
    }
    let arr = |k: &str, n: usize, d: &[f64]| -> Vec<f64> {
        node.get(k)
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|x| x.as_f64()).collect::<Vec<_>>())
            .filter(|v| v.len() == n)
            .unwrap_or_else(|| d.to_vec())
    };
    let t = arr("translation", 3, &[0.0, 0.0, 0.0]);
    let q = arr("rotation", 4, &[0.0, 0.0, 0.0, 1.0]);
    let s = arr("scale", 3, &[1.0, 1.0, 1.0]);
    let (x, y, z, w) = (q[0], q[1], q[2], q[3]);
    let r = [
        [1.0 - 2.0 * (y * y + z * z), 2.0 * (x * y - z * w), 2.0 * (x * z + y * w)],
        [2.0 * (x * y + z * w), 1.0 - 2.0 * (x * x + z * z), 2.0 * (y * z - x * w)],
        [2.0 * (x * z - y * w), 2.0 * (y * z + x * w), 1.0 - 2.0 * (x * x + y * y)],
    ];
    [
        [r[0][0] * s[0], r[0][1] * s[1], r[0][2] * s[2], t[0]],
        [r[1][0] * s[0], r[1][1] * s[1], r[1][2] * s[2], t[1]],
        [r[2][0] * s[0], r[2][1] * s[1], r[2][2] * s[2], t[2]],
        [0.0, 0.0, 0.0, 1.0],
    ]
}

struct Gltf {
    json: serde_json::Value,
    buffers: Vec<Vec<u8>>,
}

impl Gltf {
    fn accessor(&self, i: usize) -> Result<(Vec<Vec<f64>>, usize), String> {
        let a = &self.json["accessors"][i];
        let count = a["count"].as_u64().ok_or("an accessor has no count")? as usize;
        let comps = match a["type"].as_str().unwrap_or("") {
            "SCALAR" => 1,
            "VEC2" => 2,
            "VEC3" => 3,
            "VEC4" => 4,
            t => return Err(format!("accessor type {t} isn't read here")),
        };
        let ct = a["componentType"].as_u64().unwrap_or(0);
        let size = match ct {
            5126 | 5125 => 4,
            5123 | 5122 => 2,
            5121 | 5120 => 1,
            _ => return Err(format!("component type {ct} isn't read here")),
        };
        let bv_i = a["bufferView"].as_u64().ok_or("sparse or empty accessors aren't read here")? as usize;
        let bv = &self.json["bufferViews"][bv_i];
        let buf = self.buffers.get(bv["buffer"].as_u64().unwrap_or(0) as usize).ok_or("a buffer view points at no buffer")?;
        let start = bv["byteOffset"].as_u64().unwrap_or(0) as usize + a["byteOffset"].as_u64().unwrap_or(0) as usize;
        let stride = bv["byteStride"].as_u64().map(|s| s as usize).unwrap_or(comps * size);
        let normalized = a["normalized"].as_bool().unwrap_or(false);
        let mut out = Vec::with_capacity(count);
        for k in 0..count {
            let base = start + k * stride;
            let mut v = Vec::with_capacity(comps);
            for c in 0..comps {
                let at = base + c * size;
                let b = buf.get(at..at + size).ok_or("an accessor runs past its buffer")?;
                let x = match ct {
                    5126 => f32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f64,
                    5125 => u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f64,
                    5123 => {
                        let u = u16::from_le_bytes([b[0], b[1]]) as f64;
                        if normalized {
                            u / 65535.0
                        } else {
                            u
                        }
                    }
                    5122 => {
                        let s = i16::from_le_bytes([b[0], b[1]]) as f64;
                        if normalized {
                            (s / 32767.0).max(-1.0)
                        } else {
                            s
                        }
                    }
                    5121 => {
                        if normalized {
                            b[0] as f64 / 255.0
                        } else {
                            b[0] as f64
                        }
                    }
                    _ => {
                        let s = b[0] as i8 as f64;
                        if normalized {
                            (s / 127.0).max(-1.0)
                        } else {
                            s
                        }
                    }
                };
                v.push(x);
            }
            out.push(v);
        }
        Ok((out, comps))
    }
}

/// Read glTF 2.0: a `.glb`, or `.gltf` JSON whose buffers are beside it or
/// inside it as `data:` URIs.
pub fn read_gltf(bytes: &[u8], dir: Option<&Path>) -> Result<Mesh, String> {
    let (json, bin): (serde_json::Value, Option<Vec<u8>>) = if bytes.starts_with(b"glTF") {
        let le = |i: usize| u32::from_le_bytes([bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]]) as usize;
        if bytes.len() < 20 {
            return Err("the GLB is cut short".into());
        }
        let jlen = le(12);
        if le(16) != 0x4E4F_534A || bytes.len() < 20 + jlen {
            return Err("the GLB's first chunk isn't its JSON".into());
        }
        let json = serde_json::from_slice(&bytes[20..20 + jlen]).map_err(|e| format!("the GLB's JSON doesn't read: {e}"))?;
        let at = 20 + jlen;
        let bin = if bytes.len() >= at + 8 && le(at + 4) == 0x004E_4942 {
            let blen = le(at);
            Some(bytes.get(at + 8..at + 8 + blen).ok_or("the GLB's BIN chunk is cut short")?.to_vec())
        } else {
            None
        };
        (json, bin)
    } else {
        (serde_json::from_slice(bytes).map_err(|e| format!("the glTF doesn't read: {e}"))?, None)
    };
    let mut buffers = Vec::new();
    for (i, b) in json["buffers"].as_array().cloned().unwrap_or_default().iter().enumerate() {
        match b["uri"].as_str() {
            Some(uri) if uri.starts_with("data:") => {
                let data = uri.split_once(",").map(|x| x.1).ok_or("a data URI with no data")?;
                buffers.push(crate::b64::decode(data).map_err(|e| format!("buffer {i}: {e}"))?);
            }
            Some(uri) => {
                let d = dir.ok_or("the glTF names a buffer file but there's no folder to find it in")?;
                let path = d.join(uri.replace("%20", " "));
                buffers.push(std::fs::read(&path).map_err(|e| format!("couldn't read {}: {e}", path.display()))?);
            }
            None if i == 0 => buffers.push(bin.clone().ok_or("buffer 0 has no URI and there's no BIN chunk")?),
            None => return Err(format!("buffer {i} has no URI")),
        }
    }
    let g = Gltf { json, buffers };
    let mut colours: Vec<V> = Vec::new();
    for m in g.json["materials"].as_array().cloned().unwrap_or_default() {
        let f: Vec<f64> = m["pbrMetallicRoughness"]["baseColorFactor"]
            .as_array()
            .map(|a| a.iter().filter_map(|x| x.as_f64()).collect())
            .unwrap_or_else(|| vec![1.0, 1.0, 1.0, 1.0]);
        colours.push([f[0], f[1], f[2]]); // glTF colours are already linear
    }
    let mut tris = Vec::new();
    let scene_i = g.json["scene"].as_u64().unwrap_or(0) as usize;
    let roots: Vec<usize> = match g.json["scenes"][scene_i]["nodes"].as_array() {
        Some(a) => a.iter().filter_map(|x| x.as_u64().map(|v| v as usize)).collect(),
        None => (0..g.json["nodes"].as_array().map(|a| a.len()).unwrap_or(0)).collect(),
    };
    let mut stack: Vec<(usize, M4, usize)> = roots.into_iter().map(|n| (n, m4_identity(), 0)).collect();
    while let Some((ni, parent, depth)) = stack.pop() {
        if depth > 64 {
            return Err("the glTF's node tree loops or is too deep".into());
        }
        let node = &g.json["nodes"][ni];
        let world = m4_mul(&parent, &node_matrix(node));
        if let Some(mi) = node["mesh"].as_u64() {
            for prim in g.json["meshes"][mi as usize]["primitives"].as_array().cloned().unwrap_or_default() {
                if prim["mode"].as_u64().unwrap_or(4) != 4 {
                    continue;
                }
                let pos_i = prim["attributes"]["POSITION"].as_u64().ok_or("a primitive has no positions")? as usize;
                let (pos, _) = g.accessor(pos_i)?;
                let norms = match prim["attributes"]["NORMAL"].as_u64() {
                    Some(i) => Some(g.accessor(i as usize)?.0),
                    None => None,
                };
                let idx: Vec<usize> = match prim["indices"].as_u64() {
                    Some(i) => g.accessor(i as usize)?.0.into_iter().map(|v| v[0] as usize).collect(),
                    None => (0..pos.len()).collect(),
                };
                let colour = prim["material"].as_u64().map(|m| m as usize).filter(|m| *m < colours.len());
                for c in idx.chunks(3).filter(|c| c.len() == 3) {
                    if c.iter().any(|&k| k >= pos.len()) {
                        return Err("an index points past the positions".into());
                    }
                    let p = |k: usize| m4_point(&world, [pos[k][0], pos[k][1], pos[k][2]]);
                    let n = norms.as_ref().and_then(|ns| {
                        c.iter().all(|&k| k < ns.len()).then(|| {
                            let f = |k: usize| m4_normal(&world, [ns[k][0], ns[k][1], ns[k][2]]);
                            [f(c[0]), f(c[1]), f(c[2])]
                        })
                    });
                    tris.push(Tri { p: [p(c[0]), p(c[1]), p(c[2])], n, colour });
                }
            }
        }
        for ch in node["children"].as_array().cloned().unwrap_or_default() {
            if let Some(c) = ch.as_u64() {
                stack.push((c as usize, world, depth + 1));
            }
        }
    }
    Mesh::new(tris, colours)
}

/// Read a model by its extension: `.obj`, `.stl`, `.gltf`, `.glb`.
pub fn read_model(path: &Path) -> Result<Mesh, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("couldn't read {}: {e}", path.display()))?;
    let dir = path.parent();
    match path.extension().and_then(|e| e.to_str()).map(|e| e.to_lowercase()).as_deref() {
        Some("obj") => read_obj(&String::from_utf8_lossy(&bytes), dir),
        Some("stl") => read_stl(&bytes),
        Some("gltf") | Some("glb") => read_gltf(&bytes, dir),
        _ => Err(format!("{} isn't a model Atlas reads (OBJ, STL, glTF or GLB)", path.display())),
    }
}

/// Models already read, so every frame of an animation doesn't read the
/// file again.
pub fn cached(path: &Path) -> Result<std::sync::Arc<Mesh>, String> {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex, OnceLock};
    static CACHE: OnceLock<Mutex<HashMap<PathBuf, (std::time::SystemTime, Arc<Mesh>)>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let stamp = std::fs::metadata(path).and_then(|m| m.modified()).map_err(|e| format!("couldn't read {}: {e}", path.display()))?;
    if let Some((when, m)) = cache.lock().map_err(|_| "the model cache broke")?.get(path) {
        if *when == stamp {
            return Ok(m.clone());
        }
    }
    let m = Arc::new(read_model(path)?);
    cache.lock().map_err(|_| "the model cache broke")?.insert(path.to_path_buf(), (stamp, m.clone()));
    Ok(m)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CUBE_OBJ: &str = "v -1 -1 -1\nv 1 -1 -1\nv 1 1 -1\nv -1 1 -1\nv -1 -1 1\nv 1 -1 1\nv 1 1 1\nv -1 1 1\n\
        f 1 2 3 4\nf 5 8 7 6\nf 1 5 6 2\nf 2 6 7 3\nf 3 7 8 4\nf 5 1 4 8\n";

    #[test]
    fn a_cube_from_obj_is_twelve_triangles_and_is_hit_on_its_face() {
        let m = read_obj(CUBE_OBJ, None).unwrap();
        assert_eq!(m.tris.len(), 12);
        let h = m.hit([0.0, 0.0, 5.0], [0.0, 0.0, -1.0], f64::MAX).unwrap();
        assert!((h.t - 4.0).abs() < 1e-9);
        assert!(m.hit([3.0, 0.0, 5.0], [0.0, 0.0, -1.0], f64::MAX).is_none());
    }

    #[test]
    fn negative_indices_count_back_from_the_end() {
        let m = read_obj("v 0 0 0\nv 1 0 0\nv 0 1 0\nf -3 -2 -1\n", None).unwrap();
        assert_eq!(m.tris[0].p[2], [0.0, 1.0, 0.0]);
    }

    #[test]
    fn a_binary_stl_reads_its_facets() {
        let mut b = vec![0u8; 80];
        b.extend_from_slice(&1u32.to_le_bytes());
        for f in [0.0f32, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0] {
            b.extend_from_slice(&f.to_le_bytes());
        }
        b.extend_from_slice(&[0, 0]);
        let m = read_stl(&b).unwrap();
        assert_eq!(m.tris.len(), 1);
        assert_eq!(m.tris[0].p[1], [1.0, 0.0, 0.0]);
    }

    #[test]
    fn a_bvh_over_many_triangles_finds_the_same_nearest_hit_as_checking_them_all() {
        // A grid of small triangles at different depths.
        let mut tris = Vec::new();
        for i in 0..40 {
            for j in 0..40 {
                let (x, y, z) = (i as f64 * 0.1, j as f64 * 0.1, ((i * 7 + j * 13) % 11) as f64 * 0.05);
                tris.push(Tri { p: [[x, y, z], [x + 0.09, y, z], [x, y + 0.09, z]], n: None, colour: None });
            }
        }
        let m = Mesh::new(tris.clone(), vec![]).unwrap();
        let brute = |o: V, d: V| {
            let single = |t: &Tri| Mesh::new(vec![t.clone()], vec![]).unwrap().hit(o, d, f64::MAX).map(|h| h.t);
            tris.iter().filter_map(single).fold(f64::MAX, f64::min)
        };
        for k in 0..30 {
            let o = [0.02 + k as f64 * 0.13, 0.03 + k as f64 * 0.11, 5.0];
            let d = unit([0.01 * (k % 3) as f64, -0.02, -1.0]);
            let a = m.hit(o, d, f64::MAX).map(|h| h.t).unwrap_or(f64::MAX);
            assert!((a - brute(o, d)).abs() < 1e-9, "ray {k}");
        }
    }
}
