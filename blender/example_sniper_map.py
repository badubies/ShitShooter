"""Builds a grey-box map for the sniper mode with the ShitShooter map tools,
to start from: a long walled yard, runners at one end, cover scattered
across it (tall walls, low walls to crouch behind, crates), and at the other
end a platform for the sniper with the finish under it.

Saves blender/example_sniper.blend and exports "blender/example sniper.json"
(copy it into your maps folder to play it).

Run with:
blender --background --factory-startup --python blender/example_sniper_map.py -- <repo root>
"""
import math
import os
import random
import sys

import bpy

ROOT = sys.argv[sys.argv.index("--") + 1]
sys.path.insert(0, os.path.join(ROOT, "blender"))
import shitshooter_maps  # noqa: E402

shitshooter_maps.register()
scene = bpy.context.scene
for obj in list(scene.objects):
    bpy.data.objects.remove(obj)


def lin(c):
    return tuple(x / 12.92 if x <= 0.04045 else ((x + 0.055) / 1.055) ** 2.4 for x in c)


def material(name, color):
    mat = bpy.data.materials.new(name)
    mat.node_tree.nodes["Principled BSDF"].inputs["Base Color"].default_value = (*lin(color), 1.0)
    mat.diffuse_color = (*lin(color), 1.0)
    return mat


GROUND = material("Yard ground", (0.42, 0.44, 0.38))
WALL = material("Yard wall concrete", (0.55, 0.55, 0.56))
COVER = material("Cover boards", (0.62, 0.47, 0.30))
LOW = material("Low wall cinder", (0.50, 0.50, 0.52))
CRATE = material("Crate wood", (0.55, 0.40, 0.24))
DECK = material("Platform metal", (0.33, 0.36, 0.42))


def box(name, lo, hi, mat):
    verts = [(x, y, z) for z in (lo[2], hi[2]) for y in (lo[1], hi[1]) for x in (lo[0], hi[0])]
    faces = [(0, 1, 3, 2), (4, 6, 7, 5), (0, 4, 5, 1), (2, 3, 7, 6), (0, 2, 6, 4), (1, 5, 7, 3)]
    mesh = bpy.data.meshes.new(name)
    mesh.from_pydata(verts, [], faces)
    mesh.update()
    obj = bpy.data.objects.new(name, mesh)
    obj.data.materials.append(mat)
    scene.collection.objects.link(obj)
    return obj


def empty(name, kind, at, scale=(1, 1, 1), turn=0.0):
    obj = bpy.data.objects.new(name, None)
    obj.empty_display_type = kind
    obj.location = at
    obj.scale = scale
    obj.rotation_euler = (0, 0, turn)
    scene.collection.objects.link(obj)
    return obj


# The yard: WIDE across (X), LONG from the runners' end (Y = 0) to the
# platform's. Everything is measured from there.
WIDE, LONG = 30.0, 180.0
box("ground", (-WIDE, 0, -1), (WIDE, LONG, 0), GROUND)
box("wall left", (-WIDE - 1, 0, 0), (-WIDE, LONG, 5), WALL)
box("wall right", (WIDE, 0, 0), (WIDE + 1, LONG, 5), WALL)
box("wall back", (-WIDE - 1, -1, 0), (WIDE + 1, 0, 5), WALL)
box("wall far", (-WIDE - 1, LONG, 0), (WIDE + 1, LONG + 1, 5), WALL)

# The runners, in a row behind a wall they can leave round either end or
# through its gaps (which are between two spawns, not in front of one):
# nobody is shot where they come in.
for i in range(8):
    empty("spawn", "ARROWS", (-17.5 + 5 * i, 5, 0))
for x0, x1 in ((-26, -16), (-14, 14), (16, 26)):
    box("start wall", (x0, 11, 0), (x1, 11.6, 2.4), COVER)

# The platform: a deck on four legs with a wall round it low enough to shoot
# over standing, too high to step over.
TOP = 12.0
PY0, PY1 = LONG - 12, LONG - 2
box("deck", (-8, PY0, TOP - 0.6), (8, PY1, TOP), DECK)
for x in (-7.4, 6.8):
    for y in (PY0 + 0.2, PY1 - 0.8):
        box("leg", (x, y, 0), (x + 0.6, y + 0.6, TOP - 0.6), DECK)
box("parapet front", (-8, PY0, TOP), (8, PY0 + 0.3, TOP + 0.9), DECK)
box("parapet back", (-8, PY1 - 0.3, TOP), (8, PY1, TOP + 0.9), DECK)
box("parapet left", (-8, PY0, TOP), (-7.7, PY1, TOP + 0.9), DECK)
box("parapet right", (7.7, PY0, TOP), (8, PY1, TOP + 0.9), DECK)
# The sniper, on the deck, looking back down the yard (-Y).
empty("spawn sniper", "ARROWS", (0, PY0 + 4, TOP), turn=math.pi)
# Under the deck is home: inside this box, a runner has made it.
empty("finish", "CUBE", (0, PY0 + 5, 2.5), (8, 5, 2.5))

# Cover, in rows across the yard, each row's pieces shifted from the last
# one's so there is no straight run behind them. Further from the platform
# the sniper sees over less; close to it, cover hides very little.
random.seed(7)
rows = [24 + 11 * i for i in range(12)]
for r, y in enumerate(rows):
    if y > PY0 - 14:
        break
    shift = (r % 3 - 1) * 5.0 + random.uniform(-1.5, 1.5)
    x = -WIDE + 5 + shift
    while x < WIDE - 5:
        kind = random.choice(("wall", "wall", "low", "crate", "gap"))
        if kind == "wall":
            width = random.uniform(3.0, 5.0)
            box("cover wall", (x, y, 0), (x + width, y + 0.5, 2.3), COVER)
        elif kind == "low":
            width = random.uniform(3.0, 6.0)
            box("low wall", (x, y, 0), (x + width, y + 0.5, 1.15), LOW)
        elif kind == "crate":
            width = 2.0
            box("crate", (x, y - 0.8, 0), (x + 2.0, y + 1.2, 1.9), CRATE)
        else:
            width = random.uniform(2.0, 4.0)
        x += width + random.uniform(4.0, 8.0)

if scene.world is None:
    scene.world = bpy.data.worlds.new("World")
scene.world.color = lin((0.62, 0.72, 0.86))
scene.shitshooter_mode = "SNIPER"

path = os.path.join(ROOT, "blender", "example sniper.json")
ok, lines = shitshooter_maps.export(bpy.context, path)
print("\n".join(lines))
scene["shitshooter_path"] = path
bpy.ops.wm.save_as_mainfile(filepath=os.path.join(ROOT, "blender", "example_sniper.blend"))
if not ok:
    sys.exit(1)
