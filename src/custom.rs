//! Maps made in Blender. The exporter in `blender/shitshooter_maps.py`
//! writes a map as a JSON file; every `*.json` in a `maps` folder (beside
//! the game, or where it is started from: your own) or in `assets/maps`
//! (shipped with every release) is a map, listed after the built-in ones.
//! One of your own wins over a shipped one of the same name. The host sends the one being played to everybody who
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
//! other is played in free for all and team deathmatch. A file can also
//! name the modes it was made for (`modes`), and is then offered in those
//! alone. A spawn can be set aside for a role (`role`: "sniper"), and a
//! finish with no start is somewhere to get to rather than the end of a
//! course: the sniper mode is played on a map with both.
//!
//! A file can also say how it is lit (`light`: how bright the sun is, its
//! colour and where it shines from, the ambient light, the fog), which makes
//! a night of it; where its lamps and fires are (`lamps`); and which of its
//! solids glow (`glow`). What it leaves out is plain daylight.

use std::{
    path::{Path, PathBuf},
    sync::{Arc, RwLock},
};

use bevy::math::{Mat3, Vec2, Vec3};
use serde::Deserialize;

use crate::{
    course::{Course, KillBox, Respawn, Zone},
    map::{Lamp, Look, Map, PLAYER_HALF, Spawn},
    modes::{self, Needs},
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
    /// How it is lit, where that is not plain daylight.
    #[serde(default)]
    light: Option<Light>,
    /// Its lamps and fires.
    #[serde(default)]
    lamps: Vec<LampMarker>,
    solids: Vec<Solid>,
    #[serde(default)]
    spawns: Vec<SpawnMarker>,
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
    kills: Vec<Kill>,
    /// The modes it was made for, by their keys ("race", "ffa"). Without
    /// it, every mode the map has what it takes for.
    #[serde(default)]
    modes: Option<Vec<String>>,
    /// The modes it is played in (their ids), worked out by `parse`.
    #[serde(skip)]
    plays: Vec<u8>,
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
    /// It glows, this many times its colour: a lit window, a fire.
    #[serde(default)]
    glow: f32,
}

fn yes() -> bool {
    true
}

/// How a map is lit. What it leaves out is as by day.
#[derive(Deserialize, Default)]
struct Light {
    /// How bright the sun (or the moon) is, in lux: 10000 is daylight.
    #[serde(default)]
    sun: Option<f32>,
    /// Its light's colour (sRGB).
    #[serde(default)]
    sun_color: Option<[f32; 3]>,
    /// Where it shines from, seen from the middle of the map.
    #[serde(default)]
    sun_from: Option<[f32; 3]>,
    #[serde(default)]
    ambient: Option<f32>,
    /// Fog from this far to this far.
    #[serde(default)]
    fog: Option<[f32; 2]>,
}

/// A lamp or a fire: where, its colour (sRGB), how bright (lumens), how far
/// it reaches, and whether it wavers as a fire does.
#[derive(Deserialize)]
struct LampMarker {
    at: [f32; 3],
    color: [f32; 3],
    power: f32,
    #[serde(default = "lamp_range")]
    range: f32,
    #[serde(default)]
    flicker: bool,
}

fn lamp_range() -> f32 {
    25.0
}

/// No more lamps than this are lit: each one costs every frame.
const MOST_LAMPS: usize = 64;

/// A point to stand at: the feet, the way to face (x, z), and how fast to
/// be going that way.
#[derive(Deserialize, Clone, Copy)]
struct Marker {
    at: [f32; 3],
    facing: [f32; 2],
    #[serde(default)]
    speed: f32,
}

/// Where players come in: a marker, and whose it is ("sniper") if it is not
/// anybody's.
#[derive(Deserialize)]
struct SpawnMarker {
    #[serde(flatten)]
    marker: Marker,
    #[serde(default)]
    role: Option<String>,
}

/// A box turned about the vertical: half its size across, up and along,
/// and the way through it (x, z).
#[derive(Deserialize, Clone, Copy)]
struct Gate {
    center: [f32; 3],
    half: [f32; 3],
    facing: [f32; 2],
}

/// A kill zone: a gate's box, which `axes` (across, up, along: unit
/// vectors) can turn any way, as a slab under a slope. Without them it is
/// only turned about the vertical, by `facing`.
#[derive(Deserialize, Clone, Copy)]
struct Kill {
    #[serde(flatten)]
    gate: Gate,
    #[serde(default)]
    axes: Option<[[f32; 3]; 3]>,
}

impl Kill {
    fn kill_box(self) -> KillBox {
        let flat = KillBox::from(self.gate.zone());
        let Some(axes) = self.axes else { return flat };
        let axes = axes.map(|axis| Vec3::from(axis).try_normalize());
        match axes {
            [Some(across), Some(up), Some(along)] => KillBox {
                axes: Mat3::from_cols(across, up, along),
                ..flat
            },
            _ => flat,
        }
    }
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

/// What a map file is missing to be played in a mode with these needs, if
/// anything.
fn lacks(file: &File, needs: Needs) -> Option<&'static str> {
    let course = file.start.is_some() && file.finish.is_some();
    match needs {
        Needs::Arena => course.then_some("it is a course"),
        Needs::Course => (!course).then_some("it has no start and finish"),
        Needs::Hill => Some("a map file cannot mark a hill"),
        Needs::Flags => Some("a map file cannot have flags"),
        Needs::Sniper => {
            if file.start.is_some() {
                Some("it is a course")
            } else if file.finish.is_none() {
                Some("it has no finish for the runners to reach")
            } else if !file.spawns.iter().any(|spawn| spawn.role.as_deref().is_some_and(|role| role.eq_ignore_ascii_case(SNIPER_ROLE))) {
                Some("it has no spawn for the sniper")
            } else {
                None
            }
        }
    }
}

/// The role of the spawn the sniper comes in at, and the name of the place
/// the runners run for (a finish with no start).
pub const SNIPER_ROLE: &str = "sniper";
pub const FINISH_ZONE: &str = "finish";

/// The modes a map file is played in: the ones it names, if it has what
/// each needs, or else every mode it has what it takes for.
fn modes_of(file: &File) -> Result<Vec<u8>, String> {
    let Some(keys) = &file.modes else {
        return Ok((0..modes::count()).filter(|mode| lacks(file, modes::info(*mode).needs).is_none()).collect());
    };
    let mut plays = Vec::new();
    for key in keys {
        // A mode this game does not know (the map is from a newer one) is passed over.
        let Some(mode) = modes::by_key(key) else { continue };
        if let Some(why) = lacks(file, modes::info(mode).needs) {
            return Err(format!("it is made for {}, but {why}", modes::name(mode).to_lowercase()));
        }
        if !plays.contains(&mode) {
            plays.push(mode);
        }
    }
    if plays.is_empty() {
        return Err(format!("it is made for modes this game does not have ({})", keys.join(", ")));
    }
    Ok(plays)
}

/// Reads a map file, checking it can be played: `Err` says what is wrong.
fn parse(bytes: &[u8]) -> Result<File, String> {
    let mut file: File = serde_json::from_slice(bytes).map_err(|err| format!("not a map file: {err}"))?;
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
    if !file.spawns.iter().any(|spawn| spawn.role.is_none()) {
        return Err("it has no spawn".into());
    }
    for (index, lamp) in file.lamps.iter().enumerate() {
        if !lamp.at.iter().chain(&lamp.color).chain([&lamp.power, &lamp.range]).all(|x| x.is_finite()) {
            return Err(format!("lamp {index} is not a light"));
        }
    }
    if file.start.is_some() && file.finish.is_none() {
        return Err("it has a start but no finish".into());
    }
    file.plays = modes_of(&file)?;
    // A finish alone is somewhere to get to, in a mode that has one.
    let finish_used = file.plays.iter().any(|mode| modes::info(*mode).needs == Needs::Sniper);
    if file.start.is_none() && file.finish.is_some() && !finish_used {
        return Err("it has a finish but no start".into());
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
        if solid.glow > 0.0 && solid.glow.is_finite() && !map.glows.iter().any(|(color, _)| *color == solid.color) {
            map.glows.push((solid.color, solid.glow.min(100.0)));
        }
        map.ramps.push(ramp);
    }
    for marker in &file.spawns {
        let at = marker.marker.respawn();
        let spawn = Spawn { pos: at.pos, yaw: at.yaw };
        match &marker.role {
            Some(role) => map.posts.push((role.to_lowercase(), spawn)),
            None => map.spawns.push(spawn),
        }
    }
    if let (None, Some(finish)) = (file.start, file.finish) {
        map.zones.push((FINISH_ZONE.to_string(), finish.zone()));
    }
    map.lamps = file
        .lamps
        .iter()
        .take(MOST_LAMPS)
        .map(|lamp| Lamp {
            pos: lamp.at.into(),
            color: lamp.color.map(|c| c.clamp(0.0, 1.0)),
            power: lamp.power.clamp(0.0, 1.0e8),
            range: lamp.range.clamp(0.5, 200.0),
            flicker: lamp.flicker,
        })
        .collect();

    let (mut lo, mut hi) = (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY));
    for ramp in &map.ramps {
        (lo, hi) = (lo.min(ramp.min), hi.max(ramp.max));
    }
    if let (Some(start), Some(finish)) = (file.start, file.finish) {
        // The first spawn that is anybody's.
        let start_point = file.spawns.iter().find(|spawn| spawn.role.is_none()).unwrap_or(&file.spawns[0]).marker.respawn();
        map.course = Some(Course {
            start: start.zone(),
            start_point,
            gates: file.checkpoints.iter().map(|c| (c.gate.zone(), c.respawn.respawn())).collect(),
            finish: finish.zone(),
            finish_point: file.finish_spawn.map_or(start_point, Marker::respawn),
            floors: vec![lo.y - FALL_BELOW; file.checkpoints.len() + 1],
            kills: file.kills.iter().map(|kill| kill.kill_box()).collect(),
        });
    }
    map.bounds = [Vec2::new(lo.x - 10.0, lo.z - 10.0), Vec2::new(hi.x + 10.0, hi.z + 10.0)];
    let middle = (lo + hi) / 2.0;
    let size = (hi - lo).with_y(0.0).length().max(40.0);
    let sky = file.sky.unwrap_or([0.55, 0.68, 0.88]);
    let light = file.light.as_ref();
    let finite = |x: &f32| x.is_finite();
    let fog = light
        .and_then(|light| light.fog)
        .filter(|[start, end]| start.is_finite() && end.is_finite() && *start >= 0.0 && end > start)
        .map_or((size.max(200.0), size.max(200.0) * 4.0), |[start, end]| (start, end));
    // Never from below, nor from nowhere: there it would light nothing.
    let sun_from = light
        .and_then(|light| light.sun_from)
        .map(Vec3::from)
        .filter(|from| from.is_finite() && from.y > 0.0 && from.length() > 1e-3)
        .map_or(Vec3::new(160.0, 260.0, 90.0), |from| from.normalize() * 300.0);
    map.look = Look {
        sky,
        fog: Some(fog),
        sun_from,
        sun: light.and_then(|light| light.sun).filter(finite).map_or(10000.0, |sun| sun.clamp(0.0, 100_000.0)),
        sun_color: light.and_then(|light| light.sun_color).filter(|color| color.iter().all(finite)).unwrap_or([1.0; 3]),
        ambient: light.and_then(|light| light.ambient).filter(finite).map_or(480.0, |ambient| ambient.clamp(0.0, 5000.0)),
        orbit: (middle, size * 0.6, (hi.y - middle.y) + size * 0.3),
    };
    map.finish()
}

/// One map made in Blender, as this game knows it.
pub struct Entry {
    /// Its file's name, without `.json`.
    pub name: String,
    /// The modes it is played in (their ids).
    pub modes: Vec<u8>,
    /// Its file, when this game read it from one (hosting); a game that
    /// joined somebody else's is sent the map instead.
    path: Option<PathBuf>,
    /// The file's contents, once known.
    bytes: Option<Arc<Vec<u8>>>,
}

/// The maps made in Blender, in order: map `map::BUILT_IN + i` is the
/// `i`th. Shared by the hosted server's thread and the game.
static MAPS: RwLock<Vec<Entry>> = RwLock::new(Vec::new());

/// The folders maps are read from, in order (the first map of a name wins):
/// your own, `maps` beside the game and where it was started from; then the
/// ones shipped with the release, `assets/maps` in both.
pub fn folders() -> Vec<PathBuf> {
    let mut bases = Vec::new();
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        bases.push(dir.to_path_buf());
    }
    if let Ok(dir) = std::env::current_dir() {
        bases.push(dir);
    }
    let mut folders: Vec<PathBuf> = Vec::new();
    for sub in ["maps", "assets/maps"] {
        for base in &bases {
            let folder = base.join(sub);
            if !folders.iter().any(|f| same(f, &folder)) {
                folders.push(folder);
            }
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
                modes: file.plays,
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

/// The `index`th one's name.
pub fn name(index: usize) -> Option<String> {
    MAPS.read().unwrap().get(index).map(|entry| entry.name.clone())
}

/// True if the `index`th one is played in `mode`.
pub fn plays(index: usize, mode: u8) -> bool {
    MAPS.read().unwrap().get(index).is_some_and(|entry| entry.modes.contains(&mode))
}

/// Every one's name, and the modes it is played in: what a server tells
/// whoever joins.
pub fn list() -> Vec<(String, Vec<u8>)> {
    MAPS.read().unwrap().iter().map(|entry| (entry.name.clone(), entry.modes.clone())).collect()
}

/// Joined somebody's game: theirs are the maps there are. One of ours with
/// the same name in the same place keeps what we know of it until we are
/// sent theirs.
pub fn take_list(list: Vec<(String, Vec<u8>)>) {
    let mut maps = MAPS.write().unwrap();
    let old = std::mem::take(&mut *maps);
    *maps = list
        .into_iter()
        .enumerate()
        .map(|(index, (name, modes))| {
            let kept = old.get(index).filter(|entry| entry.name == name);
            Entry {
                modes,
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
    entry.modes = file.plays;
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
                "kills": [{{"center": [0, -20, -30], "half": [100, 5, 100], "facing": [0, -1]}},
                          {{"center": [0, -50, -150], "half": [100, 1, 100], "facing": [0, -1],
                            "axes": [[1, 0, 0], [0, 0.7071, -0.7071], [0, -0.7071, -0.7071]]}}]
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
        // The second kill zone slopes down 45 degrees towards -z: in it 50 m
        // down the slope, but not level with its centre 50 m on.
        assert!(course.kills[1].contains(Vec3::new(0.0, -85.35, -185.35)));
        assert!(!course.kills[1].contains(Vec3::new(0.0, -50.0, -200.0)));
    }

    /// The example made by `blender/example_surf_map.py` is a course.
    #[test]
    fn the_example_map_is_a_course() {
        let bytes = std::fs::read("blender/example surf.json").expect("the example map");
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
    fn a_map_is_played_in_the_modes_it_names_or_the_ones_it_can_be() {
        use crate::modes::{MODE_FFA, MODE_RACE, MODE_TEAMS, MODE_TRIAL};
        // A course with nothing said: both racing modes.
        assert_eq!(parse(course_file().as_bytes()).unwrap().plays, [MODE_RACE, MODE_TRIAL]);
        // Somewhere to fight with nothing said: the modes that need no more.
        let arena = format!(r#"{{"format": 1, "solids": [{}], "spawns": [{{"at": [0, 0, -5], "facing": [0, -1]}}]}}"#, cube([-5.0, -1.0, -9.0], [5.0, 0.0, 0.0]));
        assert_eq!(parse(arena.as_bytes()).unwrap().plays, [MODE_FFA, MODE_TEAMS]);
        // Naming them narrows it; one this game does not know is passed over.
        let trial_only = course_file().replace(r#""format": 1,"#, r#""format": 1, "modes": ["TRIAL", "from the future"],"#);
        assert_eq!(parse(trial_only.as_bytes()).unwrap().plays, [MODE_TRIAL]);
        // Made for a mode it has not what it takes for: said, not played.
        let wrong = arena.replace(r#""format": 1,"#, r#""format": 1, "modes": ["race"],"#);
        assert!(parse(wrong.as_bytes()).err().unwrap().contains("made for race, but it has no start and finish"));
        let hill = arena.replace(r#""format": 1,"#, r#""format": 1, "modes": ["hill"],"#);
        assert!(parse(hill.as_bytes()).err().unwrap().contains("cannot mark a hill"));
        let unknown = arena.replace(r#""format": 1,"#, r#""format": 1, "modes": ["from the future"],"#);
        assert!(parse(unknown.as_bytes()).err().unwrap().contains("modes this game does not have"));
    }

    fn sniper_file() -> String {
        format!(
            r#"{{
                "format": 1,
                "modes": ["sniper"],
                "solids": [{}],
                "spawns": [{{"at": [0, 0, -5], "facing": [0, -1]}}, {{"at": [3, 0, -5], "facing": [0, -1]}},
                           {{"at": [0, 0, -90], "facing": [0, 1], "role": "Sniper"}}],
                "finish": {{"center": [0, 2, -80], "half": [5, 2, 3], "facing": [0, -1]}}
            }}"#,
            cube([-10.0, -1.0, -100.0], [10.0, 0.0, 0.0]),
        )
    }

    #[test]
    fn a_sniper_map_has_a_spawn_for_the_sniper_and_a_finish_to_reach() {
        use crate::modes::MODE_SNIPER;
        let file = parse(sniper_file().as_bytes()).unwrap();
        assert_eq!(file.plays, [MODE_SNIPER]);
        let map = build(9, &file);
        assert!(map.course.is_none());
        assert_eq!(map.spawns.len(), 2, "the runners' spawns only");
        assert_eq!(map.posts(SNIPER_ROLE).count(), 1);
        let finish = map.zone(FINISH_ZONE).expect("a finish");
        assert!(finish.contains(Vec3::new(0.0, 0.9, -80.0)) && !finish.contains(map.spawns[0].pos));
        // Without what the mode needs, it says what is missing.
        let no_post = sniper_file().replace(r#", "role": "Sniper""#, "");
        assert!(parse(no_post.as_bytes()).err().unwrap().contains("no spawn for the sniper"));
        let no_finish = sniper_file().replace(r#""finish""#, r#""unused""#);
        assert!(parse(no_finish.as_bytes()).err().unwrap().contains("no finish for the runners"));
        // Nobody's spawn at all: nowhere for the runners.
        let only_post = sniper_file().replace(r#"{"at": [0, 0, -5], "facing": [0, -1]}, {"at": [3, 0, -5], "facing": [0, -1]},"#, "");
        assert!(parse(only_post.as_bytes()).err().unwrap().contains("no spawn"));
    }

    /// The example made by `blender/example_sniper_map.py` is a map for the
    /// sniper mode: runners at one end, the sniper up on the platform at
    /// the other, looking down the yard, with the finish under it.
    #[test]
    fn the_example_sniper_map_is_one() {
        use crate::modes::MODE_SNIPER;
        let bytes = std::fs::read("blender/example sniper.json").expect("the example map");
        let file = parse(&bytes).unwrap();
        assert_eq!(file.plays, [MODE_SNIPER]);
        let map = build(9, &file);
        assert_eq!(map.spawns.len(), 8);
        let post = map.posts(SNIPER_ROLE).next().expect("a spawn for the sniper");
        let finish = map.zone(FINISH_ZONE).expect("a finish");
        for spawn in map.spawns.iter().chain([post]) {
            assert!(map.is_free(spawn.pos, PLAYER_HALF), "a spawn inside something at {}", spawn.pos);
            assert!(!map.is_free(spawn.pos - Vec3::Y * 0.1, PLAYER_HALF), "a spawn in the air at {}", spawn.pos);
            assert!(!finish.contains(spawn.pos), "a spawn in the finish at {}", spawn.pos);
        }
        // The sniper is high up, far from the runners, facing them.
        let runners = map.spawns[0].pos;
        assert!(post.pos.y > runners.y + 10.0 && post.pos.distance(runners) > 150.0);
        let facing = crate::map::look_dir(post.yaw, 0.0);
        assert!(facing.dot((runners - post.pos).with_y(0.0).normalize()) > 0.9, "the sniper looks away from the yard");
        // Nobody is in the sniper's sights where they come in.
        let eye = post.pos + Vec3::Y * (crate::map::STAND_EYE - PLAYER_HALF.y);
        for spawn in &map.spawns {
            let to = spawn.pos - eye;
            assert!(map.raycast(eye, to.normalize(), to.length() - 0.5).is_some(), "the sniper sees the spawn at {}", spawn.pos);
        }
        // The finish is on the ground, under the platform.
        assert!(finish.contains(Vec3::new(post.pos.x, PLAYER_HALF.y, post.pos.z)));
    }

    /// A file can make a night of it: a dim blue moon, close fog, lamps and
    /// fires, and solids that glow. One that says nothing is lit as by day.
    #[test]
    fn a_map_file_says_how_it_is_lit() {
        let day = build(9, &parse(sniper_file().as_bytes()).unwrap());
        assert_eq!((day.look.sun, day.look.sun_color, day.look.ambient), (10000.0, [1.0; 3], 480.0));
        assert!(day.lamps.is_empty() && day.glows.is_empty());
        let night = sniper_file()
            .replace(
                r#""format": 1,"#,
                r#""format": 1, "sky": [0.02, 0.03, 0.06],
                   "light": {"sun": 1500, "sun_color": [0.6, 0.7, 1.0], "sun_from": [10, 40, -200], "ambient": 150, "fog": [30, 260]},
                   "lamps": [{"at": [0, 1.2, -40], "color": [1.0, 0.6, 0.3], "power": 400000, "range": 18, "flicker": true},
                             {"at": [5, 7, -60], "color": [1.0, 0.9, 0.7], "power": 900000}],"#,
            )
            .replace(r#""texture": "slab""#, r#""texture": "slab", "glow": 3.0"#);
        let map = build(9, &parse(night.as_bytes()).unwrap());
        assert_eq!((map.look.sun, map.look.sun_color, map.look.ambient), (1500.0, [0.6, 0.7, 1.0], 150.0));
        assert_eq!(map.look.fog, Some((30.0, 260.0)));
        // The moon is where the file says, far off.
        assert!(map.look.sun_from.normalize().distance(Vec3::new(10.0, 40.0, -200.0).normalize()) < 1e-4);
        assert_eq!(map.lamps.len(), 2);
        assert!(map.lamps[0].flicker && !map.lamps[1].flicker);
        assert_eq!((map.lamps[0].range, map.lamps[1].range), (18.0, 25.0));
        assert_eq!(map.glows, vec![([0.5, 0.5, 0.5], 3.0)]);
        // A sun under the ground, or fog that ends before it starts, is left as by day.
        let wrong = night.replace(r#""sun_from": [10, 40, -200]"#, r#""sun_from": [10, -40, -200]"#).replace(r#""fog": [30, 260]"#, r#""fog": [300, 260]"#);
        let map = build(9, &parse(wrong.as_bytes()).unwrap());
        assert_eq!((map.look.sun_from, map.look.fog), (day.look.sun_from, day.look.fog));
        let broken = night.replace(r#""power": 400000"#, r#""power": "bright""#);
        assert!(parse(broken.as_bytes()).is_err());
    }

    /// Every map shipped with the game can be played. A sniper map among
    /// them is laid out for the mode: the runners come in standing on the
    /// ground and out of the sniper's sight, wherever on the platform the
    /// sniper stands; the sniper is well above them and far away; and there
    /// is a way on foot from where each runner comes in to the finish.
    #[test]
    fn the_shipped_maps_can_be_played() {
        use crate::modes::MODE_SNIPER;
        let mut maps = 0;
        for path in std::fs::read_dir("assets/maps").expect("the shipped maps").flatten().map(|entry| entry.path()) {
            if !path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("json")) {
                continue;
            }
            let name = path.display();
            let bytes = std::fs::read(&path).unwrap();
            let file = parse(&bytes).unwrap_or_else(|why| panic!("{name}: {why}"));
            let map = build(9, &file);
            maps += 1;
            if !file.plays.contains(&MODE_SNIPER) {
                continue;
            }
            let post = map.posts(SNIPER_ROLE).next().expect("a spawn for the sniper");
            let finish = map.zone(FINISH_ZONE).expect("a finish");
            let stands = |pos: Vec3| map.is_free(pos, PLAYER_HALF) && !map.is_free(pos - Vec3::Y * 0.1, PLAYER_HALF);
            for spawn in map.spawns.iter().chain([post]) {
                assert!(stands(spawn.pos), "{name}: a spawn in something, or in the air, at {}", spawn.pos);
                assert!(!finish.contains(spawn.pos), "{name}: a spawn in the finish at {}", spawn.pos);
            }
            let runners = map.spawns[0].pos;
            assert!(post.pos.y > runners.y + 5.0 && post.pos.distance(runners) > 60.0, "{name}: the sniper is on top of the runners");
            // Wherever the sniper can walk to from the post, across the
            // platform and up to its edge, no runner shows where they come in.
            let towards = (runners - post.pos).with_y(0.0).normalize();
            let across = Vec3::new(-towards.z, 0.0, towards.x);
            const STEP: f32 = 0.25;
            let mut reached = std::collections::HashSet::from([(0_i32, 0_i32)]);
            let mut to_visit = vec![(0_i32, 0_i32)];
            while let Some((side, forward)) = to_visit.pop() {
                let at = post.pos + across * side as f32 * STEP + towards * forward as f32 * STEP;
                let eye = at + Vec3::Y * (crate::map::STAND_EYE - PLAYER_HALF.y);
                for spawn in &map.spawns {
                    for height in [-0.8, 0.0, 0.85] {
                        let to = spawn.pos + Vec3::Y * height - eye;
                        assert!(
                            map.raycast(eye, to.normalize(), to.length() - 0.3).is_some(),
                            "{name}: from {at} the sniper sees the spawn at {}",
                            spawn.pos
                        );
                    }
                }
                for next in [(side - 1, forward), (side + 1, forward), (side, forward - 1), (side, forward + 1)] {
                    let at = post.pos + across * next.0 as f32 * STEP + towards * next.1 as f32 * STEP;
                    if next.0.abs() < 400 && next.1.abs() < 400 && stands(at) && reached.insert(next) {
                        to_visit.push(next);
                    }
                }
            }
            assert!(reached.len() > 100, "{name}: the sniper has nowhere to walk");
            let nav = crate::nav::NavGrid::new(&map);
            for spawn in &map.spawns {
                let way = nav.path(&map, spawn.pos, finish.center);
                let last = way.as_ref().and_then(|way| way.last()).copied();
                assert!(
                    last.is_some_and(|last| (last - finish.center).with_y(0.0).length() < 4.0),
                    "{name}: no way on foot from the spawn at {} to the finish",
                    spawn.pos
                );
            }
        }
        assert!(maps > 0, "no maps in assets/maps");
    }

    #[test]
    fn broken_files_say_what_is_wrong() {
        assert!(parse(b"{").err().unwrap().contains("not a map file"));
        let no_finish = course_file().replace(r#""finish""#, r#""unused""#);
        assert!(parse(no_finish.as_bytes()).err().unwrap().contains("no finish"));
        let old = course_file().replace(r#""format": 1"#, r#""format": 0"#);
        assert!(parse(old.as_bytes()).err().unwrap().contains("export it again"));
        // A finish with no start is for the sniper mode alone.
        let no_start = course_file().replace(r#""start""#, r#""unused""#).replace(r#""format": 1,"#, r#""format": 1, "modes": ["ffa"],"#);
        assert!(parse(no_start.as_bytes()).err().unwrap().contains("a finish but no start"));
    }
}
