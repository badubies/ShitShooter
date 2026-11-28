# Making maps in Blender

Build a map in Blender, export it into the game's `maps` folder, and pick it in the lobby. It's listed after the built-in maps. When you host it, everyone who joins is sent the map, so only the host needs the file.

## Setup (once)

1. Blender 4.2 or newer: **Edit → Preferences → Add-ons**, the menu at the top right → **Install from Disk…**, and pick [`shitshooter_maps.py`](shitshooter_maps.py). Tick **ShitShooter map tools**.
2. Make a `maps` folder next to the game (`shitshooter.exe`). This is where your own maps go. If you installed with the launcher, that's `%LOCALAPPDATA%\ShitShooter\maps` on Windows, `~/Library/Application Support/ShitShooter/maps` on a Mac and `~/.local/share/ShitShooter/maps` on Linux. When running from the source with `cargo run`, use the `maps` folder at the top of the repository. Git ignores it, so work in progress never gets committed by accident.

To start from an example, open [`example_surf.blend`](example_surf.blend). It has a start deck, three surf ramps, a checkpoint, a finish deck and a kill zone. Its export, [`example surf.json`](example%20surf.json), plays once you copy it into your `maps` folder. [`example_surf_map.py`](example_surf_map.py) is the script that builds both.

## Shipping a map with the game

When a map is ready for everyone, put its `.json` in [`assets/maps/`](../assets/maps) in the repository and push. Every release carries it: the launcher installs it for every player like any other asset, and a lobby server installed with `deploy/install.sh` fetches it when it updates. Every player can then host it, and it's offered in the lobby server's lobbies. Delete it from `assets/maps/` and it goes away again with the next release.

A map in your own `maps` folder wins over a shipped one with the same name. So to rework a shipped map, export it under the same name into your own folder, try it, and copy it to `assets/maps/` when it's done.

## The loop

1. Build in Blender. The **ShitShooter** tab in the 3D view's sidebar (press **N**) has buttons that add ramps and markers at the 3D cursor.
2. **Check map** says what the game would make of the scene: what's missing, which pieces aren't convex, and so on.
3. **Export…** the first time: save `<map name>.json` into the `maps` folder. The file's name is the map's name in the game. After that, **Export again** writes to the same file in one click.
4. In the game, host a lobby, pick the map (and **Race** or **Time trial** for a course), and start.
5. To change it, edit in Blender, click **Export again**, then end the match in the game and start a new one. The host reads the file again at the start of every match and sends the new version to everyone. A new map file only shows up in the picker the next time you host.

A lobby server (`--hub`) or a dedicated one (`--server`) offers the maps in the `maps` and `assets/maps` folders next to it, and sends them to the players the same way.

If a map file can't be played (it's broken, or made by an older version of the exporter), it's left out of the picker, and the reason is in the game's log.

## How the game reads the scene

Blender's metres are the game's metres, and Z is up. Players are about 1.8 m tall and run at 9.5 m/s. On a surf ramp they top out at 45 m/s.

### Solids

- Every **visible mesh** is solid, and so is every **curve with a bevel or an extrusion** (a Bezier path with a bevel object, filled caps or not). You don't need to convert curves to meshes. Hidden objects aren't exported.
- Each loose part collides just as it looks:
  - A **curve with a bevel** (or a curve converted to a mesh and not edited since) is cut into its sections along the path, one solid each, as long as its bevel profile is **convex** (a triangle, a trapezoid). The curve's resolution sets how many sections there are: more is smoother, fewer is lighter.
  - A **convex** part (a box, a wedge, a straight prism) is one solid.
  - Anything else (a U-shaped profile, a sculpted or hand-modelled curved shape, an open surface, a plane) is built from its faces: each face becomes a thin solid slab just under it. That works for any shape but makes many more pieces. **Check map** says how each object was built.
- Built from faces, a part's faces should point outward (Mesh → Normals → Recalculate Outside); the exporter does this itself for closed parts.
- Modifiers are applied when exporting.
- A face steeper than about **45°** is surfed: you slide on it and strafe against it. A flatter one is ground you stand on. Surf ramps usually have faces of 50° to 65°.
- An object whose name starts with **`clip`** is solid but invisible. Use it for invisible walls.

### Looks

- A mesh's **material** gives it its colour (the Principled BSDF's Base Color).
- If the material's name contains one of these words, the solid gets that texture: `panels`, `slab`, `concrete`, `paint`, `plaster`, `brick`, `cinder`, `stone`, `rock`, `wood`, `boards`, `metal`, `corrugated`, `rust`, `asphalt`, `ground`. For example, "Ramp panels" or "Deck slab". Without one, it's concrete.
- The **World**'s viewport colour (World properties → Viewport Display) is the sky and fog colour.
- A material with **emission** (the Principled BSDF's Emission Strength above 0) glows in the dark, in its emission colour: a lit window, a fire, a lamp. The strength is how many times its colour it glows; 1 to 2 is a window, 5 and up is a flame.

### Light

A map with none of this is lit as by day.

- A **Sun** light is the game's sun, or its moon: where it shines from, its colour, and its strength. A strength of **4** is daylight; 0.5 to 1 with a blue colour is a moonlit night.
- **Point** lights are lamps. A thousand watts light the ground 3 m away about as the sun does by day. Tick **Custom Distance** to say how far one reaches (25 m otherwise). One whose name starts with **`fire`** wavers. They cast no shadows.
- **Spot** lights are lamps that shine one way, in their cone (160° across at most): a street lamp, a searchlight. They cast shadows, and they are the lamps that show in mist and steam, so they cost more: use a few. The game lights at most 64 lamps in all.
- A **Cube** empty named **`mist`** is a box of mist: thick at its bottom, thinning to nothing at its top, drifting on the wind, with the sun and the lamps shining through it. Its **density** custom property says how thick: 1 is light (something 30 m into it is a third fainter). Its sides are sharp, so keep them inside walls; several boxes side by side join up. (The Mist button adds one.)
- An empty named **`steam`** is a grate a plume of thicker steam comes up out of, some 2 m across at the ground and 9 m high at a scale of 1. Put it on a mesh that looks like a grate. (The Steam vent button adds one.)
- Mist and steam are only to look at and to be seen through: they stop no bullet. A map has at most 16 of each. They show where the sun (or the moon) or a Spot light shines on them, not by Point lights; in the dark they only dim what is behind them. Players can turn them off (Settings → Graphics → Fog).
- The sidebar's **Ambient light** is the light that is everywhere, in the shadows too (480 by day; 100 to 200 for a night), and **Fog from** and **Fog to** are how far away the fog starts and where nothing shows through it. Left at 0, the game sets the fog by the map's size.

### Markers (empties, by name)

Names are matched without regard to case, and Blender's `.001` endings are ignored. An empty **faces along its Y axis** (the green arrow); only its turn about Z counts. For boxes, use a **Cube** empty: its size is its display size times its scale.

| Name | What it is |
| --- | --- |
| `spawn` | Where players start, standing at the empty, facing its Y axis. Use one per player you expect (up to 16). Beyond that, players share them. |
| `start` | A box. In a time trial, leaving it starts your clock and coming back into it ends the run. The spawns go inside it. |
| `checkpoint 1`, `checkpoint 2`, … | Boxes to pass through, in order. Make them big enough that nobody can miss them. |
| `respawn 1`, `respawn 2`, … | Where a fall after that checkpoint puts you, facing its Y axis and moving at its **speed** custom property (m/s; the Checkpoint button adds one). Each checkpoint needs one. |
| `finish` | A box. Pass through it and your run is over. |
| `finish spawn` | Optional: where you wait after finishing a race. Otherwise it's the first spawn. |
| `kill` | Optional boxes that count as falling off: you go back to your last checkpoint (or the start). Otherwise you've fallen off 10 m below the lowest solid. Unlike the other boxes, a kill box can be tilted any way, so a thin slab can lie under a slope. (The others only turn about Z.) |

A map with a `start` and a `finish` is a **course**, played in **Race** and **Time trial**. A map without them is played in **Free for all** and **Team deathmatch**, and it only needs spawns.

### A map made for one mode

**Made for** in the sidebar says which mode a map is for. **Automatic** is the rule above. A map made for a mode is offered in that mode only, and **Check map** says what it still needs.

**Sniper**: one player is the sniper on a platform, and everybody else runs for it on foot, unarmed, from cover to cover. A map for it needs:

| Name | What it is |
| --- | --- |
| `spawn` | Where the runners start: far from the platform, and out of the sniper's sight, behind a wall. |
| `spawn sniper` | Where the sniper stands, up on the platform, facing its Y axis down the map. |
| `finish` | A box under the platform. A runner inside it has made it. Make it big enough to run into. |

It has no `start`. The sniper walks and cannot jump, so a wall round the platform higher than a step (0.5 m) keeps them on it; about 0.9 m can still be shot over. A runner is 1.8 m tall standing and 1.2 m crouched. Far from the platform, cover about 1.4 m high hides a crouched runner and 2.3 m a standing one. The sniper looks down on the map, so the closer a piece of cover is to the platform, the less ground it hides behind it: within 30 m or so it wants to be 3 to 4 m high, and closer together.

`blender/example_sniper_map.py` builds a plain yard to start from:

```
blender --background --factory-startup --python blender/example_sniper_map.py -- <repo root>
```

The map the mode ships with, **Sniper Street**, is built the same way, by [`sniper_street_map.py`](sniper_street_map.py): a street at night between two rows of buildings, the runners behind a roadblock at one end and the sniper on an overpass at the other, with wrecked cars, a bus on its side, containers, blast walls and the rubble of two buildings between them. It saves [`sniper_street.blend`](sniper_street.blend) and exports `assets/maps/Sniper Street.json`. Everything in it is one object a thing (a car, a row of blast walls, a heap of rubble), so to change the map, open the `.blend`, move things about and **Export again**; to change what a car or a building is made of, change the script and run it again (which puts everything back where the script has it).

## Surf tips

- Riders arrive at a ramp falling, and they can't climb a face by strafing; they can only hold their height. Put each ramp you jump to lower than the one before, so riders fall onto it rather than slam into its end. Big drops onto a steep face can't be ridden out, so keep falls to about 10 to 15 m.
- Over a gap, the face you land on should face toward where riders come from, so they fly into it rather than over its top.
- Ramp ends are blunt. Riders who come in low hit them, so start the next ramp a little before where they land.
- A sharp kink in a ramp's top line (from steep down to flat) knocks speed out of a rider. Use a few short pieces to ease a bend or a change of slope.
- Put a checkpoint before each hard part, with its respawn on a deck or on the ramp after it, moving.
