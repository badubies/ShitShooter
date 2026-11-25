"""Builds the DMR: an original semi-automatic marksman's rifle. A black upper
receiver and a round, slotted handguard under one full-length top rail, on a
dark grey polymer lower with a straight 20-round magazine for its heavier
cartridge, a long barrel with a ported muzzle brake, and a solid stock with
an adjustable cheek piece. Backup irons on the rail (a ring at the back, a
post with an orange dot between guard ears at the front), and a mount on the
rail for the reflex, 2x, 3x and scope.

Built for the game, so it is placed in the hand as it is, with no turning or
scaling in code:
- Metres. X is right, Y forward along the barrel, Z up. It is exported Y-up,
  so in the file the barrel points along -Z and up is +Y, as the game holds it.
- Its length is the DMR's `size` in src/weapons.rs (0.38 m), and its origin
  is the middle of its bounding box.
- It is designed at true size (`TRUE_LENGTH`) and shrunk by `SCALE` to that
  length, and its `SightMount` is scaled by `SCALE` too, so a sight (built
  in true metres) keeps its true size next to the gun (see PLAN.md, Round 4).
- The sights sit forward on the rail, as on the sniper, so aimed, the eye
  is over the back of the receiver and the stock is behind it, out of view.

The nodes the game looks for (see src/gun_model.rs):
- Bolt: kicked back by each shot, held back when empty and drawn back to
  cock the gun at the end of a reload. It shows through the ejection port
  on the right and carries the charging handle, which sticks out of a slot
  on the left, the side seen from the hip.
- IronSightRear, IronSightFront: hidden when a sight is mounted.
- AimPoint: the middle of the rear aperture, on the line over the front
  post, which aiming puts on the crosshair.
- MuzzlePoint: the middle of the muzzle, where the tracers start.
- SightMount: where a sight's base goes, on the rail. The sights face +Y,
  as the gun does, so they go on as they are.
- Magazine: its origin is the top of the magazine, its axes along its rake.
- Trigger, Selector, BoltCatch and MagRelease pivot or slide about their
  origins for animations to come.

Saves art/dmr.blend and exports assets/models/weapons/dmr.glb. Run with:
blender --background --factory-startup --python dmr.py -- <repo root>
"""
import math
import sys

import bmesh
import bpy
from mathutils import Vector

ROOT = sys.argv[sys.argv.index("--") + 1]

# The DMR's length in src/weapons.rs, in millimetres: how long the game holds it.
LENGTH = 380.0
# How long it is at true size, which it is laid out at; everything is shrunk
# by `SCALE` to `LENGTH`, sights on its mount included.
TRUE_LENGTH = 1000.0
SCALE = LENGTH / TRUE_LENGTH
# Laid out in true millimetres: y = 0 at the rear of the upper receiver,
# which sits on the lower at z = `JOINT_Z`. Every vertex and position is
# turned into game metres with `MM` as it is made, and the whole gun is
# centred on its origin at the end.
MM = 0.001 * SCALE
# The butt pad is the rearmost point, the muzzle brake the front.
REAR_Y = -270.0
MUZZLE_Y = REAR_Y + TRUE_LENGTH
JOINT_Z = 12.0
UPPER_TOP = 52.0
UPPER_FRONT = 262.0
HANDGUARD_FRONT = 560.0
HANDGUARD_RADIUS = 24.0
UPPER_HALF_WIDTH = 17.0
LOWER_HALF_WIDTH = 15.0
RAIL_TOP = 59.0
BORE_Z = 31.0
# The bolt's face, closed, and the chamber in front of it.
BOLT_FACE_Y = 236.0
# The line aimed along: the middle of the rear aperture and the top of the
# post. The sights are forward on the rail (see above).
SIGHT_LINE_Z = RAIL_TOP + 23.0
REAR_SIGHT_Y = 190.0
FRONT_SIGHT_Y = 526.0
FRONT_POST_Y = (FRONT_SIGHT_Y + 9.0, FRONT_SIGHT_Y + 14.0)
# Sized by how they look from the eye (PLAN.md): the aperture about 2.4
# degrees across, the post about 0.8 degrees wide, the guard ears hidden
# behind the ring.
APERTURE_RADIUS = 4.4
RING_RADIUS = 5.8
POST_HALF_WIDTH = 3.9
EARS_X = (12.5, 14.5)
# Where a sight's base is centred, on the rail.
SIGHT_MOUNT_Y = 185.0
# The grip slants back by this much from the top of its front edge.
GRIP_ANGLE = math.radians(20.0)
GRIP_TOP = (92.0, -6.0)
# The magazine: straight, for a heavier cartridge than the rifle's; the
# middle of its top, its rake forward, its length and half its depth.
MAGAZINE_TOP = (190.0, JOINT_Z)
MAGAZINE_RAKE = math.radians(3.0)
MAGAZINE_LENGTH = 125.0
MAGAZINE_HALF_DEPTH = 36.0
MAGAZINE_HALF_WIDTH = 13.0
# How far the bolt goes back (BOLT_TRAVEL in src/player.rs), which the
# charging handle's slot must leave room for.
BOLT_TRAVEL = 0.14 * TRUE_LENGTH
# The stock, from the end plate to the butt: its comb straight, a little
# under the top of the receiver. The bolt goes back into it.
STOCK_FRONT = -2.0
STOCK_BACK = REAR_Y + 12.0
COMB_Z = 44.0
STOCK_HALF_WIDTH = 17.0



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
RECEIVER = material("DMR_Receiver", (0.028, 0.030, 0.033), roughness=0.38)
POLYMER = material("DMR_Polymer", (0.05, 0.051, 0.055), roughness=0.8)
MAG = material("DMR_Magazine", (0.04, 0.041, 0.044), roughness=0.7)
RUBBER = material("DMR_Rubber", (0.018, 0.018, 0.019), roughness=0.95)
STEEL = material("DMR_Steel", (0.075, 0.076, 0.08), roughness=0.32)
BRASS = material("DMR_Brass", (0.55, 0.36, 0.09), roughness=0.35)
COPPER = material("DMR_Copper", (0.45, 0.16, 0.06), roughness=0.35)
FRONT_DOT = material("DMR_FrontDot", (1.0, 0.32, 0.02), roughness=0.4, emission=(1.0, 0.32, 0.02), strength=1.5)

COLLECTION = bpy.data.collections.new("DMR")
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



def along(y, points):
    """The z of a line through (y, z) `points` (going back), at `y`."""
    for (y0, z0), (y1, z1) in zip(points, points[1:]):
        if y1 <= y <= y0:
            return z0 + (z1 - z0) * (y - y0) / (y1 - y0)
    return points[0][1] if y > points[0][0] else points[-1][1]


# The upper receiver: chamfered along its top edges, a recessed panel along
# each side, the ejection port on the right and the charging handle's slot
# on the left.
part = Part()
w, top = UPPER_HALF_WIDTH, UPPER_TOP
part.prism("y", [(-w, JOINT_Z), (w, JOINT_Z), (w, top - 4.0), (w - 4.0, top), (-w + 4.0, top), (-w, top - 4.0)],
           0.0, UPPER_FRONT, RECEIVER)
upper = part.to_object("Upper")
part = Part()
part.box(0.0, w + 1.0, 168.0, BOLT_FACE_Y - 4.0, 23.0, 39.0, RECEIVER)
handle_y = BOLT_FACE_Y - 16.0
slot_back = handle_y - BOLT_TRAVEL - 3.0
part.box(-w - 1.0, -w + 5.0, slot_back, handle_y + 9.0, 31.0, 38.0, RECEIVER)
part.box(w - 1.2, w + 1.0, 20.0, 158.0, 20.0, 44.0, RECEIVER)
part.box(-w - 1.0, -w + 1.2, 20.0, slot_back - 8.0, 20.0, 44.0, RECEIVER)
boolean(upper, part.to_object("UpperCuts"), "DIFFERENCE")

# The handguard: a round tube about the barrel, flat on top under the rail,
# long slots along each side and the bottom, and a sling socket on the left
# near its front.
part = Part()
tube = [(x, min(z, UPPER_TOP)) for x, z in circle((0.0, BORE_Z), HANDGUARD_RADIUS, 32)]
part.prism("y", tube, UPPER_FRONT, HANDGUARD_FRONT, RECEIVER)
handguard = part.to_object("Handguard")
part = Part()
for i in range(5):
    y0 = 282.0 + 54.0 * i
    for side in (-1.0, 1.0):
        part.prism("x", rounded_rect(y0, y0 + 38.0, BORE_Z - 5.0, BORE_Z + 5.0, 5.0),
                   *sorted((side * 18.0, side * 27.0)), RECEIVER)
    part.prism("z", rounded_rect(-4.0, 4.0, y0 + 4.0, y0 + 34.0, 3.5), BORE_Z - 27.0, BORE_Z - 18.0, RECEIVER)
boolean(handguard, part.to_object("HandguardSlots"), "DIFFERENCE")
part = Part()
part.cylinder("x", 5.5, (HANDGUARD_FRONT - 20.0, BORE_Z - 9.0), -25.5, -21.0, STEEL, segments=16)
part.cylinder("x", 3.0, (HANDGUARD_FRONT - 20.0, BORE_Z - 9.0), -25.9, -25.3, RUBBER, segments=12)
part.to_object("SlingSocket")

# The rail along the top of both: a core with cross ridges, the sights'
# clamps' width.
part = Part()
part.box(-7.0, 7.0, 2.0, HANDGUARD_FRONT - 2.0, top - 0.5, RAIL_TOP - 1.0, RECEIVER)
ridge = [(-8.0, top - 0.5), (8.0, top - 0.5), (8.0, top + 2.0), (10.5, top + 4.5), (10.5, top + 5.5), (9.0, RAIL_TOP),
         (-9.0, RAIL_TOP), (-10.5, top + 5.5), (-10.5, top + 4.5), (-8.0, top + 2.0)]
y = 2.0
while y + 5.2 <= HANDGUARD_FRONT - 2.0:
    part.prism("y", ridge, y, y + 5.2, RECEIVER)
    y += 10.0
part.to_object("Rail")

# The end plate on the back of the receiver, which the stock is fixed to.
part = Part()
part.box(-LOWER_HALF_WIDTH, LOWER_HALF_WIDTH, -2.0, 0.0, -2.0, UPPER_TOP - 4.0, RECEIVER)
part.to_object("EndPlate")

# The stock, from the end plate to the butt: its comb straight, its
# underside sweeping down to the toe, narrower along the comb than below.
# On each side a sculpted pocket and a sling socket at the front; on the
# left a storage door with its catch, on the right a grooved panel; a sling
# slot through the toe; an adjustable cheek piece on the comb, its knob on
# the right; a spacer and a ribbed rubber butt pad.
BOTTOM = [(STOCK_FRONT, -2.0), (-30.0, -14.0), (-70.0, -36.0), (-130.0, -52.0), (-200.0, -64.0), (STOCK_BACK, -74.0)]


def stock_bottom(y):
    return along(y, BOTTOM)


def stock_section(y):
    """The stock across, at `y`: a little narrower along the comb."""
    top, bottom, half, comb = COMB_Z, stock_bottom(y), STOCK_HALF_WIDTH, 12.0
    right = [(half - 5.0, bottom), (half - 1.5, bottom + 1.5), (half, bottom + 5.0), (half, top - 9.0),
             (half - 1.5, top - 4.0), (comb + 1.0, top - 0.8), (comb - 2.0, top)]
    return [(x, y, z) for x, z in right + [(-x, z) for x, z in reversed(right)]]


stations = [STOCK_FRONT, -10.0, -20.0, -30.0, -45.0, -58.0, -70.0, -90.0, -110.0, -130.0, -165.0, -200.0, -230.0,
            STOCK_BACK]
part = Part()
part.loft([stock_section(y) for y in stations], POLYMER)
stock = part.to_object("Stock")
h = STOCK_HALF_WIDTH
part = Part()
pocket = [(-40.0, stock_bottom(-40.0) + 9.0), (-40.0, COMB_Z - 12.0), (-122.0, COMB_Z - 12.0),
          (-122.0, stock_bottom(-122.0) + 9.0)]
door = [(-148.0, stock_bottom(-148.0) + 12.0), (-148.0, COMB_Z - 15.0), (-222.0, COMB_Z - 15.0),
        (-222.0, stock_bottom(-222.0) + 12.0)]
for side in (-1.0, 1.0):
    part.prism("x", rounded_polygon(pocket, 8.0), *sorted((side * (h - 2.5), side * (h + 3.0))), POLYMER)
for i in range(5):
    z = COMB_Z - 18.0 - 6.0 * i
    part.prism("x", rounded_rect(-220.0, -150.0, z - 2.2, z, 1.0, per_corner=2), h - 0.9, h + 3.0, POLYMER)
# The storage door on the left: sunk into the side, the door itself set in
# it a little proud of the bottom, which leaves a groove round it.
part.prism("x", rounded_polygon(door, 6.0), -h - 3.0, -h + 1.2, POLYMER)
slot = [(-212.0, stock_bottom(-212.0) + 5.0), (-212.0, stock_bottom(-212.0) + 11.0),
        (-238.0, stock_bottom(-238.0) + 11.0), (-238.0, stock_bottom(-238.0) + 5.0)]
part.prism("x", rounded_polygon(slot, 3.0), -h - 3.0, h + 3.0, POLYMER)
boolean(stock, part.to_object("StockCuts"), "DIFFERENCE")

part = Part()
for side in (-1.0, 1.0):
    part.cylinder("x", 5.0, (-26.0, COMB_Z - 22.0), *sorted((side * (h - 1.0), side * (h + 1.2))), STEEL, segments=16)
    part.cylinder("x", 2.8, (-26.0, COMB_Z - 22.0), *sorted((side * (h + 1.1), side * (h + 1.4))), RUBBER, segments=12)
inset = [(-149.5, stock_bottom(-148.0) + 13.5), (-149.5, COMB_Z - 16.5), (-220.5, COMB_Z - 16.5),
         (-220.5, stock_bottom(-222.0) + 13.5)]
part.prism("x", rounded_polygon(inset, 5.0), -h + 0.3, -h + 1.4, POLYMER)
# The storage door's catch.
part.prism("x", rounded_rect(-160.0, -152.0, COMB_Z - 30.0, COMB_Z - 20.0, 2.0, per_corner=2), -h - 1.0, -h + 0.5,
           STEEL)
# The cheek piece: a block on the comb, a dark seam under it, a rubber pad
# on top, and its knob on the right.
piece = [(-60.0, COMB_Z - 1.0), (-60.0, COMB_Z + 4.0), (-78.0, COMB_Z + 12.0), (-215.0, COMB_Z + 12.0),
         (-215.0, COMB_Z - 1.0)]
part.prism("x", rounded_polygon(piece, 4.0, per_corner=3), -13.0, 13.0, POLYMER)
part.box(-13.4, 13.4, -212.0, -63.0, COMB_Z - 0.2, COMB_Z + 1.0, RUBBER)
part.prism("y", rounded_rect(-11.0, 11.0, COMB_Z + 11.0, COMB_Z + 13.5, 1.5, per_corner=2), -206.0, -86.0, RUBBER)
part.cylinder("x", 7.5, (-150.0, COMB_Z - 8.0), h - 1.0, h + 6.0, RUBBER, segments=18)
part.cylinder("x", 5.5, (-150.0, COMB_Z - 8.0), h + 5.5, h + 7.0, STEEL, segments=18)
part.to_object("StockFittings")

part = Part()
part.prism("y", rounded_rect(-h - 0.5, h + 0.5, -76.0, COMB_Z + 1.0, 5.0), STOCK_BACK - 4.0, STOCK_BACK + 0.5, RECEIVER)
part.to_object("ButtSpacer")
part = Part()
part.prism("y", rounded_rect(-h - 0.5, h + 0.5, -76.0, COMB_Z + 1.0, 5.0), REAR_Y, STOCK_BACK - 3.5, RUBBER)
pad = part.to_object("ButtPad")
part = Part()
for i in range(13):
    z = -70.0 + 8.5 * i
    part.box(-22.0, 22.0, REAR_Y - 1.0, REAR_Y + 1.6, z, z + 3.0, RUBBER)
boolean(pad, part.to_object("ButtPadGrooves"), "DIFFERENCE")

# The bolt, with the extractor on its right; through the port it shows, and
# when it is back the chamber does. The charging handle rides on it, out
# through the slot on the left, its knob grooved for the fingers.
part = Part()
part.box(-8.5, 8.5, BOLT_FACE_Y - BOLT_TRAVEL + 2.0, BOLT_FACE_Y, 22.0, 39.0, STEEL)
part.box(8.5, 9.3, BOLT_FACE_Y - 10.0, BOLT_FACE_Y, 29.0, 34.0, STEEL)
bolt = part.to_object("Bolt", origin=(0.0, BOLT_FACE_Y, BORE_Z))
part = Part()
part.box(-w - 1.5, -8.5, handle_y, handle_y + 6.0, 32.0, 37.0, STEEL)
part.prism("z", rounded_rect(-w - 14.0, -w - 1.5, handle_y - 5.0, handle_y + 10.0, 3.0, per_corner=3), 30.0, 39.0,
           RECEIVER)
for i in range(4):
    part.box(-w - 14.4, -w - 13.6, handle_y - 2.5 + i * 3.6, handle_y - 1.0 + i * 3.6, 30.5, 38.5, RUBBER)
part.to_object("ChargingHandle", origin=(-w, handle_y + 3.0, 34.5), parent=bolt)

# The barrel, from the chamber to the muzzle brake.
part = Part()
part.box(-7.0, 7.0, 200.0, BOLT_FACE_Y + 4.0, 23.0, 39.0, STEEL)
part.tube(10.5, 3.9, BOLT_FACE_Y, MUZZLE_Y - 70.0, 20.0, BORE_Z, STEEL)
part.to_object("Barrel")

# The muzzle brake: squared off, three ports through each side and two on top.
part = Part()
part.prism("y", rounded_rect(-13.0, 13.0, BORE_Z - 12.0, BORE_Z + 12.0, 4.5), MUZZLE_Y - 75.0, MUZZLE_Y, STEEL)
brake = part.to_object("MuzzleBrake")
part = Part()
part.cylinder("y", 4.2, (0.0, BORE_Z), MUZZLE_Y - 70.0, MUZZLE_Y + 1.0, STEEL, segments=16)
for y0 in (MUZZLE_Y - 58.0, MUZZLE_Y - 40.0, MUZZLE_Y - 22.0):
    part.prism("x", rounded_rect(y0, y0 + 10.0, BORE_Z - 7.0, BORE_Z + 7.0, 2.5), -20.0, 20.0, STEEL)
for y in (MUZZLE_Y - 49.0, MUZZLE_Y - 31.0):
    for x in (-5.0, 5.0):
        part.cylinder("z", 2.2, (x, y), BORE_Z, BORE_Z + 20.0, STEEL, segments=12)
boolean(brake, part.to_object("MuzzleBrakePorts"), "DIFFERENCE")

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
f = FRONT_SIGHT_Y
part.box(-EARS_X[1] - 1.5, EARS_X[1] + 1.5, f, f + 22.0, RAIL_TOP, RAIL_TOP + 4.0, RECEIVER)
part.prism("x", [(y0, RAIL_TOP + 3.5), (y1, RAIL_TOP + 3.5), (y1, SIGHT_LINE_Z - 2.5), (y0 + 2.5, SIGHT_LINE_Z),
                 (y0, SIGHT_LINE_Z)], -POST_HALF_WIDTH, POST_HALF_WIDTH, RECEIVER)
part.cylinder("y", 1.9, (0.0, SIGHT_LINE_Z - 2.4), y0 - 0.15, y0 + 0.2, FRONT_DOT, segments=12)
for side in (-1.0, 1.0):
    part.prism("x", [(f + 2.0, RAIL_TOP + 3.5), (f + 20.0, RAIL_TOP + 3.5), (f + 20.0, SIGHT_LINE_Z - 2.0),
                     (f + 15.0, SIGHT_LINE_Z + 2.5), (f + 7.0, SIGHT_LINE_Z + 2.5), (f + 2.0, SIGHT_LINE_Z - 1.0)],
               *sorted((side * EARS_X[0], side * EARS_X[1])), RECEIVER)
part.to_object("IronSightFront", origin=(0.0, f, RAIL_TOP))
empty("AimPoint", (0.0, REAR_SIGHT_Y, SIGHT_LINE_Z), None)
# The middle of the muzzle, where the shots come out.
empty("MuzzlePoint", (0.0, MUZZLE_Y, BORE_Z), None)
# Shrunk with the gun, so a sight on it keeps its true size next to it.
empty("SightMount", (0.0, SIGHT_MOUNT_Y, RAIL_TOP), None, scale=SCALE)

# The lower: under the upper from the end plate to the handguard, its
# magazine well flared at the bottom, the well's opening sunk into it.
d = MAGAZINE_HALF_DEPTH
well = (MAGAZINE_TOP[0] - d - 4.0, MAGAZINE_TOP[0] + d + 10.0)
part = Part()
part.prism("x", [(0.0, JOINT_Z), (well[1] + 14.0, JOINT_Z), (well[1] + 14.0, 2.0), (well[1], -4.0), (well[1], -26.0),
                 (well[1] + 4.0, -33.0), (well[0] - 4.0, -33.0), (well[0], -26.0), (well[0], -6.0), (20.0, -6.0),
                 (6.0, -2.0), (0.0, 6.0)], -LOWER_HALF_WIDTH, LOWER_HALF_WIDTH, POLYMER)
lower = part.to_object("Lower")
part = Part()
part.box(-MAGAZINE_HALF_WIDTH - 0.7, MAGAZINE_HALF_WIDTH + 0.7, MAGAZINE_TOP[0] - d - 1.0, MAGAZINE_TOP[0] + d + 1.0,
         -40.0, -4.0, POLYMER)
boolean(lower, part.to_object("MagazineWell"), "DIFFERENCE")

# The grip (lofted round, on its slant, fuller in the palm and flared at the
# bottom) and the trigger guard.
part = Part()
sections = [(6.0, -17.0, 16.0, 12.0, 3.0), (-6.0, -18.0, 17.0, 13.0, 6.0), (-45.0, -18.5, 16.5, 13.5, 7.5),
            (-88.0, -17.5, 16.0, 13.0, 7.5), (-98.0, -19.0, 18.0, 14.0, 6.0)]
part.loft([[on_grip(x, u, v) for x, u in rounded_rect(-half, half, back, front, radius)]
           for v, back, front, half, radius in sections], POLYMER)
part.band([(well[0], -26.0), (well[0], -32.0), (well[0] - 8.0, -38.0), (well[0] - 20.0, -40.0), (104.0, -40.0),
           (98.0, -33.0)], 4.5, -6.0, 6.0, POLYMER)
part.to_object("Grip")

part = Part()
# Down from its pin, its tip curling forward towards the muzzle.
part.band([(120.0, -7.0), (119.3, -13.0), (120.1, -19.0), (122.0, -24.0), (124.8, -27.0)], 4.5, -3.0, 3.0, STEEL)
part.to_object("Trigger", origin=(0.0, 120.0, -7.0))

# The fire selector on the left over the grip, turning about its hub.
part = Part()
part.cylinder("x", 5.0, (82.0, 2.0), -LOWER_HALF_WIDTH - 1.5, -LOWER_HALF_WIDTH + 0.5, STEEL, segments=14)
part.prism("x", [(82.0, 0.5), (82.0, 3.5), (66.0, 6.5), (64.0, 4.0)], -LOWER_HALF_WIDTH - 2.5,
           -LOWER_HALF_WIDTH - 1.0, STEEL)
part.to_object("Selector", origin=(-LOWER_HALF_WIDTH, 82.0, 2.0))

# The bolt catch, a paddle on the left over the well, turning on its rear.
part = Part()
b = well[0] + 2.0
part.prism("x", [(b, 0.0), (b + 18.0, 2.0), (b + 18.0, 8.0), (b + 2.0, 6.0)], -LOWER_HALF_WIDTH - 1.5,
           -LOWER_HALF_WIDTH + 0.5, STEEL)
part.to_object("BoltCatch", origin=(-LOWER_HALF_WIDTH, b + 1.0, 3.0))

# The magazine release: a button on the left behind the well.
part = Part()
part.box(-LOWER_HALF_WIDTH - 2.0, -LOWER_HALF_WIDTH + 0.5, well[0] - 12.0, well[0] - 4.0, -16.0, -8.0, STEEL)
part.to_object("MagRelease", origin=(-LOWER_HALF_WIDTH, well[0] - 8.0, -12.0))

# The magazine, built along its own axes (down it is -Z, forward +Y) and
# turned onto its rake: a straight body with a raised panel on each side
# (in the well until it drops), the feed lips with the top round in them,
# and the base plate.
mw = MAGAZINE_HALF_WIDTH
part = Part()
part.prism("z", rounded_rect(-mw, mw, -d, d, 3.5, per_corner=3), -MAGAZINE_LENGTH, -4.0, MAG)
for side in (-1.0, 1.0):
    part.prism("x", rounded_rect(-d + 9.0, d - 9.0, -MAGAZINE_LENGTH + 10.0, -14.0, 4.0),
               *sorted((side * (mw - 0.5), side * (mw + 0.8))), MAG)
part.prism("z", rounded_rect(-9.0, 9.0, -d + 2.0, d - 4.0, 2.0, per_corner=3), -4.0, 0.0, STEEL)
part.cylinder("y", 6.0, (0.0, -0.5), -d + 3.0, 16.0, BRASS)
part.prism("y", circle((0.0, -0.5), 3.9, 16), 16.0, 28.0, COPPER)
part.prism("y", circle((0.0, -0.5), 2.0, 12), 28.0, 34.0, COPPER)
part.prism("z", rounded_rect(-mw - 1.5, mw + 1.5, -d - 2.0, d + 3.0, 3.5, per_corner=3), -MAGAZINE_LENGTH - 7.0,
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
print(f"dmr: {size.x:.1f} wide, {size.z:.1f} high, {size.y:.1f} long (game mm, x{SCALE:.3f} of true size)")
assert abs(size.y - LENGTH) < 0.05, f"the DMR is {size.y:.2f} mm long, not {LENGTH}"

bpy.ops.wm.save_as_mainfile(filepath=f"{ROOT}/art/dmr.blend")
bpy.ops.export_scene.gltf(
    filepath=f"{ROOT}/assets/models/weapons/dmr.glb", export_format="GLB", export_yup=True, export_apply=True
)
