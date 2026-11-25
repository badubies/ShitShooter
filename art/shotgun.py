"""Builds the shotgun: an original tactical pump-action. A black alloy
receiver with a full-length top rail running out over a ventilated heat
shield, a heavy barrel ending in a slotted muzzle device, a magazine tube
under it clamped at the front, a ribbed polymer pump forend on twin action
bars, a pistol grip and a solid
polymer stock with a rubber cheek pad. A ghost-ring rear sight and a post with
an orange dot between guard ears, both on the rail; a mount on the rail for the
2x.

Built for the game, so it is placed in the hand as it is, with no turning or
scaling in code:
- Metres. X is right, Y forward along the barrel, Z up. It is exported Y-up,
  so in the file the barrel points along -Z and up is +Y, as the game holds it.
- Its length is the shotgun's `size` in src/weapons.rs (0.29 m), and its origin
  is the middle of its bounding box.
- It is designed at true size (`TRUE_LENGTH`) and shrunk by `SCALE` to that
  length, and its `SightMount` is scaled by `SCALE` too, so a sight (built
  in true metres) keeps its true size next to the gun (see PLAN.md, Round 4).
- Laid out on the rifle's heights (bore, rail, sight line, stock), so aimed it
  looks like the rifle: the stock's comb drops away behind the eye.

The nodes the game looks for (see src/gun_model.rs):
- Pump: the forend and its action bars; worked back and forth after each
  shot (`PUMP_AFTER` in src/player.rs), up to 0.085 of the gun's length.
- Shell: a shell lying in the receiver; shown only while one is loaded,
  rising from under the gun into the loading gate and pushed into the tube.
- IronSightRear, IronSightFront: hidden when a sight is mounted.
- AimPoint: the middle of the rear aperture, on the line over the front
  post, which aiming puts on the crosshair.
- MuzzlePoint: the middle of the muzzle, where the tracers start.
- SightMount: where a sight's base goes, on the rail. The sights face +Y,
  as the gun does, so they go on as they are.
- Trigger and Safety pivot or slide about their origins for animations to come.

Saves art/shotgun.blend and exports assets/models/weapons/Shotgun.glb. Run with:
blender --background --factory-startup --python shotgun.py -- <repo root>
"""
import math
import sys

import bmesh
import bpy
from mathutils import Vector

ROOT = sys.argv[sys.argv.index("--") + 1]

# The shotgun's length in src/weapons.rs, in millimetres: how long the game holds it.
LENGTH = 290.0
# How long it is at true size, which it is laid out at; everything is shrunk
# by `SCALE` to `LENGTH`, sights on its mount included.
TRUE_LENGTH = 800.0
SCALE = LENGTH / TRUE_LENGTH
# Laid out in true millimetres: y = 0 at the rear of the receiver. Every
# vertex and position is turned into game metres with `MM` as it is made, and
# the whole gun is centred on its origin at the end.
MM = 0.001 * SCALE
# The butt pad is the rearmost point, the muzzle device the front.
REAR_Y = -240.0
MUZZLE_Y = REAR_Y + TRUE_LENGTH
RECEIVER_BOTTOM = -6.0
RECEIVER_FRONT = 235.0
HALF_WIDTH = 17.0
UPPER_TOP = 50.0
RAIL_TOP = 57.0
BORE_Z = 30.0
BARREL_RADIUS = 12.5
# The magazine tube lies under the barrel.
TUBE_Z = BORE_Z - 25.0
TUBE_RADIUS = 11.5
TUBE_FRONT = 450.0
# The ventilated heat shield over the barrel, and the rail over it, run
# nearly to the muzzle.
SHIELD_FRONT = 524.0
# The pump: its forend, and how far it is drawn back (0.085 of the gun's
# length, as in src/player.rs).
FOREND_Y = (290.0, 420.0)
PUMP_TRAVEL = 0.085 * TRUE_LENGTH
# The loading gate under the receiver, where a shell goes in.
GATE_Y = (158.0, 224.0)
SHELL_Y = 190.0
# The line aimed along: the middle of the rear aperture and the top of the
# post, high enough that the front sight's base stays below the ring.
SIGHT_LINE_Z = 80.0
REAR_SIGHT_Y = 30.0
FRONT_POST_Y = (508.0, 513.0)
# Sized by how they look from the eye (PLAN.md): the aperture about 2.4
# degrees across, the post about 0.75 degrees wide, the guard ears hidden
# behind the ring.
APERTURE_RADIUS = 4.6
RING_RADIUS = 6.0
POST_HALF_WIDTH = 5.5
EARS_X = (15.5, 17.5)
# Where a sight's base is centred, on the rail.
SIGHT_MOUNT_Y = 100.0
# The pistol grip slants back by this much from the top of its front edge.
GRIP_ANGLE = math.radians(20.0)
GRIP_TOP = (92.0, -6.0)


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
bpy.ops.wm.read_factory_settings(use_empty=True)
RECEIVER = material("Shotgun_Receiver", (0.028, 0.030, 0.033), roughness=0.38)
POLYMER = material("Shotgun_Polymer", (0.05, 0.051, 0.055), roughness=0.8)
RUBBER = material("Shotgun_Rubber", (0.018, 0.018, 0.019), roughness=0.95)
STEEL = material("Shotgun_Steel", (0.075, 0.076, 0.08), roughness=0.32)
BARREL = material("Shotgun_Barrel", (0.04, 0.042, 0.046), roughness=0.3)
HEAD = material("Shotgun_ShellHead", (0.12, 0.12, 0.125), roughness=0.3)
HULL = material("Shotgun_Hull", (0.11, 0.008, 0.008), roughness=0.5)
FRONT_DOT = material("Shotgun_FrontDot", (1.0, 0.32, 0.02), roughness=0.4, emission=(1.0, 0.32, 0.02), strength=1.5)

COLLECTION = bpy.data.collections.new("Shotgun")
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




# The receiver: a tall alloy block chamfered along its top edges, with the
# ejection port on the right (the barrel's chamber and the bolt show in it),
# the loading gate under the front, and the slots the action bars slide into.
part = Part()
w, top = HALF_WIDTH, UPPER_TOP
part.prism("y", [(-w, RECEIVER_BOTTOM), (w, RECEIVER_BOTTOM), (w, top - 5.0), (w - 5.0, top), (-w + 5.0, top),
                 (-w, top - 5.0)], 0.0, RECEIVER_FRONT, RECEIVER)
receiver = part.to_object("Receiver")
part = Part()
part.box(4.0, w + 1.0, 112.0, 184.0, 22.0, 42.0, RECEIVER)
part.box(-10.0, 10.0, GATE_Y[0], GATE_Y[1], RECEIVER_BOTTOM - 1.0, 16.0, RECEIVER)
for side in (-1.0, 1.0):
    part.box(*sorted((side * 8.0, side * 14.0)), 205.0, RECEIVER_FRONT + 1.0, -3.0, 7.0, RECEIVER)
    # A recessed panel along the right side behind the port, and its mirror.
    part.prism("x", rounded_rect(14.0, 100.0, 14.0, 40.0, 5.0), *sorted((side * (w - 1.2), side * (w + 1.0))), RECEIVER)
boolean(receiver, part.to_object("ReceiverCuts"), "DIFFERENCE")
# Inside the port: the chamber and the bolt.
part = Part()
part.box(-9.0, 9.0, 120.0, 180.0, 24.0, 40.0, STEEL)
part.box(9.0, 10.0, 150.0, 178.0, 28.0, 33.0, STEEL)
part.to_object("ReceiverBolt")
# The end plate on the back of the receiver, which the stock is fixed to,
# with a sling socket on its left.
part = Part()
part.box(-14.0, 14.0, -2.0, 0.0, RECEIVER_BOTTOM - 2.0, 46.0, RECEIVER)
part.cylinder("x", 4.0, (-1.0, 22.0), -18.0, -14.0, STEEL, segments=14)
part.to_object("EndPlate")

# The heat shield over the barrel: wide and low, a row of rounded vents along
# each side.
part = Part()
part.prism("y", [(-15.0, 36.0), (15.0, 36.0), (18.0, 42.0), (14.0, top), (-14.0, top), (-18.0, 42.0)],
           RECEIVER_FRONT, SHIELD_FRONT, RECEIVER)
shield = part.to_object("HeatShield")
part = Part()
y = RECEIVER_FRONT + 14.0
while y + 26.0 < SHIELD_FRONT - 8.0:
    for side in (-1.0, 1.0):
        part.prism("x", rounded_rect(y, y + 26.0, 39.0, 44.0, 2.4), *sorted((side * 13.0, side * 20.0)), RECEIVER)
    y += 36.0
boolean(shield, part.to_object("HeatShieldVents"), "DIFFERENCE")

# The rail along the top, from the back of the receiver to the front sight: a
# core with cross ridges, the sights' clamps' width.
part = Part()
part.box(-7.0, 7.0, 2.0, SHIELD_FRONT, top - 0.5, RAIL_TOP - 1.0, RECEIVER)
ridge = [(-8.0, top - 0.5), (8.0, top - 0.5), (8.0, top + 2.0), (10.5, top + 4.5), (10.5, top + 5.5), (9.0, RAIL_TOP),
         (-9.0, RAIL_TOP), (-10.5, top + 5.5), (-10.5, top + 4.5), (-8.0, top + 2.0)]
y = 2.0
while y + 5.2 <= SHIELD_FRONT - 2.0:
    part.prism("y", ridge, y, y + 5.2, RECEIVER)
    y += 10.0
part.to_object("Rail")

# The barrel, from the chamber to the muzzle, and its muzzle device: a thick
# sleeve with slots cut round its front, up, down and to both sides.
part = Part()
part.tube(BARREL_RADIUS, 9.2, 190.0, MUZZLE_Y - 40.0, 20.0, BORE_Z, BARREL, segments=24)
part.to_object("Barrel")
part = Part()
part.tube(15.0, 9.2, MUZZLE_Y - 60.0, MUZZLE_Y, 40.0, BORE_Z, STEEL, segments=24)
device = part.to_object("MuzzleDevice")
part = Part()
y0, y1 = MUZZLE_Y - 30.0, MUZZLE_Y + 1.0
for z0, z1 in ((BORE_Z + 7.0, BORE_Z + 17.0), (BORE_Z - 17.0, BORE_Z - 7.0)):
    part.box(-2.0, 2.0, y0, y1, z0, z1, STEEL)
for side in (-1.0, 1.0):
    part.box(*sorted((side * 7.0, side * 17.0)), y0, y1, BORE_Z - 2.0, BORE_Z + 2.0, STEEL)
boolean(device, part.to_object("MuzzleDeviceSlots"), "DIFFERENCE")

# The magazine tube under the barrel, with the clamp at its front that holds
# it to the barrel, and a sling loop under the cap.
part = Part()
part.tube(TUBE_RADIUS, 8.0, RECEIVER_FRONT - 2.0, TUBE_FRONT, 10.0, TUBE_Z, BARREL, segments=20)
part.cylinder("y", 14.0, (0.0, TUBE_Z), TUBE_FRONT - 18.0, TUBE_FRONT + 6.0, STEEL, segments=20)
part.cylinder("y", 15.5, (0.0, BORE_Z), TUBE_FRONT - 18.0, TUBE_FRONT - 4.0, STEEL, segments=24)
part.box(-8.0, 8.0, TUBE_FRONT - 18.0, TUBE_FRONT - 4.0, TUBE_Z, BORE_Z, STEEL)
part.band([(TUBE_FRONT - 4.0, TUBE_Z - 12.0), (TUBE_FRONT - 4.0, TUBE_Z - 22.0), (TUBE_FRONT - 12.0, TUBE_Z - 22.0),
           (TUBE_FRONT - 12.0, TUBE_Z - 12.0)], 3.0, -2.0, 2.0, STEEL)
part.to_object("MagazineTube")

# The pump: a ribbed polymer forend round the tube on two steel action bars
# that run back into the receiver. It is drawn back along the bars.
part = Part()
y0, y1 = FOREND_Y
part.prism("y", rounded_rect(-21.0, 21.0, TUBE_Z - 15.0, TUBE_Z + 19.0, 9.0, per_corner=4), y0, y1, POLYMER)
part.prism("y", rounded_rect(-23.0, 23.0, TUBE_Z - 17.0, TUBE_Z + 19.0, 10.0, per_corner=4), y1 - 10.0, y1 + 4.0, POLYMER)
for i in range(6):
    y = y0 + 20.0 + i * 15.0
    part.prism("y", rounded_rect(-22.4, 22.4, TUBE_Z - 16.4, TUBE_Z + 19.0, 9.6, per_corner=4), y, y + 6.0, POLYMER)
for side in (-1.0, 1.0):
    part.box(*sorted((side * 8.5, side * 13.0)), RECEIVER_FRONT - PUMP_TRAVEL, y0 + 4.0, -2.0, 6.0, STEEL)
part.to_object("Pump", origin=(0.0, (y0 + y1) / 2, TUBE_Z))

# The iron sights on the rail. The rear: an aperture ring on a thin stem,
# with low wings either side; the front: a post with an orange dot, between
# two guard ears that hide behind the ring when aiming.
part = Part()
part.box(-10.0, 10.0, REAR_SIGHT_Y - 6.0, REAR_SIGHT_Y + 10.0, RAIL_TOP, RAIL_TOP + 4.0, RECEIVER)
part.box(-3.0, 3.0, REAR_SIGHT_Y - 1.0, REAR_SIGHT_Y + 3.5, RAIL_TOP + 3.5, SIGHT_LINE_Z - RING_RADIUS + 0.5, RECEIVER)
inner = circle((0.0, SIGHT_LINE_Z), APERTURE_RADIUS, 24)
outer = circle((0.0, SIGHT_LINE_Z), RING_RADIUS, 24)
for i in range(24):
    j = (i + 1) % 24
    part.prism("y", [inner[i], outer[i], outer[j], inner[j]], REAR_SIGHT_Y, REAR_SIGHT_Y + 2.5, RECEIVER)
for side in (-1.0, 1.0):
    part.prism("y", [(side * 9.5, RAIL_TOP + 3.5), (side * 11.5, RAIL_TOP + 3.5), (side * 11.5, SIGHT_LINE_Z - 6.0),
                     (side * 9.5, SIGHT_LINE_Z - 4.5)], REAR_SIGHT_Y - 4.0, REAR_SIGHT_Y + 8.0, RECEIVER)
part.to_object("IronSightRear", origin=(0.0, REAR_SIGHT_Y - 6.0, RAIL_TOP))
part = Part()
y0, y1 = FRONT_POST_Y
base = y0 - 9.0
part.box(-EARS_X[1] - 1.5, EARS_X[1] + 1.5, base, base + 22.0, RAIL_TOP, RAIL_TOP + 4.0, RECEIVER)
part.prism("x", [(y0, RAIL_TOP + 3.5), (y1, RAIL_TOP + 3.5), (y1, SIGHT_LINE_Z - 2.5), (y0 + 2.5, SIGHT_LINE_Z),
                 (y0, SIGHT_LINE_Z)], -POST_HALF_WIDTH, POST_HALF_WIDTH, RECEIVER)
part.cylinder("y", 2.4, (0.0, SIGHT_LINE_Z - 2.8), y0 - 0.15, y0 + 0.2, FRONT_DOT, segments=12)
for side in (-1.0, 1.0):
    part.prism("x", [(base + 2.0, RAIL_TOP + 3.5), (base + 20.0, RAIL_TOP + 3.5), (base + 20.0, SIGHT_LINE_Z - 2.0),
                     (base + 15.0, SIGHT_LINE_Z + 2.5), (base + 7.0, SIGHT_LINE_Z + 2.5),
                     (base + 2.0, SIGHT_LINE_Z - 1.0)], *sorted((side * EARS_X[0], side * EARS_X[1])), RECEIVER)
part.to_object("IronSightFront", origin=(0.0, base, RAIL_TOP))
empty("AimPoint", (0.0, REAR_SIGHT_Y, SIGHT_LINE_Z), None)
# The middle of the muzzle, where the shots come out.
empty("MuzzlePoint", (0.0, MUZZLE_Y, BORE_Z), None)
# Shrunk with the gun, so a sight on it keeps its true size next to it.
empty("SightMount", (0.0, SIGHT_MOUNT_Y, RAIL_TOP), None, scale=SCALE)

# A shell lying in the receiver, 70 mm long: shown only while one is loaded.
part = Part()
part.cylinder("y", 10.0, (0.0, BORE_Z), SHELL_Y - 35.0, SHELL_Y + 22.0, HULL, segments=16)
part.cylinder("y", 10.4, (0.0, BORE_Z), SHELL_Y + 22.0, SHELL_Y + 35.0, HEAD, segments=16)
part.to_object("Shell", origin=(0.0, SHELL_Y, BORE_Z))

# The trigger group under the receiver is part of it; the pistol grip hangs
# behind the trigger guard.
# The grip (lofted round, on its slant, fuller in the palm and flared at the
# bottom) and the trigger guard.
part = Part()
sections = [(6.0, -17.0, 16.0, 12.0, 3.0), (-6.0, -18.0, 17.0, 13.0, 6.0), (-45.0, -18.5, 16.5, 13.5, 7.5),
            (-88.0, -17.5, 16.0, 13.0, 7.5), (-98.0, -19.0, 18.0, 14.0, 6.0)]
part.loft([[on_grip(x, u, v) for x, u in rounded_rect(-half, half, back, front, radius)]
           for v, back, front, half, radius in sections], POLYMER)
part.band([(136.0, -26.0), (136.0, -32.0), (130.0, -38.0), (120.0, -40.0), (104.0, -40.0), (98.0, -33.0)], 4.5,
          -6.0, 6.0, POLYMER)
part.to_object("Grip")


part = Part()
# Down from its pin, its tip curling forward towards the muzzle.
part.band([(120.0, -7.0), (119.3, -13.0), (120.1, -19.0), (122.0, -24.0), (124.8, -27.0)], 4.5, -3.0, 3.0, STEEL)
part.to_object("Trigger", origin=(0.0, 120.0, -7.0))

# The cross-bolt safety through the front of the guard's top: a button each side.
part = Part()
part.cylinder("x", 3.6, (140.0, -9.0), -w - 2.0, w + 2.0, STEEL, segments=14)
part.cylinder("x", 4.2, (140.0, -9.0), w + 2.0, w + 3.5, RUBBER, segments=14)
part.to_object("Safety", origin=(0.0, 140.0, -9.0))

# The stock: a solid polymer stock from the end plate back to the butt. Its
# comb drops away from the receiver so it stays under the eye when aiming,
# and its top is narrow, so from the eye it is a thin strip; it is full
# below, where the eye does not see it. On each side a sculpted pocket at the
# front, a ribbed grip panel at the back, a screw at the front and a sling
# socket near the butt; a sling slot through the toe, a ribbed rubber cheek
# pad along the comb, and a ribbed rubber butt pad on a polymer plate.
STOCK_FRONT = -2.0
STOCK_BACK = REAR_Y + 14.0


def along_stock(y):
    """How far back along the stock `y` is, from 0 at its front to 1 at the butt."""
    return (y - STOCK_FRONT) / (STOCK_BACK - STOCK_FRONT)


def comb_z(y):
    return 46.0 - 22.0 * along_stock(y)


def toe_z(y):
    return -2.0 - 48.0 * along_stock(y)


def stock_half_width(y):
    return 13.0 + 1.5 * along_stock(y)


def stock_section(y):
    """The stock across, at `y`: narrow along the comb, full below it."""
    t = along_stock(y)
    top, bottom, half, comb = comb_z(y), toe_z(y), stock_half_width(y), 6.5 + 1.0 * t
    right = [(half - 5.0, bottom), (half - 1.5, bottom + 1.5), (half, bottom + 5.0), (half, top - 20.0),
             (half - 1.0, top - 15.0), (comb + 3.5, top - 6.0), (comb + 1.2, top - 1.5), (comb - 1.5, top)]
    return [(x, y, z) for x, z in right + [(-x, z) for x, z in reversed(right)]]


def along_lines(y0, y1, below_comb, above_toe):
    """A four-sided outline on the side of the stock, from `y0` back to `y1`,
    its top and bottom following the comb and the toe, as (y, z) points."""
    return [(y0, toe_z(y0) + above_toe), (y0, comb_z(y0) - below_comb), (y1, comb_z(y1) - below_comb),
            (y1, toe_z(y1) + above_toe)]


def rounded_polygon(corners, radius, per_corner=4):
    """A polygon with each corner rounded off `radius` along each side of it."""
    points = []
    for i, (px, py) in enumerate(corners):
        ends = []
        for qx, qy in (corners[i - 1], corners[(i + 1) % len(corners)]):
            length = math.hypot(qx - px, qy - py)
            r = min(radius, length / 2.0)
            ends.append((px + (qx - px) * r / length, py + (qy - py) * r / length))
        (ax, ay), (bx, by) = ends
        for k in range(per_corner + 1):
            s = k / per_corner
            points.append(((1 - s) ** 2 * ax + 2 * (1 - s) * s * px + s * s * bx,
                           (1 - s) ** 2 * ay + 2 * (1 - s) * s * py + s * s * by))
    return points


part = Part()
part.loft([stock_section(STOCK_FRONT + (STOCK_BACK - STOCK_FRONT) * i / 8) for i in range(9)], POLYMER)
part.prism("y", rounded_rect(-16.0, 16.0, -50.0, 24.0, 5.0), STOCK_BACK - 6.0, STOCK_BACK + 0.5, POLYMER)
stock = part.to_object("Stock")
part = Part()
pocket = rounded_polygon(along_lines(-30.0, -118.0, 14.0, 12.0), 8.0)
for side in (-1.0, 1.0):
    half = stock_half_width(-30.0)
    part.prism("x", pocket, *sorted((side * (half - 2.5), side * (half + 3.0))), POLYMER)
    half = stock_half_width(-200.0)
    for i in range(6):
        below = 22.0 + 5.0 * i
        rib = [(-136.0, comb_z(-136.0) - below), (-136.0, comb_z(-136.0) - below - 2.2),
               (-204.0, comb_z(-204.0) - below - 2.2), (-204.0, comb_z(-204.0) - below)]
        part.prism("x", rounded_polygon(rib, 1.0, per_corner=2), *sorted((side * (half - 0.9), side * (half + 3.0))),
                   POLYMER)
slot = rounded_polygon([(-150.0, toe_z(-150.0) + 5.0), (-150.0, toe_z(-150.0) + 11.0),
                        (-178.0, toe_z(-178.0) + 11.0), (-178.0, toe_z(-178.0) + 5.0)], 3.0)
part.prism("x", slot, -20.0, 20.0, POLYMER)
boolean(stock, part.to_object("StockCuts"), "DIFFERENCE")

part = Part()
for side in (-1.0, 1.0):
    half = stock_half_width(-14.0)
    part.cylinder("x", 3.4, (-14.0, 20.0), *sorted((side * (half - 1.0), side * (half + 0.8))), STEEL, segments=12)
    part.box(*sorted((side * (half + 0.6), side * (half + 1.0))), -14.6, -13.4, 17.6, 22.4, RUBBER)
    y, z = -210.0, toe_z(-210.0) + 16.0
    half = stock_half_width(y)
    part.cylinder("x", 5.0, (y, z), *sorted((side * (half - 1.0), side * (half + 1.2))), STEEL, segments=16)
    part.cylinder("x", 2.8, (y, z), *sorted((side * (half + 1.1), side * (half + 1.4))), RUBBER, segments=12)
part.to_object("StockFittings")

# The cheek pad along the comb, ribbed across.
part = Part()
rings = []
for i in range(7):
    y = -66.0 + (-178.0 + 66.0) * i / 6
    comb = 6.5 + 1.0 * along_stock(y) - 0.5
    rings.append([(x, y, z) for x, z in rounded_rect(-comb, comb, comb_z(y) - 2.5, comb_z(y) + 2.2, 2.0, per_corner=3)])
part.loft(rings, RUBBER)
y = -72.0
while y - 3.0 >= -172.0:
    rib = [(y, comb_z(y) + 1.0), (y, comb_z(y) + 2.9), (y - 3.0, comb_z(y - 3.0) + 2.9), (y - 3.0, comb_z(y - 3.0) + 1.0)]
    comb = 6.5 + 1.0 * along_stock(y) - 1.5
    part.prism("x", rib, -comb, comb, RUBBER)
    y -= 8.0
part.to_object("CheekPad")

part = Part()
part.prism("y", rounded_rect(-16.0, 16.0, -50.0, 24.0, 5.0), REAR_Y, STOCK_BACK - 6.0, RUBBER)
pad = part.to_object("ButtPad")
part = Part()
for i in range(8):
    z = -44.0 + 8.5 * i
    part.box(-20.0, 20.0, REAR_Y - 1.0, REAR_Y + 1.6, z, z + 3.0, RUBBER)
boolean(pad, part.to_object("ButtPadGrooves"), "DIFFERENCE")

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
print(f"shotgun: {size.x:.1f} wide, {size.z:.1f} high, {size.y:.1f} long (game mm, x{SCALE:.3f} of true size)")
assert abs(size.y - LENGTH) < 0.05, f"the shotgun is {size.y:.2f} mm long, not {LENGTH}"

bpy.ops.wm.save_as_mainfile(filepath=f"{ROOT}/art/shotgun.blend")
bpy.ops.export_scene.gltf(
    filepath=f"{ROOT}/assets/models/weapons/Shotgun.glb", export_format="GLB", export_yup=True, export_apply=True
)
