"""Builds the sniper: an original bolt-action rifle. A black round receiver
and heavy barrel in a dark aluminium chassis, a slotted handguard under one
full-length top rail, a folded bipod, a ported muzzle brake, a five-round
box magazine ahead of the trigger and a solid stock with an adjustable cheek
riser. Backup irons on the rail (a ring at the back, a post with an orange
dot between guard ears at the front), and a mount on the rail for the red
dot, reflex, 2x and scope.

Built for the game, so it is placed in the hand as it is, with no turning or
scaling in code:
- Metres. X is right, Y forward along the barrel, Z up. It is exported Y-up,
  so in the file the barrel points along -Z and up is +Y, as the game holds it.
- Its length is the sniper's `size` in src/weapons.rs (0.44 m), and its
  origin is the middle of its bounding box.
- It is designed at true size (`TRUE_LENGTH`) and shrunk by `SCALE` to that
  length, and its `SightMount` is scaled by `SCALE` too, so a sight (built
  in true metres) keeps its true size next to the gun (see PLAN.md, Round 4).
- The sights sit well forward on the rail, so aimed, the eye is over the
  back of the receiver and the stock is behind it, out of the view.

The nodes the game looks for (see src/gun_model.rs):
- Bolt: its origin is on the bore. After each shot (and to cock the gun at
  the end of a reload) it is turned about the bore to lift the handle on the
  right, drawn back, pushed home and turned down to lock (src/player.rs).
  The bolt body shows through the ejection port, the shroud at its back is
  round so turning it does not show, and the handle is its child.
- IronSightRear, IronSightFront: hidden when a sight is mounted.
- AimPoint: the middle of the rear aperture, on the line over the front
  post, which aiming puts on the crosshair.
- SightMount: where a sight's base goes, on the rail. The sights are built
  facing -Y, so the mount turns them round.
- Magazine: its origin is the top of the magazine.
- Trigger, Safety, BoltRelease, MagRelease and Bipod for animations to come.

Saves art/sniper.blend and exports assets/models/weapons/sniper.glb. Run with:
blender --background --factory-startup --python sniper.py -- <repo root>
"""
import math
import sys

import bmesh
import bpy
from mathutils import Vector

ROOT = sys.argv[sys.argv.index("--") + 1]

# The sniper's length in src/weapons.rs, in millimetres: how long the game holds it.
LENGTH = 440.0
# How long it is at true size, which it is laid out at; everything is shrunk
# by `SCALE` to `LENGTH`, sights on its mount included.
TRUE_LENGTH = 1200.0
SCALE = LENGTH / TRUE_LENGTH
# Laid out in true millimetres: y = 0 at the rear of the receiver, z = 0 on
# the bore. Every vertex and position is turned into game metres with `MM`
# as it is made, and the whole gun is centred on its origin at the end.
MM = 0.001 * SCALE
# The butt pad is the rearmost point, the muzzle brake the front.
REAR_Y = -300.0
MUZZLE_Y = REAR_Y + TRUE_LENGTH
BORE_Z = 0.0
RECEIVER_RADIUS = 17.0
RECEIVER_FRONT = 215.0
# The chassis the receiver sits in, and the handguard in front of it.
CHASSIS_HALF_WIDTH = 19.0
CHASSIS_TOP = -2.0
CHASSIS_BOTTOM = -38.0
CHASSIS_REAR = -45.0
CHASSIS_FRONT = 227.0
HANDGUARD_FRONT = 615.0
RAIL_BOTTOM = 15.0
RAIL_TOP = 26.0
# The bolt's face, closed; how far it is drawn back (BOLT_ACTION_TRAVEL in
# src/player.rs times the length); where its handle comes out, how far the
# handle hangs down when locked, and how far it is lifted to unlock it.
BOLT_FACE_Y = 200.0
BOLT_TRAVEL = 0.085 * TRUE_LENGTH
HANDLE_Y = 16.0
HANDLE_DOWN = math.radians(35.0)
HANDLE_LIFTED = math.radians(63.0) - HANDLE_DOWN
# The line aimed along: the middle of the rear aperture and the top of the
# post. The sights are forward on the rail (see above).
SIGHT_LINE_Z = RAIL_TOP + 26.0
REAR_SIGHT_Y = 150.0
FRONT_SIGHT_Y = 585.0
FRONT_POST_Y = (FRONT_SIGHT_Y + 9.0, FRONT_SIGHT_Y + 14.0)
# Sized by how they look from the eye (PLAN.md): the aperture about 2.4
# degrees across, the post about 0.8 degrees wide, the guard ears hidden
# behind the ring.
APERTURE_RADIUS = 4.6
RING_RADIUS = 6.0
POST_HALF_WIDTH = 4.7
EARS_X = (14.5, 16.5)
# Where a sight's base is centred, on the rail.
SIGHT_MOUNT_Y = 180.0
# The grip slants back by this much from the top of its front edge.
GRIP_ANGLE = math.radians(18.0)
GRIP_TOP = (22.0, CHASSIS_BOTTOM)
# The magazine: the middle of its top, how long it is and half its depth.
MAGAZINE_TOP = (132.0, -20.0)
MAGAZINE_LENGTH = 62.0
MAGAZINE_HALF_DEPTH = 38.0
# The stock, from the back of the chassis to the butt spacers: its comb is
# low enough for the bolt to go back over it.
STOCK_FRONT = CHASSIS_REAR
STOCK_BACK = REAR_Y + 20.0
COMB_Z = -15.0
STOCK_HALF_WIDTH = 16.0


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
RECEIVER = material("Sniper_Receiver", (0.028, 0.030, 0.033), roughness=0.38)
POLYMER = material("Sniper_Polymer", (0.05, 0.051, 0.055), roughness=0.8)
MAG = material("Sniper_Magazine", (0.04, 0.041, 0.044), roughness=0.7)
RUBBER = material("Sniper_Rubber", (0.018, 0.018, 0.019), roughness=0.95)
STEEL = material("Sniper_Steel", (0.075, 0.076, 0.08), roughness=0.32)
# The chassis, hard anodised, a shade off the receiver; the bolt, bare polished steel.
CHASSIS = material("Sniper_Chassis", (0.036, 0.038, 0.042), roughness=0.55)
BOLT = material("Sniper_Bolt", (0.16, 0.165, 0.17), roughness=0.25)
BRASS = material("Sniper_Brass", (0.55, 0.36, 0.09), roughness=0.35)
COPPER = material("Sniper_Copper", (0.45, 0.16, 0.06), roughness=0.35)
FRONT_DOT = material("Sniper_FrontDot", (1.0, 0.32, 0.02), roughness=0.4, emission=(1.0, 0.32, 0.02), strength=1.5)

COLLECTION = bpy.data.collections.new("Sniper")
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


# The receiver: a round tube, hollow for the bolt behind the chamber, the
# ejection port on the right, and at its back on the right the notch the
# handle turns down into and slides back out of.
part = Part()
part.cylinder("y", RECEIVER_RADIUS, (0.0, BORE_Z), 0.0, RECEIVER_FRONT, RECEIVER, segments=32)
receiver = part.to_object("Receiver")
part = Part()
part.cylinder("y", 10.0, (0.0, BORE_Z), -1.0, RECEIVER_FRONT - 12.0, RECEIVER, segments=24)
part.box(0.0, RECEIVER_RADIUS + 2.0, 62.0, 150.0, -2.0, 12.0, RECEIVER)
sector = [(0.0, BORE_Z)] + [(30.0 * math.cos(math.radians(a)), BORE_Z + 30.0 * math.sin(math.radians(a)))
                            for a in range(-52, 47, 6)]
part.prism("y", sector, -1.0, HANDLE_Y + 9.0, RECEIVER)
boolean(receiver, part.to_object("ReceiverCuts"), "DIFFERENCE")

# The barrel, heavy and tapering, from the receiver to the muzzle brake, and
# the nut that holds it, showing between the receiver and the handguard.
part = Part()
part.loft([[(x, y, z) for x, z in circle((0.0, BORE_Z), r, 24)]
           for y, r in ((RECEIVER_FRONT - 4.0, 14.5), (300.0, 13.5), (340.0, 12.5), (MUZZLE_Y - 78.0, 11.0))],
          RECEIVER)
part.cylinder("y", 19.5, (0.0, BORE_Z), RECEIVER_FRONT, CHASSIS_FRONT + 1.0, STEEL, segments=28)
part.to_object("Barrel")

# The muzzle brake: three ports through each side and two on top.
part = Part()
part.tube(14.0, 4.5, MUZZLE_Y - 80.0, MUZZLE_Y, 70.0, BORE_Z, STEEL, segments=24)
brake = part.to_object("MuzzleBrake")
part = Part()
for y0 in (MUZZLE_Y - 64.0, MUZZLE_Y - 44.0, MUZZLE_Y - 24.0):
    part.prism("x", rounded_rect(y0, y0 + 12.0, BORE_Z - 7.0, BORE_Z + 7.0, 2.5), -20.0, 20.0, STEEL)
for y in (MUZZLE_Y - 54.0, MUZZLE_Y - 34.0):
    for x in (-5.0, 5.0):
        part.cylinder("z", 2.4, (x, y), BORE_Z, BORE_Z + 20.0, STEEL, segments=12)
boolean(brake, part.to_object("MuzzleBrakePorts"), "DIFFERENCE")

# The chassis: the receiver bedded in it, a channel behind it for the bolt's
# shroud, the magazine well flared at the bottom, a recessed panel on each
# side, and a notch on the right for the bolt handle.
part = Part()
outline = [(CHASSIS_REAR, COMB_Z), (-2.0, COMB_Z), (-2.0, CHASSIS_TOP), (CHASSIS_FRONT, CHASSIS_TOP),
           (CHASSIS_FRONT, -26.0), (204.0, CHASSIS_BOTTOM), (184.0, CHASSIS_BOTTOM), (180.0, -50.0), (86.0, -50.0),
           (82.0, CHASSIS_BOTTOM), (CHASSIS_REAR + 6.0, CHASSIS_BOTTOM), (CHASSIS_REAR, CHASSIS_BOTTOM + 6.0)]
part.prism("x", rounded_polygon(outline, 3.0, per_corner=3), -CHASSIS_HALF_WIDTH, CHASSIS_HALF_WIDTH, CHASSIS)
chassis = part.to_object("Chassis")
part = Part()
part.box(-12.8, 12.8, MAGAZINE_TOP[0] - MAGAZINE_HALF_DEPTH - 1.0, MAGAZINE_TOP[0] + MAGAZINE_HALF_DEPTH + 1.0, -60.0,
         -16.0, CHASSIS)
part.box(9.0, CHASSIS_HALF_WIDTH + 2.0, HANDLE_Y - 13.0, HANDLE_Y + 12.0, -24.0, CHASSIS_TOP + 1.0, CHASSIS)
for side in (-1.0, 1.0):
    part.prism("x", rounded_rect(32.0, 76.0, -32.0, -10.0, 5.0), *sorted((side * 17.0, side * 21.0)), CHASSIS)
    part.prism("x", rounded_rect(188.0, 218.0, -22.0, -8.0, 4.0), *sorted((side * 17.0, side * 21.0)), CHASSIS)
boolean(chassis, part.to_object("ChassisCuts"), "DIFFERENCE")

# The handguard: octagonal round the barrel, rows of rounded slots along each
# side and the bottom, a sling socket on the left near its front.
part = Part()
octagon = [(-12.0, -30.0), (12.0, -30.0), (25.0, -17.0), (25.0, 6.0), (14.0, 17.0), (-14.0, 17.0), (-25.0, 6.0),
           (-25.0, -17.0)]
part.prism("y", octagon, CHASSIS_FRONT - 1.0, HANDGUARD_FRONT, CHASSIS)
handguard = part.to_object("Handguard")
part = Part()
for i in range(7):
    y0 = 250.0 + 48.0 * i
    for side in (-1.0, 1.0):
        part.prism("x", rounded_rect(y0, y0 + 32.0, -11.0, 1.0, 5.0), *sorted((side * 22.0, side * 27.0)), CHASSIS)
    part.prism("z", rounded_rect(-4.0, 4.0, y0 + 4.0, y0 + 28.0, 3.5), -33.0, -26.0, CHASSIS)
boolean(handguard, part.to_object("HandguardSlots"), "DIFFERENCE")
part = Part()
part.cylinder("x", 5.5, (HANDGUARD_FRONT - 22.0, -6.0), -27.0, -24.0, STEEL, segments=16)
part.cylinder("x", 3.0, (HANDGUARD_FRONT - 22.0, -6.0), -27.4, -26.8, RUBBER, segments=12)
part.to_object("SlingSocket")

# The rail along the top of both: a core with cross ridges, the sights'
# clamps' width.
part = Part()
part.box(-7.0, 7.0, -2.0, HANDGUARD_FRONT - 2.0, RAIL_BOTTOM, RAIL_TOP - 1.0, RECEIVER)
top = RAIL_TOP - 7.0
ridge = [(-8.0, top - 0.5), (8.0, top - 0.5), (8.0, top + 2.0), (10.5, top + 4.5), (10.5, top + 5.5), (9.0, RAIL_TOP),
         (-9.0, RAIL_TOP), (-10.5, top + 5.5), (-10.5, top + 4.5), (-8.0, top + 2.0)]
y = -2.0
while y + 5.2 <= HANDGUARD_FRONT - 2.0:
    part.prism("y", ridge, y, y + 5.2, RECEIVER)
    y += 10.0
part.to_object("Rail")

# The bolt: its body (bare steel, fluted) inside the receiver, the extractor
# at its face, and the shroud at its back, round so that turning the bolt
# does not show, with an orange pin in its end that says it is cocked.
part = Part()
part.cylinder("y", 9.5, (0.0, BORE_Z), -2.0, BOLT_FACE_Y, BOLT, segments=24)
for k in range(6):
    a = math.radians(30.0 + 60.0 * k)
    c, s = math.cos(a), math.sin(a)
    flute = [(c * r + s * w, BORE_Z + s * r - c * w) for r, w in ((8.8, -1.5), (8.8, 1.5), (9.8, 1.5), (9.8, -1.5))]
    part.prism("y", flute, 40.0, 175.0, RECEIVER)
part.box(9.0, 10.2, BOLT_FACE_Y - 14.0, BOLT_FACE_Y - 1.0, BORE_Z - 2.5, BORE_Z + 2.5, BOLT)
part.loft([[(x, y, z) for x, z in circle((0.0, BORE_Z), r, 24)]
           for y, r in ((-1.0, 11.0), (-6.0, 12.5), (-30.0, 12.5), (-38.0, 10.5), (-41.0, 8.0))], RECEIVER)
part.cylinder("y", 3.0, (0.0, BORE_Z), -41.6, -40.0, FRONT_DOT, segments=12)
bolt = part.to_object("Bolt", origin=(0.0, 0.0, BORE_Z))

# The handle, built straight out to the right and turned down onto the
# right of the chassis: a collar round the bolt, an arm swept back, and a big
# black knob with grooves round it.
part = Part()
part.cylinder("y", 10.4, (0.0, BORE_Z), HANDLE_Y - 7.0, HANDLE_Y + 7.0, BOLT, segments=24)
sweep = lambda x: HANDLE_Y - 0.22 * max(x - 8.0, 0.0)
part.loft([[(x, sweep(x) + r * math.cos(t), BORE_Z + r * math.sin(t)) for t in (i * math.tau / 14 for i in range(14))]
           for x, r in ((6.0, 4.8), (20.0, 4.2), (36.0, 3.8), (43.0, 3.8))], BOLT)
knob_y = sweep(43.0)
part.loft([[(x, knob_y + r * math.cos(t), BORE_Z + r * math.sin(t)) for t in (i * math.tau / 20 for i in range(20))]
           for x, r in ((41.0, 4.0), (43.0, 8.0), (46.0, 9.5), (56.0, 9.5), (59.0, 8.0), (60.5, 4.5))], RECEIVER)
for x in (48.0, 51.0, 54.0):
    part.cylinder("x", 9.9, (knob_y, BORE_Z), x, x + 1.2, RUBBER, segments=20)
part.to_object("BoltHandle", origin=(0.0, HANDLE_Y, BORE_Z), parent=bolt, rotation=(0.0, HANDLE_DOWN, 0.0))

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
part.cylinder("y", 2.1, (0.0, SIGHT_LINE_Z - 2.6), y0 - 0.15, y0 + 0.2, FRONT_DOT, segments=12)
for side in (-1.0, 1.0):
    part.prism("x", [(f + 2.0, RAIL_TOP + 3.5), (f + 20.0, RAIL_TOP + 3.5), (f + 20.0, SIGHT_LINE_Z - 2.0),
                     (f + 15.0, SIGHT_LINE_Z + 2.5), (f + 7.0, SIGHT_LINE_Z + 2.5), (f + 2.0, SIGHT_LINE_Z - 1.0)],
               *sorted((side * EARS_X[0], side * EARS_X[1])), RECEIVER)
part.to_object("IronSightFront", origin=(0.0, f, RAIL_TOP))
empty("AimPoint", (0.0, REAR_SIGHT_Y, SIGHT_LINE_Z), None)
# Turned round, as the sights face -Y, and shrunk with the gun.
empty("SightMount", (0.0, SIGHT_MOUNT_Y, RAIL_TOP), None, rotation=(0.0, 0.0, math.pi), scale=SCALE)

# The grip (lofted round, on its slant, fuller in the palm and flared at the
# bottom) and the trigger guard.
part = Part()
sections = [(6.0, -17.0, 16.0, 12.0, 3.0), (-6.0, -18.0, 17.0, 13.0, 6.0), (-45.0, -18.5, 16.5, 13.5, 7.5),
            (-88.0, -17.5, 16.0, 13.0, 7.5), (-98.0, -19.0, 18.0, 14.0, 6.0)]
part.loft([[on_grip(x, u, v) for x, u in rounded_rect(-half, half, back, front, radius)]
           for v, back, front, half, radius in sections], CHASSIS)
part.band([(66.0, CHASSIS_BOTTOM + 1.0), (66.0, -64.0), (60.0, -70.0), (50.0, -72.0), (34.0, -72.0), (28.0, -65.0)],
          4.5, -6.0, 6.0, CHASSIS)
part.to_object("Grip")

part = Part()
# Down from its pin, its tip curling forward towards the muzzle.
part.band([(50.0, -39.0), (49.3, -45.0), (50.1, -51.0), (52.0, -56.0), (54.8, -59.0)], 4.5, -3.0, 3.0, STEEL)
part.to_object("Trigger", origin=(0.0, 50.0, -39.0))

# The safety, a lever on the right behind the handle; the bolt release, on
# the left at the back of the receiver; the magazine release, a paddle in
# front of the trigger guard.
part = Part()
part.box(CHASSIS_HALF_WIDTH - 2.0, CHASSIS_HALF_WIDTH + 1.0, -24.0, -8.0, COMB_Z - 1.0, COMB_Z + 3.0, STEEL)
part.box(CHASSIS_HALF_WIDTH - 2.0, CHASSIS_HALF_WIDTH + 1.0, -14.0, -9.0, COMB_Z + 2.0, COMB_Z + 7.0, STEEL)
part.to_object("Safety", origin=(CHASSIS_HALF_WIDTH, -16.0, COMB_Z))
part = Part()
part.prism("x", [(4.0, 2.0), (22.0, 4.0), (22.0, 9.0), (6.0, 7.0)], -RECEIVER_RADIUS - 2.5, -RECEIVER_RADIUS + 2.0,
           STEEL)
part.to_object("BoltRelease", origin=(-RECEIVER_RADIUS, 5.0, 4.5))
part = Part()
part.prism("x", [(72.0, CHASSIS_BOTTOM + 1.0), (78.0, CHASSIS_BOTTOM + 1.0), (80.0, -51.0), (74.0, -52.0)], -6.0,
           6.0, STEEL)
part.to_object("MagRelease", origin=(0.0, 75.0, CHASSIS_BOTTOM))

# The magazine: a straight box (in the well until it drops), the feed lips
# with the top round in them, and the base plate.
d = MAGAZINE_HALF_DEPTH
part = Part()
part.prism("z", rounded_rect(-12.0, 12.0, -d, d, 3.0, per_corner=3), -MAGAZINE_LENGTH, -4.0, MAG)
part.prism("z", rounded_rect(-8.5, 8.5, -d + 2.0, d - 4.0, 2.0, per_corner=3), -4.0, 0.0, STEEL)
part.cylinder("y", 6.0, (0.0, -0.5), -d + 3.0, 18.0, BRASS)
part.prism("y", circle((0.0, -0.5), 3.9, 16), 18.0, 30.0, COPPER)
part.prism("y", circle((0.0, -0.5), 2.0, 12), 30.0, 36.0, COPPER)
part.prism("z", rounded_rect(-13.5, 13.5, -d - 2.0, d + 3.0, 3.5, per_corner=3), -MAGAZINE_LENGTH - 6.0,
           -MAGAZINE_LENGTH, POLYMER)
for side in (-1.0, 1.0):
    part.prism("x", rounded_rect(-d + 10.0, d - 10.0, -MAGAZINE_LENGTH + 8.0, -14.0, 4.0),
               *sorted((side * 11.5, side * 12.4)), MAG)
part.to_object("Magazine", origin=(0.0, 0.0, 0.0))
bpy.data.objects["Magazine"].location = Vector((0.0, *MAGAZINE_TOP)) * MM

# The stock, from the back of the chassis to the butt: its comb straight and
# low enough for the bolt to go back over it, its underside sweeping down to
# the toe. On each side a sculpted pocket and a grooved panel, a sling socket
# near the butt; a ridged bag rider along the bottom; an adjustable cheek
# riser on two posts, with its knob on the left; spacers and a ribbed rubber
# butt pad.
BOTTOM = [(STOCK_FRONT, -40.0), (-62.0, -52.0), (-90.0, -60.0), (-140.0, -70.0), (-210.0, -92.0),
          (STOCK_BACK, -112.0)]


def stock_bottom(y):
    return along(y, BOTTOM)


def stock_section(y):
    """The stock across, at `y`: a little narrower along the comb."""
    top, bottom, half, comb = COMB_Z, stock_bottom(y), STOCK_HALF_WIDTH, 11.0
    right = [(half - 5.0, bottom), (half - 1.5, bottom + 1.5), (half, bottom + 5.0), (half, top - 9.0),
             (half - 1.5, top - 4.0), (comb + 1.0, top - 0.8), (comb - 2.0, top)]
    return [(x, y, z) for x, z in right + [(-x, z) for x, z in reversed(right)]]


stations = [STOCK_FRONT, -50.0, -56.0, -62.0, -70.0, -80.0, -90.0, -105.0, -120.0, -140.0, -165.0, -190.0, -210.0,
            -235.0, -260.0, STOCK_BACK]
part = Part()
part.loft([stock_section(y) for y in stations], CHASSIS)
stock = part.to_object("Stock")
part = Part()
h = STOCK_HALF_WIDTH
pocket = [(-96.0, stock_bottom(-96.0) + 10.0), (-96.0, COMB_Z - 12.0), (-158.0, COMB_Z - 12.0),
          (-158.0, stock_bottom(-158.0) + 10.0)]
for side in (-1.0, 1.0):
    part.prism("x", rounded_polygon(pocket, 8.0), *sorted((side * (h - 2.5), side * (h + 3.0))), CHASSIS)
    for i in range(5):
        z = COMB_Z - 16.0 - 6.0 * i
        part.prism("x", rounded_rect(-252.0, -184.0, z - 2.2, z, 1.0, per_corner=2),
                   *sorted((side * (h - 0.9), side * (h + 3.0))), CHASSIS)
boolean(stock, part.to_object("StockCuts"), "DIFFERENCE")

part = Part()
# Sling sockets near the butt.
for side in (-1.0, 1.0):
    y, z = -266.0, stock_bottom(-266.0) + 16.0
    part.cylinder("x", 5.0, (y, z), *sorted((side * (h - 1.0), side * (h + 1.2))), STEEL, segments=16)
    part.cylinder("x", 2.8, (y, z), *sorted((side * (h + 1.1), side * (h + 1.4))), RUBBER, segments=12)
# The bag rider: ridged blocks along the bottom.
y = -150.0
while y - 9.0 >= -262.0:
    b0, b1 = stock_bottom(y), stock_bottom(y - 9.0)
    part.prism("x", [(y, b0 + 1.0), (y - 9.0, b1 + 1.0), (y - 9.0, b1 - 4.0), (y, b0 - 4.0)], -9.0, 9.0, RUBBER)
    y -= 12.0
# The cheek riser on its posts, with a rubber pad on top and its knob on the left.
for y in (-182.0, -240.0):
    part.cylinder("z", 4.5, (0.0, y), COMB_Z - 1.0, COMB_Z + 6.0, STEEL, segments=14)
riser = [(-158.0, COMB_Z + 5.0), (-158.0, COMB_Z + 11.0), (-174.0, COMB_Z + 19.0), (-262.0, COMB_Z + 19.0),
         (-262.0, COMB_Z + 5.0)]
part.prism("x", rounded_polygon(riser, 4.0, per_corner=3), -13.0, 13.0, CHASSIS)
part.prism("y", rounded_rect(-11.0, 11.0, COMB_Z + 18.0, COMB_Z + 20.5, 1.5, per_corner=2), -258.0, -178.0, RUBBER)
for y in range(-250, -180, 8):
    part.prism("x", rounded_rect(y, y + 4.0, COMB_Z + 9.0, COMB_Z + 15.0, 1.5, per_corner=2), -13.6, 13.6, RECEIVER)
part.cylinder("x", 7.5, (-209.0, COMB_Z - 9.0), -h - 6.0, -h + 1.0, RUBBER, segments=18)
part.cylinder("x", 5.5, (-209.0, COMB_Z - 9.0), -h - 7.0, -h - 5.5, STEEL, segments=18)
part.to_object("StockFittings")

# The spacers that set the length, and the butt pad.
part = Part()
for i, mat in enumerate((RECEIVER, CHASSIS, RECEIVER)):
    part.prism("y", rounded_rect(-h - 0.5, h + 0.5, -114.0, COMB_Z + 1.0, 5.0), STOCK_BACK - 4.0 * (i + 1),
               STOCK_BACK - 4.0 * i + 0.5, mat)
part.to_object("ButtSpacers")
part = Part()
part.prism("y", rounded_rect(-h - 0.5, h + 0.5, -114.0, COMB_Z + 1.0, 5.0), REAR_Y, STOCK_BACK - 12.0 + 0.5, RUBBER)
pad = part.to_object("ButtPad")
part = Part()
for i in range(11):
    z = -108.0 + 8.5 * i
    part.box(-20.0, 20.0, REAR_Y - 1.0, REAR_Y + 1.6, z, z + 3.0, RUBBER)
boolean(pad, part.to_object("ButtPadGrooves"), "DIFFERENCE")

# The bipod, folded back under the handguard: its clamp and pivot, and two
# legs (an outer tube, the inner leg out of it, a rubber foot).
part = Part()
part.box(-11.0, 11.0, HANDGUARD_FRONT - 52.0, HANDGUARD_FRONT - 26.0, -38.0, -29.0, CHASSIS)
part.cylinder("x", 5.0, (HANDGUARD_FRONT - 40.0, -42.0), -13.0, 13.0, STEEL, segments=14)
for x in (-7.5, 7.5):
    part.cylinder("y", 5.0, (x, -43.0), 470.0, HANDGUARD_FRONT - 36.0, CHASSIS, segments=14)
    part.cylinder("y", 3.6, (x, -43.0), 380.0, 470.0, STEEL, segments=12)
    part.box(x - 4.5, x + 4.5, 368.0, 382.0, -49.0, -37.0, RUBBER)
part.to_object("Bipod", origin=(0.0, HANDGUARD_FRONT - 40.0, -42.0))


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
print(f"sniper: {size.x:.1f} wide, {size.z:.1f} high, {size.y:.1f} long (game mm, x{SCALE:.3f} of true size)")
assert abs(size.y - LENGTH) < 0.05, f"the sniper is {size.y:.2f} mm long, not {LENGTH}"

bpy.ops.wm.save_as_mainfile(filepath=f"{ROOT}/art/sniper.blend")
bpy.ops.export_scene.gltf(
    filepath=f"{ROOT}/assets/models/weapons/sniper.glb", export_format="GLB", export_yup=True, export_apply=True
)
