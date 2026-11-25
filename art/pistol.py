"""Builds the pistol: an original compact striker-fired pistol, a dark slide
on a coyote polymer frame, with three-dot iron sights (two white dots on the
rear, an orange one on the front post) and a mount on the slide for the reflex
and the 2x sight.

Built for the game, so it is placed in the hand as it is, with no turning or
scaling in code:
- Metres. X is right, Y forward along the barrel, Z up. It is exported Y-up,
  so in the file the barrel points along -Z and up is +Y, as the game holds it.
- Its length is the pistol's `size` in src/weapons.rs (0.15 m), and its origin
  is the middle of its bounding box (where the box gun's middle would be).

The nodes the game looks for (see src/gun_model.rs):
- Slide: kicked back by each shot and held back when empty. It carries the
  iron sights, the sight mount and the aim point, so they ride with it.
- IronSightRear, IronSightFront: hidden when a sight is mounted.
- AimPoint: on the line over the top of the rear blade and the front post,
  which aiming puts on the crosshair.
- SightMount: where a sight's base goes. The sights are built facing -Y, so
  the mount turns them round.
- Magazine: its origin is the top of the magazine, on the grip's slant.
- Trigger, SlideStop and MagRelease pivot or slide about their origins for
  animations to come.

Saves art/pistol.blend and exports assets/models/weapons/pistol.glb. Run with:
blender --background --factory-startup --python pistol.py -- <repo root>
"""
import math
import sys

import bmesh
import bpy
from mathutils import Vector

ROOT = sys.argv[sys.argv.index("--") + 1]

# Laid out in millimetres, the slide's rear bottom edge at y = 0, z = 0. Every
# vertex is turned into metres as it is made, and the whole gun is centred on
# its origin at the end.
MM = 0.001
# The pistol's length in src/weapons.rs.
LENGTH = 150.0
# The tang of the frame over the web of the hand is the rearmost point.
TANG_Y = -6.5
SLIDE_FRONT = LENGTH + TANG_Y
SLIDE_HALF_WIDTH = 12.5
SLIDE_TOP = 27.0
BORE_Z = 13.5
# The top of the rear blade and of the front post: the line aimed along.
SIGHT_LINE_Z = 33.0
REAR_SIGHT_Y = (2.0, 10.0)
FRONT_SIGHT_Y = (SLIDE_FRONT - 9.5, SLIDE_FRONT - 3.5)
# Wide enough to see the front post with a gap of light either side.
NOTCH_HALF_WIDTH = 2.4
POST_HALF_WIDTH = 1.8
# The grip slants back by this much; the magazine follows it.
GRIP_ANGLE = math.radians(17.0)
MAGAZINE_TOP = (37.0, -3.0)
# How far forward of the rear of the slide a sight's base is centred.
SIGHT_MOUNT_Y = 20.0


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
FRAME = material("Pistol_Frame", (0.13, 0.095, 0.058), roughness=0.75)
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


def empty(name, at, parent, rotation=(0.0, 0.0, 0.0)):
    obj = bpy.data.objects.new(name, None)
    obj.empty_display_size = 0.006
    COLLECTION.objects.link(obj)
    place(obj, Vector(at) * MM, parent, rotation)
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


# The slide: its side profile and its cross-section, which has its top edges
# bevelled, cut out of each other; then serrations front and rear, the
# ejection port on the right, and the hole the muzzle shows through.
part = Part()
part.prism("x", [(0.0, 0.0), (SLIDE_FRONT - 7.0, 0.0), (SLIDE_FRONT, 6.0), (SLIDE_FRONT, 23.0),
                 (SLIDE_FRONT - 3.0, SLIDE_TOP), (1.5, SLIDE_TOP), (0.0, SLIDE_TOP - 1.5)], -14.0, 14.0, SLIDE)
slide = part.to_object("Slide")
part = Part()
w = SLIDE_HALF_WIDTH
part.prism("y", [(-w, 0.0), (w, 0.0), (w, 20.0), (9.0, SLIDE_TOP), (-9.0, SLIDE_TOP), (-w, 20.0)],
           -1.0, SLIDE_FRONT + 1.0, SLIDE)
boolean(slide, part.to_object("SlideSection"), "INTERSECT")
part = Part()
for side in (-1.0, 1.0):
    for y0, count in ((5.0, 7), (SLIDE_FRONT - 32.0, 5)):
        for i in range(count):
            y = y0 + i * 3.0
            part.box(*sorted((side * 11.4, side * 14.0)), y, y + 1.3, 3.0, 26.0, SLIDE)
part.box(-1.0, 14.0, 68.0, 96.0, 15.0, 30.0, SLIDE)
part.cylinder("y", 7.2, (0.0, BORE_Z), SLIDE_FRONT - 10.0, SLIDE_FRONT + 2.0, SLIDE)
boolean(slide, part.to_object("SlideCuts"), "DIFFERENCE")

# The rear sight: a wide blade with a square notch, a white dot either side;
# the front: a narrow post with an orange dot. Their tops are the sight line.
part = Part()
y0, y1 = REAR_SIGHT_Y
n = NOTCH_HALF_WIDTH
part.prism("y", [(-10.0, SLIDE_TOP - 0.5), (10.0, SLIDE_TOP - 0.5), (10.0, SIGHT_LINE_Z), (n, SIGHT_LINE_Z),
                 (n, SIGHT_LINE_Z - 3.0), (-n, SIGHT_LINE_Z - 3.0), (-n, SIGHT_LINE_Z), (-10.0, SIGHT_LINE_Z)],
           y0, y1, SLIDE)
for x in (-5.6, 5.6):
    part.cylinder("y", 0.95, (x, SIGHT_LINE_Z - 1.9), y0 - 0.15, y0 + 0.2, REAR_DOT, segments=12)
part.to_object("IronSightRear", origin=(0.0, y0, SLIDE_TOP), parent=slide)
part = Part()
y0, y1 = FRONT_SIGHT_Y
part.prism("x", [(y0, SLIDE_TOP - 0.5), (y1, SLIDE_TOP - 0.5), (y1, SIGHT_LINE_Z - 3.0), (y1 - 2.0, SIGHT_LINE_Z),
                 (y0, SIGHT_LINE_Z)], -POST_HALF_WIDTH, POST_HALF_WIDTH, SLIDE)
part.cylinder("y", 1.05, (0.0, SIGHT_LINE_Z - 1.8), y0 - 0.15, y0 + 0.2, FRONT_DOT, segments=12)
part.to_object("IronSightFront", origin=(0.0, y0, SLIDE_TOP), parent=slide)
empty("AimPoint", (0.0, REAR_SIGHT_Y[0], SIGHT_LINE_Z), slide)
empty("SightMount", (0.0, SIGHT_MOUNT_Y, SLIDE_TOP), slide, rotation=(0.0, 0.0, math.pi))
# A plate over the striker at the back of the slide.
part = Part()
part.box(-6.5, 6.5, -0.5, 0.3, 6.0, 20.0, STEEL)
part.to_object("StrikerPlate", parent=slide)

# The barrel: its chamber shows through the ejection port, its muzzle through
# the front of the slide, and more of it when the slide is back.
part = Part()
part.box(-6.8, 6.8, 58.0, 100.0, 7.0, 25.5, STEEL)
part.tube(6.5, 4.6, 98.0, SLIDE_FRONT - 0.3, 20.0, BORE_Z, STEEL)
part.to_object("Barrel")

# The frame: the rails under the slide, the dust cover with an accessory rail,
# the trigger guard, the tang, and the grip (lofted round, on its slant, a
# little fuller in the palm and flared at the magazine well).
part = Part()
part.prism("x", [(-4.0, 0.0), (SLIDE_FRONT - 1.0, 0.0), (SLIDE_FRONT - 1.0, -10.0), (SLIDE_FRONT - 5.0, -14.0),
                 (97.0, -14.0), (94.0, -9.0), (20.0, -9.0), (10.0, -10.0), (2.0, -8.5), (-4.5, -5.0),
                 (TANG_Y, -2.5)], -11.0, 11.0, FRAME)
part.box(-5.0, 5.0, 102.0, 138.0, -16.0, -13.5, FRAME)
for i in range(5):
    y = 102.0 + i * 8.0
    part.box(-8.0, 8.0, y, y + 4.0, -17.5, -13.5, FRAME)
part.band([(95.0, -8.0), (95.5, -26.0), (93.0, -31.0), (88.0, -33.0), (60.0, -33.0), (50.0, -30.0)], 4.5,
          -5.5, 5.5, FRAME)
sections = [(2.0, -21.0, 21.0, 10.5, 3.0), (-8.0, -21.5, 21.0, 13.5, 7.0), (-35.0, -22.0, 21.0, 14.2, 8.0),
            (-66.0, -21.5, 20.5, 13.6, 7.5), (-73.0, -23.0, 22.0, 14.6, 7.0)]
part.loft([[on_grip(x, u, v) for x, u in rounded_rect(-half, half, back, front, radius)]
           for v, back, front, half, radius in sections], FRAME)
# The takedown pin, through the frame over the trigger.
part.cylinder("x", 1.6, (86.0, -4.5), -11.4, 11.4, STEEL, segments=12)
frame = part.to_object("Frame")

part = Part()
part.band([(72.0, -8.0), (72.6, -13.0), (71.8, -18.0), (70.0, -22.0), (67.5, -24.5)], 4.2, -3.0, 3.0, STEEL)
part.to_object("Trigger", origin=(0.0, 72.0, -9.0))

# The slide stop, on the left over the trigger, turning on its front pin.
part = Part()
part.prism("x", [(91.0, -6.5), (91.0, -2.5), (66.0, -2.0), (59.0, -1.0), (57.0, -3.0), (59.0, -7.0), (66.0, -6.5)],
           -12.6, -10.8, STEEL)
part.cylinder("x", 2.0, (88.0, -4.5), -13.0, -12.4, STEEL, segments=12)
part.to_object("SlideStop", origin=(-11.0, 88.0, -4.5))

part = Part()
part.box(-14.6, -12.0, 47.0, 54.0, -14.0, -9.5, STEEL)
part.to_object("MagRelease", origin=(-12.0, 50.5, -11.75))

# The magazine, built along its own axes (down the grip is -Z) and turned onto
# the grip's slant: the body (hidden in the grip until it drops), the feed
# lips with the top round in them, and the base plate under the grip.
part = Part()
part.prism("z", rounded_rect(-10.5, 10.5, -15.0, 15.5, 2.5, per_corner=3), -72.5, -4.0, STEEL)
part.prism("z", rounded_rect(-7.0, 7.0, -15.0, 12.0, 2.0, per_corner=3), -4.0, 0.0, STEEL)
part.cylinder("y", 4.7, (0.0, -1.0), -14.0, 4.0, BRASS)
part.prism("y", circle((0.0, -1.0), 4.5, 16), 4.0, 8.0, COPPER)
part.prism("y", circle((0.0, -1.0), 3.0, 16), 8.0, 11.0, COPPER)
part.prism("z", rounded_rect(-12.0, 12.0, -16.5, 17.5, 3.0, per_corner=3), -77.5, -72.5, FRAME)
part.box(-9.0, 9.0, 15.0, 19.0, -77.5, -74.0, FRAME)
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
size = (high - low) / MM
print(f"pistol: {size.x:.1f} wide, {size.z:.1f} high, {size.y:.1f} long (mm)")
assert abs(size.y - LENGTH) < 0.05, f"the pistol is {size.y:.2f} mm long, not {LENGTH}"

bpy.ops.wm.save_as_mainfile(filepath=f"{ROOT}/art/pistol.blend")
bpy.ops.export_scene.gltf(
    filepath=f"{ROOT}/assets/models/weapons/pistol.glb", export_format="GLB", export_yup=True, export_apply=True
)
