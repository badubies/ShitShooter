"""Builds Sniper Street, the sniper mode's map, with the ShitShooter map tools:
a street at night, long after everybody left. The runners start behind an
old roadblock at one end and run for the overpass at the other, where the
sniper stands and the finish is under it. Buildings line both sides; between
them the road is full of wrecked cars, a bus on its side, containers, blast
walls, the rubble of two fallen buildings and whatever else was left, to run
from one to the next behind.

The sniper looks down on the street, so the closer cover is to the overpass
the less ground it hides behind it: the far end is cars to crouch behind,
the near end trucks, stacked containers and tall blast walls.

Everything is made of convex pieces, a few to each thing (a car is a body,
a cabin, a roof, four wheels and two bumpers), joined into one object a
thing, so that in Blender a car is picked up and put somewhere else as a
car. Saves blender/sniper_street.blend and exports
"assets/maps/Sniper Street.json", the map as it ships.

Run with:
blender --background --factory-startup --python blender/sniper_street_map.py -- <repo root>
"""
import math
import os
import random
import sys

import bmesh
import bpy
from mathutils import Euler, Matrix, Vector

ROOT = sys.argv[sys.argv.index("--") + 1]
sys.path.insert(0, os.path.join(ROOT, "blender"))
import shitshooter_maps  # noqa: E402

shitshooter_maps.register()
scene = bpy.context.scene
for obj in list(scene.objects):
    bpy.data.objects.remove(obj)

rng = random.Random(11)
rad = math.radians
TAU = math.tau


# ---------------------------------------------------------------- materials


def lin(c):
    return tuple(x / 12.92 if x <= 0.04045 else ((x + 0.055) / 1.055) ** 2.4 for x in c)


COLORS = {}


def material(name, color, glow=0.0):
    """A colour (sRGB) under a name that names its texture (see the README).
    The game tells materials apart by their colour, so no two share one."""
    assert color not in COLORS, f"{name} has the colour of {COLORS[color]}"
    COLORS[color] = name
    mat = bpy.data.materials.new(name)
    bsdf = mat.node_tree.nodes["Principled BSDF"]
    bsdf.inputs["Base Color"].default_value = (*lin(color), 1.0)
    bsdf.inputs["Roughness"].default_value = 0.9
    if glow:
        bsdf.inputs["Emission Color"].default_value = (*lin(color), 1.0)
        bsdf.inputs["Emission Strength"].default_value = glow
    mat.diffuse_color = (*lin(color), 1.0)
    return mat


ROAD = material("Road asphalt", (0.15, 0.15, 0.16))
LINE = material("Road line paint", (0.50, 0.46, 0.30))
LINE_WHITE = material("Lane line paint", (0.49, 0.49, 0.47))
PAVEMENT = material("Pavement slab", (0.36, 0.36, 0.37))
GROUND = material("Lot ground", (0.21, 0.20, 0.18))

BRICK_RED = material("Wall brick red", (0.38, 0.21, 0.17))
BRICK_BROWN = material("Wall brick brown", (0.32, 0.25, 0.20))
WALL_GREY = material("Wall concrete grey", (0.41, 0.41, 0.42))
PLASTER_PALE = material("Wall plaster pale", (0.52, 0.49, 0.43))
PLASTER_GREEN = material("Wall plaster green", (0.34, 0.39, 0.35))
CINDER = material("Wall cinder", (0.34, 0.34, 0.36))
STONE = material("Wall stone", (0.40, 0.38, 0.34))
FAR = material("Far wall concrete", (0.17, 0.18, 0.21))
DARK = material("Window dark paint", (0.035, 0.04, 0.055))
LIT = material("Window lit", (1.0, 0.70, 0.36), glow=1.2)
LIT_COLD = material("Window lit cold", (0.62, 0.80, 1.0), glow=0.9)
AMBER = material("Signal amber", (1.0, 0.62, 0.10), glow=5.0)
BOARDS = material("Boarded up boards", (0.40, 0.31, 0.21))
SHUTTER = material("Shutter corrugated", (0.30, 0.32, 0.34))
DOOR = material("Door wood", (0.25, 0.18, 0.13))
AWNING_RED = material("Awning paint red", (0.40, 0.14, 0.13))
AWNING_GREEN = material("Awning paint green", (0.15, 0.30, 0.24))
FLOOR_SLAB = material("Floor slab", (0.44, 0.43, 0.41))
NEON = material("Neon sign", (1.0, 0.10, 0.07), glow=5.0)

GLASS = material("Glass dark paint", (0.06, 0.08, 0.10))
TYRE = material("Tyre black paint", (0.05, 0.05, 0.05))
TRIM = material("Trim dark metal", (0.11, 0.11, 0.12))
BURNT = material("Burnt rust", (0.27, 0.15, 0.09))
SOOT = material("Soot dark rust", (0.10, 0.08, 0.07))
PAINTS = [
    material("Car paint red", (0.44, 0.12, 0.10)),
    material("Car paint blue", (0.13, 0.21, 0.36)),
    material("Car paint white", (0.62, 0.62, 0.60)),
    material("Car paint yellow", (0.66, 0.52, 0.12)),
    material("Car paint green", (0.16, 0.28, 0.21)),
    material("Car paint black", (0.08, 0.08, 0.09)),
    material("Car paint silver", (0.46, 0.48, 0.50)),
]
RED, BLUE, WHITE, YELLOW, GREEN, BLACK, SILVER = PAINTS
BUS = material("Bus paint cream", (0.56, 0.51, 0.39))
BUS_STRIPE = material("Bus paint stripe", (0.22, 0.33, 0.50))
BOX_WHITE = material("Truck box corrugated", (0.55, 0.55, 0.53))
ARMY = material("Army paint olive", (0.21, 0.25, 0.16))
BOX_RED = material("Container corrugated red", (0.46, 0.16, 0.12))
BOX_BLUE = material("Container corrugated blue", (0.14, 0.26, 0.43))
BOX_GREEN = material("Container corrugated green", (0.18, 0.34, 0.26))
BOX_RUST = material("Container corrugated worn", (0.41, 0.26, 0.16))
FENCE = material("Fence corrugated", (0.33, 0.30, 0.27))

T_WALL = material("Blast wall concrete", (0.47, 0.47, 0.45))
BARRIER = material("Barrier concrete", (0.52, 0.51, 0.48))
BRIDGE = material("Overpass concrete", (0.44, 0.44, 0.43))
RUBBLE = material("Rubble concrete", (0.38, 0.37, 0.36))
RUBBLE_ROCK = material("Rubble rock", (0.30, 0.29, 0.28))
RUBBLE_BRICK = material("Rubble brick", (0.40, 0.23, 0.18))
HESCO = material("Earth bag plaster", (0.45, 0.40, 0.29))
DRUM = material("Drum metal blue", (0.15, 0.22, 0.31))
DRUM_RUST = material("Drum rust", (0.33, 0.19, 0.11))
FIRE = material("Fire glow", (1.0, 0.42, 0.08), glow=7.0)
POLE = material("Pole metal", (0.17, 0.18, 0.19))
LAMP = material("Lamp lit", (1.0, 0.82, 0.55), glow=6.0)
DUMPSTER = material("Dumpster metal green", (0.13, 0.26, 0.19))
CRATE = material("Crate wood", (0.43, 0.32, 0.20))
PLANKS = material("Planks boards", (0.48, 0.39, 0.25))
TARP = material("Tarp paint blue", (0.20, 0.29, 0.40))
PAPER = material("Litter paint", (0.50, 0.50, 0.46))
SHELTER = material("Shelter panels", (0.24, 0.27, 0.30))
SOFA = material("Sofa plaster brown", (0.33, 0.24, 0.20))
APPLIANCE = material("Fridge paint white", (0.66, 0.66, 0.63))
MATTRESS = material("Mattress plaster", (0.55, 0.53, 0.47))
CONE = material("Cone paint orange", (0.80, 0.33, 0.08))
HYDRANT = material("Hydrant paint red", (0.52, 0.12, 0.10))


def collection(name):
    made = bpy.data.collections.new(name)
    scene.collection.children.link(made)
    return made


STREET = collection("Street")
BUILDINGS = collection("Buildings")
WRECKS = collection("Wrecks")
COVER = collection("Cover")
DEBRIS = collection("Debris")
LIGHTS = collection("Lights")
MARKERS = collection("Markers and clips")


# ---------------------------------------------------------------- shapes


def hull_of(points):
    """The convex hull of points: its corners and faces."""
    bm = bmesh.new()
    for p in points:
        bm.verts.new(p)
    result = bmesh.ops.convex_hull(bm, input=bm.verts[:], use_existing_faces=False)
    on_hull = {e for e in result["geom"] if isinstance(e, bmesh.types.BMVert)}
    inside = [v for v in bm.verts if v not in on_hull]
    if inside:
        bmesh.ops.delete(bm, geom=inside, context="VERTS")
    bm.verts.index_update()
    verts = [v.co.copy() for v in bm.verts]
    faces = [[v.index for v in f.verts] for f in bm.faces]
    bm.free()
    return verts, faces


class Prop:
    """One thing on the map: convex pieces, each of one material, that
    become one mesh object. The exporter makes a solid of each piece. It
    welds corners that are in the same place, which would join two pieces
    into one that is not convex, so a piece with a corner where another
    already has one is moved a millimetre."""

    CELL = 0.002

    def __init__(self, name, into):
        self.name, self.into = name, into
        self.verts, self.faces, self.slots, self.mats = [], [], [], []
        self.taken = {}

    def _key(self, p):
        return (round(p.x / self.CELL), round(p.y / self.CELL), round(p.z / self.CELL))

    def _crowded(self, points):
        for p in points:
            kx, ky, kz = self._key(p)
            for dx in (-1, 0, 1):
                for dy in (-1, 0, 1):
                    for dz in (-1, 0, 1):
                        for q in self.taken.get((kx + dx, ky + dy, kz + dz), ()):
                            if (p - q).length < 0.0006:
                                return True
        return False

    def piece(self, points, faces, mat, m=None):
        points = [Vector(p) for p in points]
        if m is not None:
            points = [m @ p for p in points]
        while self._crowded(points):
            points = [p + Vector((0.0011, 0.0009, 0.0007)) for p in points]
        base = len(self.verts)
        for p in points:
            self.verts.append(p)
            self.taken.setdefault(self._key(p), []).append(p)
        if mat not in self.mats:
            self.mats.append(mat)
        slot = self.mats.index(mat)
        for face in faces:
            self.faces.append([base + i for i in face])
            self.slots.append(slot)

    def prism(self, outline, lo, hi, mat, axis="x", m=None):
        """A convex outline (its corners in turn) pushed along an axis from
        lo to hi. The outline is in the other two axes, in their order."""
        point = {"x": lambda t, a, b: (t, a, b), "y": lambda t, a, b: (a, t, b), "z": lambda t, a, b: (a, b, t)}[axis]
        n = len(outline)
        points = [point(t, a, b) for t in (lo, hi) for a, b in outline]
        faces = [list(range(n))[::-1], [n + i for i in range(n)]]
        faces += [[i, (i + 1) % n, n + (i + 1) % n, n + i] for i in range(n)]
        self.piece(points, faces, mat, m)

    def box(self, lo, hi, mat, m=None):
        self.prism([(lo[1], lo[2]), (hi[1], lo[2]), (hi[1], hi[2]), (lo[1], hi[2])], lo[0], hi[0], mat, "x", m)

    def obox(self, center, size, mat, rot=(0.0, 0.0, 0.0), m=None):
        """A box of this size about its centre, turned (about X, then Y, then Z)."""
        turn = Matrix.Translation(center) @ Euler(rot).to_matrix().to_4x4()
        half = [s / 2 for s in size]
        self.box([-h for h in half], half, mat, turn if m is None else m @ turn)

    def cyl(self, center, radius, length, mat, axis="z", sides=8, m=None):
        """A drum, a wheel, a pole: along an axis, about its centre."""
        ring = [(radius * math.cos(TAU * (i + 0.5) / sides), radius * math.sin(TAU * (i + 0.5) / sides)) for i in range(sides)]
        move = Matrix.Translation(center)
        self.prism(ring, -length / 2, length / 2, mat, axis, move if m is None else m @ move)

    def hull(self, points, mat, m=None):
        verts, faces = hull_of([Vector(p) for p in points])
        self.piece(verts, faces, mat, m)

    def chunk(self, center, radii, mat, rot=None, corners=10, m=None):
        """A lump of rubble: the hull of points about a centre, flat underneath."""
        points = []
        for _ in range(corners):
            v = Vector((rng.gauss(0, 1), rng.gauss(0, 1), rng.gauss(0, 1))).normalized()
            s = rng.uniform(0.72, 1.0)
            points.append(Vector((v.x * radii[0] * s, v.y * radii[1] * s, max(v.z * radii[2] * s, -0.45 * radii[2]))))
        turn = Matrix.Translation(center) @ Euler(rot or (0.0, 0.0, rng.uniform(0, TAU))).to_matrix().to_4x4()
        self.hull(points, mat, turn if m is None else m @ turn)

    def build(self, at=(0.0, 0.0, 0.0), yaw=0.0, pitch=0.0, roll=0.0):
        """The object, standing at `at`: rolled about its length (Y), tipped
        about X, then turned about the vertical."""
        mesh = bpy.data.meshes.new(self.name)
        mesh.from_pydata(self.verts, [], self.faces)
        for mat in self.mats:
            mesh.materials.append(mat)
        mesh.polygons.foreach_set("material_index", self.slots)
        bm = bmesh.new()
        bm.from_mesh(mesh)
        bmesh.ops.recalc_face_normals(bm, faces=bm.faces[:])
        bm.to_mesh(mesh)
        bm.free()
        mesh.update()
        obj = bpy.data.objects.new(self.name, mesh)
        obj.location = at
        obj.rotation_euler = (pitch, roll, yaw)
        self.into.objects.link(obj)
        return obj


def empty(name, kind, at, scale=(1, 1, 1), turn=0.0):
    obj = bpy.data.objects.new(name, None)
    obj.empty_display_type = kind
    obj.location = at
    obj.scale = scale
    obj.rotation_euler = (0, 0, turn)
    MARKERS.objects.link(obj)
    return obj


def light(name, at, color, watts, reach):
    """A lamp, or a fire if its name starts with "fire" (those waver). A
    thousand watts light the ground 3 m away about as the sun does by day."""
    data = bpy.data.lights.new(name, "POINT")
    data.color = lin(color)
    data.energy = watts
    data.use_custom_distance = True
    data.cutoff_distance = reach
    data.shadow_soft_size = 0.2
    obj = bpy.data.objects.new(name, data)
    obj.location = at
    LIGHTS.objects.link(obj)
    return obj


# ---------------------------------------------------------------- the lie of the land

# The street runs along Y from the runners' end (Y = 0) to the overpass, X
# across it. The road is ROAD_HALF either side of the middle, the pavements
# reach from there to the buildings' fronts at FRONT.
LONG = 180.0
ROAD_HALF, FRONT, KERB = 8.0, 12.5, 0.16
# A street crosses between these, pavements and all.
CROSS0, CROSS1 = 84.0, 96.0
# The overpass: its deck's top, and where it starts and ends along the street.
TOP = 12.0
PY0, PY1 = LONG - 12, LONG - 2
EYE = TOP + 1.6  # the sniper's eyes, standing


def high_enough(y, runner, behind):
    """How high cover at y has to be to hide a runner that tall (1.8 m
    standing, 1.2 crouched) from the sniper, up to `behind` metres back."""
    away = PY0 - y
    return (behind * EYE + runner * away) / (away + behind)


def street():
    p = Prop("ground", STREET)
    p.box((-46, -24, -1.0), (46, LONG + 40, -0.03), GROUND)
    p.build()
    p = Prop("road", STREET)
    p.box((-ROAD_HALF, -6, -0.6), (ROAD_HALF, LONG + 18, 0.0), ROAD)
    for s in (-1, 1):
        p.box((min(s * ROAD_HALF, s * 26), CROSS0 + 2, -0.6), (max(s * ROAD_HALF, s * 26), CROSS1 - 2, 0.0), ROAD)
    p.build()
    p = Prop("pavements", STREET)
    for s in (-1, 1):
        x0, x1 = sorted((s * ROAD_HALF, s * (FRONT + 2.6)))
        for y0, y1 in ((-6, CROSS0), (CROSS1, LONG + 18)):
            p.box((x0, y0, -0.6), (x1, y1, KERB), PAVEMENT)
        x0, x1 = sorted((s * ROAD_HALF, s * 26))
        for y0, y1 in ((CROSS0, CROSS0 + 2), (CROSS1 - 2, CROSS1)):
            p.box((x0, y0, -0.6), (x1, y1, KERB), PAVEMENT)
    p.build()
    # What is left of the paint: the double line down the middle, the lane
    # lines either side, and the crossings at the junction.
    p = Prop("road lines", STREET)
    y = 14.0
    while y < PY0 - 4:
        if not CROSS0 - 5 < y < CROSS1 + 2 and rng.random() < 0.75:
            for x in (-0.14, 0.14):
                p.box((x - 0.06, y, 0.0), (x + 0.06, y + 5.0, 0.012), LINE)
        y += 7.5
    y = 16.0
    while y < PY0 - 4:
        for x in (-4.0, 4.0):
            if not CROSS0 - 5 < y < CROSS1 + 2 and rng.random() < 0.6:
                p.box((x - 0.07, y, 0.0), (x + 0.07, y + 2.4, 0.012), LINE_WHITE)
        y += 8.0
    for y0 in (CROSS0 - 3.6, CROSS1 + 0.8):
        x = -7.2
        while x < 7.0:
            if rng.random() < 0.8:
                p.box((x, y0, 0.0), (x + 0.5, y0 + 2.8, 0.012), LINE_WHITE)
            x += 1.1
    p.build()


# ---------------------------------------------------------------- buildings


def building(name, wall, length, floors, depth=13.0, ground=4.2, storey=3.3, lit=(), cold=(), boarded=(), shut=(), doors=(), awnings=(), roof=0):
    """A building in its own frame: its front along +Y from 0 to `length`
    on X = 0, the street on its -X side and the building behind it. The
    front is piers and the bands of wall between the storeys, with the dark
    inside showing between them: windows are given as (bay, storey) with the
    ground floor 0."""
    height = ground + (floors - 1) * storey
    p = Prop(name, BUILDINGS)
    p.box((0.5, 0, 0), (depth, length, height), wall)
    p.box((0.38, 0.2, 0.3), (0.5, length - 0.2, height - 0.5), DARK)
    bays = max(2, round(length / 3.1))
    bay = length / bays
    pier = bay * 0.48
    for k in range(bays + 1):
        p.box((0, max(0.0, k * bay - pier / 2), 0), (0.5, min(length, k * bay + pier / 2), height), wall)
    bands = [(0.0, 0.45), (ground - 0.85, ground + 0.95 if floors > 1 else height)]
    for f in range(1, floors):
        z = ground + (f - 1) * storey
        bands.append((z + 2.6, z + storey + 0.95 if f < floors - 1 else height))
    for z0, z1 in bands:
        p.box((0.05, 0, z0), (0.5, length, z1), wall)
    p.box((-0.22, 0, height - 0.3), (0.62, length, height + 0.6), wall)

    def window(i, f):
        y0, y1 = i * bay + pier / 2, (i + 1) * bay - pier / 2
        if f == 0:
            return y0, y1, 0.45, ground - 0.85
        z = ground + (f - 1) * storey
        return y0, y1, z + 0.95, z + 2.6

    for panes, mat, front in ((lit, LIT, 0.31), (cold, LIT_COLD, 0.31), (boarded, BOARDS, 0.2)):
        for i, f in panes:
            y0, y1, z0, z1 = window(i % bays, min(f, floors - 1))
            p.box((front, y0, z0), (0.38, y1, z1), mat)
    for i in shut:
        y0, y1, z0, z1 = window(i % bays, 0)
        p.box((0.24, y0, z0), (0.38, y1, z1), SHUTTER)
    for i in doors:
        y0, y1, _, _ = window(i % bays, 0)
        p.box((0.02, y0 + 0.15, 0), (0.3, y1 - 0.15, 2.5), DOOR)
    for i0, i1, mat in awnings:
        y0, y1 = i0 * bay + 0.15, (i1 + 1) * bay - 0.15
        p.obox((-0.85, (y0 + y1) / 2, ground - 0.95), (1.9, y1 - y0, 0.06), mat, rot=(0, rad(-13), 0))
    # What stands on the roof, for the ones the sniper looks down on.
    for k in range(roof):
        y = length * (k + 0.5) / roof + rng.uniform(-1.5, 1.5)
        w, d, h = rng.uniform(1.6, 3.5), rng.uniform(1.6, 3.0), rng.uniform(0.9, 2.4)
        p.box((3.0, y - w / 2, height), (3.0 + d, y + w / 2, height + h), rng.choice((wall, SHUTTER)))
    return p, height


def stand(p, side, y0, y1, back=0.0):
    """Stands a building (or a ruin) along the street between y0 and y1 on
    the left (side -1) or the right (1), its front `back` behind the others'."""
    if side > 0:
        return p.build((FRONT + back, y0, 0))
    return p.build((-FRONT - back, y1, 0), yaw=math.pi)


def ruin(name, wall, length, depth=13.0):
    """A building that came down: its back wall, a few piers of its front
    and two of its floors, lying where they fell. In a building's frame."""
    p = Prop(name, BUILDINGS)
    third = length / 3
    for k, h in enumerate((11.0, 7.5, 13.5)):
        p.box((depth - 0.6, k * third, 0), (depth, (k + 1) * third, h), wall)
    for h, reach in ((4.2, 0.55), (7.5, 0.3)):
        p.box((depth - 3.6, 0.4, h - 0.3), (depth - 0.6, length * reach, h), FLOOR_SLAB)
    bays = max(2, round(length / 3.1))
    bay = length / bays
    for k in range(bays + 1):
        h = rng.choice((0.9, 1.6, 2.8, 4.2, 4.2, 6.8, 7.5))
        if k not in (0, bays) and rng.random() < 0.2:
            continue
        p.box((0, max(0.0, k * bay - 0.75), 0), (0.5, min(length, k * bay + 0.75), h), wall)
        if h >= 4.2 and k < bays and rng.random() < 0.6:
            p.box((0.05, k * bay, 3.35), (0.5, (k + 1) * bay, 4.2 + rng.uniform(0.2, 0.9)), wall)
        if k < bays and rng.random() < 0.7:
            p.box((0.05, k * bay, 0), (0.5, (k + 1) * bay, rng.uniform(0.3, 0.9)), wall)
    p.obox((depth * 0.55, length * 0.33, 2.3), (depth * 0.8, length * 0.5, 0.35), FLOOR_SLAB, rot=(rad(4), rad(-22), rad(5)))
    p.obox((depth * 0.5, length * 0.74, 1.4), (depth * 0.7, length * 0.4, 0.35), FLOOR_SLAB, rot=(rad(-6), rad(-14), rad(-8)))
    return p


def buildings():
    # The left side, from the runners' end.
    p, _ = building("flats", BRICK_RED, 20, 5, boarded=((1, 1), (4, 2), (2, 0)), shut=(0, 4), doors=(3,))
    stand(p, -1, -6, 14)
    p, _ = building("office block", WALL_GREY, 23, 7, boarded=((2, 1),), cold=((5, 5),), shut=(1, 2, 5), doors=(3,))
    stand(p, -1, 17, 40)
    stand(ruin("fallen house", BRICK_BROWN, 18), -1, 40, 58)
    p, _ = building("shops", PLASTER_PALE, 26, 4, lit=((6, 2),), boarded=((1, 1), (4, 3)), shut=(0, 3, 6), doors=(2, 5),
                    awnings=((0, 1, AWNING_RED), (3, 4, AWNING_GREEN), (6, 7, AWNING_RED)))
    stand(p, -1, 58, CROSS0)
    p, _ = building("tenement", BRICK_BROWN, 22, 6, boarded=((0, 1), (3, 1), (5, 2), (2, 4)), shut=(1, 4), doors=(2,))
    stand(p, -1, CROSS1, 118)
    p, _ = building("depot", CINDER, 25, 3, depth=14, storey=3.6, shut=(0, 1, 2, 4, 5, 6), doors=(3,), roof=3)
    stand(p, -1, 121, 146, back=2.5)
    p, _ = building("bank", STONE, 20.5, 5, boarded=((1, 0), (4, 0), (2, 2)), doors=(3,))
    stand(p, -1, 146, 166.5)
    # The right side.
    p, _ = building("hotel", PLASTER_GREEN, 26, 6, lit=((2, 4),), boarded=((5, 1), (0, 2)), shut=(1, 6), doors=(4,),
                    awnings=((3, 5, AWNING_GREEN),))
    stand(p, 1, -6, 20)
    p, _ = building("store", BRICK_RED, 24, 4, boarded=((0, 0), (1, 0), (6, 1)), shut=(3, 4), doors=(5,),
                    awnings=((0, 2, AWNING_RED),))
    stand(p, 1, 20, 44)
    p, _ = building("tower", WALL_GREY, 23, 8, cold=((1, 6),), boarded=((3, 1), (4, 1), (6, 2)), shut=(0, 2, 5), doors=(3,))
    stand(p, 1, 47, 70)
    p, diner_top = building("diner", PLASTER_PALE, 14, 2, boarded=((0, 0), (3, 1)), doors=(2,), awnings=((0, 1, AWNING_RED),), roof=2)
    stand(p, 1, 70, CROSS0)
    stand(ruin("fallen works", WALL_GREY, 28), 1, CROSS1, 124)
    p, _ = building("mill", BRICK_BROWN, 24, 5, boarded=((1, 1), (2, 1), (6, 3)), shut=(0, 3, 7), doors=(5,))
    stand(p, 1, 124, 148)
    p, _ = building("exchange", PLASTER_GREEN, 18.5, 6, boarded=((0, 1), (2, 0), (4, 3)), doors=(1,))
    stand(p, 1, 148, 166.5)
    # Past the overpass, and what closes the street at either end.
    p, _ = building("far flats", STONE, 18, 6, boarded=((1, 1), (3, 2)))
    stand(p, -1, 179.5, 197.5)
    p, _ = building("far block", WALL_GREY, 18, 7, cold=((2, 5),))
    stand(p, 1, 179.5, 197.5)
    p, _ = building("end block", BRICK_RED, 60, 8, lit=((9, 5), (14, 2)), boarded=((7, 1), (11, 3)))
    p.build((30, 197.5, 0), yaw=math.pi / 2)
    p, _ = building("back block", WALL_GREY, 60, 6, lit=((6, 3),), boarded=((8, 1), (9, 1), (12, 2)), shut=(7, 8, 10, 11), doors=(9,))
    p.build((-30, -6, 0), yaw=-math.pi / 2)
    # The cross street ends at a wall of buildings either way.
    p, _ = building("corner block", PLASTER_PALE, 34, 5, cold=((3, 3),))
    p.build((-27, 107, 0), yaw=math.pi)
    p, _ = building("corner house", BRICK_BROWN, 34, 6, lit=((7, 4),))
    p.build((27, 73, 0))
    # A neon sign still burning over the diner's door.
    p = Prop("diner sign", BUILDINGS)
    p.box((-0.5, 0, 0), (0, 0.12, 0.9), TRIM)
    p.box((-0.44, -0.03, 0.12), (-0.06, 0.0, 0.78), NEON)
    p.box((-0.44, 0.12, 0.12), (-0.06, 0.15, 0.78), NEON)
    p.build((FRONT, 76.2, 4.3))
    light("neon", (FRONT - 1.0, 76.2, 4.4), (1.0, 0.16, 0.10), 220, 11)
    # The city beyond, dark against the sky.
    # Here and there somebody still has a light on.
    p = Prop("skyline", BUILDINGS)
    for x0, x1, y0, y1, h in ((-44, -30, 20, 48, 34), (-46, -31, 100, 132, 44), (30, 45, 40, 66, 38), (31, 44, 126, 150, 30),
                              (-20, 6, 203, 220, 46), (10, 34, 204, 222, 36), (-40, -8, -40, -22, 32), (4, 36, -42, -22, 40)):
        p.box((x0, y0, 0), (x1, y1, h), FAR)
        for _ in range(rng.randint(1, 3)):
            z, mat = rng.uniform(h * 0.45, h - 3.0), rng.choice((LIT, LIT, LIT_COLD))
            if y0 > LONG:  # seen from the street: on its near side
                x = rng.uniform(x0 + 2, x1 - 2)
                p.box((x, y0 - 0.06, z), (x + 1.3, y0 + 0.02, z + 1.6), mat)
            elif y1 < 0:
                x = rng.uniform(x0 + 2, x1 - 2)
                p.box((x, y1 - 0.02, z), (x + 1.3, y1 + 0.06, z + 1.6), mat)
            else:
                y, x = rng.uniform(y0 + 2, y1 - 2), x1 if x1 < 0 else x0
                p.box((x - 0.06, y, z), (x + 0.06, y + 1.3, z + 1.6), mat)
    p.build()


# ---------------------------------------------------------------- the overpass


def overpass():
    """The sniper's platform: a road on piers across the end of the street,
    with a wall along each edge low enough to shoot over standing and too
    high to step over. Its ends are gone."""
    reach = 31.0
    p = Prop("overpass", STREET)
    p.box((-reach, PY0, TOP - 1.3), (reach, PY1, TOP - 0.05), BRIDGE)
    p.box((-reach, PY0 + 0.3, TOP - 0.05), (reach, PY1 - 0.3, TOP), ROAD)
    p.box((-reach, PY0, TOP - 0.05), (reach, PY0 + 0.3, TOP + 0.9), BARRIER)
    p.box((-reach, PY1 - 0.3, TOP - 0.05), (reach, PY1, TOP + 0.9), BARRIER)
    for x in (-25.0, -10.4, 10.4, 25.0):
        p.box((x - 0.9, PY0 + 3.2, 0), (x + 0.9, PY1 - 3.2, TOP - 2.3), BRIDGE)
        p.box((x - 2.3, PY0 + 0.9, TOP - 2.3), (x + 2.3, PY1 - 0.9, TOP - 1.3), BRIDGE)
    # Where it broke off: slabs hanging from the ends.
    for s in (-1, 1):
        p.obox((s * (reach + 2.2), PY0 + 3.0, TOP - 2.1), (5.5, 5.4, 0.9), BRIDGE, rot=(rad(4), rad(s * 38), rad(3)))
        p.obox((s * (reach + 1.4), PY1 - 2.4, TOP - 1.4), (3.6, 4.2, 0.9), BRIDGE, rot=(rad(-6), rad(s * 22), rad(-5)))
    p.build()
    # The sniper keeps to the stretch over the street: wrecks stand either
    # side of it, and nothing gets past them.
    for s in (-1, 1):
        p = Prop("clip deck", MARKERS)
        p.box((s * 9.6 - 0.1, PY0 + 0.3, TOP), (s * 9.6 + 0.1, PY1 - 0.3, TOP + 4), BARRIER)
        p.build()
    box_truck(-15.9, PY0 + 6.0, 97, z=TOP)
    car("sedan", BLACK, -11.0, PY0 + 2.9, 187, burnt=True, z=TOP)
    car("suv", WHITE, 12.1, PY0 + 6.8, 75, burnt=True, z=TOP)
    car("hatch", BLUE, 12.6, PY0 + 2.7, -20, z=TOP)
    car("sedan", RED, 17.5, PY0 + 5.0, 8, z=TOP)
    p = Prop("deck barriers", COVER)
    jersey(p, Matrix.Translation((-10.3, PY0 + 7.6, TOP)) @ Matrix.Rotation(rad(95), 4, "Z"))
    jersey(p, Matrix.Translation((-22.0, PY0 + 3.0, TOP)) @ Matrix.Rotation(rad(20), 4, "Z"))
    jersey(p, Matrix.Translation((22.5, PY0 + 6.5, TOP)) @ Matrix.Rotation(rad(-35), 4, "Z"))
    p.build()


def jersey(p, m):
    """A road barrier 3 m long, along X."""
    p.prism([(-0.3, 0), (0.3, 0), (0.3, 0.18), (0.1, 0.85), (-0.1, 0.85), (-0.3, 0.18)], -1.5, 1.5, BARRIER, "x", m)


# ---------------------------------------------------------------- wrecks

# Cars, in their own frame: the nose at +Y. Length, width, how far off the
# ground, the top of the body, the roof, where the cabin's glass starts and
# ends at the bottom and at the top, the wheels' radius and how far from
# the middle they are.
CARS = {
    "sedan": (4.5, 1.80, 0.20, 0.86, 1.42, (-1.35, 1.00), (-0.85, 0.45), 0.33, 1.40),
    "hatch": (4.0, 1.75, 0.20, 0.90, 1.50, (-1.95, 0.85), (-1.80, 0.30), 0.32, 1.25),
    "suv": (4.7, 1.95, 0.26, 1.02, 1.78, (-2.25, 1.05), (-2.10, 0.50), 0.40, 1.45),
    "pickup": (5.3, 1.95, 0.28, 1.00, 1.80, (-0.45, 1.30), (-0.35, 0.80), 0.40, 1.65),
}


def car(kind, paint, x, y, yaw, burnt=False, door=0, roll=0.0, pitch=0.0, z=0.0):
    """A car left where it stopped, its nose turned `yaw` degrees from the
    way to the overpass. Burnt, it is rust on its rims. `door` -1 or 1 has
    the left or right door standing open. Rolled 90 it lies on its side,
    180 on its roof (lift it with `z`)."""
    length, width, clear, belt, top, (b0, b1), (t0, t1), radius, axle = CARS[kind]
    body, glass, tyre, trim = (BURNT, SOOT, SOOT, SOOT) if burnt else (paint, GLASS, TYRE, TRIM)
    if burnt:
        drop = clear - 0.07
        clear, belt, top, radius = clear - drop, belt - drop, top - drop, radius * 0.7
    w, half = width / 2, length / 2
    p = Prop(kind, WRECKS)
    p.box((-w, -half + 0.07, clear), (w, half - 0.07, belt), body)
    p.prism([(b0, belt), (b1, belt), (t1, top - 0.04), (t0, top - 0.04)], -w + 0.07, w - 0.07, glass)
    p.box((-w + 0.05, t0 - 0.05, top - 0.04), (w - 0.05, t1 + 0.05, top), body)
    for sx in (-1, 1):
        for sy in (-1, 1):
            p.cyl((sx * (w - 0.10), sy * axle, radius), radius, 0.22, tyre, "x")
    p.box((-w - 0.02, half - 0.14, clear + 0.04), (w + 0.02, half, clear + 0.30), trim)
    p.box((-w - 0.02, -half, clear + 0.04), (w + 0.02, -half + 0.14, clear + 0.30), trim)
    if kind == "pickup":
        for sx in (-1, 1):
            p.box((min(sx * w, sx * (w - 0.08)), -half + 0.07, belt), (max(sx * w, sx * (w - 0.08)), b0, belt + 0.42), body)
        p.box((-w, -half + 0.07, belt), (w, -half + 0.15, belt + 0.42), body)
    if door:
        swing = rad(58)
        p.obox((door * (w + math.sin(swing) * 0.52), 0.55 - math.cos(swing) * 0.52, (clear + belt) / 2 + 0.05),
               (0.05, 1.04, belt - clear - 0.1), body, rot=(0, 0, math.pi + door * swing))
    return p.build((x, y, z), yaw=rad(yaw), roll=rad(roll), pitch=rad(pitch))


def van(paint, x, y, yaw, burnt=False, roll=0.0, z=0.0):
    """A van, tall enough to stand behind far from the overpass."""
    length, width, top, clear = 5.3, 2.0, 2.2, 0.3
    body, glass, tyre = (BURNT, SOOT, SOOT) if burnt else (paint, GLASS, TYRE)
    if burnt:
        clear, top = 0.08, top - 0.22
    w, half = width / 2, length / 2
    p = Prop("van", WRECKS)
    p.box((-w, -half, clear), (w, 1.4, top), body)
    p.prism([(1.4, clear), (half, clear), (half, 1.05), (2.15, top), (1.4, top)], -w, w, body)
    slope = math.atan2(0.5, top - 1.05)
    p.obox((0, (half + 2.15) / 2 + 0.012, (1.05 + top) / 2 + 0.005), (width - 0.3, 0.04, 0.95), glass, rot=(slope, 0, 0))
    radius = 0.26 if burnt else 0.36
    for sx in (-1, 1):
        for sy in (-1.75, 1.7):
            p.cyl((sx * (w - 0.10), sy, radius), radius, 0.22, tyre, "x")
    p.box((-w - 0.02, half - 0.05, clear), (w + 0.02, half + 0.1, clear + 0.3), SOOT if burnt else TRIM)
    return p.build((x, y, z), yaw=rad(yaw), roll=rad(roll))


def box_truck(x, y, yaw, burnt=False, z=0.0, roll=0.0):
    """A delivery truck: 3.3 m of box to stand behind."""
    cab, box, glass, tyre = (BURNT, BURNT, SOOT, SOOT) if burnt else (WHITE, BOX_WHITE, GLASS, TYRE)
    p = Prop("box truck", WRECKS)
    p.box((-1.15, 2.1, 0.55), (1.15, 3.8, 2.45), cab)
    p.box((-1.0, 3.8, 1.45), (1.0, 3.83, 2.25), glass)
    p.box((-1.2, -3.8, 1.0), (1.2, 2.0, 3.3), box)
    p.box((-0.9, -3.7, 0.45), (0.9, 2.2, 1.0), TRIM)
    p.box((-1.2, 3.8, 0.45), (1.2, 3.95, 0.8), TRIM)
    for sx in (-1, 1):
        for sy in (2.95, -2.2):
            p.cyl((sx * 1.06, sy, 0.46), 0.46, 0.3, tyre, "x")
    return p.build((x, y, z), yaw=rad(yaw), roll=rad(roll))


def bus(x, y, yaw, roll=0.0, z=0.0, length=11.5):
    """A city bus. On its side (rolled 90) it is a wall across two lanes."""
    w, half, top = 2.55 / 2, length / 2, 3.1
    p = Prop("bus", WRECKS)
    p.box((-w, -half, 0.38), (w, half, top), BUS)
    p.box((-w - 0.012, -half + 0.6, 1.55), (w + 0.012, half - 1.3, 2.45), GLASS)
    p.box((-w - 0.008, -half + 0.01, 0.95), (w + 0.008, half - 0.01, 1.15), BUS_STRIPE)
    p.box((-w + 0.15, half - 0.02, 1.3), (w - 0.15, half + 0.012, 2.6), GLASS)
    p.box((-w + 0.3, -half - 0.012, 1.7), (w - 0.3, -half + 0.02, 2.5), GLASS)
    for sx in (-1, 1):
        for sy in (half - 2.2, -half + 2.6):
            p.cyl((sx * (w - 0.14), sy, 0.5), 0.5, 0.3, TYRE, "x")
    return p.build((x, y, z), yaw=rad(yaw), roll=rad(roll))


def trailer(x, y, yaw, mat=BOX_WHITE):
    """A lorry's trailer, 4 m high: cover right up to the overpass."""
    p = Prop("trailer", WRECKS)
    p.box((-1.27, -6.6, 1.25), (1.27, 6.6, 4.05), mat)
    p.box((-1.1, -6.5, 0.95), (1.1, 6.0, 1.25), TRIM)
    p.box((-1.2, -6.62, 0.45), (1.2, -6.5, 0.62), TRIM)
    for sx in (-1, 1):
        p.box((sx * 0.9 - 0.08, 3.4, 0), (sx * 0.9 + 0.08, 3.6, 0.95), TRIM)
        for sy in (-4.4, -5.7):
            p.cyl((sx * 1.05, sy, 0.5), 0.5, 0.42, TYRE, "x")
    for sx in (-0.6, -0.2, 0.2, 0.6):
        p.box((sx - 0.03, -6.64, 1.35), (sx + 0.03, -6.6, 3.95), TRIM)
    return p.build((x, y, 0), yaw=rad(yaw))


def tractor(x, y, yaw, paint=RED):
    """The lorry that pulled it."""
    p = Prop("lorry", WRECKS)
    p.box((-1.2, 0.2, 0.75), (1.2, 2.5, 3.3), paint)
    p.box((-1.0, 2.5, 0.75), (1.0, 4.1, 1.95), paint)
    p.box((-1.05, 2.5, 2.05), (1.05, 2.53, 3.0), GLASS)
    p.box((-0.55, -3.3, 0.6), (0.55, 4.0, 0.95), TRIM)
    p.box((-1.1, 4.1, 0.5), (1.1, 4.25, 0.9), TRIM)
    for sx in (-1, 1):
        p.cyl((sx * 1.28, 0.05, 2.4), 0.08, 3.0, TRIM, "z")
        for sy in (3.2, -1.2, -2.5):
            p.cyl((sx * 1.05, sy, 0.5), 0.5, 0.36, TYRE, "x")
    return p.build((x, y, 0), yaw=rad(yaw))


def carrier(x, y, yaw, burnt=False):
    """What the army left behind: an eight-wheeled carrier with a turret."""
    hull, tyre = (BURNT, SOOT) if burnt else (ARMY, TYRE)
    p = Prop("carrier", WRECKS)
    p.prism([(-3.4, 0.95), (-3.0, 0.5), (2.6, 0.5), (3.5, 1.25), (3.2, 1.95), (-3.4, 1.95)], -1.4, 1.4, hull)
    p.cyl((0, 0.5, 2.22), 0.95, 0.55, hull, "z")
    p.cyl((0, 2.6, 2.25), 0.07, 2.6, TRIM, "y")
    p.box((-0.5, -2.6, 1.95), (0.5, -1.4, 2.12), hull)
    for sx in (-1, 1):
        for sy in (-2.45, -0.95, 0.55, 2.0):
            p.cyl((sx * 1.33, sy, 0.55 if not burnt else 0.42), 0.55 if not burnt else 0.42, 0.42, tyre, "x")
    return p.build((x, y, 0 if not burnt else -0.13), yaw=rad(yaw))


def helicopter(x, y, yaw):
    """It came down on the junction: the cabin on its skids, leaning, the
    tail broken off behind it, what is left of the blades hanging."""
    p = Prop("helicopter", WRECKS)
    lean = Matrix.Rotation(rad(13), 4, "Y") @ Matrix.Rotation(rad(-5), 4, "X")
    pod = []
    for ring_y, rx, z0, z1 in ((2.9, 0.25, 0.9, 1.5), (2.3, 0.85, 0.55, 2.0), (0.8, 1.1, 0.4, 2.45), (-1.4, 1.05, 0.5, 2.45), (-2.6, 0.55, 0.95, 2.2)):
        pod += [(-rx, ring_y, z0 + 0.35), (rx, ring_y, z0 + 0.35), (-rx, ring_y, z1 - 0.3), (rx, ring_y, z1 - 0.3),
                (-rx * 0.6, ring_y, z0), (rx * 0.6, ring_y, z0), (-rx * 0.6, ring_y, z1), (rx * 0.6, ring_y, z1)]
    p.hull(pod, ARMY, lean)
    p.hull([(-0.8, 2.34, 1.15), (0.8, 2.34, 1.15), (-0.75, 2.2, 2.02), (0.75, 2.2, 2.02), (-0.3, 2.95, 1.0), (0.3, 2.95, 1.0),
            (-0.28, 2.93, 1.5), (0.28, 2.93, 1.5), (-0.6, 2.75, 1.85), (0.6, 2.75, 1.85)], GLASS, lean @ Matrix.Translation((0, 0.03, 0.02)))
    p.cyl((0, 0, 2.7), 0.16, 0.6, TRIM, "z", m=lean)
    p.cyl((0, 0, 3.0), 0.35, 0.14, TRIM, "z", m=lean)
    for turn, reach, droop in ((25, 5.6, 14), (150, 5.2, 19), (262, 2.1, 6)):
        blade = lean @ Matrix.Translation((0, 0, 3.0)) @ Matrix.Rotation(rad(turn), 4, "Z") @ Matrix.Rotation(rad(droop), 4, "Y")
        p.box((0.2, -0.16, -0.03), (reach, 0.16, 0.03), TRIM, blade)
    for sx in (-1, 1):
        p.box((sx * 1.0 - 0.05, -1.7, 0.0), (sx * 1.0 + 0.05, 2.3, 0.09), TRIM, lean)
        for sy in (-1.0, 1.4):
            p.box((sx * 0.85 - 0.04, sy - 0.04, 0.05), (sx * 0.85 + 0.04, sy + 0.04, 0.6), TRIM, lean)
    # The tail, snapped and lying to one side.
    tail = Matrix.Translation((0.5, -2.7, 0.15)) @ Matrix.Rotation(rad(-24), 4, "Z") @ Matrix.Rotation(rad(-9), 4, "X")
    p.hull([(-0.45, 0, 0.9), (0.45, 0, 0.9), (-0.4, 0, 1.75), (0.4, 0, 1.75), (-0.12, -5.4, 1.3), (0.12, -5.4, 1.3),
            (-0.12, -5.4, 1.62), (0.12, -5.4, 1.62)], ARMY, tail)
    p.prism([(-4.7, 1.5), (-5.6, 1.5), (-6.0, 3.0), (-5.5, 3.0)], -0.05, 0.05, ARMY, "x", tail)
    return p.build((x, y, 0), yaw=rad(yaw))


# ---------------------------------------------------------------- cover that was put there


def blast_walls(x, y, count, height=2.6, yaw=0.0):
    """A row of concrete blast walls, 1.4 m each, centred here and running
    across the street (along X) unless turned. They no longer stand quite
    straight."""
    p = Prop("blast walls", COVER)
    for i in range(count):
        m = Matrix.Translation(((i - (count - 1) / 2) * 1.4, rng.uniform(-0.05, 0.05), 0)) @ Euler(
            (rad(rng.uniform(-1.5, 1.5)), 0, rad(rng.uniform(-1.2, 1.2)))).to_matrix().to_4x4()
        p.box((-0.72, -0.14, 0), (0.72, 0.14, height), T_WALL, m)
        p.box((-0.72, -0.55, 0), (0.72, 0.55, 0.3), T_WALL, m)
    return p.build((x, y, 0), yaw=rad(yaw))


def earth_bags(x, y, count, yaw=0.0, stack=1):
    """Wire baskets of earth, 1.1 m square and 1.37 high, in a row."""
    p = Prop("earth bags", COVER)
    for i in range(count):
        for k in range(stack):
            if k and rng.random() < 0.3:
                break
            cx, cy = (i - (count - 1) / 2) * 1.12 + rng.uniform(-0.02, 0.02), rng.uniform(-0.06, 0.06)
            p.obox((cx, cy, k * 1.37 + 0.685), (1.08, 1.08, 1.37), HESCO, rot=(0, 0, rad(rng.uniform(-3, 3))))
    return p.build((x, y, 0), yaw=rad(yaw))


def container(mat, x, y, yaw, z=0.0, stacked=None):
    """A shipping container, 6 m by 2.4, 2.6 high; `stacked` is the colour
    of another on top of it."""
    p = Prop("container", COVER)
    for k, colour in enumerate((mat, stacked) if stacked else (mat,)):
        m = Matrix.Translation((0.1 * k, -0.25 * k, 2.59 * k)) @ Matrix.Rotation(rad(4 * k), 4, "Z")
        p.box((-1.22, -3.03, 0), (1.22, 3.03, 2.59), colour, m)
        for sx in (-0.75, -0.25, 0.25, 0.75):
            p.box((sx - 0.03, 3.03, 0.12), (sx + 0.03, 3.07, 2.47), TRIM, m)
    return p.build((x, y, z), yaw=rad(yaw))


def fence(x, y, length, yaw=0.0, height=2.6):
    """Corrugated sheets on posts: what somebody shut an alley with."""
    p = Prop("fence", COVER)
    n = max(1, round(length / 1.25))
    width = length / n
    for i in range(n):
        cx = (i - (n - 1) / 2) * width
        p.obox((cx, rng.uniform(-0.03, 0.03), (height + rng.uniform(-0.25, 0.1)) / 2), (width + 0.04, 0.05, height + rng.uniform(-0.3, 0.1)),
               FENCE if rng.random() < 0.7 else BOX_RUST, rot=(rad(rng.uniform(-2, 2)), 0, rad(rng.uniform(-2, 2))))
    for i in range(n + 1):
        p.box(((i - n / 2) * width - 0.05, 0.03, 0), ((i - n / 2) * width + 0.05, 0.13, height - 0.2), TRIM)
    return p.build((x, y, 0), yaw=rad(yaw))


def drums(x, y, count, fire=None, yaw=0.0):
    """Oil drums in a huddle. With `fire` (watts), the first has a fire in it."""
    p = Prop("drums", COVER)
    spots = [(0, 0), (0.64, 0.1), (0.28, 0.62), (-0.5, 0.45), (0.9, 0.7)]
    for i in range(count):
        dx, dy = spots[i % len(spots)]
        if fire and i == 0:
            p.cyl((dx, dy, 0.44), 0.29, 0.88, DRUM_RUST)
            p.cyl((dx, dy, 0.88), 0.25, 0.05, FIRE)
        elif rng.random() < 0.2:
            p.cyl((dx + 0.3, dy + 0.9, 0.29), 0.29, 0.88, rng.choice((DRUM, DRUM_RUST)), "x")
        else:
            p.cyl((dx, dy, 0.44), 0.29, 0.88, rng.choice((DRUM, DRUM_RUST)))
    obj = p.build((x, y, 0), yaw=rad(yaw))
    if fire:
        light("fire", (x, y, 1.35), (1.0, 0.55, 0.22), fire, 15)
    return obj


def crates(x, y, count, yaw=0.0, z=0.0):
    """Crates and pallets, stacked as they were left."""
    p = Prop("crates", COVER)
    spots = [(0, 0, 0), (1.15, 0.1, 0), (0.4, 1.1, 0), (0.1, 0.05, 1.0), (1.2, 1.2, 0), (-1.0, 0.5, 0)]
    for i in range(count):
        dx, dy, dz = spots[i % len(spots)]
        s = rng.choice((0.9, 1.0, 1.0))
        p.obox((dx, dy, dz + s / 2), (s, s, s), CRATE, rot=(0, 0, rad(rng.uniform(-12, 12))))
    for k in range(3):
        p.obox((-0.2, -1.3, 0.07 + 0.15 * k), (1.2, 1.0, 0.13), PLANKS, rot=(0, 0, rad(rng.uniform(-6, 6))))
    return p.build((x, y, z), yaw=rad(yaw))


def dumpster(x, y, yaw):
    p = Prop("dumpster", COVER)
    p.box((-0.95, -0.55, 0.08), (0.95, 0.55, 1.15), DUMPSTER)
    p.prism([(-0.57, 1.15), (0.57, 1.15), (0.57, 1.24), (-0.57, 1.42)], -0.97, 0.97, TRIM)
    return p.build((x, y, KERB if abs(x) > ROAD_HALF else 0), yaw=rad(yaw))


def tyres(x, y, count):
    p = Prop("tyres", DEBRIS)
    spots = [(0, 0, 0), (0.05, 0.04, 0.23), (0.72, 0.1, 0), (-0.02, 0.0, 0.46), (0.4, 0.75, 0), (0.75, 0.12, 0.23)]
    for i in range(count):
        dx, dy, dz = spots[i % len(spots)]
        p.cyl((dx, dy, dz + 0.115), 0.34, 0.22, TYRE)
    return p.build((x, y, KERB if abs(x) > ROAD_HALF else 0), yaw=rng.uniform(0, TAU))


def lamp_post(x, y, towards, lit=0.0, fallen=None):
    """A street lamp on the kerb, its arm out over the road (`towards` -1
    or 1: the way the road is). `lit` (watts) is one that still burns;
    `fallen` (degrees) one lying across the road that way."""
    p = Prop("lamp post", STREET)
    p.cyl((0, 0, 0.5), 0.16, 1.0, POLE)
    p.cyl((0, 0, 3.7), 0.085, 7.4, POLE)
    if fallen is None:
        p.box((0, -0.05, 7.24), (1.9, 0.05, 7.36), POLE)
        p.box((1.5, -0.17, 7.12), (2.35, 0.17, 7.26), POLE)
        p.box((1.58, -0.12, 7.09), (2.27, 0.12, 7.125), LAMP if lit else GLASS)
        obj = p.build((x, y, KERB), yaw=0.0 if towards > 0 else math.pi)
    else:
        obj = p.build((x, y, KERB + 0.12), yaw=rad(fallen), roll=rad(86))
    if lit:
        light("lamp", (x + towards * 1.9, y, 6.75), (1.0, 0.74, 0.44), lit, 24)
    return obj


def bus_stop(x, y, towards):
    """A shelter on the pavement: a roof, a back, and an end towards the overpass."""
    p = Prop("bus stop", COVER)
    p.box((-0.85, -2.0, 2.45), (0.85, 2.0, 2.53), SHELTER)
    p.box((0.74, -2.0, 0.2), (0.8, 2.0, 2.3), SHELTER)
    p.box((-0.5, 1.94, 0.2), (0.74, 2.0, 2.3), SHELTER)
    for sx, sy in ((-0.75, -1.95), (-0.75, 1.95), (0.77, -1.95), (0.77, 1.95)):
        p.box((sx - 0.04, sy - 0.04, 0), (sx + 0.04, sy + 0.04, 2.45), TRIM)
    p.box((0.3, -1.2, 0.42), (0.7, 1.2, 0.5), PLANKS)
    return p.build((x, y, KERB), yaw=0.0 if towards < 0 else math.pi)


def scaffold(side, y0, y1):
    """Scaffolding up a front, over the pavement: its boards keep the
    sniper's eyes off whoever is under them, and a sheet hangs at its end."""
    p = Prop("scaffold", COVER)
    length = y1 - y0
    n = max(2, round(length / 2.4))
    for i in range(n + 1):
        for d in (-0.22, -1.62):
            p.box((d - 0.035, i * length / n - 0.035, 0), (d + 0.035, i * length / n + 0.035, 6.9), TRIM)
    for z in (2.25, 4.5):
        p.box((-1.68, 0, z - 0.06), (-0.16, length, z), PLANKS)
        p.box((-1.68, 0, z + 0.95), (-1.62, length, z + 1.0), TRIM)
    for i in range(n):
        if rng.random() < 0.55:
            a, b = i * length / n + 0.05, (i + 1) * length / n - 0.05
            p.box((-1.71, a, 2.3), (-1.68, b, 4.4 - rng.uniform(0, 0.8)), TARP)
    return p, length


def traffic_light(x, y, yaw, fallen=False):
    p = Prop("traffic light", STREET)
    p.cyl((0, 0, 2.9), 0.09, 5.8, POLE)
    p.box((0, -0.05, 5.5), (4.2, 0.05, 5.62), POLE)
    for at in (2.2, 3.9):
        p.box((at - 0.17, -0.16, 4.75), (at + 0.17, 0.16, 5.5), TRIM)
    if fallen:
        return p.build((x, y, KERB + 0.15), yaw=rad(yaw), roll=rad(84))
    # One lamp on it still shows amber.
    p.box((2.2 - 0.1, -0.19, 4.98), (2.2 + 0.1, -0.16, 5.2), AMBER)
    at = Matrix.Rotation(rad(yaw), 4, "Z") @ Vector((2.2, -0.9, 4.9))
    light("signal", (x + at.x, y + at.y, KERB + at.z), (1.0, 0.62, 0.15), 260, 13)
    return p.build((x, y, KERB), yaw=rad(yaw))


# ---------------------------------------------------------------- rubble


def slab(p, center, wide, up, rot, mat=FLOOR_SLAB):
    """A piece of a floor, broken off all round."""
    corners = sorted(rng.uniform(0, TAU) for _ in range(7))
    # Near enough its four corners, and a few more, so that it stays a slab.
    corners = [TAU * (k + 0.5) / 4 + rng.uniform(-0.25, 0.25) for k in range(4)] + corners[:3]
    reach = 2 ** 0.5 / 2
    outline = [(math.cos(a) * wide * reach * rng.uniform(0.86, 1.0), math.sin(a) * up * reach * rng.uniform(0.86, 1.0)) for a in sorted(corners)]
    points = [(x, y, z) for z in (-0.16, 0.16) for x, y in outline]
    p.hull(points, mat, Matrix.Translation(center) @ Euler(rot).to_matrix().to_4x4())


def junk(x, y, things, yaw=0.0, spread=1.4):
    """What people left: `things` names them, in a row from here."""
    p = Prop("junk", DEBRIS)
    for i, thing in enumerate(things):
        m = Matrix.Translation((i * spread + rng.uniform(-0.2, 0.2), rng.uniform(-0.3, 0.3), 0)) @ Matrix.Rotation(rng.uniform(-0.5, 0.5), 4, "Z")
        if thing == "sofa":
            p.box((-0.95, -0.4, 0.1), (0.95, 0.4, 0.45), SOFA, m)
            p.box((-0.95, 0.2, 0.45), (0.95, 0.42, 0.9), SOFA, m)
            for sx in (-1, 1):
                p.box((min(sx * 0.97, sx * 0.75), -0.4, 0.45), (max(sx * 0.97, sx * 0.75), 0.2, 0.66), SOFA, m)
        elif thing == "fridge":
            p.obox((0, 0, 0.36), (0.72, 1.7, 0.7), APPLIANCE, m=m)
            p.obox((0.2, 0.3, 0.73), (0.03, 0.5, 0.04), TRIM, m=m)
        elif thing == "mattress":
            p.obox((0, 0, 0.46), (1.9, 0.2, 0.95), MATTRESS, rot=(rad(-22), 0, 0), m=m)
        elif thing == "cone":
            p.hull([(sx * r, sy * r, z) for r, z in ((0.19, 0.0), (0.19, 0.04), (0.04, 0.7)) for sx in (-1, 1) for sy in (-1, 1)], CONE, m)
        elif thing == "cone down":
            lie = m @ Matrix.Translation((0, 0, 0.19)) @ Matrix.Rotation(rad(82), 4, "X")
            p.hull([(sx * r, sy * r, z) for r, z in ((0.19, 0.0), (0.19, 0.04), (0.04, 0.7)) for sx in (-1, 1) for sy in (-1, 1)], CONE, lie)
        elif thing == "sign":
            lean = m @ Matrix.Rotation(rad(rng.uniform(-14, 14)), 4, "Y")
            p.cyl((0, 0, 1.1), 0.035, 2.2, POLE, m=lean)
            p.cyl((0, -0.05, 2.0), 0.36, 0.04, HYDRANT, "y", m=lean)
        elif thing == "bin":
            p.cyl((0, 0, 0.45), 0.27, 0.9, POLE, m=m)
        elif thing == "bin down":
            p.cyl((0, 0, 0.27), 0.27, 0.9, POLE, "x", m=m)
        elif thing == "hydrant":
            p.cyl((0, 0, 0.32), 0.12, 0.64, HYDRANT, m=m)
            p.cyl((0, 0, 0.68), 0.15, 0.08, HYDRANT, m=m)
        elif thing == "mailbox":
            p.box((-0.28, -0.25, 0), (0.28, 0.25, 1.1), DRUM, m)
        elif thing == "bench":
            p.box((-0.9, -0.22, 0.4), (0.9, 0.22, 0.46), PLANKS, m)
            p.box((-0.9, 0.18, 0.46), (0.9, 0.24, 0.9), PLANKS, m)
            for sx in (-0.75, 0.75):
                p.box((sx - 0.04, -0.2, 0), (sx + 0.04, 0.22, 0.4), TRIM, m)
        elif thing == "case":
            p.obox((0, 0, 0.13), (0.7, 0.48, 0.24), rng.choice((DOOR, TARP, AWNING_GREEN)), m=m)
        elif thing == "pallets":
            for k in range(rng.randint(2, 5)):
                p.obox((0, 0, 0.07 + 0.15 * k), (1.2, 1.0, 0.13), PLANKS, rot=(0, 0, rad(rng.uniform(-8, 8))), m=m)
        elif thing == "door":
            p.obox((0, 0, 0.9), (0.9, 0.06, 2.0), DOOR, rot=(rad(-28), 0, 0), m=m)
    return p.build((x, y, KERB if abs(x) > ROAD_HALF else 0), yaw=rad(yaw))


def rubble(name, x, y, rx, ry, height, count, mats, slabs=()):
    """A heap of rubble: a solid heart, lumps all over it, and slabs of
    floor standing in it across the street, each given as (x, y, how wide,
    how long up its slope, tilted how far from flat, turned how far)."""
    p = Prop(name, DEBRIS)
    for dx, dy, part in ((0, 0, 0.72), (-0.25, -0.42, 0.52), (0.2, 0.4, 0.55), (0.3, -0.1, 0.5), (-0.3, 0.25, 0.45)):
        p.chunk((dx * rx, dy * ry, height * part * 0.25), (rx * part, ry * part, height * part * 1.25), mats[0], corners=14)
    for _ in range(count):
        a, d = rng.uniform(0, TAU), math.sqrt(rng.random())
        px, py = math.cos(a) * d * rx, math.sin(a) * d * ry
        size = rng.uniform(0.3, 1.0) * (1.25 - 0.6 * d)
        z = height * max(0.0, 1 - d ** 1.4) * rng.uniform(0.45, 0.85)
        p.chunk((px, py, max(z, size * 0.25)), (size * rng.uniform(0.7, 1.3), size * rng.uniform(0.7, 1.3), size * rng.uniform(0.4, 0.8)),
                rng.choice(mats), rot=(rad(rng.uniform(-25, 25)), rad(rng.uniform(-25, 25)), rng.uniform(0, TAU)))
    for sx, sy, wide, up, tilt, turn in slabs:
        z = up * abs(math.sin(rad(tilt))) / 2 - 0.2
        slab(p, (sx, sy, z), wide, up, (rad(tilt), rad(rng.uniform(-6, 6)), rad(turn)))
    return p.build((x, y, 0))


def litter(name, y0, y1, flats, lumps, near=None):
    """What lies about the road between y0 and y1: flat scraps (paper,
    boards, tiles), and lumps small enough to run over. `near` (x, y,
    spread) heaps them round a place instead."""
    p = Prop(name, DEBRIS)

    def spot():
        if near:
            return rng.gauss(near[0], near[2]), rng.gauss(near[1], near[2])
        return rng.uniform(-FRONT + 0.5, FRONT - 0.5), rng.uniform(y0, y1)

    for _ in range(flats):
        x, y = spot()
        if not -FRONT + 0.4 < x < FRONT - 0.4:
            continue
        z = KERB if abs(x) > ROAD_HALF + 0.6 else 0.0
        if ROAD_HALF - 0.6 < abs(x) < ROAD_HALF + 0.6:
            continue
        p.obox((x, y, z + 0.011), (rng.uniform(0.3, 1.2), rng.uniform(0.2, 0.6), 0.02), rng.choice((PAPER, PLANKS, RUBBLE, RUBBLE_BRICK)),
               rot=(0, 0, rng.uniform(0, TAU)))
    for _ in range(lumps):
        x, y = spot()
        if not -FRONT + 0.6 < x < FRONT - 0.6 or ROAD_HALF - 0.6 < abs(x) < ROAD_HALF + 0.6:
            continue
        z = KERB if abs(x) > ROAD_HALF else 0.0
        s = rng.uniform(0.12, 0.26)
        p.chunk((x, y, z + s * 0.3), (s * 1.4, s * 1.2, s * 0.7), rng.choice((RUBBLE, RUBBLE_ROCK, RUBBLE_BRICK)), corners=7)
    return p.build()


# ---------------------------------------------------------------- the street, end to end


def start():
    """The runners come in behind an old roadblock: a row of blast walls
    with gaps in it, and a shorter wall behind each gap, to walk round. From
    nowhere on the overpass is there a line through a gap to where a runner
    comes in: nobody is shot there."""
    for i in range(8):
        x = -10.5 + 3.0 * i
        empty("spawn", "ARROWS", (x, 5, KERB if abs(x) > ROAD_HALF else 0))
    for x in (-9.0, -3.0, 3.0, 9.0):
        blast_walls(x, 11.6, 3, height=3.6 if x in (-3.0, 9.0) else 2.6)
    for x in (-11.7, -6.0, 0.0, 6.0, 11.7):
        blast_walls(x, 8.8, 2)
    earth_bags(-9.6, 14.3, 3)
    earth_bags(3.4, 14.5, 2, stack=2)
    car("sedan", SILVER, 8.6, 14.6, 196, burnt=True)
    car("hatch", GREEN, -4.2, 2.2, 80)
    van(WHITE, 5.5, 0.6, 262)
    drums(0.3, 1.6, 3, fire=330)
    crates(-8.5, 1.8, 4, yaw=20)
    tyres(8.6, 9.8, 5)
    tyres(-3.2, 10.0, 3)
    container(BOX_RUST, -9.4, -3.4, 90)
    container(BOX_BLUE, -0.6, -3.6, 87)
    bus(7.5, -3.3, 84, length=9.0)


def far_stretch():
    """From the roadblock to the fallen house: the traffic that never got
    out, low cars to crouch behind and a few things to stand behind."""
    car("sedan", RED, -6.2, 19.5, 186, door=1)
    van(WHITE, -2.1, 22.0, 173)
    car("sedan", BLACK, 2.4, 19.0, 12, burnt=True)
    car("suv", BLACK, 6.3, 22.8, -6)
    dumpster(-10.9, 25.0, 4)
    bus_stop(10.6, 26.5, -1)
    car("sedan", YELLOW, 1.7, 27.6, 38)
    box_truck(5.7, 32.2, 2)
    car("sedan", BLUE, -5.6, 28.6, 199, door=-1)
    lamp_post(8.5, 30.5, -1, lit=1500)
    car("hatch", GREEN, -1.6, 33.6, 168)
    van(BLUE, -4.6, 39.2, 82, burnt=True, roll=90, z=1.02)
    car("sedan", SILVER, 2.2, 38.0, -14)
    car("suv", WHITE, 6.6, 42.4, 9, door=-1)
    tyres(-11.2, 34.0, 4)
    crates(10.6, 37.5, 3, yaw=95, z=KERB)
    # The alley beside the flats.
    fence(-17.5, 15.5, 3.0, yaw=90)
    dumpster(-14.2, 16.1, 4)
    lamp_post(-8.5, 44.5, 1, fallen=-8)
    litter("litter 1", 13, 45, 46, 12)
    junk(9.2, 16.5, ("hydrant", "bin"), yaw=90, spread=5.5)
    junk(-9.0, 21.0, ("mailbox", "bin down", "case"), yaw=85, spread=1.6)
    junk(-11.0, 31.0, ("bench",), yaw=90)
    junk(0.2, 24.6, ("case", "case"), yaw=40, spread=0.9)
    junk(10.8, 33.0, ("pallets", "door"), yaw=95, spread=1.7)


def fallen_house():
    """The house on the left came down across its pavement and a lane."""
    rubble("house rubble", -9.6, 49.0, 5.4, 8.0, 2.7, 70, (RUBBLE, RUBBLE_BRICK, RUBBLE_BRICK, RUBBLE_ROCK),
           slabs=((3.6, -3.6, 4.4, 3.8, 62, 10), (4.2, 4.4, 4.0, 4.2, 118, -12), (0.6, 0.4, 3.4, 3.0, 35, 40)))
    litter("house litter", 40, 58, 30, 26, near=(-5.0, 49.0, 3.6))
    # Across the road from it, a pile-up, one of them still burning.
    car("sedan", RED, 3.4, 49.6, 71, burnt=True)
    p = Prop("car fire", WRECKS)
    p.chunk((0, 0, 0.1), (0.5, 0.45, 0.35), FIRE, corners=9)
    p.build((3.9, 50.6, 0.95))
    light("fire car", (3.9, 50.5, 1.9), (1.0, 0.5, 0.18), 620, 18)
    car("hatch", WHITE, 6.0, 53.6, -32)
    car("pickup", GREEN, 0.8, 55.4, 152)
    tyres(7.6, 48.2, 3)
    junk(-3.6, 44.2, ("sofa", "mattress"), yaw=20, spread=2.3)
    junk(-4.2, 55.6, ("fridge", "case", "door"), yaw=-30, spread=1.5)
    junk(10.2, 52.0, ("bin", "hydrant", "bench"), yaw=90, spread=3.2)


def middle_stretch():
    """On to the junction: a bus on its side, a container off its lorry,
    and the army's roadblock, where cover has to be taller."""
    bus(3.6, 63.4, 62, roll=90, z=1.29)
    container(BOX_BLUE, -4.6, 62.0, 14)
    lamp_post(-8.5, 66.5, 1, lit=1500)
    car("sedan", WHITE, -6.5, 70.2, 178)
    car("suv", RED, -2.2, 73.4, 160, burnt=True)
    car("sedan", BLUE, 6.9, 70.5, 4, door=1)
    earth_bags(2.2, 71.6, 3, yaw=8, stack=2)
    blast_walls(4.4, 78.6, 4, height=3.6, yaw=-5)
    carrier(-3.6, 79.6, 82)
    drums(10.4, 73.6, 4, yaw=40)
    crates(10.7, 79.5, 5, yaw=185, z=KERB)
    dumpster(-10.8, 76.0, 88)
    car("hatch", YELLOW, -9.9, 61.5, 172)
    litter("litter 2", 58, 84, 40, 12)
    junk(-1.0, 76.2, ("cone", "cone down", "cone", "sign"), yaw=8, spread=1.5)
    junk(7.2, 81.2, ("cone", "cone"), yaw=-20, spread=1.3)
    junk(-11.2, 68.0, ("bench", "bin"), yaw=90, spread=2.4)
    junk(9.0, 65.0, ("mailbox", "hydrant"), yaw=90, spread=4.0)


def junction():
    """The cross street, shut off a little way down either side, and the
    helicopter that came down in the middle of it."""
    helicopter(0.6, 90.4, 28)
    van(SILVER, -6.0, 93.4, -86)
    car("sedan", GREEN, 6.2, 87.0, 97)
    car("suv", BLUE, 5.6, 93.8, 64, burnt=True)
    traffic_light(-8.6, 96.8, 0)
    traffic_light(8.6, 83.2, 180, fallen=True)
    # The left arm: somebody camped here.
    container(BOX_GREEN, -23.2, 86.9, 3, stacked=BOX_RED)
    container(BOX_RUST, -23.4, 92.95, -2)
    drums(-16.2, 89.6, 3, fire=380)
    crates(-18.4, 92.8, 5, yaw=30)
    earth_bags(-14.4, 86.6, 3, yaw=90)
    tyres(-19.5, 86.4, 6)
    # The right arm: a lorry tried it.
    box_truck(17.8, 89.6, 80, burnt=True)
    blast_walls(23.0, 90.0, 8, height=3.6, yaw=90)
    car("sedan", BLACK, 14.5, 93.2, 250)
    for s in (-1, 1):
        p = Prop("clip cross street", MARKERS)
        p.box((s * 25.5 - 0.3, CROSS0 - 1, 0), (s * 25.5 + 0.3, CROSS1 + 1, 30), T_WALL)
        p.build()
    litter("litter 3", 84, 96, 24, 8)
    junk(-17.6, 88.0, ("mattress", "sofa"), yaw=200, spread=2.4)
    junk(-20.5, 94.2, ("pallets", "case"), yaw=10, spread=1.5)
    junk(16.5, 86.4, ("cone down", "sign", "bin down"), yaw=10, spread=1.8)


def fallen_works():
    """Past the junction the works on the right came down over half the
    road, and a lorry jack-knifed across the rest."""
    rubble("works rubble", 9.6, 109.5, 5.4, 11.0, 3.3, 96, (RUBBLE, RUBBLE, RUBBLE_ROCK, RUBBLE_BRICK),
           slabs=((-2.8, -6.6, 4.6, 4.4, 64, -8), (-3.2, 4.6, 4.4, 4.6, 116, 9), (-0.5, -0.8, 4.0, 3.2, 30, 55), (-1.6, 10.6, 3.6, 3.6, 70, 20)))
    litter("works litter", 96, 124, 36, 30, near=(3.5, 110.0, 4.2))
    trailer(-4.8, 110.6, 62)
    tractor(-5.2, 103.6, 190)
    car("sedan", SILVER, -9.6, 101.4, 176, burnt=True)
    car("hatch", RED, -1.4, 99.6, 150)
    car("sedan", YELLOW, 1.4, 117.6, 31, burnt=True)
    # The alley beside the tenement, and the fire in its mouth.
    fence(-17.5, 119.5, 3.0, yaw=90)
    drums(-13.6, 119.4, 2, fire=300)
    dumpster(-10.9, 116.4, 86)
    junk(6.0, 99.4, ("fridge", "door", "sofa"), yaw=10, spread=2.0)
    junk(-9.2, 106.5, ("sign", "bin", "case"), yaw=90, spread=1.7)
    junk(-15.2, 119.8, ("mattress", "pallets"), yaw=180, spread=1.6)


def near_stretch():
    """The last of it, where the sniper looks almost straight down: tall
    blast walls, lorries and stacked containers, closer together."""
    blast_walls(0.1, 122.6, 4, height=3.6, yaw=4)
    blast_walls(-7.0, 127.2, 3, height=3.6, yaw=-6)
    blast_walls(6.2, 128.4, 3, height=3.6, yaw=7)
    lamp_post(8.5, 121.8, -1, lit=700)
    car("suv", BLACK, -2.5, 131.4, 171)
    van(WHITE, 5.9, 134.6, 6, burnt=True)
    box_truck(-4.0, 137.4, 96)
    earth_bags(1.8, 133.6, 3, yaw=84, stack=2)
    car("sedan", BLUE, 2.1, 139.6, -20, roll=180, z=1.42)
    # The depot's yard, set back on the left.
    container(BOX_RED, -13.4, 131.5, 2, stacked=BOX_BLUE)
    crates(-13.2, 138.6, 6, yaw=10, z=KERB)
    tyres(-11.6, 124.6, 5)
    # Scaffolding up the mill, on the right.
    p, length = scaffold(1, 126.0, 146.0)
    p.box((-1.68, length - 0.05, 0.1), (-0.16, length, 2.2), TARP)
    stand(p, 1, 126.0, 146.0)
    container(BOX_GREEN, 3.4, 147.2, 8, stacked=BOX_RUST)
    container(BOX_BLUE, -4.9, 146.2, 92)
    blast_walls(-9.4, 152.0, 3, height=3.6, yaw=5)
    blast_walls(0.0, 154.2, 3, height=3.6, yaw=-3)
    blast_walls(8.6, 151.2, 3, height=3.6, yaw=-6)
    trailer(4.6, 160.8, 97, mat=BOX_RUST)
    earth_bags(-5.6, 158.2, 3, yaw=6, stack=2)
    car("pickup", WHITE, -0.4, 150.6, 200, burnt=True)
    lamp_post(-8.5, 141.5, 1, fallen=4)
    litter("litter 4", 96, 162, 60, 14)
    junk(-9.0, 134.5, ("pallets", "pallets", "case"), yaw=95, spread=1.5)
    junk(1.2, 126.0, ("cone", "sign", "cone down"), yaw=175, spread=1.4)
    junk(-10.6, 147.5, ("hydrant", "bin down"), yaw=90, spread=3.0)
    junk(9.4, 156.0, ("pallets", "case", "door"), yaw=80, spread=1.5)


def under_the_overpass():
    """Home: a camp under the deck, shut in either side and behind."""
    drums(-5.6, PY0 + 6.4, 3, fire=360)
    drums(6.2, PY0 + 5.2, 2, fire=360)
    crates(-2.4, PY0 + 8.2, 5, yaw=12)
    earth_bags(3.0, PY0 + 9.0, 2, yaw=90)
    tyres(7.8, PY0 + 8.0, 4)
    junk(-8.0, PY0 + 8.6, ("mattress", "mattress", "case"), yaw=5, spread=2.1)
    junk(1.0, PY0 + 4.2, ("sofa", "pallets"), yaw=170, spread=2.4)
    for s in (-1, 1):
        blast_walls(s * 15.2, PY0 + 5.0, 10, height=3.6, yaw=90)
        p = Prop("clip under the overpass", MARKERS)
        p.box((s * 15.6 - 0.2, 164.0, 0), (s * 15.6 + 0.2, 182.0, 30), T_WALL)
        p.build()
    # The road beyond is walled off with containers.
    container(BOX_RED, -9.2, 188.4, 90, stacked=BOX_GREEN)
    container(BOX_BLUE, -3.0, 188.8, 88, stacked=BOX_RUST)
    container(BOX_RUST, 3.2, 188.5, 91, stacked=BOX_BLUE)
    container(BOX_GREEN, 9.4, 188.7, 90, stacked=BOX_RED)
    car("suv", SILVER, -3.8, 183.2, 170)
    car("sedan", GREEN, 5.2, 184.0, -8, burnt=True)
    # A runner under the deck has made it.
    empty("finish", "CUBE", (0, PY0 + 5, 2.5), (14.6, 5, 2.5))
    # The sniper, on the deck, looking back down the street (-Y).
    empty("spawn sniper", "ARROWS", (0, PY0 + 4, TOP), turn=math.pi)


def alleys():
    """The alley on the right, between the store and the tower."""
    fence(17.5, 45.5, 3.0, yaw=90)
    dumpster(14.4, 45.0, 94)
    crates(15.6, 46.0, 2, yaw=10)
    for x, y in ((-18.0, 15.5), (-18.0, 119.5), (18.0, 45.5)):
        p = Prop("clip alley", MARKERS)
        p.box((x - 0.3, y - 1.8, 0), (x + 0.3, y + 1.8, 30), T_WALL)
        p.build()


street()
buildings()
overpass()
start()
far_stretch()
fallen_house()
middle_stretch()
junction()
fallen_works()
near_stretch()
under_the_overpass()
alleys()

# ---------------------------------------------------------------- the night

# The moon, behind the sniper and off to the left: it lights the side of
# everything the sniper sees, and leaves the runners' side of it dark.
moon = bpy.data.objects.new("moon", bpy.data.lights.new("moon", "SUN"))
moon.data.color = lin((0.62, 0.72, 1.0))
moon.data.energy = 0.7  # 4 is daylight
moon.rotation_euler = Vector((0, 0, 1)).rotation_difference(Vector((-0.42, 0.62, 0.66)).normalized()).to_euler()
moon.location = (-20, LONG + 20, 60)
LIGHTS.objects.link(moon)
if scene.world is None:
    scene.world = bpy.data.worlds.new("World")
scene.world.color = lin((0.016, 0.022, 0.045))
scene.shitshooter_mode = "SNIPER"
scene.shitshooter_ambient = 175.0
scene.shitshooter_fog_start = 45.0
scene.shitshooter_fog_end = 430.0

path = os.path.join(ROOT, "assets", "maps", "Sniper Street.json")
ok, lines = shitshooter_maps.export(bpy.context, path)
print("\n".join(line for line in lines if "cut into sections" not in line))
scene["shitshooter_path"] = path
bpy.ops.wm.save_as_mainfile(filepath=os.path.join(ROOT, "blender", "sniper_street.blend"))
if not ok:
    sys.exit(1)
