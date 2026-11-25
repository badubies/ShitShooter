"""Builds the SMG: an original compact personal defence weapon, a black upper
receiver with a full-length top rail on a dark grey polymer lower, its
magazine ahead of the trigger guard and its stock folded along the right
side. A ghost-ring rear sight and a post with an orange dot between guard
ears, both on the rail; a mount on the rail for the reflex and 2x.

Built for the game, so it is placed in the hand as it is, with no turning or
scaling in code:
- Metres. X is right, Y forward along the barrel, Z up. It is exported Y-up,
  so in the file the barrel points along -Z and up is +Y, as the game holds it.
- Its length is the SMG's `size` in src/weapons.rs (0.2 m), and its origin
  is the middle of its bounding box.
- It is designed at true size (`TRUE_LENGTH`, stock folded) and shrunk by
  `SCALE` to that length, and its `SightMount` is scaled by `SCALE` too, so a
  sight (built in true metres) keeps its true size next to the gun (see
  PLAN.md, Round 4).

The nodes the game looks for (see src/gun_model.rs):
- Bolt: kicked back by each shot, held back when empty and drawn back to
  cock the gun at the end of a reload. It shows through the ejection port
  on the right, and carries the charging handle, which sticks out of a slot
  on the left, the side seen from the hip.
- IronSightRear, IronSightFront: hidden when a sight is mounted.
- AimPoint: the middle of the rear aperture, on the line over the front
  post, which aiming puts on the crosshair.
- MuzzlePoint: the middle of the muzzle, where the tracers start.
- SightMount: where a sight's base goes, on the rail. The sights face +Y,
  as the gun does, so they go on as they are.
- Magazine: its origin is the top of the magazine, its axes along its rake.
- Trigger, Selector, MagRelease and Stock pivot or slide about their
  origins for animations to come.

Saves art/smg.blend and exports assets/models/weapons/smg.glb. Run with:
blender --background --factory-startup --python smg.py -- <repo root>
"""
import math
import sys

import bmesh
import bpy
from mathutils import Vector

ROOT = sys.argv[sys.argv.index("--") + 1]

# The SMG's length in src/weapons.rs, in millimetres: how long the game holds it.
LENGTH = 200.0
# How long it is at true size, stock folded, which it is laid out at;
# everything is shrunk by `SCALE` to `LENGTH`, sights on its mount included.
TRUE_LENGTH = 410.0
SCALE = LENGTH / TRUE_LENGTH
# Laid out in true millimetres: y = 0 at the rear of the upper receiver,
# which sits on the lower at z = `JOINT_Z`.
# Every vertex and position is turned into game metres with `MM` as it is
# made, and the whole gun is centred on its origin at the end.
MM = 0.001 * SCALE
# The stock's hinge is the rearmost point, the muzzle device the front.
HINGE_Y = -6.0
REAR_Y = HINGE_Y - 6.0
MUZZLE_Y = REAR_Y + TRUE_LENGTH
JOINT_Z = 12.0
UPPER_TOP = 48.0
UPPER_FRONT = 292.0
UPPER_HALF_WIDTH = 15.0
LOWER_HALF_WIDTH = 13.5
RAIL_TOP = 55.0
BORE_Z = 30.0
# The bolt's face, closed, and the chamber in front of it.
BOLT_FACE_Y = 236.0
# The line aimed along: the middle of the rear aperture and the top of the post.
SIGHT_LINE_Z = 68.0
REAR_SIGHT_Y = 8.0
FRONT_POST_Y = (245.0, 249.0)
# Sized by how they look from the eye (PLAN.md): the aperture about 2.4
# degrees across, the post about 0.8 degrees wide, the guard ears hidden
# behind the ring.
APERTURE_RADIUS = 3.6
RING_RADIUS = 4.5
POST_HALF_WIDTH = 2.8
# Where a sight's base is centred, on the rail.
SIGHT_MOUNT_Y = 95.0
# The grip slants back by this much from the top of its front edge.
GRIP_ANGLE = math.radians(18.0)
GRIP_TOP = (110.0, -2.0)
# The magazine: the middle of its top, its rake forward, and how much it
# curves forward along its length.
MAGAZINE_TOP = (214.0, JOINT_Z)
MAGAZINE_RAKE = math.radians(4.0)
MAGAZINE_BEND = 0.0011
MAGAZINE_LENGTH = 150.0
# How far the bolt goes back (BOLT_TRAVEL in src/player.rs), which the
# charging handle's slot must leave room for.
BOLT_TRAVEL = 0.14 * TRUE_LENGTH


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
RECEIVER = material("SMG_Receiver", (0.028, 0.030, 0.033), roughness=0.38)
POLYMER = material("SMG_Polymer", (0.05, 0.051, 0.055), roughness=0.8)
MAG = material("SMG_Magazine", (0.04, 0.041, 0.044), roughness=0.7)
RUBBER = material("SMG_Rubber", (0.018, 0.018, 0.019), roughness=0.95)
STEEL = material("SMG_Steel", (0.075, 0.076, 0.08), roughness=0.32)
BRASS = material("SMG_Brass", (0.55, 0.36, 0.09), roughness=0.35)
COPPER = material("SMG_Copper", (0.45, 0.16, 0.06), roughness=0.35)
FRONT_DOT = material("SMG_FrontDot", (1.0, 0.32, 0.02), roughness=0.4, emission=(1.0, 0.32, 0.02), strength=1.5)

COLLECTION = bpy.data.collections.new("SMG")
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
    the top of the grip) to the layout."""
    s, c = math.sin(GRIP_ANGLE), math.cos(GRIP_ANGLE)
    return (x, GRIP_TOP[0] + u * c + v * s, GRIP_TOP[1] - u * s + v * c)


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


# The upper receiver: chamfered along its top edges, a recessed panel along
# each side, the ejection port on the right, the charging handle's slot on
# the left, and vents along the front.
part = Part()
w, top = UPPER_HALF_WIDTH, UPPER_TOP
part.prism("y", [(-w, JOINT_Z), (w, JOINT_Z), (w, top - 4.0), (w - 4.0, top), (-w + 4.0, top), (-w, top - 4.0)],
           0.0, UPPER_FRONT, RECEIVER)
upper = part.to_object("Upper")
part = Part()
part.box(0.0, w + 1.0, 192.0, BOLT_FACE_Y, 22.0, 40.0, RECEIVER)
slot_back = BOLT_FACE_Y - 12.0 - BOLT_TRAVEL - 3.0
part.box(-w - 1.0, -w + 5.0, slot_back, BOLT_FACE_Y - 2.0, 29.0, 36.0, RECEIVER)
for side in (-1.0, 1.0):
    part.box(*sorted((side * (w - 1.2), side * (w + 1.0))), 26.0, 152.0, 20.0, 40.0, RECEIVER)
    for i in range(5):
        y = 250.0 + i * 8.0
        part.box(*sorted((side * (w - 3.0), side * (w + 1.0))), y, y + 4.0, 22.0, 38.0, RECEIVER)
boolean(upper, part.to_object("UpperCuts"), "DIFFERENCE")

# The rail: a core along the top with cross ridges, the sights' clamps' width.
part = Part()
part.box(-7.0, 7.0, 2.0, UPPER_FRONT - 2.0, top - 0.5, RAIL_TOP - 1.0, RECEIVER)
ridge = [(-8.0, top - 0.5), (8.0, top - 0.5), (8.0, top + 2.0), (10.5, top + 4.5), (10.5, top + 5.5), (9.0, RAIL_TOP),
         (-9.0, RAIL_TOP), (-10.5, top + 5.5), (-10.5, top + 4.5), (-8.0, top + 2.0)]
y = 2.0
while y + 5.2 <= UPPER_FRONT - 2.0:
    part.prism("y", ridge, y, y + 5.2, RECEIVER)
    y += 10.0
part.to_object("Rail")

# The end plate, the stock's hinge bracket on its right, and a sling socket on
# its left.
part = Part()
part.box(-13.0, 13.0, -4.0, 0.0, 4.0, 46.0, RECEIVER)
part.box(13.0, 21.0, HINGE_Y, 0.0, 4.0, 9.0, RECEIVER)
part.box(13.0, 21.0, HINGE_Y, 0.0, 21.0, 26.0, RECEIVER)
part.cylinder("x", 4.0, (-1.0, 30.0), -17.0, -12.0, STEEL, segments=14)
part.to_object("EndPlate")

# The bolt, with the extractor on its right; through the port it shows, and
# when it is back the chamber does. The charging handle rides on it, out
# through the slot on the left, its knob grooved for the fingers.
part = Part()
part.box(-9.0, 9.0, 120.0, BOLT_FACE_Y, 21.0, 41.0, STEEL)
part.box(9.0, 9.8, BOLT_FACE_Y - 10.0, BOLT_FACE_Y, 28.0, 33.0, STEEL)
bolt = part.to_object("Bolt", origin=(0.0, BOLT_FACE_Y, BORE_Z))
part = Part()
handle_y = BOLT_FACE_Y - 12.0
part.box(-w - 1.5, -9.0, handle_y, handle_y + 6.0, 30.0, 35.0, STEEL)
part.prism("z", rounded_rect(-w - 13.0, -w - 1.5, handle_y - 4.0, handle_y + 9.0, 2.5, per_corner=3), 28.0, 37.0,
           RECEIVER)
for i in range(3):
    part.box(-w - 13.4, -w - 12.6, handle_y - 1.5 + i * 3.5, handle_y + i * 3.5, 28.5, 36.5, RUBBER)
part.to_object("ChargingHandle", origin=(-w, handle_y + 3.0, 32.5), parent=bolt)

# The barrel, from the chamber to past the front of the upper.
part = Part()
part.box(-7.0, 7.0, 200.0, BOLT_FACE_Y + 4.0, 22.0, 38.0, STEEL)
part.tube(8.0, 3.0, BOLT_FACE_Y, 345.0, 20.0, BORE_Z, STEEL)
part.to_object("Barrel")

# The muzzle device: ported on top and on the sides.
part = Part()
part.tube(12.0, 5.5, 338.0, MUZZLE_Y, 25.0, BORE_Z, STEEL, segments=24)
muzzle = part.to_object("Muzzle")
part = Part()
for y in (358.0, 368.0, 378.0):
    part.box(-3.5, 3.5, y, y + 5.0, BORE_Z + 4.0, BORE_Z + 14.0, STEEL)
for side in (-1.0, 1.0):
    for y in (362.0, 376.0):
        part.box(*sorted((side * 5.0, side * 14.0)), y, y + 8.0, BORE_Z - 3.5, BORE_Z + 3.5, STEEL)
boolean(muzzle, part.to_object("MuzzlePorts"), "DIFFERENCE")

# The iron sights on the rail. The rear: an aperture ring on a thin stem,
# with low wings either side; the front: a post with an orange dot, between
# two guard ears that hide behind the ring when aiming.
part = Part()
part.box(-9.0, 9.0, REAR_SIGHT_Y - 6.0, REAR_SIGHT_Y + 8.0, RAIL_TOP, RAIL_TOP + 4.0, RECEIVER)
part.box(-2.5, 2.5, REAR_SIGHT_Y - 1.0, REAR_SIGHT_Y + 3.0, RAIL_TOP + 3.5, SIGHT_LINE_Z - RING_RADIUS + 0.5, RECEIVER)
inner = circle((0.0, SIGHT_LINE_Z), APERTURE_RADIUS, 24)
outer = circle((0.0, SIGHT_LINE_Z), RING_RADIUS, 24)
for i in range(24):
    j = (i + 1) % 24
    part.prism("y", [inner[i], outer[i], outer[j], inner[j]], REAR_SIGHT_Y, REAR_SIGHT_Y + 2.0, RECEIVER)
for side in (-1.0, 1.0):
    part.prism("y", [(side * 7.0, RAIL_TOP + 3.5), (side * 8.5, RAIL_TOP + 3.5), (side * 8.5, SIGHT_LINE_Z - 4.0),
                     (side * 7.0, SIGHT_LINE_Z - 3.0)], REAR_SIGHT_Y - 4.0, REAR_SIGHT_Y + 6.0, RECEIVER)
part.to_object("IronSightRear", origin=(0.0, REAR_SIGHT_Y - 6.0, RAIL_TOP))
part = Part()
y0, y1 = FRONT_POST_Y
part.box(-9.0, 9.0, 238.0, 256.0, RAIL_TOP, RAIL_TOP + 4.0, RECEIVER)
part.prism("x", [(y0, RAIL_TOP + 3.5), (y1, RAIL_TOP + 3.5), (y1, SIGHT_LINE_Z - 2.0), (y0 + 2.0, SIGHT_LINE_Z),
                 (y0, SIGHT_LINE_Z)], -POST_HALF_WIDTH, POST_HALF_WIDTH, RECEIVER)
part.cylinder("y", 1.3, (0.0, SIGHT_LINE_Z - 1.9), y0 - 0.15, y0 + 0.2, FRONT_DOT, segments=12)
for side in (-1.0, 1.0):
    part.prism("x", [(241.0, RAIL_TOP + 3.5), (253.0, RAIL_TOP + 3.5), (253.0, SIGHT_LINE_Z - 2.0),
                     (249.0, SIGHT_LINE_Z + 2.0), (244.0, SIGHT_LINE_Z + 2.0), (241.0, SIGHT_LINE_Z - 1.0)],
               *sorted((side * 9.0, side * 10.5)), RECEIVER)
part.to_object("IronSightFront", origin=(0.0, 238.0, RAIL_TOP))
empty("AimPoint", (0.0, REAR_SIGHT_Y, SIGHT_LINE_Z), None)
# The middle of the muzzle, where the shots come out.
empty("MuzzlePoint", (0.0, MUZZLE_Y, BORE_Z), None)
# Shrunk with the gun, so a sight on it keeps its true size next to it.
empty("SightMount", (0.0, SIGHT_MOUNT_Y, RAIL_TOP), None, scale=SCALE)

# The lower: under the upper from the end plate to the magazine well, the
# well's opening sunk into its bottom; a hand stop under the front.
part = Part()
part.prism("x", [(2.0, JOINT_Z), (246.0, JOINT_Z), (246.0, -20.0), (242.0, -26.0), (186.0, -26.0), (186.0, -4.0),
                 (20.0, -4.0), (8.0, -2.0), (2.0, 4.0)], -LOWER_HALF_WIDTH, LOWER_HALF_WIDTH, POLYMER)
lower = part.to_object("Lower")
part = Part()
part.box(-12.0, 12.0, MAGAZINE_TOP[0] - 20.0, MAGAZINE_TOP[0] + 20.0, -30.0, -6.0, POLYMER)
boolean(lower, part.to_object("MagazineWell"), "DIFFERENCE")
part = Part()
part.prism("x", [(262.0, JOINT_Z + 0.5), (UPPER_FRONT - 2.0, JOINT_Z + 0.5), (UPPER_FRONT - 2.0, 8.0),
                 (284.0, -10.0), (278.0, -10.0), (270.0, 6.0)], -10.0, 10.0, POLYMER)
part.to_object("HandStop")

# The grip (lofted round, on its slant, fuller in the palm and flared at the
# bottom) and the trigger guard.
part = Part()
sections = [(4.0, -15.0, 15.0, 11.0, 3.0), (-6.0, -16.0, 15.5, 12.0, 6.0), (-40.0, -16.5, 15.0, 12.5, 7.0),
            (-80.0, -15.5, 14.5, 12.0, 7.0), (-90.0, -17.0, 16.0, 13.0, 6.0)]
part.loft([[on_grip(x, u, v) for x, u in rounded_rect(-half, half, back, front, radius)]
           for v, back, front, half, radius in sections], POLYMER)
part.band([(186.0, -20.0), (186.0, -28.0), (181.0, -33.0), (172.0, -35.0), (132.0, -35.0), (122.0, -27.0)], 4.5,
          -5.5, 5.5, POLYMER)
part.to_object("Grip")

part = Part()
# Down from its pin, its tip curling forward towards the muzzle.
part.band([(150.0, -3.0), (149.4, -9.0), (150.2, -15.0), (152.0, -20.0), (154.5, -23.0)], 4.5, -3.0, 3.0, STEEL)
part.to_object("Trigger", origin=(0.0, 150.0, -3.0))

# The fire selector on the left over the grip, turning about its hub.
part = Part()
part.cylinder("x", 4.5, (100.0, 4.0), -LOWER_HALF_WIDTH - 1.5, -LOWER_HALF_WIDTH + 0.5, STEEL, segments=14)
part.prism("x", [(100.0, 2.5), (100.0, 5.5), (86.0, 8.0), (84.0, 6.0)], -LOWER_HALF_WIDTH - 2.5,
           -LOWER_HALF_WIDTH - 1.0, STEEL)
part.to_object("Selector", origin=(-LOWER_HALF_WIDTH, 100.0, 4.0))

# The magazine release: a button on the left of the well, behind the magazine.
part = Part()
part.box(-LOWER_HALF_WIDTH - 2.0, -LOWER_HALF_WIDTH + 0.5, 238.0, 244.0, -16.0, -8.0, STEEL)
part.to_object("MagRelease", origin=(-LOWER_HALF_WIDTH, 241.0, -12.0))

# The stock, folded forward along the right side, below the ejection port:
# its hinge knuckle, two struts and the butt plate, its pad facing forward.
part = Part()
part.cylinder("z", HINGE_Y - REAR_Y, (19.0, HINGE_Y), 9.5, 20.5, RECEIVER, segments=16)
for z0 in (2.0, 22.0):
    part.box(17.0, 22.0, HINGE_Y, 168.0, z0, z0 + 6.0, RECEIVER)
part.box(17.0, 22.0, HINGE_Y, 4.0, 2.0, 28.0, RECEIVER)
part.prism("y", rounded_rect(16.0, 44.0, -38.0, 24.0, 7.0), 166.0, 175.0, RECEIVER)
part.prism("y", rounded_rect(17.0, 43.0, -37.0, 23.0, 6.5), 175.0, 181.0, RUBBER)
part.to_object("Stock", origin=(19.0, HINGE_Y, 15.0))

# The magazine, built along its own axes (down it is -Z, forward +Y) and
# turned onto its rake: a curved body (in the well until it drops), the
# feed lips with the top round in them, and the base plate.
bend = lambda v: MAGAZINE_BEND * v * v
part = Part()
levels = [-4.0 - (MAGAZINE_LENGTH - 4.0) * i / 8 for i in range(9)]
part.loft([[(x, u + bend(v), v) for x, u in rounded_rect(-11.0, 11.0, -17.0, 17.0, 2.5, per_corner=3)]
           for v in levels], MAG, smooth=False)
part.prism("z", rounded_rect(-7.5, 7.5, -16.0, 14.0, 2.0, per_corner=3), -4.0, 0.0, STEEL)
part.cylinder("y", 4.6, (0.0, -0.5), -14.0, 4.0, BRASS)
part.prism("y", circle((0.0, -0.5), 4.4, 16), 4.0, 8.0, COPPER)
part.prism("y", circle((0.0, -0.5), 2.8, 16), 8.0, 12.0, COPPER)
foot = bend(-MAGAZINE_LENGTH)
part.prism("z", rounded_rect(-12.5, 12.5, -18.5 + foot, 20.5 + foot, 3.0, per_corner=3), -MAGAZINE_LENGTH - 7.0,
           -MAGAZINE_LENGTH, POLYMER)
magazine = part.to_object("Magazine")
place(magazine, Vector((0.0, *MAGAZINE_TOP)) * MM, None, (MAGAZINE_RAKE, 0.0, 0.0))


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
print(f"smg: {size.x:.1f} wide, {size.z:.1f} high, {size.y:.1f} long (game mm, x{SCALE:.3f} of true size)")
assert abs(size.y - LENGTH) < 0.05, f"the SMG is {size.y:.2f} mm long, not {LENGTH}"

bpy.ops.wm.save_as_mainfile(filepath=f"{ROOT}/art/smg.blend")
bpy.ops.export_scene.gltf(
    filepath=f"{ROOT}/assets/models/weapons/smg.glb", export_format="GLB", export_yup=True, export_apply=True
)
