//! Maps made in Blender. The exporter in `blender/shitshooter_maps.py`
//! writes a map as a JSON file; every `*.json` in a `maps` folder (beside
//! the game, or where it is started from) is a map, listed after the
//! built-in ones. The host sends the one being played to everybody who
//! joins, so only the host needs the file, and reads it again at the start
//! of each match: export again from Blender, start a match, and you are
//! riding the new version.
//!
//! A map file is in the game's coordinates (metres, Y up). It holds solids,
//! each a convex shape you collide with (the exporter makes each loose part
//! of a mesh one, its convex hull), and markers: where players spawn, and
//! for a course, the start, the checkpoints (each with where a fall after it
//! puts you back), the finish and zones that count as falling off. A map
//! with a start and a finish is a course, raced in the racing modes; any
//! other is played in free for all and team deathmatch.

use std::{
    path::{Path, PathBuf},
    sync::{Arc, RwLock},
};

use bevy::math::{Vec2, Vec3};
use serde::Deserialize;

use crate::{
    course::{Course, Respawn, Zone},
    map::{Look, Map, PLAYER_HALF, Spawn},
    ramp::Ramp,
};

/// The file format this game reads (the exporter writes it as `format`).
pub const FORMAT: u32 = 1;
/// Below the lowest solid by this much, you have fallen off.
const FALL_BELOW: f32 = 10.0;

#[derive(Deserialize)]
struct File {
    format: u32,
    /// The sky's colour (sRGB), and the fog's.
    #[serde(default)]
    sky: Option<[f32; 3]>,
    solids: Vec<Solid>,
    #[serde(default)]
    spawns: Vec<Marker>,
    #[serde(default)]
    start: Option<Gate>,
    #[serde(default)]
    checkpoints: Vec<Checkpoint>,
    #[serde(default)]
    finish: Option<Gate>,
    /// Where you wait once through the finish of a race (else the first spawn).
    #[serde(default)]
    finish_spawn: Option<Marker>,
    /// Go into one of these and you have fallen off.
    #[serde(default)]
    kills: Vec<Gate>,
}

#[derive(Deserialize)]
struct Solid {
    corners: Vec<[f32; 3]>,
    /// Each face as indices into `corners`, going round its edge.
    faces: Vec<Vec<usize>>,
    /// sRGB.
    color: [f32; 3],
    /// A texture by name (`textures::Pattern::named`); else one by colour.
    #[serde(default)]
    texture: Option<String>,
    /// An invisible solid still collides.
    #[serde(default = "yes")]
    visible: bool,
}

fn yes() -> bool {
    true
}

/// A point to stand at: the feet, the way to face (x, z), and how fast to
/// be going that way.
#[derive(Deserialize, Clone, Copy)]
struct Marker {
    at: [f32; 3],
    facing: [f32; 2],
    #[serde(default)]
    speed: f32,
}

/// A box turned about the vertical: half its size across, up and along,
/// and the way through it (x, z).
#[derive(Deserialize, Clone, Copy)]
struct Gate {
    center: [f32; 3],
    half: [f32; 3],
    facing: [f32; 2],
}

#[derive(Deserialize)]
struct Checkpoint {
    gate: Gate,
    respawn: Marker,
}

/// The yaw that looks along `dir` (x, z).
fn yaw_along(dir: Vec2) -> f32 {
    (-dir.x).atan2(-dir.y)
}

fn way(facing: [f32; 2]) -> Vec2 {
    Vec2::from(facing).try_normalize().unwrap_or(Vec2::NEG_Y)
}

impl Marker {
    fn respawn(self) -> Respawn {
        let dir = way(self.facing);
        Respawn {
            pos: Vec3::from(self.at) + Vec3::Y * (PLAYER_HALF.y + 0.05),
            yaw: yaw_along(dir),
            vel: Vec3::new(dir.x, 0.0, dir.y) * self.speed,
        }
    }
}

impl Gate {
    fn zone(self) -> Zone {
        Zone {
            center: self.center.into(),
            half: Vec3::from(self.half).abs(),
            yaw: yaw_along(way(self.facing)),
        }
    }
}

/// Reads a map file, checking it can be played: `Err` says what is wrong.
fn parse(bytes: &[u8]) -> Result<File, String> {
    let file: File = serde_json::from_slice(bytes).map_err(|err| format!("not a map file: {err}"))?;
    if file.format != FORMAT {
        return Err(format!("made for format {}, this game reads format {FORMAT}: export it again", file.format));
    }
    if file.solids.is_empty() {
        return Err("it has nothing in it".into());
    }
    for (index, solid) in file.solids.iter().enumerate() {
        let finite = solid.corners.iter().flatten().all(|x| x.is_finite());
        let faces_ok = solid.faces.iter().all(|face| face.len() >= 3 && face.iter().all(|i| *i < solid.corners.len()));
        if solid.corners.len() < 4 || solid.faces.len() < 4 || !finite || !faces_ok {
            return Err(format!("solid {index} is not a solid shape"));
        }
    }
    if file.spawns.is_empty() {
        return Err("it has no spawn".into());
    }
    match (&file.start, &file.finish) {
        (Some(_), None) => return Err("it has a start but no finish".into()),
        (None, Some(_)) => return Err("it has a finish but no start".into()),
        _ => {}
    }
    Ok(file)
}

/// Builds the map from a file `parse` took, as map `id`.
fn build(id: u8, file: &File) -> Map {
    let mut map = Map::empty(id);
    for solid in &file.solids {
        let corners = solid.corners.iter().map(|c| Vec3::from(*c)).collect();
        let mut ramp = Ramp::solid(corners, solid.faces.clone(), solid.color);
        ramp.visible = solid.visible;
        if let Some(texture) = &solid.texture
            && !map.textures.iter().any(|(color, _)| *color == solid.color)
        {
            map.textures.push((solid.color, texture.clone()));
        }
        map.ramps.push(ramp);
    }
    map.spawns = file
        .spawns
        .iter()
        .map(|marker| {
            let at = marker.respawn();
            Spawn { pos: at.pos, yaw: at.yaw }
        })
        .collect();

    let (mut lo, mut hi) = (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY));
    for ramp in &map.ramps {
        (lo, hi) = (lo.min(ramp.min), hi.max(ramp.max));
    }
    if let (Some(start), Some(finish)) = (file.start, file.finish) {
        let start_point = file.spawns[0].respawn();
        map.course = Some(Course {
            start: start.zone(),
            start_point,
            gates: file.checkpoints.iter().map(|c| (c.gate.zone(), c.respawn.respawn())).collect(),
            finish: finish.zone(),
            finish_point: file.finish_spawn.map_or(start_point, Marker::respawn),
            floors: vec![lo.y - FALL_BELOW; file.checkpoints.len() + 1],
            kills: file.kills.iter().map(|kill| kill.zone()).collect(),
        });
    }
    map.bounds = [Vec2::new(lo.x - 10.0, lo.z - 10.0), Vec2::new(hi.x + 10.0, hi.z + 10.0)];
    let middle = (lo + hi) / 2.0;
    let size = (hi - lo).with_y(0.0).length().max(40.0);
    let sky = file.sky.unwrap_or([0.55, 0.68, 0.88]);
    map.look = Look {
        sky,
        fog: Some((size.max(200.0), size.max(200.0) * 4.0)),
        sun_from: Vec3::new(160.0, 260.0, 90.0),
        sun: 10000.0,
        ambient: 480.0,
        orbit: (middle, size * 0.6, (hi.y - middle.y) + size * 0.3),
    };
    map.finish()
}

/// One map made in Blender, as this game knows it.
pub struct Entry {
    /// Its file's name, without `.json`.
    pub name: String,
    /// Raced in the racing modes (it has a start and a finish), rather than
    /// fought on.
    pub course: bool,
    /// Its file, when this game read it from one (hosting); a game that
    /// joined somebody else's is sent the map instead.
    path: Option<PathBuf>,
    /// The file's contents, once known.
    bytes: Option<Arc<Vec<u8>>>,
}

/// The maps made in Blender, in order: map `map::BUILT_IN + i` is the
/// `i`th. Shared by the hosted server's thread and the game.
static MAPS: RwLock<Vec<Entry>> = RwLock::new(Vec::new());

/// The folders maps are read from: `maps` beside the game, and `maps` where
/// it was started from.
pub fn folders() -> Vec<PathBuf> {
    let mut folders = Vec::new();
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        folders.push(dir.join("maps"));
    }
    if let Ok(dir) = std::env::current_dir() {
        let here = dir.join("maps");
        if !folders.iter().any(|f| same(f, &here)) {
            folders.push(here);
        }
    }
    folders
}

fn same(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

/// Reads every map in the maps folders, in order of name; ones that cannot
/// be played are left out, and said why. What is wrong with each, for
/// whoever is told (the log, the console).
pub fn load() -> Vec<String> {
    let mut found: Vec<(String, PathBuf)> = Vec::new();
    for folder in folders() {
        let Ok(dir) = std::fs::read_dir(&folder) else { continue };
        for path in dir.flatten().map(|entry| entry.path()) {
            if path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("json"))
                && let Some(name) = path.file_stem().and_then(|s| s.to_str())
                && !found.iter().any(|(known, _)| known.eq_ignore_ascii_case(name))
            {
                found.push((name.to_string(), path));
            }
        }
    }
    found.sort_by_key(|(name, _)| name.to_lowercase());
    let mut problems = Vec::new();
    let mut maps = Vec::new();
    for (name, path) in found {
        let read = std::fs::read(&path).map_err(|err| err.to_string());
        match read.and_then(|bytes| parse(&bytes).map(|file| (bytes, file))) {
            Ok((bytes, file)) => maps.push(Entry {
                name,
                course: file.start.is_some(),
                path: Some(path),
                bytes: Some(Arc::new(bytes)),
            }),
            Err(why) => problems.push(format!("map {}: {why}", path.display())),
        }
        if maps.len() >= (u8::MAX - crate::map::BUILT_IN) as usize {
            break;
        }
    }
    *MAPS.write().unwrap() = maps;
    problems
}

/// How many there are.
pub fn count() -> usize {
    MAPS.read().unwrap().len()
}

/// The `index`th one's name, and whether it is a course.
pub fn about(index: usize) -> Option<(String, bool)> {
    MAPS.read().unwrap().get(index).map(|entry| (entry.name.clone(), entry.course))
}

/// Every one's name, and whether it is a course: what a server tells
/// whoever joins.
pub fn list() -> Vec<(String, bool)> {
    MAPS.read().unwrap().iter().map(|entry| (entry.name.clone(), entry.course)).collect()
}

/// Joined somebody's game: theirs are the maps there are. One of ours with
/// the same name in the same place keeps what we know of it until we are
/// sent theirs.
pub fn take_list(list: Vec<(String, bool)>) {
    let mut maps = MAPS.write().unwrap();
    let old = std::mem::take(&mut *maps);
    *maps = list
        .into_iter()
        .enumerate()
        .map(|(index, (name, course))| {
            let kept = old.get(index).filter(|entry| entry.name == name);
            Entry {
                course,
                path: kept.and_then(|entry| entry.path.clone()),
                bytes: kept.and_then(|entry| entry.bytes.clone()),
                name,
            }
        })
        .collect();
}

/// The `index`th one's file, to send.
pub fn bytes(index: usize) -> Option<Arc<Vec<u8>>> {
    MAPS.read().unwrap().get(index)?.bytes.clone()
}

/// Sent the `index`th one's file: keep it, if it can be played.
pub fn take_bytes(index: usize, bytes: Vec<u8>) -> Result<(), String> {
    let file = parse(&bytes)?;
    let mut maps = MAPS.write().unwrap();
    let entry = maps.get_mut(index).ok_or("no such map")?;
    entry.course = file.start.is_some();
    entry.bytes = Some(Arc::new(bytes));
    Ok(())
}

/// Reads the `index`th one's file again: true if it changed (and can still
/// be played; if it cannot, the last good one is kept, and said why).
pub fn reload(index: usize) -> Result<bool, String> {
    let path = {
        let maps = MAPS.read().unwrap();
        let Some(path) = maps.get(index).and_then(|entry| entry.path.clone()) else { return Ok(false) };
        path
    };
    let bytes = std::fs::read(&path).map_err(|err| format!("map {}: {err}", path.display()))?;
    let changed = self::bytes(index).is_none_or(|old| *old != bytes);
    if changed {
        take_bytes(index, bytes).map_err(|why| format!("map {}: {why}", path.display()))?;
    }
    Ok(changed)
}

/// The `index`th one built as map `id`, if we have it.
pub fn map(index: usize, id: u8) -> Option<Map> {
    let bytes = bytes(index)?;
    let file = parse(&bytes).ok()?;
    Some(build(id, &file))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A cube from `lo` to `hi`, as the exporter writes one.
    fn cube(lo: [f32; 3], hi: [f32; 3]) -> String {
        let c = |x: bool, y: bool, z: bool| [if x { hi[0] } else { lo[0] }, if y { hi[1] } else { lo[1] }, if z { hi[2] } else { lo[2] }];
        let corners: Vec<[f32; 3]> = (0..8).map(|i| c(i & 1 != 0, i & 2 != 0, i & 4 != 0)).collect();
        format!(
            r#"{{"corners": {corners:?}, "faces": [[0,2,3,1],[4,5,7,6],[0,1,5,4],[2,6,7,3],[0,4,6,2],[1,3,7,5]], "color": [0.5, 0.5, 0.5], "texture": "slab"}}"#
        )
    }

    fn course_file() -> String {
        format!(
            r#"{{
                "format": 1,
                "solids": [{}, {}],
                "spawns": [{{"at": [0, 0, -5], "facing": [0, -1]}}],
                "start": {{"center": [0, 2, -5], "half": [5, 2, 5], "facing": [0, -1]}},
                "checkpoints": [{{"gate": {{"center": [0, 2, -30], "half": [5, 4, 0.5], "facing": [0, -1]}},
                                  "respawn": {{"at": [0, 0, -32], "facing": [0, -1], "speed": 20}}}}],
                "finish": {{"center": [0, 2, -50], "half": [5, 4, 0.5], "facing": [0, -1]}},
                "kills": [{{"center": [0, -20, -30], "half": [100, 5, 100], "facing": [0, -1]}}]
            }}"#,
            cube([-5.0, -1.0, -60.0], [5.0, 0.0, 0.0]),
            cube([-5.0, -1.0, -100.0], [5.0, 0.0, -70.0]),
        )
    }

    #[test]
    fn a_course_file_builds_a_course() {
        let file = parse(course_file().as_bytes()).unwrap();
        let map = build(9, &file);
        let course = map.course.as_ref().expect("a course");
        assert_eq!(course.gates.len(), 1);
        assert_eq!(map.ramps.len(), 2);
        assert_eq!(map.textures, vec![([0.5, 0.5, 0.5], "slab".to_string())]);
        // Standing on the floor at the spawn, not in it.
        assert!(map.is_free(map.spawns[0].pos, PLAYER_HALF));
        assert!(!map.is_free(map.spawns[0].pos - Vec3::Y * 0.1, PLAYER_HALF));
        assert!(course.start.contains(course.start_point.pos));
        // Back on after the checkpoint moving the way it faces.
        let respawn = course.gates[0].1;
        assert!((respawn.vel - Vec3::new(0.0, 0.0, -20.0)).length() < 1e-4);
        // Through a gate facing -z is going -z.
        assert!(course.finish.entered(Vec3::new(0.0, 2.0, -49.0), Vec3::new(0.0, 2.0, -51.0)).is_some());
        assert!(course.fallen(Vec3::new(0.0, -20.0, -30.0), 1));
        // A course catches a fall before the map's safety net does.
        assert!(course.floors.iter().all(|floor| *floor > map.abyss));
        assert!(!course.fallen(Vec3::new(0.0, 1.0, -30.0), 1));
    }

    /// The example made by `blender/example_surf_map.py` is a course.
    #[test]
    fn the_example_map_is_a_course() {
        let bytes = std::fs::read("maps/example surf.json").expect("the example map");
        let map = build(9, &parse(&bytes).unwrap());
        let course = map.course.as_ref().unwrap();
        assert_eq!(course.gates.len(), 1);
        assert_eq!(map.spawns.len(), 16);
        for spawn in &map.spawns {
            assert!(map.is_free(spawn.pos, PLAYER_HALF), "a spawn inside the deck at {}", spawn.pos);
            assert!(!map.is_free(spawn.pos - Vec3::Y * 0.1, PLAYER_HALF), "a spawn in the air at {}", spawn.pos);
            assert!(course.start.contains(spawn.pos));
        }
    }

    #[test]
    fn broken_files_say_what_is_wrong() {
        assert!(parse(b"{").err().unwrap().contains("not a map file"));
        let no_finish = course_file().replace(r#""finish""#, r#""unused""#);
        assert!(parse(no_finish.as_bytes()).err().unwrap().contains("no finish"));
        let old = course_file().replace(r#""format": 1"#, r#""format": 0"#);
        assert!(parse(old.as_bytes()).err().unwrap().contains("export it again"));
    }
}
