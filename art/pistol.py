"""Builds the pistol: an original heavy hand cannon in the manner of a Desert
Eagle, a big black slide with a slotted rib along its top over a wedge-shaped
barrel shroud, a dark grey frame with a square trigger guard and a fat grip
with ridged panels, and three-dot iron sights on the rib (two white dots on the
rear, an orange one on the front post) and a mount on the rib for the reflex and
the 2x sight.

Built for the game, so it is placed in the hand as it is, with no turning or
scaling in code:
- Metres. X is right, Y forward along the barrel, Z up. It is exported Y-up,
  so in the file the barrel points along -Z and up is +Y, as the game holds it.
- Its length is the pistol's `size` in src/weapons.rs (0.19 m), and its origin
  is the middle of its bounding box (where the box gun's middle would be).
- It is designed at true size (`TRUE_LENGTH`) and shrunk by `SCALE` to that
  length, and its `SightMount` is scaled by `SCALE` too, so a sight (built
  in true metres) keeps its true size next to the gun (see PLAN.md, Round 4).

The nodes the game looks for (see src/gun_model.rs):
- Slide: kicked back by each shot and held back when empty. It carries the
  iron sights, the sight mount and the aim point, so they ride with it.
- IronSightRear, IronSightFront: hidden when a sight is mounted.
- AimPoint: on the line over the top of the rear blade and the front post,
  which aiming puts on the crosshair.
- MuzzlePoint: the middle of the muzzle, where the tracers start. On the
  frame, not the slide, as the barrel does not move.
- SightMount: where a sight's base goes. The sights face +Y, as the gun
  does, so they go on as they are.
- Magazine: its origin is the top of the magazine, on the grip's slant.
- Trigger, SlideStop, MagRelease and Safety pivot or slide about their
  origins for animations to come.

Saves art/pistol.blend and exports assets/models/weapons/pistol.glb. Run with:
blender --background --factory-startup --python pistol.py -- <repo root>
"""
import math
import sys

import bmesh
import bpy
from mathutils import Vector

ROOT = sys.argv[sys.argv.index("--") + 1]

# The pistol's length in src/weapons.rs, in millimetres: how long the game holds it.
LENGTH = 190.0
# How long it is at true size (a six inch barrel), which it is laid out at;
# everything is shrunk by `SCALE` to `LENGTH`, sights on its mount included.
TRUE_LENGTH = 270.0
SCALE = LENGTH / TRUE_LENGTH
# Laid out in true millimetres, the slide's rear bottom edge at y = 0, z = 0.
# Every vertex and position is turned into game metres with `MM` as it is
# made, and the whole gun is centred on its origin at the end.
MM = 0.001 * SCALE
# The tang of the frame over the web of the hand is the rearmost point.
TANG_Y = -10.0
SLIDE_FRONT = TRUE_LENGTH + TANG_Y
SLIDE_HALF_WIDTH = 14.5
SLIDE_TOP = 38.0
# The rib along the slide's top, which the sights stand on.
RIB_HALF_WIDTH = 6.5
RIB_TOP = 42.0
BORE_Z = 24.0
# The top of the rear blade and of the front post: the line aimed along.
SIGHT_LINE_Z = 50.5
REAR_SIGHT_Y = (8.0, 20.0)
FRONT_SIGHT_Y = (SLIDE_FRONT - 24.0, SLIDE_FRONT - 15.0)
# A wide notch, its edges close to the white dots, so little of the rear
# blade stands between them and the front post. About 2.2 degrees across
# from the eye (0.18 m) and a post about 0.75 degrees: 7 and 4.4 mm in game
# metres, so these are those over `SCALE`.
NOTCH_HALF_WIDTH = 5.0
POST_HALF_WIDTH = 3.1
# The grip slants back by this much; the magazine follows it.
GRIP_ANGLE = math.radians(15.0)
MAGAZINE_TOP = (42.0, -4.0)
GRIP_BOTTOM = -92.0
# How far forward of the rear of the slide a sight's base is centred.
SIGHT_MOUNT_Y = 46.0


def material(name, linear, roughness=0.5, emission=None, strength=1.0):
    """Plain, not metallic, like the other weapons (colours are linear)."""
    mat = bpy.data.materials.new(name)
    mat.use_nodes = True
    bsdf = mat.node_tree.nodes["Principled BSDF"]
    bsdf.inputs["Base Color"].default_value = (*linear, 1.0)
    bsdf.inputs["Metallic"].default_value = 0.0
    bsdf.inputs["Roughness"].default_value = roughness
    if emission:
        bsdf.inputs["Emission Color"].default_value = (*emission, 1.0)
        bsdf.inputs["Emission Strength"].default_value = strength
    mat.diffuse_color = (*linear, 1.0)
    return mat


bpy.ops.wm.read_factory_settings(use_empty=True)
SLIDE = material("Pistol_Slide", (0.028, 0.030, 0.033), roughness=0.38)
FRAME = material("Pistol_Frame", (0.05, 0.051, 0.055), roughness=0.8)
STEEL = material("Pistol_Steel", (0.075, 0.076, 0.08), roughness=0.32)
BRASS = material("Pistol_Brass", (0.55, 0.36, 0.09), roughness=0.35)
COPPER = material("Pistol_Copper", (0.45, 0.16, 0.06), roughness=0.35)
REAR_DOT = material("Pistol_RearDot", (0.85, 0.85, 0.8), roughness=0.4, emission=(0.85, 0.85, 0.8), strength=0.4)
FRONT_DOT = material("Pistol_FrontDot", (1.0, 0.32, 0.02), roughness=0.4, emission=(1.0, 0.32, 0.02), strength=1.5)

COLLECTION = bpy.data.collections.new("Pistol")
bpy.context.scene.collection.children.link(COLLECTION)


def to_3d(axis, point, along):
    """A point of a flat shape across `axis`, `along` it."""
    a, b = point
    return {"x": (along, a, b), "y": (a, along, b), "z": (a, b, along)}[axis]


class Part:
    """The mesh of one object, built in the layout's millimetres."""

    def __init__(self):
        self.bm = bmesh.new()
        self.materials = []

    def index(self, mat):
        if mat not in self.materials:
            self.materials.append(mat)
        return self.materials.index(mat)

    def vert(self, point):
        return self.bm.verts.new(Vector(point) * MM)

    def closed(self, faces, mat, smooth=False):
        index = self.index(mat)
        for face in faces:
            face.material_index = index
            face.smooth = smooth
        bmesh.ops.recalc_face_normals(self.bm, faces=faces)

    def prism(self, axis, points, a0, a1, mat):
        """A flat shape across `axis` (its other two axes in order), from `a0` to `a1` along it."""
        low = [self.vert(to_3d(axis, p, a0)) for p in points]
        high = [self.vert(to_3d(axis, p, a1)) for p in points]
        faces = [self.bm.faces.new(low[::-1]), self.bm.faces.new(high)]
        n = len(points)
        for i in range(n):
            j = (i + 1) % n
            faces.append(self.bm.faces.new((low[i], low[j], high[j], high[i])))
        self.closed(faces, mat)

    def box(self, x0, x1, y0, y1, z0, z1, mat):
        self.prism("z", [(x0, y0), (x1, y0), (x1, y1), (x0, y1)], z0, z1, mat)

    def cylinder(self, axis, radius, centre, a0, a1, mat, segments=16):
        self.prism(axis, circle(centre, radius, segments), a0, a1, mat)

    def band(self, points, thickness, x0, x1, mat):
        """A bar `thickness` thick along a line of (y, z) points, from `x0` to `x1` across."""
        self.prism("x", offset(points, thickness / 2) + offset(points, -thickness / 2)[::-1], x0, x1, mat)

    def loft(self, rings, mat, smooth=True):
        """A solid through rings of 3D points (all of one count), capped at both ends."""
        rings = [[self.vert(p) for p in ring] for ring in rings]
        n = len(rings[0])
        faces = [self.bm.faces.new(rings[0][::-1]), self.bm.faces.new(rings[-1])]
        for a, b in zip(rings, rings[1:]):
            for i in range(n):
                j = (i + 1) % n
                faces.append(self.bm.faces.new((a[i], a[j], b[j], b[i])))
        self.closed(faces, mat)
        for face in faces[2:]:
            face.smooth = smooth

    def tube(self, radius, bore, y0, y1, depth, z, mat, segments=20):
        """The barrel: solid at `y0`, open at `y1`, its bore `depth` deep."""
        rim = lambda r, y: [self.vert((x, y, zz)) for x, zz in circle((0.0, z), r, segments)]
        back, front, mouth, bottom = rim(radius, y0), rim(radius, y1), rim(bore, y1), rim(bore, y1 - depth)
        faces = [self.bm.faces.new(back[::-1]), self.bm.faces.new(bottom[::-1])]
        for i in range(segments):
            j = (i + 1) % segments
            faces.append(self.bm.faces.new((back[i], back[j], front[j], front[i])))
            faces.append(self.bm.faces.new((front[i], front[j], mouth[j], mouth[i])))
            faces.append(self.bm.faces.new((mouth[i], mouth[j], bottom[j], bottom[i])))
        index = self.index(mat)
        for face in faces:
            face.material_index = index
        # A closed solid with the bore sunk into it, so this turns the bore's walls inwards.
        bmesh.ops.recalc_face_normals(self.bm, faces=faces)

    def to_object(self, name, origin=(0.0, 0.0, 0.0), parent=None, rotation=(0.0, 0.0, 0.0)):
        """An object of this mesh whose origin is at `origin` (layout millimetres),
        turned by `rotation` round it. The mesh is given in the turned frame."""
        origin = Vector(origin) * MM
        bmesh.ops.translate(self.bm, verts=self.bm.verts, vec=-origin)
        mesh = bpy.data.meshes.new(name)
        self.bm.to_mesh(mesh)
        self.bm.free()
        for mat in self.materials:
            mesh.materials.append(mat)
        obj = bpy.data.objects.new(name, mesh)
        COLLECTION.objects.link(obj)
        place(obj, origin, parent, rotation)
        return obj


def place(obj, origin, parent, rotation):
    """Puts `obj` at `origin` (metres, in the layout), under `parent` if any."""
    obj.rotation_euler = rotation
    if parent:
        obj.parent = parent
        obj.location = origin - parent.location
    else:
        obj.location = origin


def empty(name, at, parent, rotation=(0.0, 0.0, 0.0), scale=1.0):
    obj = bpy.data.objects.new(name, None)
    obj.empty_display_size = 0.006
    COLLECTION.objects.link(obj)
    place(obj, Vector(at) * MM, parent, rotation)
    obj.scale = (scale, scale, scale)
    return obj


def circle(centre, radius, segments):
    return [(centre[0] + radius * math.cos(i * math.tau / segments), centre[1] + radius * math.sin(i * math.tau / segments))
            for i in range(segments)]


def offset(points, distance):
    """A line of 2D points moved sideways by `distance` (to the left going along it), mitred at the joints."""
    out = []
    for i, (px, py) in enumerate(points):
        normals = []
        segs = []
        if i > 0:
            segs.append((points[i - 1], points[i]))
        if i + 1 < len(points):
            segs.append((points[i], points[i + 1]))
        for (ax, ay), (bx, by) in segs:
            length = math.hypot(bx - ax, by - ay)
            normals.append((-(by - ay) / length, (bx - ax) / length))
        nx, ny = sum(n[0] for n in normals), sum(n[1] for n in normals)
        length = math.hypot(nx, ny)
        nx, ny = nx / length, ny / length
        miter = nx * normals[0][0] + ny * normals[0][1]
        out.append((px + nx * distance / miter, py + ny * distance / miter))
    return out


def rounded_rect(a0, a1, b0, b1, radius, per_corner=4):
    """A rectangle with rounded corners, as (a, b) points going round anticlockwise."""
    centres = [(a1 - radius, b0 + radius, -90), (a1 - radius, b1 - radius, 0), (a0 + radius, b1 - radius, 90),
               (a0 + radius, b0 + radius, 180)]
    points = []
    for ca, cb, start in centres:
        for k in range(per_corner + 1):
            angle = math.radians(start + 90 * k / per_corner)
            points.append((ca + radius * math.cos(angle), cb + radius * math.sin(angle)))
    return points


def on_grip(x, u, v):
    """From the grip's own axes (u forward across it, v up along it, both from
    the top of the magazine) to the layout."""
    s, c = math.sin(GRIP_ANGLE), math.cos(GRIP_ANGLE)
    return (x, MAGAZINE_TOP[0] + u * c + v * s, MAGAZINE_TOP[1] - u * s + v * c)


def bake(obj):
    """Applies its modifiers to its mesh."""
    depsgraph = bpy.context.evaluated_depsgraph_get()
    mesh = bpy.data.meshes.new_from_object(obj.evaluated_get(depsgraph))
    old = obj.data
    obj.modifiers.clear()
    obj.data = mesh
    mesh.name = obj.name
    bpy.data.meshes.remove(old)


def boolean(obj, cutter, operation):
    modifier = obj.modifiers.new(cutter.name, "BOOLEAN")
    modifier.object = cutter
    modifier.operation = operation
    modifier.solver = "EXACT"
    bake(obj)
    bpy.data.objects.remove(cutter)


GRIP = material("Pistol_Grip", (0.018, 0.018, 0.02), roughness=0.9)

# The slide: its side profile and its cross-section (a rib along the top, the
# top edges bevelled) cut out of each other; then serrations at the back and
# the front, the big ejection port on the right, and the hole the muzzle
# shows through.
SF = SLIDE_FRONT
part = Part()
part.prism("x", [(0.0, 0.0), (SF - 6.0, 0.0), (SF, 6.0), (SF, RIB_TOP - 5.0),
                 (SF - 4.0, RIB_TOP), (9.0, RIB_TOP), (0.0, SLIDE_TOP - 9.0)], -16.0, 16.0, SLIDE)
slide = part.to_object("Slide")
part = Part()
w, r = SLIDE_HALF_WIDTH, RIB_HALF_WIDTH
part.prism("y", [(-w, 0.0), (w, 0.0), (w, SLIDE_TOP - 9.0), (w - 5.0, SLIDE_TOP), (r, SLIDE_TOP), (r, RIB_TOP),
                 (-r, RIB_TOP), (-r, SLIDE_TOP), (-(w - 5.0), SLIDE_TOP), (-w, SLIDE_TOP - 9.0)],
           -1.0, SF + 1.0, SLIDE)
boolean(slide, part.to_object("SlideSection"), "INTERSECT")
part = Part()
for side in (-1.0, 1.0):
    for y0, count in ((8.0, 8), (SF - 40.0, 5)):
        for i in range(count):
            y = y0 + i * 5.0
            part.box(*sorted((side * (w - 2.0), side * (w + 1.5))), y, y + 2.2, 5.0, SLIDE_TOP - 11.0, SLIDE)
# Slots across the rib.
for i in range(16):
    y = 58.0 + i * 8.5
    part.box(-r - 1.0, r + 1.0, y, y + 3.5, RIB_TOP - 2.0, RIB_TOP + 1.0, SLIDE)
part.box(-1.0, w + 2.0, 100.0, 172.0, 10.0, 35.0, SLIDE)
part.cylinder("y", 11.8, (0.0, BORE_Z), SF - 14.0, SF + 2.0, SLIDE, segments=24)
boolean(slide, part.to_object("SlideCuts"), "DIFFERENCE")

# The rear sight: a low blade on the rib, its sides drawn in and its top
# corners cut away so it hides little round the notch, a white dot either
# side; the front: a narrow post on a ramp with an orange dot. Their tops are
# the sight line.
part = Part()
y0, y1 = REAR_SIGHT_Y
n, top = NOTCH_HALF_WIDTH, SIGHT_LINE_Z
right = [(9.8, SLIDE_TOP - 0.5), (9.6, top - 2.0), (8.8, top), (n, top), (n, top - 4.0)]
part.prism("y", right + [(-x, z) for x, z in reversed(right)], y0, y1, SLIDE)
for x in (-7.3, 7.3):
    part.cylinder("y", 1.5, (x, top - 2.4), y0 - 0.15, y0 + 0.2, REAR_DOT, segments=12)
part.to_object("IronSightRear", origin=(0.0, y0, SLIDE_TOP), parent=slide)
part = Part()
y0, y1 = FRONT_SIGHT_Y
part.prism("x", [(y0 - 5.0, SLIDE_TOP - 0.5), (y1, SLIDE_TOP - 0.5), (y1, RIB_TOP + 2.0), (y0 - 5.0, RIB_TOP - 0.5)],
           -8.0, 8.0, SLIDE)
part.prism("x", [(y0, RIB_TOP), (y1, RIB_TOP), (y1, SIGHT_LINE_Z - 4.0), (y1 - 3.0, SIGHT_LINE_Z),
                 (y0, SIGHT_LINE_Z)], -POST_HALF_WIDTH, POST_HALF_WIDTH, SLIDE)
part.cylinder("y", 1.9, (0.0, SIGHT_LINE_Z - 2.6), y0 - 0.15, y0 + 0.2, FRONT_DOT, segments=12)
part.to_object("IronSightFront", origin=(0.0, y0, SLIDE_TOP), parent=slide)
empty("AimPoint", (0.0, REAR_SIGHT_Y[0], SIGHT_LINE_Z), slide)
# The middle of the muzzle, where the shots come out.
empty("MuzzlePoint", (0.0, SF, BORE_Z), None)
# Shrunk with the gun, so a sight on it keeps its true size next to it.
empty("SightMount", (0.0, SIGHT_MOUNT_Y, RIB_TOP), slide, scale=SCALE)
# A plate over the striker at the back of the slide.
part = Part()
part.box(-9.0, 9.0, -0.5, 0.3, 8.0, 30.0, STEEL)
part.to_object("StrikerPlate", parent=slide)

# The barrel: its chamber shows through the ejection port, its muzzle through
# the front of the slide, and more of it when the slide is back.
part = Part()
part.box(-9.5, 9.5, 96.0, 176.0, BORE_Z - 10.5, BORE_Z + 8.0, STEEL)
part.tube(9.5, 6.4, 150.0, SF - 0.3, 26.0, BORE_Z, STEEL, segments=24)
part.to_object("Barrel")

# The frame: the wedge round the barrel (a triangle seen from the front) with
# a rail under it, the body under the slide, the square trigger guard, the
# tang, and the grip (lofted round, on its slant, fat in the palm).
part = Part()
part.prism("y", [(-14.0, 0.5), (14.0, 0.5), (8.5, -10.0), (4.5, -18.0), (-4.5, -18.0), (-8.5, -10.0)],
           112.0, SF - 1.5, FRAME)
for i in range(6):
    y = 122.0 + i * 20.0
    part.box(-9.0, 9.0, y, y + 8.0, -20.0, -16.0, FRAME)
part.prism("x", [(-4.0, 0.5), (124.0, 0.5), (124.0, -14.0), (66.0, -14.0), (44.0, -17.0), (22.0, -22.0),
                 (6.0, -22.0), (-3.0, -15.0), (-8.5, -9.0), (TANG_Y, -3.0)], -14.0, 14.0, FRAME)
part.band([(121.0, -13.0), (122.0, -34.0), (116.0, -43.0), (100.0, -46.0), (50.0, -46.0)], 7.0, -6.0, 6.0, FRAME)
sections = [(2.0, -24.0, 24.0, 14.5, 5.0), (-10.0, -25.5, 25.5, 16.0, 8.0), (-50.0, -26.0, 25.5, 16.4, 8.5),
            (-88.0, -25.5, 25.0, 16.0, 8.0), (GRIP_BOTTOM, -26.0, 25.5, 16.4, 8.5)]
part.loft([[on_grip(x, u, v) for x, u in rounded_rect(-half, half, back, front, radius)]
           for v, back, front, half, radius in sections], FRAME)
# Ridged rubber panels on both sides of the grip.
for side in (-1.0, 1.0):
    x0, x1 = (16.0, 17.3) if side > 0 else (-17.3, -16.0)
    part.prism("x", [on_grip(0.0, u, v)[1:] for u, v in ((-19.0, -16.0), (19.0, -16.0), (19.0, -83.0), (-19.0, -83.0))],
               x0, x1, GRIP)
    for k in range(10):
        v = -20.0 - k * 6.4
        part.prism("x", [on_grip(0.0, u, vv)[1:] for u, vv in ((-17.0, v), (17.0, v), (17.0, v - 1.8), (-17.0, v - 1.8))],
                   side * 16.2, side * 18.0, GRIP)
# The takedown pin, through the frame over the trigger.
part.cylinder("x", 2.2, (104.0, -6.0), -14.4, 14.4, STEEL, segments=12)
frame = part.to_object("Frame")

part = Part()
# Down from its pin, its tip curling forward towards the muzzle.
part.band([(92.0, -10.0), (91.4, -17.0), (92.4, -24.0), (94.6, -29.0), (98.0, -32.0)], 4.8, -3.5, 3.5, STEEL)
part.to_object("Trigger", origin=(0.0, 92.0, -11.0))

# The slide stop, on the left over the trigger, turning on its front pin.
part = Part()
part.prism("x", [(112.0, -8.0), (112.0, -3.5), (82.0, -3.0), (74.0, -1.5), (71.0, -4.0), (74.0, -8.5), (82.0, -8.0)],
           -17.2, -14.8, STEEL)
part.cylinder("x", 2.4, (108.0, -5.7), -17.6, -14.4, STEEL, segments=12)
part.to_object("SlideStop", origin=(-16.0, 108.0, -5.7))

# The magazine release, a button on the left behind the trigger guard.
part = Part()
part.box(-19.6, -15.8, 55.0, 64.0, -29.0, -20.0, STEEL)
part.to_object("MagRelease", origin=(-16.0, 59.5, -24.5))

# The safety: a lever on each side of the frame behind the slide stop, with a
# knurled thumb pad.
part = Part()
for side in (-1.0, 1.0):
    part.box(*sorted((side * 14.2, side * 19.0)), 8.0, 30.0, -9.0, -3.0, STEEL)
    for k in range(4):
        y = 10.0 + k * 5.0
        part.box(*sorted((side * 19.0, side * 20.2)), y, y + 2.4, -9.0, -3.0, STEEL)
part.to_object("Safety", origin=(0.0, 19.0, -6.0))

# The magazine, built along its own axes (down the grip is -Z) and turned onto
# the grip's slant: the body (hidden in the grip until it drops), the feed
# lips with the top round (a fat .50 cartridge) in them, and the base plate
# under the grip.
part = Part()
part.prism("z", rounded_rect(-11.0, 11.0, -20.0, 21.0, 3.0, per_corner=3), GRIP_BOTTOM, -4.0, STEEL)
part.prism("z", rounded_rect(-8.5, 8.5, -20.0, 17.0, 2.5, per_corner=3), -4.0, 0.0, STEEL)
part.cylinder("y", 6.8, (0.0, -0.5), -20.0, 6.0, BRASS, segments=20)
part.prism("y", circle((0.0, -0.5), 6.4, 20), 6.0, 15.0, COPPER)
part.prism("y", circle((0.0, -0.5), 4.2, 20), 15.0, 20.0, COPPER)
part.prism("z", rounded_rect(-14.0, 14.0, -24.0, 25.0, 3.5, per_corner=3), GRIP_BOTTOM - 5.5, GRIP_BOTTOM, FRAME)
part.box(-10.0, 10.0, 23.0, 27.0, GRIP_BOTTOM - 5.5, GRIP_BOTTOM - 1.5, FRAME)
magazine = part.to_object("Magazine")
place(magazine, Vector((0.0, *MAGAZINE_TOP)) * MM, None, (-GRIP_ANGLE, 0.0, 0.0))


# Everything moves so the middle of the gun's bounds is the origin.
bpy.context.view_layer.update()
meshes = [obj for obj in COLLECTION.objects if obj.type == "MESH"]
corners = [obj.matrix_world @ Vector(c) for obj in meshes for c in obj.bound_box]
low = Vector([min(c[i] for c in corners) for i in range(3)])
high = Vector([max(c[i] for c in corners) for i in range(3)])
middle = (low + high) / 2
for obj in COLLECTION.objects:
    if obj.parent is None:
        obj.location -= middle
size = (high - low) / 0.001
print(f"pistol: {size.x:.1f} wide, {size.z:.1f} high, {size.y:.1f} long (game mm, x{SCALE:.3f} of true size)")
assert abs(size.y - LENGTH) < 0.05, f"the pistol is {size.y:.2f} mm long, not {LENGTH}"

bpy.ops.wm.save_as_mainfile(filepath=f"{ROOT}/art/pistol.blend")
bpy.ops.export_scene.gltf(
    filepath=f"{ROOT}/assets/models/weapons/pistol.glb", export_format="GLB", export_yup=True, export_apply=True
)
