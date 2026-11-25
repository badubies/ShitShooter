"""Renders a gun model built for the game, to check it before it goes in: from
both sides, from above, from behind and in front, down its iron sights as
aiming holds them, in its reload pose, and with each sight given mounted and
looked through. Flat colours (Workbench), so it is quick. Also prints the
gun's size and where its named nodes are, in the game's axes.

Run with:
blender --background --factory-startup --python preview.py -- \\
    <gun.glb> <out dir> <sight relief, metres> [sight.glb ...]

The sight relief is the gun's `sight_relief` in src/weapons.rs: aiming holds
the AimPoint (iron sights) or a sight's AimPoint that far in front of the eye.
"""
import math
import os
import sys

import bpy
from mathutils import Vector

args = sys.argv[sys.argv.index("--") + 1:]
GUN, OUT, RELIEF, SIGHTS = args[0], args[1], float(args[2]), args[3:]
os.makedirs(OUT, exist_ok=True)

# The game's vertical field of view (settings.rs) and the iron sights' zoom (weapons.rs).
FOV = math.radians(75.0 * 0.85)
# How far back a slide or bolt goes when cocked, as a share of the gun's
# length (SLIDE_TRAVEL and BOLT_TRAVEL in player.rs).
TRAVEL = {"Slide": 0.25, "Bolt": 0.14}


def import_glb(path):
    before = set(bpy.data.objects)
    bpy.ops.import_scene.gltf(filepath=path)
    return [obj for obj in bpy.data.objects if obj not in before]


bpy.ops.wm.read_factory_settings(use_empty=True)
gun = import_glb(GUN)
scene = bpy.context.scene
scene.render.engine = "BLENDER_WORKBENCH"
scene.display.shading.light = "STUDIO"
scene.display.shading.color_type = "MATERIAL"
scene.display.shading.show_cavity = True
scene.world = bpy.data.worlds.new("Backdrop")
scene.world.color = (0.45, 0.5, 0.55)
camera = bpy.data.objects.new("Camera", bpy.data.cameras.new("Camera"))
scene.collection.objects.link(camera)
scene.camera = camera
lens = camera.data
lens.clip_start = 0.002


def bounds(objects):
    corners = [obj.matrix_world @ Vector(c) for obj in objects if obj.type == "MESH" for c in obj.bound_box]
    low = Vector([min(c[i] for c in corners) for i in range(3)])
    high = Vector([max(c[i] for c in corners) for i in range(3)])
    return low, high


def look(at, eye):
    camera.location = eye
    camera.rotation_euler = (Vector(at) - Vector(eye)).to_track_quat("-Z", "Y").to_euler()


def render(name, width=1200, height=900):
    scene.render.resolution_x, scene.render.resolution_y = width, height
    scene.render.filepath = f"{OUT}/{name}.png"
    bpy.ops.render.render(write_still=True)


def node(name):
    return bpy.data.objects.get(name)


def shown(objects, on):
    for obj in objects:
        obj.hide_render = not on


def perspective():
    """A plain 50 mm lens, whatever the last picture used."""
    lens.type = "PERSP"
    lens.sensor_fit = "AUTO"
    lens.lens = 50


def sight_picture(name, aim):
    """Looking along the barrel from `RELIEF` behind `aim`, and zoomed in on it."""
    perspective()
    lens.sensor_fit = "VERTICAL"
    eye = aim - Vector((0.0, RELIEF, 0.0))
    look(aim + Vector((0.0, 1.0, 0.0)), eye)
    lens.angle_y = FOV
    render(name, 1600, 900)
    lens.angle_y = math.radians(6.0)
    render(f"{name}_zoom", 1600, 900)


low, high = bounds(gun)
size = high - low
middle = (low + high) / 2
print(f"[preview] {size.x * 1000:.1f} wide, {size.z * 1000:.1f} high, {size.y * 1000:.1f} long (mm); "
      f"middle off the origin by {middle.length * 1000:.2f} mm")
# Blender's Y forward and Z up are the game's -Z and +Y.
for name in ("Slide", "Bolt", "Magazine", "Pump", "Shell", "Trigger", "AimPoint", "SightMount"):
    if obj := node(name):
        at = obj.matrix_world.translation
        print(f"[preview] {name} at game ({at.x * 1000:.1f}, {at.z * 1000:.1f}, {-at.y * 1000:.1f}) mm")

# The outside, ortho from the sides and above, then from behind and in front.
lens.type = "ORTHO"
lens.ortho_scale = max(size) * 1.15
for name, eye in (("right", (1, 0, 0)), ("left", (-1, 0, 0)), ("top", (0, 0, 1))):
    look(middle, middle + Vector(eye))
    render(name)
perspective()
reach = size.y * 2.2
look(middle, middle + Vector((0.9, -1.1, 0.55)) * reach)
render("rear34")
look(middle, middle + Vector((-0.8, 1.2, 0.3)) * reach)
render("front34")

if aim := node("AimPoint"):
    sight_picture("irons", aim.matrix_world.translation.copy())
else:
    print("[preview] no AimPoint: aiming would line up the top of the gun instead")

# The reload pose: the slide or bolt back, the magazine part way out down its own axis.
moved = []
for name in ("Slide", "Bolt"):
    if obj := node(name):
        moved.append((obj, obj.location.copy()))
        obj.location.y -= TRAVEL[name] * size.y
if obj := node("Magazine"):
    moved.append((obj, obj.location.copy()))
    obj.location += obj.matrix_world.to_3x3() @ Vector((0.0, 0.0, -1.0)) * size.y * 0.35
perspective()
look(middle, middle + Vector((1.0, -0.8, 0.3)) * reach)
render("reload")
for obj, location in moved:
    obj.location = location

# Each sight on the mount, the iron sights hidden as the game hides them.
irons = [obj for obj in gun if obj.name.startswith("IronSight")]
mount = node("SightMount")
for path in SIGHTS:
    if not mount:
        print("[preview] no SightMount: no sight can go on")
        break
    tag = os.path.splitext(os.path.basename(path))[0]
    sight = import_glb(path)
    for obj in sight:
        if obj.parent is None:
            obj.parent = mount
            obj.matrix_parent_inverse.identity()
    bpy.context.view_layer.update()
    shown(irons, False)
    s_low, s_high = bounds(sight)
    s_middle = (s_low + s_high) / 2
    perspective()
    look(s_middle, s_middle + Vector((0.9, -1.1, 0.6)) * size.y * 0.9)
    render(f"sight_{tag}")
    dot = next(obj for obj in sight if obj.name.startswith("AimPoint"))
    sight_picture(f"sight_{tag}_through", dot.matrix_world.translation.copy())
    for obj in sight:
        bpy.data.objects.remove(obj)
    shown(irons, True)
