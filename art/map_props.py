"""Builds every model the outdoor maps are dressed with: buildings, trees,
rocks and props, in metres, Z up, standing on the origin (z = 0 is the
ground outside). The front of a building faces -Y.

Saves art/map_props.blend (every model, side by side), exports
assets/models/map/<name>.glb, and writes src/props.rs: each model's
collision boxes and marked points, in game coordinates (Y up), so the
server, which loads no models, collides with the same shapes.

Run with:
blender --background --factory-startup --python map_props.py -- <repo root> <preview dir>
"""
import math
import random
import sys

import bmesh
import bpy
from mathutils import Matrix, Vector

ROOT, PREVIEW = sys.argv[sys.argv.index("--") + 1:][:2]

# ---------------------------------------------------------------- materials


def lin(c):
    return tuple(x / 12.92 if x <= 0.04045 else ((x + 0.055) / 1.055) ** 2.4 for x in c)


MATERIALS = {}


def material(name, color, metallic=0.0, roughness=0.85, emission=None, strength=1.0):
    """Colours are sRGB, like the game's; Blender wants them linear."""
    if name in MATERIALS:
        return MATERIALS[name]
    mat = bpy.data.materials.new(name)
    mat.use_nodes = True
    bsdf = mat.node_tree.nodes["Principled BSDF"]
    bsdf.inputs["Base Color"].default_value = (*lin(color), 1.0)
    bsdf.inputs["Metallic"].default_value = metallic
    bsdf.inputs["Roughness"].default_value = roughness
    if emission:
        bsdf.inputs["Emission Color"].default_value = (*lin(emission), 1.0)
        bsdf.inputs["Emission Strength"].default_value = strength
    mat.diffuse_color = (*lin(color), 1.0)
    MATERIALS[name] = mat
    return mat


def palette():
    return {
        "plaster": material("Plaster", (0.86, 0.83, 0.76)),
        "plaster_warm": material("PlasterWarm", (0.85, 0.72, 0.52)),
        "plaster_blue": material("PlasterBlue", (0.62, 0.70, 0.74)),
        "stone": material("Stone", (0.50, 0.49, 0.46)),
        "stone_light": material("StoneLight", (0.68, 0.66, 0.61)),
        "brick": material("Brick", (0.55, 0.26, 0.18)),
        "trim": material("Trim", (0.93, 0.92, 0.88)),
        "wood": material("Wood", (0.42, 0.28, 0.16)),
        "wood_light": material("WoodLight", (0.66, 0.50, 0.32)),
        "wood_grey": material("WoodGrey", (0.45, 0.42, 0.38)),
        "barn_red": material("BarnRed", (0.55, 0.15, 0.11)),
        "roof_red": material("RoofRed", (0.58, 0.22, 0.14)),
        "roof_dark": material("RoofDark", (0.24, 0.25, 0.27)),
        "roof_green": material("RoofGreen", (0.22, 0.34, 0.28)),
        "concrete": material("Concrete", (0.60, 0.59, 0.56)),
        "concrete_dark": material("ConcreteDark", (0.40, 0.40, 0.39)),
        "metal": material("Metal", (0.50, 0.53, 0.55), metallic=0.6, roughness=0.5),
        "metal_dark": material("MetalDark", (0.22, 0.23, 0.25), metallic=0.6, roughness=0.5),
        "sheet": material("Sheet", (0.54, 0.58, 0.56), metallic=0.4, roughness=0.6),
        "sheet_rust": material("SheetRust", (0.55, 0.36, 0.22), metallic=0.3, roughness=0.8),
        "window": material("WindowDark", (0.08, 0.09, 0.11), roughness=0.3),
        "olive": material("Olive", (0.33, 0.36, 0.22)),
        "canvas": material("Canvas", (0.42, 0.42, 0.30)),
        "sand": material("Sandbag", (0.66, 0.59, 0.42)),
        "hay": material("Hay", (0.83, 0.69, 0.36)),
        "rust": material("Rust", (0.48, 0.24, 0.14), metallic=0.2, roughness=0.9),
        "tyre": material("Tyre", (0.08, 0.08, 0.08)),
        "red": material("SignalRed", (0.72, 0.12, 0.10)),
        "white": material("PaintWhite", (0.90, 0.90, 0.88)),
        "bark": material("Bark", (0.30, 0.21, 0.14)),
        "bark_birch": material("BarkBirch", (0.86, 0.85, 0.80)),
        "pine": material("PineNeedles", (0.13, 0.30, 0.17)),
        "pine_dark": material("PineDark", (0.10, 0.24, 0.15)),
        "leaf": material("Leaves", (0.28, 0.45, 0.16)),
        "leaf_light": material("LeavesLight", (0.42, 0.56, 0.20)),
        "leaf_birch": material("LeavesBirch", (0.50, 0.62, 0.25)),
        "bush": material("Bush", (0.24, 0.40, 0.15)),
        "rock": material("Rock", (0.47, 0.46, 0.43)),
        "rock_dark": material("RockDark", (0.36, 0.35, 0.33)),
        "moss": material("Moss", (0.33, 0.40, 0.21)),
        "lamp": material("Lamp", (1.0, 0.9, 0.7), emission=(1.0, 0.85, 0.55), strength=3.0),
    }


# ---------------------------------------------------------------- building kit


class Prop:
    """One model: a single mesh, its collision boxes and solids, and marked points."""

    def __init__(self, name):
        self.name = name
        self.bm = bmesh.new()
        self.mats = []
        self.colliders = []
        self.solids = []
        self.points = []

    def mat(self, m):
        if m not in self.mats:
            self.mats.append(m)
        return self.mats.index(m)

    def collide(self, x0, y0, z0, x1, y1, z1):
        self.colliders.append((min(x0, x1), min(y0, y1), min(z0, z1), max(x0, x1), max(y0, y1), max(z0, z1)))

    def solid(self, points):
        """Collides as the convex hull of these points: a ramp in the game,
        smooth where boxes would be steps. Its faces stood on if gentle
        enough (up to about 45 degrees), slid off if not."""
        bm = bmesh.new()
        for point in points:
            bm.verts.new(Vector(point))
        bmesh.ops.remove_doubles(bm, verts=bm.verts[:], dist=1e-4)
        out = bmesh.ops.convex_hull(bm, input=bm.verts[:])
        inside = list({g for g in out["geom_interior"] + out["geom_unused"] if isinstance(g, bmesh.types.BMVert)})
        bmesh.ops.delete(bm, geom=inside, context="VERTS")
        # One face for each flat side, without corners partway along an edge.
        bmesh.ops.dissolve_limit(bm, angle_limit=math.radians(0.5), verts=bm.verts[:], edges=bm.edges[:])
        bm.verts.index_update()
        corners = [tuple(v.co) for v in bm.verts]
        faces = [[v.index for v in f.verts] for f in bm.faces]
        bm.free()
        self.solids.append((corners, faces))

    def ramp(self, a, b, base, left, right):
        """Collides as a wedge (like `Ramp::new` in the game): a ridge from a
        to b, sloping down either side to a level base at height `base`,
        reaching out `left` and `right` metres for every metre it drops."""
        a, b = Vector(a), Vector(b)
        along = Vector((b.x - a.x, b.y - a.y, 0.0)).normalized()
        right_way = Vector((along.y, -along.x, 0.0))
        corners = []
        for top in (a, b):
            foot = Vector((top.x, top.y, base))
            drop = top.z - base
            corners += [top, foot - right_way * left * drop, foot + right_way * right * drop]
        self.solid(corners)

    def box(self, x0, y0, z0, x1, y1, z1, m, collide=True):
        x0, x1 = sorted((x0, x1))
        y0, y1 = sorted((y0, y1))
        z0, z1 = sorted((z0, z1))
        if x1 - x0 < 1e-4 or y1 - y0 < 1e-4 or z1 - z0 < 1e-4:
            return
        corners = [Vector((x, y, z)) for z in (z0, z1) for y in (y0, y1) for x in (x0, x1)]
        self.hull(corners, m)
        if collide:
            self.collide(x0, y0, z0, x1, y1, z1)
        return (x0, y0, z0, x1, y1, z1)

    def hull(self, corners, m):
        """Eight corners, ordered like `box` makes them (x fastest, then y, then z)."""
        v = [self.bm.verts.new(c) for c in corners]
        index = self.mat(m)
        for face in ((0, 2, 3, 1), (4, 5, 7, 6), (0, 1, 5, 4), (2, 6, 7, 3), (0, 4, 6, 2), (1, 3, 7, 5)):
            f = self.bm.faces.new([v[i] for i in face])
            f.material_index = index

    def beam(self, a, b, w, d, m, up=Vector((0, 0, 1))):
        """A bar of `w` by `d` from point a to point b (visual only)."""
        a, b = Vector(a), Vector(b)
        axis = (b - a).normalized()
        side = up.cross(axis)
        if side.length < 1e-4:
            side = Vector((0, 1, 0)).cross(axis)
        side.normalize()
        top = axis.cross(side).normalized()
        corners = []
        for sz in (-1, 1):
            for sy in (-1, 1):
                for end in (a, b):
                    corners.append(end + side * (sy * w / 2) + top * (sz * d / 2))
        self.hull(corners, m)

    def poly(self, points, m):
        f = self.bm.faces.new([self.bm.verts.new(Vector(p)) for p in points])
        f.material_index = self.mat(m)
        return f

    def prism(self, points, depth_axis, d0, d1, m):
        """A flat shape (points in the plane across `depth_axis`) pushed out
        from d0 to d1. Points are (u, v) in the other two axes, anticlockwise."""
        def place(u, v, d):
            if depth_axis == "x":
                return Vector((d, u, v))
            if depth_axis == "y":
                return Vector((u, d, v))
            return Vector((u, v, d))
        n = len(points)
        a = [self.bm.verts.new(place(u, v, d0)) for u, v in points]
        b = [self.bm.verts.new(place(u, v, d1)) for u, v in points]
        index = self.mat(m)
        faces = [a[::-1], b]
        for i in range(n):
            j = (i + 1) % n
            faces.append([a[i], a[j], b[j], b[i]])
        for verts in faces:
            f = self.bm.faces.new(verts)
            f.material_index = index
        # Wound one way or the other depending on the axis: fix it up.
        bmesh.ops.recalc_face_normals(self.bm, faces=list({f for v in a + b for f in v.link_faces}))

    def cylinder(self, r0, r1, z0, z1, m, segments=8, x=0.0, y=0.0, matrix=None, caps=True):
        mat = matrix or Matrix.Translation((x, y, (z0 + z1) / 2))
        out = bmesh.ops.create_cone(
            self.bm, cap_ends=caps, cap_tris=False, segments=segments,
            radius1=r0, radius2=r1, depth=z1 - z0, matrix=mat,
        )
        index = self.mat(m)
        for f in {f for v in out["verts"] for f in v.link_faces}:
            f.material_index = index
        return out["verts"]

    def blob(self, center, size, m, rng, rough=0.18, subdivisions=1):
        out = bmesh.ops.create_icosphere(self.bm, subdivisions=subdivisions, radius=1.0)
        index = self.mat(m)
        for v in out["verts"]:
            n = v.co.normalized()
            k = 1.0 + rng.uniform(-rough, rough)
            v.co = Vector(center) + Vector((n.x * size[0], n.y * size[1], n.z * size[2])) * k
        for f in {f for v in out["verts"] for f in v.link_faces}:
            f.material_index = index
        return out["verts"]

    def mark(self, name, x, y, z):
        self.points.append((name, (x, y, z)))


def wall(p, axis, start, end, t0, t1, z0, z1, m, openings=(), frame=None, sill=None, collide=True):
    """A wall running along `axis` ("x" or "y") from `start` to `end`, between
    t0 and t1 across it, from z0 to z1, with openings (centre, width, bottom,
    top) cut through; openings may sit above one another. `frame` trims the
    openings on both faces, `sill` adds a sill outside (on the t0 face)."""
    def piece(a, b, lo, hi):
        if hi - lo < 1e-4 or b - a < 1e-4:
            return
        if axis == "x":
            p.box(a, t0, lo, b, t1, hi, m, collide)
        else:
            p.box(t0, a, lo, t1, b, hi, m, collide)

    spans = [(c - w / 2, c + w / 2, lo, hi) for c, w, lo, hi in openings]
    cuts = sorted({start, end, *[v for a, b, _, _ in spans for v in (a, b) if start < v < end]})
    for a, b in zip(cuts, cuts[1:]):
        mid = (a + b) / 2
        holes = sorted((lo, hi) for x0, x1, lo, hi in spans if x0 <= mid <= x1)
        z = z0
        for lo, hi in holes:
            piece(a, b, z, max(z, lo))
            z = max(z, hi)
        piece(a, b, z, z1)

    if not frame:
        return
    ft, out = 0.12, 0.05
    for a, b, bottom, top in spans:
        for lo, hi in ((t0 - out, t0), (t1, t1 + out)):
            def trim(a2, b2, zl, zh):
                if axis == "x":
                    p.box(a2, lo, zl, b2, hi, zh, frame, False)
                else:
                    p.box(lo, a2, zl, hi, b2, zh, frame, False)
            trim(a - ft, a, bottom, top)
            trim(b, b + ft, bottom, top)
            trim(a - ft, b + ft, top, top + ft)
            if bottom > z0 + 0.05:
                trim(a - ft, b + ft, bottom - ft, bottom)
        if sill and bottom > z0 + 0.05:
            if axis == "x":
                p.box(a - 0.15, t0 - 0.18, bottom - 0.08, b + 0.15, t0, bottom, sill, False)
            else:
                p.box(t0 - 0.18, a - 0.15, bottom - 0.08, t0, b + 0.15, bottom, sill, False)


def walls_around(p, x0, y0, x1, y1, t, z0, z1, m, openings, frame=None, sill=None):
    """Four walls on the inside of the rectangle, `openings` keyed by side:
    "front" (-Y), "back" (+Y), "left" (-X), "right" (+X)."""
    wall(p, "x", x0, x1, y0, y0 + t, z0, z1, m, openings.get("front", ()), frame, sill)
    # The back wall's outside face is at +Y: give it the sill on that side by flipping.
    wall(p, "x", x0, x1, y1 - t, y1, z0, z1, m, openings.get("back", ()), frame, None)
    wall(p, "y", y0 + t, y1 - t, x0, x0 + t, z0, z1, m, openings.get("left", ()), frame, sill)
    wall(p, "y", y0 + t, y1 - t, x1 - t, x1, z0, z1, m, openings.get("right", ()), frame, None)


def gable_roof(p, x0, y0, x1, y1, eave, ridge, m, gable_m, along="x", over=0.4, thick=0.18, floor=None):
    """A pitched roof over the rectangle, its ridge running along `along`,
    with filled gable ends. Collides as one ramp under its top: walk up it
    and stand on the ridge. The ramp is solid down to `floor`, by default
    the outer edge of the eaves, so stand it on a ceiling."""
    if along == "x":
        cy = (y0 + y1) / 2
        half = (y1 - y0) / 2 + over
        drop = over * (ridge - eave) / ((y1 - y0) / 2)
        # Two roof planes as slanted slabs.
        for sign in (-1, 1):
            outer = cy + sign * half
            pts = []
            for z_off in (0.0, thick):
                for yy, zz in ((outer, eave - drop), (cy, ridge)):
                    for xx in (x0 - over, x1 + over):
                        pts.append(Vector((xx, yy, zz + z_off)))
            # Reorder to box order: x fastest, then "y" (eave->ridge), then z.
            p.hull(pts, m)
        for xx in (x0, x1):
            p.prism([(y0, eave), (y1, eave), (cy, ridge)], "x", xx - 0.0, xx + (0.25 if xx == x0 else -0.25), gable_m)
        reach = (y1 - y0) / 2 / (ridge - eave)
        a, b = (x0 - over, cy, ridge + thick), (x1 + over, cy, ridge + thick)
    else:
        cx = (x0 + x1) / 2
        half = (x1 - x0) / 2 + over
        drop = over * (ridge - eave) / ((x1 - x0) / 2)
        for sign in (-1, 1):
            outer = cx + sign * half
            pts = []
            for z_off in (0.0, thick):
                for xx, zz in ((outer, eave - drop), (cx, ridge)):
                    for yy in (y0 - over, y1 + over):
                        pts.append(Vector((xx, yy, zz + z_off)))
            # Box order wants x fastest: swap the roles of the loops.
            pts = [pts[i] for i in (0, 2, 1, 3, 4, 6, 5, 7)]
            p.hull(pts, m)
        for yy in (y0, y1):
            p.prism([(x0, eave), (x1, eave), (cx, ridge)], "y", yy, yy + (0.25 if yy == y0 else -0.25), gable_m)
        reach = (x1 - x0) / 2 / (ridge - eave)
        a, b = (cx, y0 - over, ridge + thick), (cx, y1 + over, ridge + thick)
    # The top of the roof slabs, from the ridge down to the edge of the eaves.
    p.ramp(a, b, eave - drop + thick if floor is None else floor, reach, reach)


def stairs(p, x0, x1, y_start, y_end, z0, z1, steps, m):
    """Solid steps from z0 up to z1, climbing from y_start towards y_end."""
    rise = (z1 - z0) / steps
    run = (y_end - y_start) / steps
    flight(p, [p.box(x0, y_start + run * k, z0, x1, y_start + run * (k + 1), z0 + rise * (k + 1), m, False)
               for k in range(steps)])


def flight(p, steps):
    """Collides a flight of steps, boxes (x0, y0, z0, x1, y1, z1) from the
    bottom one up, as one slope over the front edges of the steps: walked up
    smoothly, where the boxes would lift you a step at a time. It starts on
    the level the flight climbs from, a step out in front of the first."""
    first, second = steps[0], steps[1]
    middle = lambda b: Vector(((b[0] + b[3]) / 2, (b[1] + b[4]) / 2, 0.0))
    run = middle(second) - middle(first)
    rise = second[5] - first[5]
    points = [Vector((x, y, z)) for b in steps for x in (b[0], b[3]) for y in (b[1], b[4]) for z in (b[2], b[5])]
    top = [Vector((x, y, first[5])) for x in (first[0], first[3]) for y in (first[1], first[4])]
    front = min(c.dot(run) for c in top)
    points += [c - run - Vector((0, 0, rise)) for c in top if c.dot(run) < front + 1e-4]
    p.solid(points)


def slab_with_hole(p, x0, y0, x1, y1, z0, z1, hole, m):
    """A floor with a rectangular hole (hx0, hy0, hx1, hy1) in it."""
    hx0, hy0, hx1, hy1 = hole
    p.box(x0, y0, z0, hx0, y1, z1, m)
    p.box(hx1, y0, z0, x1, y1, z1, m)
    p.box(hx0, y0, z0, hx1, hy0, z1, m)
    p.box(hx0, hy1, z0, hx1, y1, z1, m)


def ring(p, x0, y0, x1, y1, inset, z0, z1, m, collide=False):
    """A band round the outside of the rectangle, `inset` wide inwards from
    its edge. Never a solid slab: one across the whole footprint would show
    as a ceiling inside and fight the real ceiling or floor for the pixels."""
    p.box(x0, y0, z0, x1, y0 + inset, z1, m, collide)
    p.box(x0, y1 - inset, z0, x1, y1, z1, m, collide)
    p.box(x0, y0 + inset, z0, x0 + inset, y1 - inset, z1, m, collide)
    p.box(x1 - inset, y0 + inset, z0, x1, y1 - inset, z1, m, collide)


FLOOR = 0.15
DOOR_TOP = 2.35
SILL = 1.0
WIN_TOP = 2.15


def window(c, w=1.2):
    return (c, w, SILL, WIN_TOP)


def door(c, w=1.3):
    return (c, w, FLOOR, DOOR_TOP)


# ---------------------------------------------------------------- buildings


def house(name, wall_m, roof_m, C):
    """One storey, two rooms, a door front and back, a pitched roof."""
    p = Prop(name)
    W, D, H = 4.0, 3.0, 3.0
    p.box(-W - 0.2, -D - 0.2, -1.0, W + 0.2, D + 0.2, FLOOR, C["stone"])
    t = 0.25
    walls_around(p, -W, -D, W, D, t, FLOOR, H, wall_m, {
        "front": [door(-1.0), window(-2.9), window(2.4)],
        "back": [door(2.0), window(-2.2)],
        "left": [window(0.0)],
        "right": [window(-1.2), window(1.4)],
    }, frame=C["trim"], sill=C["trim"])
    # Partition with a doorway.
    wall(p, "y", -D + t, D - t, 0.45, 0.6, FLOOR, H, wall_m, [(-1.6, 1.1, FLOOR, DOOR_TOP)])
    p.box(-W, -D, H, W, D, H + 0.2, wall_m)
    gable_roof(p, -W, -D, W, D, H + 0.2, H + 2.3, roof_m, wall_m, along="x")
    p.box(1.6, 0.8, H + 0.2, 2.3, 1.5, H + 3.0, C["brick"])
    # Doorstep.
    p.box(-1.8, -D - 0.7, FLOOR - 0.1, -0.2, -D - 0.2, FLOOR, C["stone_light"], False)
    return p


def house_tall(name, wall_m, C):
    """Two storeys, stairs up the left wall, a flat roof with a parapet."""
    p = Prop(name)
    W, D = 4.5, 3.5
    F2 = 3.5
    TOP = 6.8
    t = 0.25
    p.box(-W - 0.2, -D - 0.2, -1.0, W + 0.2, D + 0.2, FLOOR, C["stone"])
    up = lambda c, w=1.2: (c, w, F2 + 0.95, F2 + 2.1)
    walls_around(p, -W, -D, W, D, t, FLOOR, TOP, wall_m, {
        "front": [door(-2.4), window(0.6), window(3.0), up(-2.4), up(0.6), up(3.0)],
        "back": [window(-2.0), door(2.6), up(-2.0), up(0.6), up(2.6)],
        "left": [up(2.0)],
        "right": [window(-1.6), window(1.6), up(-1.6), up(1.6)],
    }, frame=C["trim"], sill=C["trim"])
    # Upper floor, with the stairwell cut out along the left wall.
    sx0, sx1 = -W + t, -W + t + 1.2
    # A metre of floor in front of the first step, to walk onto it from.
    y_start, y_end = -D + t + 1.0, 1.25
    slab_with_hole(p, -W + t, -D + t, W - t, D - t, F2 - 0.2, F2, (sx0, -D + t, sx1, y_end), C["wood"])
    stairs(p, sx0, sx1, y_start, y_end, FLOOR, F2, 11, C["wood_light"])
    # A rail along the stairwell upstairs.
    p.box(sx1, -D + t, F2, sx1 + 0.08, y_end - 0.9, F2 + 1.0, C["wood"])
    # Roof and parapet.
    p.box(-W, -D, TOP, W, D, TOP + 0.2, C["concrete"])
    wall(p, "x", -W, W, -D, -D + 0.2, TOP + 0.2, TOP + 1.0, wall_m)
    wall(p, "x", -W, W, D - 0.2, D, TOP + 0.2, TOP + 1.0, wall_m)
    wall(p, "y", -D + 0.2, D - 0.2, -W, -W + 0.2, TOP + 0.2, TOP + 1.0, wall_m)
    wall(p, "y", -D + 0.2, D - 0.2, W - 0.2, W, TOP + 0.2, TOP + 1.0, wall_m)
    # A band between the floors, and a cap on the parapet.
    ring(p, -W - 0.06, -D - 0.06, W + 0.06, D + 0.06, 0.06, F2 - 0.15, F2 + 0.05, C["trim"])
    ring(p, -W - 0.06, -D - 0.06, W + 0.06, D + 0.06, 0.26, TOP + 1.0, TOP + 1.08, C["trim"], True)
    # A water tank on the roof.
    p.cylinder(0.6, 0.6, TOP + 0.2, TOP + 1.6, C["metal"], 10, x=2.6, y=1.8)
    p.collide(2.0, 1.2, TOP + 0.2, 3.2, 2.4, TOP + 1.6)
    return p


def barn(C):
    """Open at both ends, a hayloft over the back half, stairs up to it."""
    p = Prop("barn")
    W, D, H = 5.0, 8.0, 5.4
    t = 0.2
    p.box(-W - 0.15, -D - 0.15, -1.0, W + 0.15, D + 0.15, FLOOR, C["concrete_dark"])
    big = (0.0, 4.0, FLOOR, 3.9)
    low = lambda c: (c, 0.9, 1.3, 2.1)
    high = lambda c: (c, 0.9, 3.9, 4.7)
    walls_around(p, -W, -D, W, D, t, FLOOR, H, C["barn_red"], {
        "front": [big, high(-3.2), high(3.2)],
        "back": [big],
        "left": [low(-4.5), low(-0.5), high(3.5), high(6.5)],
        "right": [low(-4.0), high(3.5), high(6.5), (-0.5, 1.3, FLOOR, DOOR_TOP)],
    }, frame=C["white"])
    # Big door leaves, swung open against the walls.
    for sy in (-1, 1):
        yy = sy * (D + 0.02)
        for sx in (-1, 1):
            x_in = sx * 2.0
            p.box(x_in + sx * 0.05, yy, FLOOR, x_in + sx * 0.14, yy + sy * 2.0, 3.85, C["barn_red"], False)
    # Hayloft over the back half.
    LOFT = 3.0
    loft_front = 1.5
    p.box(-W + t, loft_front, LOFT - 0.2, W - t, D - t, LOFT, C["wood"])
    stairs(p, -W + t, -W + t + 1.1, loft_front - 4.6, loft_front, FLOOR, LOFT, 9, C["wood_light"])
    # Rail along the loft edge, open where the stairs arrive.
    p.box(-W + t + 1.1, loft_front, LOFT, W - t, loft_front + 0.08, LOFT + 1.0, C["wood"])
    # Posts under the loft.
    for x in (-2.0, 2.0):
        p.box(x - 0.12, loft_front + 0.1, FLOOR, x + 0.12, loft_front + 0.34, LOFT - 0.2, C["wood"])
    # Bales on the loft and on the floor.
    for x, y, z in ((2.6, 5.5, LOFT), (2.6, 6.8, LOFT), (1.2, 6.8, LOFT), (3.6, -5.5, FLOOR), (-3.2, -6.4, FLOOR)):
        p.box(x - 0.6, y - 0.45, z, x + 0.6, y + 0.45, z + 0.8, C["hay"])
    # The ceiling stops at the gable ends (they stand on the walls from H up),
    # and is solid: you cannot jump up through it from the loft.
    p.box(-W, -D + 0.25, H, W, D - 0.25, H + 0.05, C["wood"])
    gable_roof(p, -W, -D, W, D, H, H + 3.4, C["roof_dark"], C["barn_red"], along="y", over=0.5, floor=H)
    # A white cross brace on each big door frame.
    return p


def warehouse(C):
    """A big shed with roller doors, a mezzanine along one wall and crates."""
    p = Prop("warehouse")
    W, D, H = 7.0, 11.0, 6.6
    t = 0.2
    p.box(-W - 0.3, -D - 0.3, -1.0, W + 0.3, D + 0.3, FLOOR, C["concrete"])
    roller = lambda c: (c, 4.6, FLOOR, 4.8)
    high = lambda c: (c, 1.6, 4.4, 5.6)
    walls_around(p, -W, -D, W, D, t, FLOOR, H, C["sheet"], {
        "front": [door(3.0), high(-3.0)],
        "back": [roller(0.0)],
        "left": [roller(-4.0), high(4.0), high(7.5)],
        "right": [high(-3.0), high(1.0), high(6.0)],
    }, frame=C["metal_dark"])
    # Ribs down the walls, outside.
    for k in range(-5, 6):
        y = k * 2.0
        for x in (-W - 0.06, W):
            if x < 0 and -6.5 < y < -1.5:
                continue
            p.box(x, y - 0.06, FLOOR, x + 0.06, y + 0.06, H, C["metal_dark"], False)
    # Mezzanine along the right wall, stairs up from the front.
    MZ = 3.4
    mx0 = W - t - 3.0
    my0 = -D + t + 4.6
    p.box(mx0, my0, MZ - 0.2, W - t, D - t, MZ, C["metal_dark"])
    stairs(p, W - t - 1.4, W - t, -D + t + 1.0, my0, FLOOR, MZ, 9, C["metal"])
    p.box(mx0 - 0.06, my0, MZ, mx0, D - t, MZ + 1.0, C["metal"])
    for y in (my0 + 1.0, 0.5, 5.5, D - t - 0.4):
        p.box(mx0 + 0.05, y - 0.12, FLOOR, mx0 + 0.3, y + 0.12, MZ - 0.2, C["metal_dark"])
    # Crates on the floor.
    for x, y, s, z in ((-4.5, -7.5, 1.2, 0), (-4.5, -6.2, 1.2, 0), (-4.5, -6.85, 1.2, 1.2), (-1.0, 3.0, 1.4, 0),
                       (-5.2, 6.0, 1.2, 0), (-3.9, 6.0, 1.2, 0), (0.8, -2.0, 1.0, 0)):
        p.box(x - s / 2, y - s / 2, FLOOR + z, x + s / 2, y + s / 2, FLOOR + z + s, C["wood_light"])
    p.box(-W, -D, H, W, D, H + 0.25, C["sheet"])
    # Skylights.
    for y in (-6.0, 0.0, 6.0):
        p.box(-2.0, y - 1.2, H + 0.25, 2.0, y + 1.2, H + 0.4, C["window"])
    ring(p, -W - 0.1, -D - 0.1, W + 0.1, D + 0.1, 0.1, H, H + 0.15, C["metal_dark"])
    return p


def chapel(C):
    """A stone chapel with a bell tower at the front. The belfry is reached
    with a jump pad at the foot of the tower: it throws you up the open
    half of the tower and onto the floor of the belfry."""
    p = Prop("chapel")
    W, D0, D1, H = 4.0, -6.0, 8.0, 5.4
    t = 0.3
    p.box(-W - 0.2, D0 - 0.2, -1.0, W + 0.2, D1 + 0.2, FLOOR, C["stone"])
    tall = lambda c: (c, 0.9, 1.9, 4.3)
    walls_around(p, -W, D0, W, D1, t, FLOOR, H, C["plaster"], {
        "front": [(0.0, 1.6, FLOOR, 2.8)],
        "back": [door(2.5)],
        "left": [tall(-3.0), tall(0.5), tall(4.0)],
        "right": [tall(-3.0), tall(0.5), tall(4.0)],
    }, frame=C["stone_light"])
    p.box(-W, D0, H, W, D1, H + 0.2, C["plaster"])
    gable_roof(p, -W, D0, W, D1, H + 0.2, H + 3.2, C["roof_red"], C["plaster"], along="y", over=0.3)
    # Pews: you can step up onto the seats.
    for y in range(-3, 6, 2):
        for sx in (-1, 1):
            x0, x1 = sorted((sx * 0.8, sx * 3.3))
            p.box(x0, y - 0.25, FLOOR, x1, y + 0.25, FLOOR + 0.45, C["wood"])
            p.box(x0, y + 0.2, FLOOR + 0.45, x1, y + 0.3, FLOOR + 1.0, C["wood"], False)
    # The tower, in front of the nave, with stairs up its four walls to the belfry.
    TW, TY0, TY1 = 3.0, D0 - 6.0, D0
    TT = 0.4
    BELFRY = 10.0
    TOP = 15.0
    arch = lambda c: (c, 2.0, BELFRY + 1.0, BELFRY + 3.6)
    p.box(-TW - 0.2, TY0 - 0.2, -1.0, TW + 0.2, TY1, FLOOR, C["stone"])
    wall(p, "x", -TW, TW, TY0, TY0 + TT, FLOOR, TOP, C["stone_light"], [(0.0, 1.6, FLOOR, 2.8), arch(0.0)], C["stone"])
    wall(p, "y", TY0 + TT, TY1, -TW, -TW + TT, FLOOR, TOP, C["stone_light"], [arch((TY0 + TY1) / 2)], C["stone"])
    wall(p, "y", TY0 + TT, TY1, TW - TT, TW, FLOOR, TOP, C["stone_light"], [arch((TY0 + TY1) / 2)], C["stone"])
    # Above the nave roof the tower has its own back wall, with an arch.
    wall(p, "x", -TW + TT, TW - TT, TY1 - TT, TY1, H + 0.2, TOP, C["stone_light"], [arch(0.0)], C["stone"])
    ix0, ix1 = -TW + TT, TW - TT
    iy0, iy1 = TY0 + TT, TY1 - TT
    S = 1.2
    rise = (BELFRY - FLOOR) / 30
    z = FLOOR
    wood = C["wood_light"]

    def step(x0, y0, x1, y1, top, collide=True):
        return p.box(x0, y0, max(FLOOR, top - 0.6), x1, y1, top, wood, collide)

    # Up the left wall (+y), along the back (+x), back down the length of
    # the right wall (-y), along the front (-x), turning on a landing in
    # each corner and coming out on the belfry floor over the first one.
    ly, lx = iy1 - iy0 - 2 * S, ix1 - ix0 - 2 * S
    flights = [
        (8, lambda k, n: (ix0, iy0 + S + ly * k / n, ix0 + S, iy0 + S + ly * (k + 1) / n)),
        (8, lambda k, n: (ix0 + S + lx * k / n, iy1 - S, ix0 + S + lx * (k + 1) / n, iy1)),
        (7, lambda k, n: (ix1 - S, iy1 - S - ly * (k + 1) / n, ix1, iy1 - S - ly * k / n)),
        (7, lambda k, n: (ix1 - S - lx * (k + 1) / n, iy0, ix1 - S - lx * k / n, iy0 + S)),
    ]
    landings = [(ix0, iy1 - S, ix0 + S, iy1), (ix1 - S, iy1 - S, ix1, iy1), (ix1 - S, iy0, ix1, iy0 + S)]
    for index, (n, at) in enumerate(flights):
        steps = []
        for k in range(n):
            z += rise
            steps.append(step(*at(k, n), z, False))
        flight(p, steps)
        if index < len(landings):
            step(*landings[index], z)
    # The belfry floor, open over the last flight and the landing it starts
    # from (your head would hit it there).
    p.box(ix0, iy0 + S, BELFRY - 0.3, ix1, iy1, BELFRY, C["wood"])
    p.box(ix0, iy0, BELFRY - 0.3, ix0 + S, iy0 + S, BELFRY, C["wood"])
    p.box(ix0, iy0, TOP, ix1, iy1, TOP + 0.3, C["stone"])
    # A bell hanging over the floor.
    p.cylinder(0.25, 0.55, TOP - 1.5, TOP - 0.5, C["metal"], 10, x=0.0, y=(iy0 + iy1) / 2 + 0.6)
    p.mark("belfry", 0.0, (iy0 + iy1) / 2 + 0.6, BELFRY)
    # Spire.
    p.box(-TW - 0.1, TY0 - 0.1, TOP + 0.3, TW + 0.1, TY1 + 0.1, TOP + 0.5, C["stone"])
    cx, cy = 0.0, (TY0 + TY1) / 2
    apex = Vector((cx, cy, TOP + 5.0))
    base = [Vector((x, y, TOP + 0.5)) for x, y in ((-TW, TY0), (TW, TY0), (TW, TY1), (-TW, TY1))]
    for i in range(4):
        p.poly([base[i], base[(i + 1) % 4], apex], C["roof_dark"])
    p.solid(base + [apex])
    # A cross on the spire.
    p.box(-0.06, cy - 0.06, TOP + 5.0, 0.06, cy + 0.06, TOP + 6.0, C["metal_dark"], False)
    p.box(-0.35, cy - 0.06, TOP + 5.55, 0.35, cy + 0.06, TOP + 5.67, C["metal_dark"], False)
    return p


def ruin(C):
    """A house with its roof gone and its walls broken off at odd heights."""
    p = Prop("ruin")
    rng = random.Random(7)
    W, D, H = 4.5, 3.5, 3.2
    t = 0.3
    p.box(-W - 0.2, -D - 0.2, -1.0, W + 0.2, D + 0.2, FLOOR, C["stone"])

    def broken(axis, start, end, t0, t1, gaps):
        pos = start
        while pos < end - 1e-3:
            step = min(rng.uniform(0.6, 1.3), end - pos)
            mid = pos + step / 2
            if not any(a <= mid <= b for a, b in gaps):
                top = rng.choice((0.9, 1.2, 1.7, 2.3, 2.8, H, H))
                if axis == "x":
                    p.box(pos, t0, FLOOR, pos + step, t1, top, C["brick"])
                else:
                    p.box(t0, pos, FLOOR, t1, pos + step, top, C["brick"])
            pos += step

    broken("x", -W, W, -D, -D + t, [(-1.2, 0.2), (2.0, 3.0)])
    broken("x", -W, W, D - t, D, [(1.0, 2.6)])
    broken("y", -D + t, D - t, -W, -W + t, [(-0.4, 0.6)])
    broken("y", -D + t, D - t, W - t, W, [(-2.0, -0.6), (1.5, 2.5)])
    # Rubble.
    for _ in range(9):
        x, y = rng.uniform(-W + 0.8, W - 0.8), rng.uniform(-D + 0.8, D - 0.8)
        s = rng.uniform(0.3, 0.7)
        p.blob((x, y, FLOOR + s * 0.3), (s * 1.3, s, s * 0.6), C["brick"] if rng.random() < 0.5 else C["stone"], rng, 0.3, 1)
    # A fallen beam and a slab leaning on the wall.
    p.beam((-3.6, -2.4, FLOOR + 0.1), (0.5, 1.8, 1.9), 0.25, 0.25, C["wood"])
    p.box(1.4, -1.2, FLOOR, 3.6, 0.8, FLOOR + 0.45, C["stone"])
    return p


def bunker(C):
    """Low and concrete, with firing slits and a way in at the back."""
    p = Prop("bunker")
    W, D, H = 3.6, 2.6, 2.4
    t = 0.5
    p.box(-W - 0.3, -D - 0.3, -1.0, W + 0.3, D + 0.3, FLOOR, C["concrete_dark"])
    slit = lambda c, w: (c, w, 1.3, 1.75)
    walls_around(p, -W, -D, W, D, t, FLOOR, H, C["concrete"], {
        "front": [slit(0.0, 4.2)],
        "back": [door(-1.8, 1.2)],
        "left": [slit(0.0, 2.0)],
        "right": [slit(0.0, 2.0)],
    })
    p.box(-W - 0.3, -D - 0.3, H, W + 0.3, D + 0.3, H + 0.45, C["concrete"])
    # A blast wall in front of the door.
    p.box(-3.4, D + 1.0, FLOOR - 0.15, -0.6, D + 1.4, 1.9, C["concrete"])
    # Sandbags on the roof.
    for x in (-2.8, -1.6, -0.4, 0.8, 2.0, 3.2):
        p.box(x - 0.55, -D - 0.2, H + 0.45, x + 0.55, -D + 0.35, H + 0.75, C["sand"], False)
    p.collide(-W - 0.3, -D - 0.2, H + 0.45, W + 0.3, -D + 0.35, H + 0.75)
    return p


def shed(C):
    p = Prop("shed")
    W, D, H = 2.0, 1.5, 2.5
    t = 0.12
    p.box(-W - 0.1, -D - 0.1, -0.6, W + 0.1, D + 0.1, FLOOR, C["concrete_dark"])
    walls_around(p, -W, -D, W, D, t, FLOOR, H, C["wood_grey"], {
        "front": [door(-0.8, 1.1)],
        "right": [(0.0, 0.8, 1.2, 1.9)],
    }, frame=C["wood"])
    p.box(-W, -D, H, W, D, H + 0.1, C["wood_grey"])
    gable_roof(p, -W, -D, W, D, H + 0.1, H + 1.1, C["sheet_rust"], C["wood_grey"], along="x", over=0.3)
    return p


def tent(C):
    """A command tent for the team bases."""
    p = Prop("tent")
    W, D, H, R = 3.0, 2.2, 1.6, 3.2
    p.prism([(-D, 0.0), (D, 0.0), (D, H), (0.0, R), (-D, H)], "x", -W, W, C["olive"])
    p.collide(-W, -D, 0.0, W, D, H)
    p.ramp((-W, 0.0, R), (W, 0.0, R), H, D / (R - H), D / (R - H))
    for x in (-W, 0.0, W):
        p.box(x - 0.05, -0.05, 0.0, x + 0.05, 0.05, R + 0.25, C["wood"], False)
    # Guy lines' pegs and a crate of radios outside.
    p.box(-W - 1.2, -D - 0.6, 0.0, -W - 0.2, -D + 0.4, 0.8, C["olive"])
    return p


def radio_mast(C):
    """A tall lattice mast, red and white, landmark on a hilltop."""
    p = Prop("radio_mast")
    H = 26.0
    base = 1.6
    legs = [(-1, -1), (1, -1), (1, 1), (-1, 1)]
    def at(sx, sy, z):
        s = base * (1.0 - z / H * 0.8)
        return Vector((sx * s, sy * s, z))
    bands = 8
    for k in range(bands):
        z0, z1 = H * k / bands, H * (k + 1) / bands
        m = C["red"] if k % 2 == 0 else C["white"]
        for i, (sx, sy) in enumerate(legs):
            p.beam(at(sx, sy, z0), at(sx, sy, z1), 0.14, 0.14, m)
            nx, ny = legs[(i + 1) % 4]
            p.beam(at(sx, sy, z0), at(nx, ny, z1), 0.06, 0.06, C["metal"])
            p.beam(at(sx, sy, z0), at(nx, ny, z0), 0.07, 0.07, C["metal"])
    p.cylinder(0.05, 0.03, H, H + 4.0, C["metal_dark"], 6)
    p.blob((0, 0, H + 4.1), (0.15, 0.15, 0.15), C["lamp"], random.Random(1), 0.0, 1)
    for sx, sy in legs:
        x, y = sx * base, sy * base
        p.box(x - 0.4, y - 0.4, -0.5, x + 0.4, y + 0.4, 0.4, C["concrete"])
        p.collide(x - 0.15, y - 0.15, 0.4, x + 0.15, y + 0.15, 6.0)
    # An equipment hut beside it.
    p.box(2.6, -1.2, -0.5, 5.0, 1.2, 2.4, C["concrete"])
    p.box(2.5, -1.3, 2.4, 5.1, 1.3, 2.55, C["metal_dark"])
    return p


def smokestack(C):
    p = Prop("smokestack")
    H = 22.0
    p.box(-1.6, -1.6, -0.6, 1.6, 1.6, 1.2, C["concrete"])
    p.cylinder(1.25, 0.85, 1.2, H, C["brick"], 12)
    for z in (6.0, 14.0, H - 1.0):
        r = 1.25 - (1.25 - 0.85) * (z - 1.2) / (H - 1.2) + 0.08
        p.cylinder(r, r, z, z + 0.4, C["concrete"], 12)
    p.collide(-0.9, -0.9, 1.2, 0.9, 0.9, H)
    return p


def silo(C):
    p = Prop("silo")
    p.box(-2.2, -2.2, -0.6, 2.2, 2.2, 0.3, C["concrete"])
    p.cylinder(2.0, 2.0, 0.3, 10.0, C["metal"], 14)
    p.cylinder(2.0, 0.3, 10.0, 11.6, C["sheet_rust"], 14)
    for z in (2.5, 5.0, 7.5):
        p.cylinder(2.05, 2.05, z, z + 0.15, C["metal_dark"], 14)
    p.collide(-1.45, -1.45, 0.3, 1.45, 1.45, 11.0)
    p.collide(-2.0, -0.6, 0.3, 2.0, 0.6, 10.0)
    p.collide(-0.6, -2.0, 0.3, 0.6, 2.0, 10.0)
    return p


def bridge(C):
    """A concrete road bridge, 16 m long along Y, deck on top at 0.3 m."""
    p = Prop("bridge")
    L, W = 8.0, 3.6
    p.box(-W, -L, -0.3, W, L, 0.3, C["concrete"])
    for sx in (-1, 1):
        x0, x1 = sorted((sx * W, sx * (W - 0.3)))
        p.box(x0, -L, 0.3, x1, L, 1.1, C["concrete"])
        p.box(x0 - 0.05, -L, 1.1, x1 + 0.05, L, 1.2, C["concrete_dark"], False)
    for y in (-3.5, 3.5):
        p.box(-W + 0.4, y - 0.5, -4.0, W - 0.4, y + 0.5, -0.3, C["concrete_dark"])
    return p


# ---------------------------------------------------------------- props


def sandbags(C):
    p = Prop("sandbags")
    L, rows = 1.5, 3
    for r in range(rows):
        off = 0.25 if r % 2 else 0.0
        x = -L + off
        z0 = r * 0.3
        while x < L - 0.2:
            x1 = min(x + 0.55, L)
            p.box(x + 0.02, -0.33, z0, x1 - 0.02, 0.33, z0 + 0.3, C["sand"], False)
            x = x1
    p.collide(-L, -0.35, 0.0, L, 0.35, rows * 0.3)
    return p


def barrier(C):
    p = Prop("barrier")
    p.prism([(-0.32, 0.0), (0.32, 0.0), (0.3, 0.12), (0.12, 0.35), (0.1, 0.85), (-0.1, 0.85), (-0.12, 0.35), (-0.3, 0.12)],
            "x", -1.5, 1.5, C["concrete"])
    p.collide(-1.5, -0.3, 0.0, 1.5, 0.3, 0.85)
    return p


def crate(C, name="crate", s=1.2):
    p = Prop(name)
    h = s / 2
    p.box(-h, -h, 0.0, h, h, s, C["wood_light"])
    e = 0.08
    for z in (0.0, s - e + 0.01):
        for y in (-h - 0.01, h - e + 0.01):
            p.box(-h - 0.01, y, z, h + 0.01, y + e, z + e, C["wood"], False)
        for x in (-h - 0.01, h - e + 0.01):
            p.box(x, -h - 0.01, z, x + e, h + 0.01, z + e, C["wood"], False)
    for x in (-h - 0.01, h - e + 0.01):
        for y in (-h - 0.01, h - e + 0.01):
            p.box(x, y, 0.0, x + e, y + e, s + 0.01, C["wood"], False)
    return p


def crates(C):
    p = Prop("crates")
    for x, y, z, s in ((-0.65, 0.0, 0.0, 1.2), (0.65, 0.1, 0.0, 1.2), (0.0, 0.05, 1.2, 1.2), (1.7, -0.6, 0.0, 0.8)):
        h = s / 2
        p.box(x - h, y - h, z, x + h, y + h, z + s, C["wood_light"])
        p.box(x - h - 0.01, y - h - 0.01, z + s * 0.45, x + h + 0.01, y + h + 0.01, z + s * 0.55, C["wood"], False)
    return p


def barrels(C):
    p = Prop("barrels")
    for x, y, m in ((-0.35, 0.0, C["red"]), (0.35, 0.1, C["olive"]), (0.0, 0.62, C["metal"])):
        p.cylinder(0.3, 0.3, 0.0, 0.9, m, 10, x=x, y=y)
        p.cylinder(0.31, 0.31, 0.25, 0.3, C["metal_dark"], 10, x=x, y=y)
        p.cylinder(0.31, 0.31, 0.6, 0.65, C["metal_dark"], 10, x=x, y=y)
        p.collide(x - 0.27, y - 0.27, 0.0, x + 0.27, y + 0.27, 0.9)
    return p


def fence(C):
    """A wooden fence 4 m long along X."""
    p = Prop("fence")
    for x in (-2.0, 0.0, 2.0):
        p.box(x - 0.06, -0.06, -0.3, x + 0.06, 0.06, 1.15, C["wood"], False)
    for z in (0.4, 0.85):
        p.box(-2.0, -0.03, z, 2.0, 0.03, z + 0.14, C["wood_grey"], False)
    p.collide(-2.0, -0.08, 0.0, 2.0, 0.08, 1.1)
    return p


def haybale(C):
    p = Prop("haybale")
    rot = Matrix.Translation((0, 0, 0.7)) @ Matrix.Rotation(math.pi / 2, 4, "Y")
    p.cylinder(0.7, 0.7, -0.65, 0.65, C["hay"], 12, matrix=rot)
    p.collide(-0.65, -0.6, 0.0, 0.65, 0.6, 1.3)
    return p


def car(C):
    """A burnt-out car, 4.2 m long along X."""
    p = Prop("car")
    body = C["rust"]
    p.box(-2.1, -0.85, 0.3, 2.1, 0.85, 0.95, body)
    # Cabin: narrower at the top.
    p.hull([Vector(v) for v in (
        (-1.0, -0.8, 0.95), (1.2, -0.8, 0.95), (-1.0, 0.8, 0.95), (1.2, 0.8, 0.95),
        (-0.5, -0.65, 1.45), (0.8, -0.65, 1.45), (-0.5, 0.65, 1.45), (0.8, 0.65, 1.45),
    )], C["metal_dark"])
    p.collide(-1.0, -0.8, 0.95, 1.2, 0.8, 1.45)
    for x in (-1.35, 1.35):
        for y in (-0.8, 0.8):
            m = Matrix.Translation((x, y, 0.33)) @ Matrix.Rotation(math.pi / 2, 4, "X")
            p.cylinder(0.33, 0.33, -0.12, 0.12, C["tyre"], 10, matrix=m)
    p.collide(-2.1, -0.85, 0.0, 2.1, 0.85, 0.95)
    return p


def container(C):
    p = Prop("container")
    L, W, H = 3.0, 1.2, 2.6
    p.box(-L, -W, 0.0, L, W, H, C["roof_green"])
    for k in range(-14, 15):
        x = k * 0.2
        for y in (-W - 0.04, W):
            p.box(x - 0.04, y, 0.1, x + 0.04, y + 0.04, H - 0.1, C["roof_green"], False)
    for x in (-L - 0.03, L - 0.05):
        p.box(x, -W - 0.06, 0.0, x + 0.08, W + 0.06, H + 0.03, C["metal_dark"], False)
    return p


def flagpole(C):
    p = Prop("flagpole")
    p.box(-0.6, -0.6, -0.4, 0.6, 0.6, 0.3, C["concrete"])
    p.cylinder(0.07, 0.05, 0.3, 9.0, C["metal"], 8)
    p.blob((0, 0, 9.05), (0.12, 0.12, 0.12), C["metal"], random.Random(2), 0.0, 1)
    p.collide(-0.1, -0.1, 0.3, 0.1, 0.1, 9.0)
    p.mark("top", 0.0, 0.0, 8.9)
    return p


def power_pole(C):
    p = Prop("power_pole")
    p.cylinder(0.13, 0.1, -0.5, 8.5, C["wood"], 8)
    p.box(-1.1, -0.06, 7.6, 1.1, 0.06, 7.75, C["wood"], False)
    for x in (-0.9, 0.0, 0.9):
        p.cylinder(0.05, 0.05, 7.75, 7.95, C["white"], 6, x=x)
    p.collide(-0.12, -0.12, 0.0, 0.12, 0.12, 8.0)
    return p


def lamp_post(C):
    p = Prop("lamp_post")
    p.cylinder(0.08, 0.06, 0.0, 4.6, C["metal_dark"], 8)
    p.beam((0, 0, 4.5), (0.9, 0, 4.7), 0.08, 0.08, C["metal_dark"])
    p.box(0.7, -0.15, 4.45, 1.1, 0.15, 4.65, C["lamp"], False)
    p.collide(-0.08, -0.08, 0.0, 0.08, 0.08, 4.6)
    return p


def well(C):
    p = Prop("well")
    p.cylinder(0.9, 0.9, 0.0, 0.8, C["stone"], 10)
    p.cylinder(0.7, 0.7, 0.8, 0.81, C["window"], 10)
    for x in (-0.8, 0.8):
        p.box(x - 0.07, -0.07, 0.8, x + 0.07, 0.07, 2.1, C["wood"], False)
    p.prism([(-1.1, 2.0), (1.1, 2.0), (0.0, 2.6)], "y", -0.6, 0.6, C["roof_red"])
    p.collide(-0.8, -0.8, 0.0, 0.8, 0.8, 0.8)
    return p


# ---------------------------------------------------------------- nature


def pine(name, height, seed, C):
    p = Prop(name)
    rng = random.Random(seed)
    trunk = 0.18 + height * 0.012
    p.cylinder(trunk, trunk * 0.5, -0.4, height * 0.75, C["bark"], 7)
    layers = 4 if height > 8 else 3
    for k in range(layers):
        frac = k / layers
        z0 = height * (0.22 + 0.58 * frac)
        r = height * 0.26 * (1.0 - frac * 0.7) * rng.uniform(0.9, 1.1)
        z1 = z0 + height * 0.36
        verts = p.cylinder(r, 0.0, z0, z1, C["pine"] if k % 2 == 0 else C["pine_dark"], 8,
                           matrix=Matrix.Translation((rng.uniform(-0.1, 0.1), rng.uniform(-0.1, 0.1), (z0 + z1) / 2))
                           @ Matrix.Rotation(rng.uniform(0, math.tau), 4, "Z"))
        for v in verts:
            if v.co.z < z1 - 0.1:
                v.co.x += rng.uniform(-0.25, 0.25)
                v.co.y += rng.uniform(-0.25, 0.25)
                v.co.z += rng.uniform(-0.2, 0.15)
    p.collide(-trunk * 0.9, -trunk * 0.9, 0.0, trunk * 0.9, trunk * 0.9, height * 0.6)
    return p


def broadleaf(name, height, seed, trunk_m, leaf_ms, C, slender=False):
    p = Prop(name)
    rng = random.Random(seed)
    trunk = (0.14 if slender else 0.26) + height * 0.01
    crown = height * 0.45
    p.cylinder(trunk, trunk * 0.65, -0.4, height * 0.6, trunk_m, 7)
    # A few branches up into the crown.
    for k in range(3):
        a = k * math.tau / 3 + rng.uniform(-0.4, 0.4)
        p.beam((0, 0, height * 0.45), (math.cos(a) * crown * 0.25, math.sin(a) * crown * 0.25, height * 0.66),
               trunk * 0.7, trunk * 0.7, trunk_m)
    blobs = 4 if slender else 5
    for k in range(blobs):
        a = k * math.tau / blobs + rng.uniform(-0.3, 0.3)
        d = crown * (0.15 if k == 0 else 0.45) * rng.uniform(0.8, 1.1)
        cx, cy = math.cos(a) * d, math.sin(a) * d
        cz = height * (0.72 + rng.uniform(-0.06, 0.08)) if k else height * 0.82
        s = crown * rng.uniform(0.45, 0.6) * (0.75 if slender else 1.0)
        p.blob((cx, cy, cz), (s, s, s * (1.1 if slender else 0.8)), leaf_ms[k % len(leaf_ms)], rng, 0.12, 2)
    p.collide(-trunk * 0.9, -trunk * 0.9, 0.0, trunk * 0.9, trunk * 0.9, height * 0.55)
    return p


def bush(name, seed, C):
    p = Prop(name)
    rng = random.Random(seed)
    for k in range(4):
        a = k * math.tau / 4 + rng.uniform(-0.5, 0.5)
        d = rng.uniform(0.2, 0.6)
        s = rng.uniform(0.55, 0.85)
        p.blob((math.cos(a) * d, math.sin(a) * d, s * 0.55), (s, s, s * 0.75), C["bush"] if k % 2 else C["leaf"], rng, 0.25, 1)
    # No collision: bushes hide you but stop nothing.
    return p


def rock(name, size, seed, C, layers=3, flat=False, detail=2):
    """A lumpy boulder sunk a little into the ground. It collides as the
    hull of its furthest points in 42 directions, all round."""
    p = Prop(name)
    rng = random.Random(seed)
    sx, sy, sz = size
    out = bmesh.ops.create_icosphere(p.bm, subdivisions=detail, radius=1.0)
    verts = out["verts"]
    bumps = [(Vector((rng.uniform(-1, 1), rng.uniform(-1, 1), rng.uniform(-0.3, 1))).normalized(), rng.uniform(0.1, 0.25))
             for _ in range(7)]
    for v in verts:
        n = v.co.normalized()
        k = 1.0
        for direction, strength in bumps:
            k += strength * max(0.0, n.dot(direction)) ** 3
        k += rng.uniform(-0.05, 0.05)
        # Flatten the top a little and the bottom a lot.
        z = n.z
        if z < -0.2:
            z = -0.2 + (z + 0.2) * 0.3
        v.co = Vector((n.x * sx * k, n.y * sy * k, (z * sz * k) + sz * 0.25))
        if flat and v.co.z > sz * 0.9:
            v.co.z = sz * 0.9 + (v.co.z - sz * 0.9) * 0.2
    index = p.mat(C["rock"])
    moss = p.mat(C["moss"])
    dark = p.mat(C["rock_dark"])
    faces = list({f for v in verts for f in v.link_faces})
    for f in faces:
        c = f.calc_center_median()
        f.material_index = moss if f.normal.z > 0.75 and c.z > sz * 0.6 and rng.random() < 0.6 else (dark if rng.random() < 0.25 else index)
    # Few enough corners to collide with quickly.
    ball = bmesh.new()
    bmesh.ops.create_icosphere(ball, subdivisions=2, radius=1.0)
    directions = [v.co.copy() for v in ball.verts]
    ball.free()
    p.solid({max(verts, key=lambda v: v.co.dot(d)).co.to_tuple() for d in directions})
    return p


# ---------------------------------------------------------------- output


def finish(prop):
    mesh = bpy.data.meshes.new(prop.name)
    bm = prop.bm
    # Every piece is its own closed shape: point all their faces outwards.
    bmesh.ops.recalc_face_normals(bm, faces=bm.faces[:])
    bm.normal_update()
    bm.to_mesh(mesh)
    bm.free()
    for m in prop.mats:
        mesh.materials.append(m)
    for poly in mesh.polygons:
        poly.use_smooth = False
    obj = bpy.data.objects.new(prop.name, mesh)
    bpy.context.scene.collection.objects.link(obj)
    return obj


def export(obj):
    bpy.ops.object.select_all(action="DESELECT")
    obj.select_set(True)
    bpy.context.view_layer.objects.active = obj
    saved = obj.location.copy()
    obj.location = (0, 0, 0)
    bpy.ops.export_scene.gltf(
        filepath=f"{ROOT}/assets/models/map/{obj.name}.glb",
        export_format="GLB",
        export_yup=True,
        export_apply=True,
        use_selection=True,
        export_materials="EXPORT",
    )
    obj.location = saved


def game(x, y, z):
    """Blender (Z up, front -Y) to the game (Y up): x, z, -y."""
    return (x, z, -y)


def rust_source(props):
    def f(v):
        s = f"{v:.3f}".rstrip("0")
        return s + "0" if s.endswith(".") else s

    out = [
        "//! Generated by `art/map_props.py`: do not edit by hand. Every model the",
        "//! outdoor maps are dressed with, its collision boxes and marked points,",
        "//! in the model's own space (metres, Y up, standing on the origin).",
        "",
        "// Not every model is placed on a map, and a corner may land near a",
        "// constant like 1/pi.",
        "#![allow(dead_code, clippy::approx_constant)]",
        "",
        "/// A convex solid: its corners, and its faces as indices into them.",
        "pub struct Solid {",
        "    pub corners: &'static [[f32; 3]],",
        "    pub faces: &'static [&'static [u8]],",
        "}",
        "",
        "pub struct PropModel {",
        "    pub name: &'static str,",
        "    /// Under `assets/`.",
        "    pub path: &'static str,",
        "    /// Boxes as [min x, min y, min z, max x, max y, max z].",
        "    pub colliders: &'static [[f32; 6]],",
        "    /// Roofs, rocks and stairs: smooth to walk on, where boxes would be",
        "    /// steps.",
        "    pub solids: &'static [Solid],",
        "    /// Named spots on the model, like where a jump pad goes.",
        "    pub points: &'static [(&'static str, [f32; 3])],",
        "}",
        "",
    ]
    for i, prop in enumerate(props):
        out.append(f"pub const {prop.name.upper()}: usize = {i};")
    out.append("")
    out.append(f"pub const PROPS: [PropModel; {len(props)}] = [")
    for prop in props:
        out.append("    PropModel {")
        out.append(f"        name: \"{prop.name}\",")
        out.append(f"        path: \"models/map/{prop.name}.glb\",")
        out.append("        colliders: &[")
        for x0, y0, z0, x1, y1, z1 in prop.colliders:
            a = game(x0, y1, z0)
            b = game(x1, y0, z1)
            out.append("            [" + ", ".join(f(v) for v in (*a, *b)) + "],")
        out.append("        ],")
        if prop.solids:
            out.append("        solids: &[")
            for corners, faces in prop.solids:
                out.append("            Solid {")
                out.append("                corners: &[")
                for c in corners:
                    out.append("                    [" + ", ".join(f(v) for v in game(*c)) + "],")
                out.append("                ],")
                out.append("                faces: &[" + ", ".join("&[" + ", ".join(map(str, face)) + "]" for face in faces) + "],")
                out.append("            },")
            out.append("        ],")
        else:
            out.append("        solids: &[],")
        if prop.points:
            out.append("        points: &[")
            for name, (x, y, z) in prop.points:
                out.append(f"            (\"{name}\", [" + ", ".join(f(v) for v in game(x, y, z)) + "]),")
            out.append("        ],")
        else:
            out.append("        points: &[],")
        out.append("    },")
    out.append("];")
    return "\n".join(out) + "\n"


bpy.ops.wm.read_factory_settings(use_empty=True)
C = palette()
builders = [
    lambda: house("house", C["plaster"], C["roof_red"], C),
    lambda: house("house_warm", C["plaster_warm"], C["roof_dark"], C),
    lambda: house_tall("house_tall", C["plaster_blue"], C),
    lambda: house_tall("house_tall_brick", C["brick"], C),
    lambda: barn(C),
    lambda: warehouse(C),
    lambda: chapel(C),
    lambda: ruin(C),
    lambda: bunker(C),
    lambda: shed(C),
    lambda: tent(C),
    lambda: radio_mast(C),
    lambda: smokestack(C),
    lambda: silo(C),
    lambda: bridge(C),
    lambda: sandbags(C),
    lambda: barrier(C),
    lambda: crate(C),
    lambda: crates(C),
    lambda: barrels(C),
    lambda: fence(C),
    lambda: haybale(C),
    lambda: car(C),
    lambda: container(C),
    lambda: flagpole(C),
    lambda: power_pole(C),
    lambda: lamp_post(C),
    lambda: well(C),
    lambda: pine("pine", 12.0, 1, C),
    lambda: pine("pine_small", 7.0, 2, C),
    lambda: broadleaf("oak", 9.0, 3, C["bark"], [C["leaf"], C["leaf_light"]], C),
    lambda: broadleaf("birch", 8.5, 4, C["bark_birch"], [C["leaf_birch"], C["leaf_light"]], C, slender=True),
    lambda: bush("bush", 5, C),
    lambda: rock("rock_large", (2.2, 1.8, 2.2), 11, C, detail=3),
    lambda: rock("rock_medium", (1.2, 1.0, 1.1), 12, C, layers=2),
    lambda: rock("rock_small", (0.6, 0.5, 0.45), 13, C, layers=1),
    lambda: rock("rock_flat", (2.4, 1.8, 0.8), 14, C, layers=1, flat=True, detail=3),
    lambda: rock("cliff", (5.0, 3.2, 4.5), 15, C, layers=4, detail=3),
]
props = [build() for build in builders]
objects = []
row_x, row_y, row_depth = 0.0, 0.0, 0.0
for prop in props:
    colliders, points, name = prop.colliders, prop.points, prop.name
    obj = finish(prop)
    objects.append(obj)
    export(obj)
    size = obj.dimensions
    if row_x > 90:
        row_x = 0.0
        row_y += row_depth + 4
        row_depth = 0.0
    obj.location = (row_x + size.x / 2, row_y + size.y / 2, 0)
    row_x += size.x + 4
    row_depth = max(row_depth, size.y)
    solids = [len(corners) for corners, _ in prop.solids]
    print("BUILT", name, len(obj.data.vertices), "verts", len(colliders), "boxes", solids, "solid corners", [round(d, 2) for d in size])

bpy.ops.wm.save_as_mainfile(filepath=f"{ROOT}/art/map_props.blend")
with open(f"{ROOT}/src/props.rs", "w", newline="\n") as file:
    file.write(rust_source(props))

# Previews: each group of models lined up and rendered.
scene = bpy.context.scene
world = bpy.data.worlds.new("World")
world.use_nodes = True
world.node_tree.nodes["Background"].inputs["Color"].default_value = (0.55, 0.65, 0.8, 1)
world.node_tree.nodes["Background"].inputs["Strength"].default_value = 0.7
scene.world = world
sun = bpy.data.objects.new("Sun", bpy.data.lights.new("Sun", "SUN"))
sun.data.energy = 3.5
sun.rotation_euler = (math.radians(50), math.radians(10), math.radians(-35))
scene.collection.objects.link(sun)
ground = bpy.data.meshes.new("Ground")
gbm = bmesh.new()
bmesh.ops.create_grid(gbm, x_segments=1, y_segments=1, size=400)
gbm.to_mesh(ground)
gobj = bpy.data.objects.new("Ground", ground)
ground.materials.append(material("GroundPreview", (0.35, 0.45, 0.25)))
scene.collection.objects.link(gobj)
scene.render.engine = "BLENDER_EEVEE"
scene.view_settings.view_transform = "Standard"
scene.render.resolution_x, scene.render.resolution_y = 1600, 900
cam = bpy.data.objects.new("Cam", bpy.data.cameras.new("Cam"))
scene.collection.objects.link(cam)
scene.camera = cam

groups = {
    "buildings": ["house", "house_warm", "house_tall", "house_tall_brick", "barn", "warehouse", "chapel", "ruin", "bunker", "shed"],
    "landmarks": ["tent", "radio_mast", "smokestack", "silo", "bridge", "flagpole", "power_pole", "lamp_post", "well"],
    "props": ["sandbags", "barrier", "crate", "crates", "barrels", "fence", "haybale", "car", "container"],
    "nature": ["pine", "pine_small", "oak", "birch", "bush", "rock_large", "rock_medium", "rock_small", "rock_flat", "cliff"],
}
by_name = {o.name: o for o in objects}
for group, names in groups.items():
    for o in objects:
        o.hide_render = o.name not in names
    x = 0.0
    for n in names:
        o = by_name[n]
        o.location = (x + o.dimensions.x / 2, 0, 0)
        x += o.dimensions.x + 2.0
    width = x
    tallest = max(by_name[n].dimensions.z for n in names)
    cam.data.lens = 35
    dist = max(width * 0.95, tallest * 1.6)
    cam.location = (width / 2, -dist, dist * 0.45)
    cam.rotation_euler = (math.radians(68), 0, 0)
    scene.render.filepath = f"{PREVIEW}/props_{group}.png"
    bpy.ops.render.render(write_still=True)
print("DONE", len(props))
