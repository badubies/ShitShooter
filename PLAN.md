# Plan

## Round 4: original weapons

Round 4 is done. The pistol, the SMG, the rifle and the sniper are built
here, and every model (the shotgun and knife too) is now built for the game
and placed as it is. Step 7 was dropped by the user. Keep this section as
the guide to the guns.

Replace the downloaded guns (rifle, SMG, sniper) with originals built by
scripts in `art/`, one gun at a time. The shotgun, the knife and all four
sights were made here already. Each new gun gets:

- reload and cocking parts the code can move;
- iron sights that line up and do not hide much of the view;
- a mount for the sights we made;
- a build at the right scale and facing, so that the code places it as it
  is, with no turning or scaling (the user asked for this specifically).

### Done so far

- `art/pistol.py` builds the pistol (`assets/models/weapons/pistol.glb`).
  It is a compact striker-fired design, a black slide on a dark grey frame,
  with three-dot iron sights. It takes the reflex and the 2x; the red dot is
  too big for its slide. The downloaded M1911 is gone.
- `art/smg.py` builds the SMG (`assets/models/weapons/smg.glb`), a compact
  PDW 410 mm long at true size: a black upper with a full-length rail on a
  dark grey lower, the magazine (curved, 36 rounds) ahead of the trigger
  guard, the stock folded along the right side, below the ejection port. Its
  `Bolt` shows through the port and carries the charging handle, which
  sticks out of a slot on the left, the side seen from the hip, so cocking
  shows. Irons: a ghost ring (`APERTURE_RADIUS`, `RING_RADIUS`) and a post
  with an orange dot between guard ears. It takes the red dot, reflex and
  2x. The downloaded MP7 is gone.
- `art/rifle.py` builds the rifle (`assets/models/weapons/rifle.glb`), a
  carbine 840 mm long at true size, built like the SMG: one top rail over
  the upper and an octagonal slotted handguard, a curved 30-round magazine
  ahead of the trigger, a slotted flash hider, the same bolt and charging
  handle, and a solid stock with a rubber cheek pad (it was a skeleton at
  first; the user wanted more body). It takes the red dot, reflex and 2x. The
  downloaded M4A1 is gone.
- `art/sniper.py` builds the sniper (`assets/models/weapons/sniper.glb`), a
  bolt-action rifle 1200 mm long at true size: a black round receiver and
  heavy barrel in a dark aluminium chassis, a slotted handguard under one
  full-length rail, a folded bipod, a ported muzzle brake, a five-round box
  magazine, and a solid stock with an adjustable cheek riser on posts. Its
  bolt is worked by hand (see the node names below). It takes the red dot,
  reflex, 2x and scope. The downloaded SCAR is gone.
- Every magazine reload ends by cocking the gun (slide or bolt drawn back,
  slowly, then let go); see the node names below.
- Every weapon model is placed as it is (`Model` in `src/weapons.rs`):
  metres, `size[2]` long, barrel along -Z, centred. `fit_models`
  (`src/gun_model.rs`) warns when a model's length or centre is off. The
  shotgun and knife (`art/shotgun.blend`, `art/knife.blend`, made by hand)
  had the fit the game used to do for them (turned half round, scaled,
  centred) baked into their files, and `Fit` is gone.
- The sights face forward (+Y in Blender) as the guns do, so a gun's
  `SightMount` puts them on as they are, with no turn. `art/sights.py`
  builds the reflex and 2x that way; the red dot and scope
  (`art/red_dot.blend`, `art/scope.blend`, made by hand) were turned half
  round in their files. In the game everything looks as it did.
- Iron sights with an `AimPoint`. A gun's own `AimPoint` node (outside any
  sight) becomes `Fitted::iron_aim`. Aiming down the irons holds it
  `sight_relief` in front of the eye, as for a sight's dot, and hides the
  crosshair.
- `art/preview.py` renders a gun in Blender: its sides, down its sights, its
  reload pose, and each sight mounted and looked through. It also prints
  where its nodes are. See "Checking a gun" below.
- The self-test photographs every gun aimed down its irons too. It draws off
  screen, so covering its window does not spoil the pictures. Its window
  opens without taking focus.
- A lobby setting, "Bots: Shoot / Hold fire", in the match picker
  (`ClientMsg::SetBotsHoldFire`, and `--bots-hold-fire` on the command line).
  Bots holding fire still move and turn to their enemies. Protocol bumped
  to 15.

### What the user wants (learned on the pistol)

- No tan or brown. Dark finishes: a black slide or receiver, dark grey
  polymer.
- Get the details right. The pistol's trigger first curled the wrong way:
  its tip must curl forward, towards the muzzle.
- Iron sights must not hide much of the view. The pistol's rear blade is
  narrow and low, with its top corners cut away. Its notch is wide (7.2 mm),
  with the edges close to the white dots, so little metal stands between the
  dots and the front post. The front post is 4.4 mm wide with a big orange
  dot. Show the user the sight picture before calling a gun done.
- They keep using the computer while tests run. Use the self-test (it draws
  off screen) and Blender in the background; never take focus.
- Run the self-test with `--bots-hold-fire`, or with `--bots 0`, unless the
  check needs bots that shoot (only the hurt and dead pictures do).

### Decided: how big to build the long guns

The user chose this; do not ask again. Every gun is designed at true size and
shrunk, as a whole, to the length the game holds it (`size[2]` in
`src/weapons.rs`). Its `SightMount` is shrunk by the same factor, so a sight
(built in true metres) keeps its true size next to the gun. The scale lives
in the model file, never in code. `art/pistol.py` shows how; there
`SCALE` is 1, because the game holds the pistol at a real compact's length.

In the gun's script:

```python
LENGTH = 200.0            # size[2] in src/weapons.rs, in mm: how long the game holds it
TRUE_LENGTH = 410.0       # how long the design is at true size
SCALE = LENGTH / TRUE_LENGTH
MM = 0.001 * SCALE        # every vertex and position goes through MM
...
empty("SightMount", at, parent, scale=SCALE)
```

Lay everything out in true millimetres; the script's closing check (the
middle on the origin, length `LENGTH`) stays as it is in `pistol.py`. Use
these true lengths. Each matches how big the sights look on the gun it
replaces today, so the sights' look does not change:

| Gun | `size[2]` | `TRUE_LENGTH` | `SCALE` | What to design |
|---|---|---|---|---|
| SMG | 200 mm | 410 mm | 0.488 | a compact PDW, stock folded |
| Rifle | 300 mm | 840 mm | 0.357 | a carbine with a top rail |
| Sniper | 440 mm | 1200 mm | 0.367 | a bolt-action rifle |

Size the iron sights by how they look from the eye, not by real sizes. The
pistol's look good:

- **Notch:** about 2.3 degrees wide.
- **Post:** about 0.8 degrees wide.
- **Light gap:** clear either side of the post.

Work them out in game metres (true mm times `SCALE`). Aiming holds the
`AimPoint` (the rear aperture or notch) `sight_relief` from the eye: 0.08 m
for the long guns. The front post is that plus the sight radius away. Angle
in degrees is about 57.3 times width over distance. On the rifle that
comes to:

- a rear aperture about 3.2 mm across in game metres, 9 mm true;
- a post about 4.6 mm in game metres, 13 mm true, some 0.33 m from the eye.

Check with `art/preview.py`'s `irons_zoom.png` and show the user. Keep the
rear sight's frame thin, as on the pistol. The camera's near plane is
0.01 m, so nothing within 1 cm of the eye is drawn.

### A long gun's stock

Aimed, a gun's aim point is held `sight_relief` (0.08 m) from the eye, which
for a long gun is further forward of the eye than a real one is held. So
the front half of its stock is ahead of the eye, right under the sights,
and shows as a wedge up the bottom of the view. A solid stock on a buffer
tube in line with the bore filled the bottom of the screen on the rifle.
What fixed it:

- the comb dropping away from the receiver towards the butt, and narrow
  along the top (about 14 mm, widening only 15 mm or more below it), so
  from the eye the stock is a strip no wider than the receiver. A skeleton
  of two narrow bars did this too, but the user found it too skeletal, so
  the rifle's stock is now solid, full below the comb, with pockets, a
  ribbed panel, a cheek pad, a sling slot and sockets;
- the rear sight well forward of the back of the rail (30 mm), which brings
  the whole gun back towards the eye, so more of the stock is behind it;
- the sight line high enough over the rail (80 mm above the bottom of the
  upper on the rifle) that the front sight's base stays under the ring.

The sniper goes further: its sights sit well forward on the rail (the rear
iron 150 mm and the mount 180 mm ahead of the back of the receiver), so the
eye is over the back of the receiver and the stock is all behind it. Aimed,
none of it shows. The scope still sits over the action, clear of the bolt
handle. If a stock still showed too much, a shorter `sight_relief` for the
irons alone (a new field) would put it behind the eye.

### How a gun is built (follow `art/pistol.py`)

- Lay it out in true millimetres with whatever zero is handy (the pistol
  uses the slide's rear bottom edge). `MM` (`0.001 * SCALE`) turns every
  vertex and position into game metres, so the gun comes out shrunk to
  `LENGTH` (see above).
- In Blender, X is right, Y forward along the barrel and Z up. The Y-up
  export makes that the game's barrel along -Z, up +Y.
- At the end, move everything so the middle of the bounds is the origin, and
  assert the length equals `size[2]` (the pistol script does both).
- Shapes are prisms of 2D outlines (`Part.prism`), bars along a line
  (`Part.band`), lofts through rings (`Part.loft`, the grip), and tubes
  (`Part.tube`, the barrel). Exact booleans (`boolean()`, baked at once) do
  the cut-outs: serrations, the ejection port, the muzzle.
- Materials are plain, not metallic, colours linear (`material()`).
  Emission on the sight dots.
- Parts that move are their own objects, origins at their pivots. Anything
  that rides on a part is its child: the pistol's sights, `AimPoint` and
  `SightMount` are children of `Slide`, the SMG's charging handle of
  `Bolt`; the SMG's sights, on a fixed rail, have no parent. `Part.to_object(name, origin=...,
  parent=...)` takes the origin as (x, y, z) in layout millimetres. Getting
  the order wrong once put the trigger's pivot 72 mm out to the side.
- Node names the code looks for (`PART_NAMES` in `gun_model.rs`):
  - `Magazine`, origin at its top, axes along its own slant.
  - `Slide` or `Bolt`: kicked back `SHOT_KICK * length` by each shot, held
    back by its catch (`BOLT_CAUGHT` of its travel) when empty. Its travel
    is `SLIDE_TRAVEL` or `BOLT_TRAVEL` times the length (`src/player.rs`).
    Every magazine reload cocks it: the magazine is changed quickly, then
    it is drawn all the way back, slowly (`BOLT_PULL` to `BOLT_PULLED`),
    while the gun is pushed forward against the pull, and let go at
    `BOLT_HOME`. The user asked for a slow pull and a long travel. Each step has its sound
    (`Sound::BoltPull`, `Sound::Bolt`).
  - `Pump`, and `Shell` for guns that load round by round.
  - `IronSight...`: everything hidden while a sight is on.
  - `AimPoint`: the iron sights' aim point.
  - `MuzzlePoint`: the middle of the muzzle, where the tracers of the gun's
    shots start (`Fitted::muzzle`; not on a part that moves).
  - `SightMount`: where a sight's base sits, scaled by `SCALE`. Not turned:
    the sights face +Y like the gun.
  Name other parts too (`Trigger`, `SlideStop`, `MagRelease`, a charging
  handle...), with sensible pivots, for animations to come.
- `src/weapons.rs`: point the gun's `Model` at the new file and list its
  `sights`. The long guns take the red dot,
  reflex and 2x; the sniper takes the scope too. Every gun must take `TWO_X`
  (a test checks it). Then delete the downloaded model and its `.blend`
  (`git rm`): git keeps them.
- Build: `/Applications/Blender.app/Contents/MacOS/Blender --background
  --factory-startup --python art/<gun>.py -- .` (from the repo root). It
  saves `art/<gun>.blend` and exports `assets/models/weapons/<gun>.glb`.

### What moves today, and what the new guns need

`follow_camera` in `src/player.rs` drives the first-person parts; other
players' guns only drop their magazine (`remote.rs`).

- SMG and rifle: `Magazine` and a `Bolt` (the bolt carrier showing through
  the ejection port, or the charging handle) work as they are. On the SMG
  the handle is a child of the bolt; its slot is cut long enough for
  `BOLT_TRAVEL`.
- Sniper: a bolt action (`Cycle::Bolt`). Its `Bolt` has its origin on the
  bore and the handle (on the right) as a child. After each shot it is
  turned up about the bore by `BOLT_LIFT` and drawn back
  `BOLT_ACTION_TRAVEL` from `BOLT_BACK`, stays open, and is pushed home and
  turned down by `interval - BOLT_LOCK` (`bolt_cycle` in `player.rs`). A
  magazine reload cocks it the same way, slowly (`bolt_cocking`), and ends
  with `Sound::BoltHome`, not the slam of a bolt let go. It is not kicked by
  a shot or held back when empty. `BOLT_LIFT_TIME`, `BOLT_DRAW_TIME` and
  `BOLT_PUSH_TIME` are the beats `Sound::BoltBack` and `Sound::BoltHome`
  keep to.
- Not animated yet on any gun: the trigger, slide stop and magazine release.
  A trigger pull per shot would be cheap.

### Checking a gun

1. Run the build script. It prints the size and asserts the length.
2. Render it:
   `Blender --background --factory-startup --python art/preview.py --
   <gun.glb> <out dir> <sight_relief> assets/models/sights/reflex.glb
   assets/models/sights/2x.glb`
   Paths are absolute or from the repo root; the out dir goes in the
   scratchpad. Look at `irons_zoom.png` (the sight picture), `reload.png`,
   the sides, and `sight_*.png` for each sight on its mount. It prints
   where `Slide`, `Magazine`, `AimPoint` and `SightMount` are in the game's
   axes: -Z is forward.
3. In the game: `cargo run -- --selftest <dir> --host --bots-hold-fire`.
   The gallery is `gun_<name>_<hip|aimed|irons|reload>.png`, taken during
   the countdown. It fits 6 frames for each of 20 views into 2.6 s, so do
   not add views without making room.
4. `cargo build` without new warnings, then `cargo test --no-default-features`.
   Commit straight to `main` (do not push), one gun a commit.

### Pitfalls met so far

- Do not run `cargo fmt`: the repo is not formatted with it, so it would
  touch every file. Indent by hand.
- A `Screenshot` of an image must name the camera's target exactly, scale
  factor and all (`Screenshot(RenderTarget::Image(...))`). With
  `Screenshot::image(handle)` (scale 1) it reads a texture nothing drew
  into, and the picture is black.
- Changing `ClientMsg` or `ServerMsg` means bumping the protocol id in
  `protocol_id` (now `shitshooter-proto-15`).
- `dropped magazines: []` at the end of the self-test is how `main` already
  was; it is not a regression.
- The red dot and scope were built here, in Blender by hand
  (`art/red_dot.blend`, `art/scope.blend`), with no scripts. The reflex and
  2x come from `art/sights.py`.

### Steps

1. [x] Pistol (`art/pistol.py`).
2. [x] The long guns' scale: decided, see above.
3. [x] SMG (`art/smg.py`).
4. [x] Rifle (`art/rifle.py`).
5. [x] Sniper (`art/sniper.py`), with its bolt action.
6. [x] The shotgun and knife re-exported as built for the game, the sights
   turned to face +Y, the mounts' turn dropped, and `Fit` removed.
7. [-] Dropped (the user's call): a trigger pull per shot; other players'
   slides and bolts moving.

## Round 3: Hamlet, a close-quarters map

A village about the size of Depot (68 m inside its town wall), for free for
all and team deathmatch only (`map::plays`; the lobby's mode and map
buttons skip what does not go together, `--map` refuses it). Built from the
Valley's models: a warehouse, a barn, two tall houses, a dozen houses and
two ruins to run through, with walled yards, stalls, containers and
woodpiles between them, on a low rise with the square on top.

- [x] `src/hamlet.rs`: the layout, levelled plots, the outskirts (decor
  only), spawns spread over the open ground facing their longest view,
  armor up on the mezzanine, loft, both upper floors, the square's house
  and the terrace.
- [x] Its tests hold it to the brief: every spot outdoors within 5 m of
  cover, no wide eye-to-eye sightline of 42 m or more (Depot: 90 spots in
  the open, lines of 64 m), nothing standing in anything else, bots
  getting from every spawn to every other.
- [x] Bots no longer cut corners into door frames and walls (it got them
  stuck for good in Hamlet's narrow lanes): `nav::pass_waypoints` only
  drops a waypoint early when a player can run straight at the next one,
  straightened paths are checked with a whole player, and waypoints are
  moved off door frames.
- Protocol bumped to 13: older clients would load a map they do not know
  as Arena.

Worth knowing: `HAMLET_PREVIEW=out.ppm cargo test --no-default-features
hamlet::tests::preview -- --ignored` draws it from above (cover heights,
spawns, armor, open spots and the longest sightlines);
`hamlet::tests::print_survey` prints the sightlines next to Depot's.

## Round 2: a big map with capture points

A Battlefield-style map, much bigger than Arena and Depot (320 m across,
playable), with hills, roads, a creek, buildings you can enter, rocks and
trees, and a new mode, Conquest, played on capture points.

1. [x] Map engine: a heightfield `Terrain` in `Map` (walking, bullets,
   grenades, line of sight), step-up onto low ledges and stairs, snapping
   down slopes and stairs, a grid so thousands of blocks stay fast.
2. [x] Assets: `art/map_props.py` builds every building, rock, tree and prop
   in Blender, exports `assets/models/map/*.glb`, and writes their collision
   boxes to `src/props.rs` (generated: the server has no renderer).
3. [x] The map, "Valley": terrain, roads, creek, five flag sites, two team
   bases, scattered trees and rocks; drawn with a vertex-coloured terrain
   mesh, a daylight sky and fog.
4. [x] Conquest mode: five flags, tickets (a death costs one, holding more
   flags bleeds the enemy), spawning at your base or flags you hold; bots
   path-find to flags (`src/nav.rs`); HUD flag row, capture bar and flag
   markers. Small maps get three flags too.
5. [x] Minimap, and picking where to spawn on the death screen.

Done so far, worth knowing:

- Colliding models only turn by quarter turns, so their boxes stay upright.
  Roofs collide as a stepped shell, hollow inside. Stairs need a metre of
  floor in front of the first step: you cannot step onto a stair from the
  side (the box spans two steps, 0.67 m).
- `src/valley.rs` tests walk a player up the stairs of the tall house, the
  barn loft, the warehouse mezzanine and the chapel tower to the belfry.
- Re-run `art/map_props.py` after changing a model; it rewrites
  `src/props.rs`. The selftest plays on Valley with `--map depot` (it
  presses "next map" once); add `--port` if 7777 is busy.

- Conquest: `src/conquest.rs` (client), `update_conquest` and
  `conquest_spawn_group` in `src/server.rs`. Bots path-find with `src/nav.rs`
  (0.5 m cells over `map.bounds`, built when the server picks the map, about
  12 ms for Valley). The selftest in conquest ends standing on a flag
  (`flag.png`) and on the death screen (`deploy.png`, faked on the client).
- `src/minimap.rs` paints the map from above when it loads (256 px). In
  conquest the death screen shows it bigger, and a click on the base or a
  held flag sends `ClientMsg::SpawnAt`; the server keeps it until changed,
  and spawns at the base while that flag is not held.

Decided (first guesses, tuned in play): flags capture in 10 s for one
player, faster with more (up to 3); 250 tickets a team; holding more flags
bleeds the other team 0.25 tickets a second per flag of difference.
All five flags start neutral; you spawn at your base or a flag your team
holds, mostly the one nearest a flag still to take, never one with an
enemy on it.

---

## Round 1 (done)

Ideas from the GitHub discussions and PR #4, in the order to build them.
Each step is its own commit to `main`. Tick a step off when it is done.

Already done: #1 paste the lobby address (4f389e0). Air strafing (#5) is in,
see `src/player.rs` `AIR_WISH_SPEED` / `AIR_ACCEL`; only tuning is left (step 9).

Picked from PR #4: **loadout layout 1 (Arrows)**, **weapon panel 1 (Compact)**,
**reload style 2 (Ring)**.

## Before you start

- **Bevy 0.20.1.** Many APIs differ from older Bevy versions. Check names
  against the sources in `~/.cargo/registry/src/*/bevy_*-0.20.1/`, not from
  memory.
- **Build and test:** `cargo build`, then `cargo test` (the tests are in
  `src/server.rs` and other modules). Fix every warning the change adds.
- **Check it in the game.** `cargo run -- --host --bots 3` starts a lobby with
  bots, skipping the menu. `cargo run -- --selftest <dir>` plays a short match
  against bots by itself and saves screenshots to `<dir>` (`src/devtools.rs`;
  add `--host` for the match, without it only the menus; it plays against 3
  bots, or `--bots <N>`, and `--bots-hold-fire` keeps them from shooting, for
  pictures that dying would spoil; the pictures of being hurt and dead need
  bots that shoot). Its window opens
  without taking focus, to watch or to leave behind other windows: it draws
  off screen, so covering it does not spoil the pictures;
  extend its script when a step adds something to look at. F12 saves a
  screenshot to `screenshots/`. Feel and tuning (steps 3, 7, 9) need the user
  to play: ask them.
- **Git:** commit straight to `main`, never on a branch. One step, one
  commit, or a few if a step is big. Do not push unless the user asks: every
  push to `main` builds a release that the launcher installs for every player.
- **Commit messages:** look at `git log`. The subject is one plain sentence
  about what changes for the player; the body says why. End with the
  `Co-Authored-By` line the session gives you.
- **Blender 5.3:** `C:/Program Files/Blender Foundation/Blender 5.3/blender.exe`
  (on the Mac, 5.2: `/Applications/Blender.app/Contents/MacOS/Blender`).
  Run scripts with `--background --factory-startup --python <script> -- <args>`;
  `art/grenades.py` is an example. Export with the settings it uses:
  `export_format="GLB", export_yup=True, export_apply=True`. Colours in
  scripts are sRGB and must be made linear for Blender (see `material()`).
- **glTF guns** (`M4A1`, `MP7`, `SCAR`) are JSON with the
  mesh data embedded as base64, so a node can be renamed with a text edit.
- **Prototype branch:** steps 4 and 5 read REDXICAN's prototype. `pull/4/head`
  still works after the PR is closed.

## Order

1. [x] Damage direction (#2)
2. [x] Bodies stay after death (#3)
3. [x] Shotgun choke when aiming (#7, part 1)
4. [x] Loadout menu, layout 1 (PR #4)
5. [x] Weapon panel, type 1 with the ring reload (PR #4)
6. [x] Blender pass: magazines, shotgun sight mount, 2x sight (#7 part 2, reload prep)
7. [x] Reload animations on the guns
8. [x] 2x sight on every gun, and zoom set by the sight (#7, part 2)
9. [x] Air strafe tuning (#5)

Later, not now: surf ramps (#6), see the last section.

Why this order: 1–3 are small and need no network or model changes. 4 comes
before 5 because it builds the gun preview camera, and the panel needs gun
images from it. All Blender work is done in one pass (6) before the code that
needs it (7, 8).

---

## Decided

- Shotgun reload: only firing or switching stops a shell reload. Aiming does
  not stop it, and you cannot aim until it is done (as for every reload today).
- Bodies tip over onto the floor (a short animation, no ragdoll physics). They
  do not block players or bullets. Bots' bodies stay too.
- Reloading a magazine gun drops the old magazine. It falls to the floor and
  disappears after a few seconds, and it does not collide with players. The
  shotgun drops nothing.
- Model edits: where a magazine is already its own node, rename it in the
  glTF file directly. Use Blender (import, edit, export) only where it is
  merged into another mesh, so the other guns are not changed by a re-export.
- Every number in this plan (choke spread and time, how long bodies stay,
  zooms, time per shell) is a first guess, tuned in play.

## 1. Damage direction (#2)

"Subtle, like Apex."

- On `ServerMsg::Hit` with `victim == me` and `attacker != me`, take the
  attacker's position (their `RemotePlayer` transform; bots are remote
  players too) relative to our yaw.
- Show a thin arc on a ring around the crosshair, pointing at them, and fade it
  over about 1 s. Several hits from different sides each get their own arc.
  The arc keeps pointing at where the hit came from as we turn.
- Grenade hits: `ServerMsg::Hit` says nothing about grenades, but a frag's
  `ServerMsg::Explosion { pos, .. }` arrives with it. If we got an explosion
  within `GRENADE_RADIUS` of us in the last ~0.2 s, point at its `pos`, not at
  the thrower.
- Files: `src/player.rs` (next to `damage_flash`, line ~386), new overlay in
  `src/ui.rs` (next to `Overlay::DamageFlash`). No protocol change.
- Check: with `--bots`, get shot from the side and from behind, and by a
  frag.

## 2. Bodies stay after death (#3)

Today a dead player is just hidden (`src/remote.rs`, ~line 168), so there is
nothing to teabag.

- When a remote player goes from alive to dead, spawn a copy of their body
  where they died: lying flat on the floor, in their paint job, no gun.
- Keep it about 8 s, then sink it into the floor and despawn it. Despawn
  bodies when the match ends (`DespawnOnExit`).
- Crouching is already sent and shown, so the teabag works once bodies exist.
- Client only. No protocol change. A player who respawns at once (a click)
  must not take their body with them: the body is its own entity, not the
  `RemotePlayer`.
- Check: kill a bot; the body lies where it fell, stays, then sinks, while
  the bot respawns elsewhere.

## 3. Shotgun choke (#7, part 1)

Tighter pellets the longer you aim, like a choke.

- New `Weapon` field, e.g. `choke: Option<Choke { spread, time }>`. Only the
  shotgun has one.
- While aimed, the cone goes from `aim_spread` down to the choke spread over
  `time` (about 0.5 s). Unaiming or firing resets it.
- Spread is picked on the shooter's side (`src/combat.rs`, ~line 338), so the
  server does not change.
- Feedback: the crosshair today is only a dot (`src/ui.rs`, ~line 1440). For
  the shotgun while aiming, add a thin ring around it that shrinks with the
  spread, and a soft click (a new `synth_*` sound in `src/audio.rs`) once
  fully choked.
- Start from: choke spread 0.015 (aim spread is 0.045), 0.5 s.
- In the game it is just "Shotgun". No Apex names.

## 4. Loadout menu, layout 1 (PR #4)

Read the prototype first. Get the branch with
`git fetch origin pull/4/head:loadout-ui-prototype`, then read files with
`git show loadout-ui-prototype:prototypes/loadout-ui/<file>`. Start with
`README.md`, then `types.js` (type `arrows`), `loadout.js` and `viewer.js`.
Do not add the `prototypes/` folder to `main`.

- Replaces `loadout_button`, `sight_button` and `grenade_button` in
  `src/ui.rs`, in the pause menu, the lobby and the death screen.
- Death screen: today it is one line of text (`Hud::DeadLoadout`), the
  weapon and grenade keys step forward through the choices (`while_dead` in
  `src/player.rs`), and the mouse stays locked so a left click respawns. With
  the card:
  - Show the same card under the respawn countdown, 3D view included.
  - Free the mouse while dead, so the arrows can be clicked. Lock it again on
    respawn (`grab_cursor` in `src/player.rs`).
  - Respawn now with a "Respawn" button under the card, or a left click
    anywhere off the card. Say which in the countdown line.
  - Keep the weapon and grenade keys as shortcuts.
  - An automatic respawn at the end of the countdown closes the card. A pick
    is saved at once, so nothing is lost.
- A 3D view of the gun at the top, with its chosen sight on it.
- Each slot has ‹ › arrows for the gun and small arrows for the sight; the
  grenade has ‹ ›. Pointing at an arrow previews the gun or sight it would
  give, before the click.
- Rules stay as in `src/settings.rs`: never the same gun twice (taking the
  other slot's gun swaps them), and each gun keeps its own sight. A gun with no
  mount shows "No sight mount".
- 3D view: a second `Camera3d` that renders to an `Image`, shown with an
  `ImageNode`. The gun goes on its own render layer. Reuse the fitting steps
  from `src/gun_model.rs`.
- Also render a small side image of each gun (with its sight) for step 5.
- Pause menu card stays the size it is now. In the lobby it may be about
  100 px taller.
- Esc while dead opens the pause menu as today; the card in it and the one on
  the death screen must not both take clicks.
- Check at 1280×720 and 1920×1080: pause menu, lobby and death screen.
  Picks must still be saved (`Settings::save` in `src/settings.rs`; the
  old buttons go through `cycle_gun` and friends), and the grenade must still
  reach the server (`ClientMsg::Loadout`, sent from `src/net.rs`; guns and
  sights are the client's own).

## 5. Weapon panel, type 1 with the ring reload (PR #4)

Prototype: `hud.js`, `hud.css` (type `compact`, reload `ring`).

- Replaces the bottom right weapon panel in `src/ui.rs` (`slot_chip`,
  `grenade_chip`, `Hud::Ammo`, `Bar::Ammo`).
- One row: grenade icons, the image of the gun in hand (from step 4), ammo.
- Only the gun in hand shows. On a switch, a strip with all three slots shows
  for about 1.5 s, then fades out. The gun image slides in on a switch.
- Grenades: the word GRENADE and one icon per grenade. A thrown one goes grey,
  fills from the bottom while it recharges and pulses when ready.
- Ammo: a little dim when full, orange at a quarter or less (the same rule as
  today), a short kick on each shot.
- Ring reload: a ring fills around the seconds left. Needs a round progress
  shape: a UI material, or a ring texture with a fill.
- The panel fades after 3 s without an action, but never during a reload or
  with low ammo.
- Once 4 and 5 are in: close draft PR #4 with a thank-you comment that links
  the commits (agreed, no need to ask).

## 6. Blender pass

What the models have today:

| Gun | Model | Magazine | Notes |
|---|---|---|---|
| Rifle | `M4A1.gltf` | own node `Mag` | ready, just rename to `Magazine` |
| SMG | `MP7.gltf` | unknown (nodes are `Cube.0xx_low`) | find it |
| Shotgun | `Shotgun.glb` | tube (`MagTube`, `MagCap`), loads by shell | `Pump` is its own node; needs a shell mesh; no `SightMount` |
| Sniper | `SCAR.gltf` | unknown (`Cube.0xx`); `Cube.002` hangs below the receiver, likely the mag | find it |
| Pistol | `M1911Tactical.gltf` | unknown (`Cube.0xx`); `Cube.006` sits inside the grip, likely the mag | find it; the slide could lock back too |

Only the shotgun, knife and sights have `.blend` sources in `art/`. For the
glTF guns, import into Blender and save a `.blend` in `art/` so later edits have
a source.

For each gun:

- Find the magazine. If it is its own node, rename it `Magazine` in the glTF
  file, without Blender. If it is merged into another mesh, separate it in
  Blender (Edit Mode → P → Selection), name it `Magazine` and re-export.
- Set the magazine's origin to the top of the magazine, where it meets the gun,
  so it can slide out cleanly.
- Optional parts the code can animate, if they are their own objects: `Bolt`
  (rifle, SMG, sniper), `Slide` (pistol).
- Shotgun: add a `SightMount` empty on top of the receiver, rename the bead to
  `IronSight...`, and model a small `Shell` (its own file, or a hidden node).
- New 2x sight: `art/2x.blend` → `assets/models/sights/2x.glb`, with an
  `AimPoint` empty at the dot, like the other sights. It goes on every gun,
  so keep it small enough to suit the pistol too.
- Re-export, then check every gun still sits right in first person, aimed and
  at the hip (the fitting measures the model, so a moved part can shift it).
- Leave the `.blend1` backups alone; `.gitignore` already skips them. Commit
  the new `.blend` files with the exported models.

Done. What the models have now:

- Every magazine was already its own node (rifle `Mag`, SMG `Cube.015_low`,
  sniper `Cube.004`, pistol `Cube.013`; `Cube.002` and `Cube.006` were the
  grip and the backstrap). Each now hangs under a new empty `Magazine` at the
  top of the magazine, as `MagazineBody`, so no vertex moved.
- Pistol: `Cube.001` is the slide, now `Slide`. No gun has a separate bolt.
- Shotgun: the `PumpRib*` are children of `Pump`, so moving `Pump` moves them;
  `Bead` is `IronSightBead`; `SightMount` sits on the receiver (the file is in
  metres, so scale 1); `Shell` (a 12 gauge shell along the barrel, brass to
  the back) hides inside the receiver, so the code must hide it.
- `art/m4a1.blend`, `mp7.blend`, `scar.blend`, `m1911.blend` are imports of
  the glTF files, as sources. The reflex and 2x sights are built by `art/sights.py`.
- The self-test now looks at every gun from the hip and aimed during the
  countdown (`gun_<name>_<hip|aimed>.png`).

## 7. Reload animations on the guns

- `src/gun_model.rs`: when fitting a model, also find nodes named `Magazine`,
  `Bolt`, `Slide`, `Pump` and keep their entities and rest transforms.
- `src/player.rs` view model: drive them from reload progress (`reload`, line
  ~717). Mag guns: tip the gun (exists), the magazine drops out and away, a new
  one comes up and goes in (reuse the `magazine_out` curve that the box guns
  use), then the bolt or slide snaps forward.
- The old magazine could drop as a short-lived prop that falls to the floor.
- Shotgun (decided): a real shell-by-shell reload.
  - Each shell takes about 0.4 s and adds one round. It keeps going until the
    tube is full; firing (or switching) stops it after the shell in progress.
  - An empty tube ends with the pump racked; a partial one does not.
  - `Weapon` gets a way to say "per shell" (e.g. `reload_per_round: bool`,
    with `reload` meaning the time per shell). `src/player.rs` reload logic
    adds one round per step instead of filling the magazine at the end.
  - Server: it does not track players' ammo, only bots' (`src/server.rs`,
    bot reload ~line 1515). So only the bots' reload changes there: they load
    shell by shell too.
  - The HUD ring from step 5 fills per shell, or shows shells left to load.
  - The shell mesh from step 6 is pushed into the loading port each step.
- Remote players (`src/remote.rs`): at least move the magazine on their gun
  too, so a reload reads from across the map.
- Reload sounds per step (mag out, mag in, bolt, shell in, pump). There are
  no sound files: every sound is made in code in `src/audio.rs`, so these are
  new `synth_*` functions.

## 8. 2x sight on every gun, and zoom set by the sight (#7, part 2)

2x sight:

- `src/weapons.rs`: add `TWO_X` to `SIGHTS` (no overlay) and to the `sights`
  list of every gun (not the knife).
- The loadout from step 4 then shows it with no extra work.
- Called "2x sight" in game, not "HCOG".
- Step 6 must check the 2x model fits every gun's `SightMount` (size, height,
  and that the dot lines up when aimed on each one).

Zoom comes only from the sight, never from the gun:

- Today each gun has its own `Weapon::zoom` (0.55–0.8) and only the scope
  overrides it (`LocalPlayer::zoom` in `src/player.rs`).
- Remove `Weapon::zoom`. Make `Sight::zoom` a plain `f32` that every sight
  sets, and have `LocalPlayer::zoom` read only the sight.
- Values to start from: iron sights 0.85, red dot 0.75, reflex 0.75, 2x 0.5,
  scope 0.25. So the same sight zooms the same on every gun.
- The sniper on iron sights gets the plain iron sight zoom (0.85) like every
  other gun (decided). Its default sight stays as it is.
- Aim sensitivity already follows `zoom()`, so it needs no change. Check
  everything else that reads `zoom` (bots, third person, the loadout
  preview), and the help text in `src/ui.rs` if it mentions zoom.

## 9. Air strafe tuning (#5)

- This step is done with the user: it is about feel, which only playing
  shows. Offer them two or three settings to try against the videos in
  discussion #5 (`gh api graphql` can read it), by tuning `AIR_WISH_SPEED` and
  `AIR_ACCEL` in `src/player.rs`.
- Optional: a host setting for air acceleration in the lobby, sent to every
  player (like `sv_airaccelerate`). Movement runs on each client, so the value
  must reach all of them.

Done. The lobby has an "Air accel" row (10, 20, 30, 100; `--airaccel` for a
dedicated server), sent with the roster (`AIR_ACCEL_CHOICES` in
`src/protocol.rs`). Played and picked: 20 is the default "for now".
`AIR_WISH_SPEED` stays 1.0. Worth knowing for later tuning: movement runs
once per rendered frame, so at a high frame rate the air acceleration bites
less per frame than at 60 fps (Source avoids that with a fixed tick).

## Later: surf ramps (#6)

Not in this round. Notes for when it is picked up: the map is only upright boxes (`Block` in `src/map.rs`), and `move_and_slide`
stops each axis that hits one, so a slope needs a new shape everywhere the map
is used:

- A ramp primitive (a wedge, or a box turned about one axis).
- Player movement: slide along the surface (clip velocity to the plane). On a
  slope steeper than about 45° there is no friction and you do not count as
  standing, so air strafing keeps you on it.
- Grenades (`step_grenade`), bullets (`raycast`), the server's checks and
  bots' line of sight.
- Drawing it, and a surf area in the map, maybe as its own test map first.
