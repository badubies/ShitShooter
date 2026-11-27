"""Builds a small example surf course with the ShitShooter map tools, to
start from: a start deck, three ramps to transfer between (off the left
face of one onto the right face of the next, and back), a checkpoint, a
finish deck and a kill zone under it all.

Saves blender/example_surf.blend and exports maps/example surf.json.

Run with:
blender --background --factory-startup --python blender/example_surf_map.py -- <repo root>
"""
import math
import os
import sys

import bmesh
import bpy
from mathutils import Vector

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


RAMP = material("Ramp panels", (0.30, 0.52, 0.72))
DECK = material("Deck slab", (0.58, 0.58, 0.60))


def solid(name, verts, faces, mat):
    mesh = bpy.data.meshes.new(name)
    mesh.from_pydata(verts, [], faces)
    mesh.update()
    obj = bpy.data.objects.new(name, mesh)
    obj.data.materials.append(mat)
    scene.collection.objects.link(obj)
    return obj


def box(name, lo, hi, mat):
    verts = [(x, y, z) for z in (lo[2], hi[2]) for y in (lo[1], hi[1]) for x in (lo[0], hi[0])]
    faces = [(0, 1, 3, 2), (4, 6, 7, 5), (0, 4, 5, 1), (2, 3, 7, 6), (0, 2, 6, 4), (1, 5, 7, 3)]
    return solid(name, verts, faces, mat)


def ramp(name, top_from, top_to, height, slope_deg=55.0):
    """A ridge whose top runs from `top_from` to `top_to` (it may slope),
    its two faces `height` tall at `slope_deg`."""
    reach = height / math.tan(math.radians(slope_deg))
    verts = []
    for top in (Vector(top_from), Vector(top_to)):
        verts += [top, top + Vector((-reach, 0, -height)), top + Vector((reach, 0, -height))]
    faces = [(0, 1, 4, 3), (0, 3, 5, 2), (1, 2, 5, 4), (0, 2, 1), (3, 4, 5)]
    return solid(name, verts, faces, RAMP)


def empty(name, kind, at, scale=(1, 1, 1), speed=None):
    obj = bpy.data.objects.new(name, None)
    obj.empty_display_type = kind
    obj.location = at
    obj.scale = scale
    if speed is not None:
        obj["speed"] = speed
    scene.collection.objects.link(obj)
    return obj


# The start deck, 100 m up, and the players on it, facing +Y down the course.
box("start deck", (-10, -24, 96), (10, 0, 100), DECK)
for row in range(4):
    for col in range(4):
        empty("spawn", "ARROWS", (-6 + 4 * col, -20 + 4 * row, 100))
empty("start", "CUBE", (0, -12, 102), (10, 12, 3))

# Off the deck onto a long ramp, its top a little below the deck.
ramp("ramp 1", (0, 6, 96), (0, 130, 74), 22)
# Off its left face, across the gap, onto the right face of the next,
# further left and lower: the checkpoint is in the gap.
empty("checkpoint 1", "CUBE", (-14, 140, 70), (16, 1, 16))
ramp("ramp 2", (-34, 136, 70), (-34, 260, 50), 22)
empty("respawn 1", "ARROWS", (-30, 142, 70), speed=25.0)
# And back right onto the third.
ramp("ramp 3", (0, 266, 44), (0, 390, 26), 22)

# The finish deck at the end, low enough to land on.
box("finish deck", (-16, 400, 2), (16, 470, 6), DECK)
empty("finish", "CUBE", (0, 402, 16), (16, 1, 14))
empty("finish spawn", "ARROWS", (0, 455, 6))
# Fall below the ramps and you are back at the last checkpoint.
empty("kill", "CUBE", (0, 220, -10), (300, 400, 8))

if scene.world is None:
    scene.world = bpy.data.worlds.new("World")
scene.world.color = lin((0.55, 0.68, 0.88))

ok, lines = shitshooter_maps.export(bpy.context, os.path.join(ROOT, "maps", "example surf.json"))
print("\n".join(lines))
scene["shitshooter_path"] = os.path.join(ROOT, "maps", "example surf.json")
bpy.ops.wm.save_as_mainfile(filepath=os.path.join(ROOT, "blender", "example_surf.blend"))
if not ok:
    sys.exit(1)
