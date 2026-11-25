"""Builds the reflex, the 2x and the 3x sight as one family: a short housing
of thin walls round a big window, a trapezoid with rounded corners that flares
a little towards the front, on a low clamp. Little of it gets in the way when
aiming. The reflex is short with a dot in a ring; the 2x is longer, its lens
set back under a hood, with a chevron in a ring; the 3x is the 2x made
bulkier: thicker walls, a bigger window under a deeper hood, a rim round the
eyepiece, turrets on top and on the left, and holdover dots under its
chevron. Metres, Z up, the front towards +Y
(as the guns face, so a gun's mount puts them on as they are), the base on
the origin, like the other sights. Small enough for the pistol.
Saves art/reflex.blend, art/2x.blend and art/3x.blend and exports
assets/models/sights/reflex.glb, 2x.glb and 3x.glb. Run with:
blender --background --factory-startup --python sights.py -- <repo root>
"""
import math
import sys

import bmesh
import bpy
from mathutils import Matrix

ROOT = sys.argv[sys.argv.index("--") + 1]

# The middle of the window, where the aim point is, on both.
AXIS_Z = 0.023
WALL = 0.0015
CORNER = 0.0035
# The top of the window is this much narrower than its bottom.
TOP_NARROWING = 0.82


def material(name, linear, alpha=1.0, roughness=0.5, emission=None, strength=1.0):
    """The same materials as the other sights (colours are linear here)."""
    mat = bpy.data.materials.new(name)
    mat.use_nodes = True
    bsdf = mat.node_tree.nodes["Principled BSDF"]
    bsdf.inputs["Base Color"].default_value = (*linear, 1.0)
    bsdf.inputs["Metallic"].default_value = 0.0
    bsdf.inputs["Roughness"].default_value = roughness
    bsdf.inputs["Alpha"].default_value = alpha
    if emission:
        bsdf.inputs["Emission Color"].default_value = (*emission, 1.0)
        bsdf.inputs["Emission Strength"].default_value = strength
    if alpha < 1.0:
        mat.surface_render_method = "BLENDED"
    mat.diffuse_color = (*linear, alpha)
    return mat


class Builder:
    def __init__(self, name):
        # Nothing else in the file: not the default cube, camera and light.
        bpy.ops.wm.read_factory_settings(use_empty=True)
        self.body = material("Sight_Body", (0.035, 0.036, 0.04))
        self.mark = material("Sight_Dot", (1.0, 0.04, 0.03), roughness=0.45, emission=(1.0, 0.04, 0.03), strength=2.0)
        # Faint, so the window reads as open.
        self.lens = material("Sight_Lens", (0.6, 0.85, 1.0), alpha=0.08, roughness=0.05)
        self.collection = bpy.data.collections.new(name)
        bpy.context.scene.collection.children.link(self.collection)

    def finish(self, name, bm, mat):
        mesh = bpy.data.meshes.new(name)
        bm.to_mesh(mesh)
        bm.free()
        mesh.materials.append(mat)
        obj = bpy.data.objects.new(name, mesh)
        self.collection.objects.link(obj)
        return obj

    def box(self, name, x0, x1, y0, y1, z0, z1):
        bm = bmesh.new()
        bmesh.ops.create_cube(bm, size=1.0, matrix=Matrix.Translation(((x0 + x1) / 2, (y0 + y1) / 2, (z0 + z1) / 2))
                              @ Matrix.Diagonal((x1 - x0, y1 - y0, z1 - z0, 1.0)))
        return self.finish(name, bm, self.body)

    def cylinder_x(self, name, radius, x0, x1, y, z):
        bm = bmesh.new()
        bmesh.ops.create_cone(bm, cap_ends=True, segments=12, radius1=radius, radius2=radius, depth=x1 - x0,
                              matrix=Matrix.Translation(((x0 + x1) / 2, y, z)) @ Matrix.Rotation(math.pi / 2, 4, "Y"))
        return self.finish(name, bm, self.body)

    def cylinder_z(self, name, radius, x, y, z0, z1, segments=16):
        bm = bmesh.new()
        bmesh.ops.create_cone(bm, cap_ends=True, segments=segments, radius1=radius, radius2=radius, depth=z1 - z0,
                              matrix=Matrix.Translation((x, y, (z0 + z1) / 2)))
        return self.finish(name, bm, self.body)

    def housing(self, name, width, height, y_front, y_back, flare, wall=WALL):
        """A tube of walls `wall` thick round a window `width` by `height` at the
        back, `flare` times that at the front, open at both ends."""
        bm = bmesh.new()
        rims = [[bm.verts.new((x, y, z)) for x, z in outline(width * scale, height * scale, CORNER + extra)]
                for extra in (wall, 0.0) for y, scale in ((y_front, flare), (y_back, 1.0))]
        outer_front, outer_back, inner_front, inner_back = rims
        n = len(outer_front)
        for i in range(n):
            j = (i + 1) % n
            # The walls are shaded smooth round the corners, the rims flat.
            bm.faces.new((outer_front[i], outer_front[j], outer_back[j], outer_back[i])).smooth = True
            bm.faces.new((inner_front[j], inner_front[i], inner_back[i], inner_back[j])).smooth = True
            bm.faces.new((outer_front[j], outer_front[i], inner_front[i], inner_front[j]))
            bm.faces.new((outer_back[i], outer_back[j], inner_back[j], inner_back[i]))
        bmesh.ops.recalc_face_normals(bm, faces=bm.faces)
        rim_edges = [bm.edges.get((rim[i], rim[(i + 1) % n])) for rim in rims for i in range(n)]
        bmesh.ops.split_edges(bm, edges=rim_edges)
        return self.finish(name, bm, self.body)

    def glass(self, name, width, height, y, scale):
        """Glass filling a window `width` by `height` scaled by `scale`."""
        bm = bmesh.new()
        bm.faces.new([bm.verts.new((x, y, z)) for x, z in outline(width * scale, height * scale, CORNER)])
        return self.finish(name, bm, self.lens)

    def flat(self, name, points, y):
        """A flat shape of the reticle, facing the eye."""
        bm = bmesh.new()
        bm.faces.new([bm.verts.new((x, y, AXIS_Z + z)) for x, z in points])
        return self.finish(name, bm, self.mark)

    def dot(self, name, radius, y):
        return self.flat(name, circle(radius, 12), y)

    def ring(self, name, outer, inner, y, segments=32):
        bm = bmesh.new()
        rims = [[bm.verts.new((x, y, AXIS_Z + z)) for x, z in circle(radius, segments)] for radius in (outer, inner)]
        for i in range(segments):
            j = (i + 1) % segments
            bm.faces.new((rims[0][i], rims[0][j], rims[1][j], rims[1][i]))
        return self.finish(name, bm, self.mark)

    def aim_point(self, y):
        aim = bpy.data.objects.new("AimPoint", None)
        aim.empty_display_size = 0.005
        aim.location = (0.0, y, AXIS_Z)
        self.collection.objects.link(aim)

    def save(self, file):
        bpy.ops.wm.save_as_mainfile(filepath=f"{ROOT}/art/{file}.blend")
        bpy.ops.export_scene.gltf(
            filepath=f"{ROOT}/assets/models/sights/{file}.glb", export_format="GLB", export_yup=True,
            export_apply=True
        )


def circle(radius, segments):
    return [(radius * math.cos(i * math.tau / segments), radius * math.sin(i * math.tau / segments))
            for i in range(segments)]


def outline(width, height, radius, per_corner=6):
    """The window's outline round the axis, as (x, z): a trapezoid narrower at
    the top, its corners rounded by `radius` about the same centres whatever
    the radius, so a wall is the same thickness all round."""
    bottom, top = width / 2 - CORNER, width * TOP_NARROWING / 2 - CORNER
    low, high = -height / 2 + CORNER, height / 2 - CORNER
    centres = [(bottom, low), (top, high), (-top, high), (-bottom, low)]
    points = []
    for i, (cx, cz) in enumerate(centres):
        before, after = centres[i - 1], centres[(i + 1) % 4]
        # From the outward normal of the edge coming in to that of the edge going out.
        start = math.atan2(-(cx - before[0]), cz - before[1])
        end = math.atan2(-(after[0] - cx), after[1] - cz)
        while end < start:
            end += math.tau
        for k in range(per_corner + 1):
            a = start + (end - start) * k / per_corner
            points.append((cx + radius * math.cos(a), AXIS_Z + cz + radius * math.sin(a)))
    return points


def inside(flare, y_front, y_back, y):
    """How much bigger than at the back the window of a housing is at `y`, less
    a little, so glass there fits inside its walls."""
    return flare - (flare - 1.0) * (y - y_front) / (y_back - y_front) - 0.005


def clamp(b, y0, y1, top):
    """The low clamp on the rail, a bolt through it on the left."""
    b.box("Base", -0.0105, 0.0105, y0, y1, 0.0, top)
    b.box("Clamp", -0.013, -0.0105, y0 + 0.006, y1 - 0.006, 0.0, top * 0.8)
    b.cylinder_x("ClampBolt", 0.0025, -0.016, -0.013, (y0 + y1) / 2, top * 0.45)


# The reflex: short, the glass at the front, a dot in a ring.
b = Builder("Reflex")
width, height = 0.024, 0.019
b.housing("Housing", width, height, 0.014, -0.010, flare=1.15)
clamp(b, -0.010, 0.014, AXIS_Z - height / 2 - WALL + 0.0003)
glass_y = 0.0125
b.glass("Glass", width, height, glass_y, inside(1.15, 0.014, -0.010, glass_y))
mark_y = glass_y - 0.0004
b.dot("Dot", 0.0006, mark_y)
b.ring("Ring", 0.0040, 0.0035, mark_y)
b.aim_point(mark_y)
b.save("reflex")

# The 2x: the same housing, a little bigger and longer, the glass set back
# under a hood, a chevron in a ring, and a low adjuster on the left.
b = Builder("TwoX")
width, height = 0.026, 0.021
b.housing("Housing", width, height, 0.020, -0.012, flare=1.2)
clamp(b, -0.011, 0.017, AXIS_Z - height / 2 - WALL + 0.0003)
glass_y = 0.0135
b.glass("Glass", width, height, glass_y, inside(1.2, 0.020, -0.012, glass_y))
mark_y = glass_y - 0.0004
# Two arms down from the aim point, a small dot at their tip.
arm, thick, spread = 0.0032, 0.00045, math.radians(38)
for side, name in ((-1, "ChevronLeft"), (1, "ChevronRight")):
    sx, sz = side * math.sin(spread), -math.cos(spread)
    nx, nz = -sz * thick / 2, sx * thick / 2
    b.flat(name, [(nx, nz), (-nx, -nz), (sx * arm - nx, sz * arm - nz), (sx * arm + nx, sz * arm + nz)], mark_y)
b.dot("Dot", 0.0005, mark_y)
b.ring("Ring", 0.0052, 0.0047, mark_y)
b.box("Adjuster", -width / 2 - WALL - 0.0025, -width / 2 - WALL + 0.0005, -0.003, 0.006, AXIS_Z - 0.007,
      AXIS_Z - 0.001)
b.aim_point(mark_y)
b.save("2x")

# The 3x: the 2x made bulkier. Thicker walls round a bigger window, the glass
# set further back under a deeper hood, a rim round the eyepiece, an
# elevation turret on top and a windage turret on the left, a longer clamp
# with two bolts, and a chevron in a ring with holdover dots under it.
b = Builder("ThreeX")
width, height, wall = 0.030, 0.024, 0.0026
front, back = 0.026, -0.014
b.housing("Housing", width, height, front, back, flare=1.22, wall=wall)
b.housing("Eyepiece", width + 0.0016, height + 0.0016, back, back - 0.004, flare=1.0, wall=wall)
top = AXIS_Z - height / 2 - wall + 0.0003
b.box("Base", -0.012, 0.012, -0.013, 0.023, 0.0, top)
b.box("Clamp", -0.0148, -0.012, -0.007, 0.017, 0.0, top * 0.8)
for y in (-0.003, 0.013):
    b.cylinder_x(f"ClampBolt{int(y > 0)}", 0.0025, -0.018, -0.0148, y, top * 0.45)
# The turrets: on top, and on the left. The wall there slopes in towards the
# top and out towards the front, so the windage turret's end is set at a
# depth that is inside the wall all over it: past the outside of the wall
# everywhere, so it sits flush, and short of the window everywhere, so it
# does not show through it.
turret_y, turret_radius = 0.004, 0.0048
roof = AXIS_Z + height / 2 + wall
b.cylinder_z("ElevationTurret", turret_radius, 0.0, turret_y, roof - 0.001, roof + 0.0045, segments=20)
b.cylinder_z("ElevationCap", 0.0052, 0.0, turret_y, roof + 0.0045, roof + 0.006, segments=20)


def side_x(y, dz, extra):
    """How far out the side of the 3x's housing is at `y`, `dz` above the
    axis: its window (`extra` 0) or the outside of its wall (`extra` the
    wall), on the straight run between the corners, as `outline` makes it."""
    scale = 1.0 + (1.22 - 1.0) * (y - back) / (front - back)
    bottom, top = width * scale / 2 - CORNER, width * scale * TOP_NARROWING / 2 - CORNER
    low, high = -height * scale / 2 + CORNER, height * scale / 2 - CORNER
    # Along the line through the corners' centres, then out by the radius.
    along = (dz - low) / (high - low)
    centre = bottom + (top - bottom) * along
    slope = math.atan2(top - bottom, high - low)
    return centre + (CORNER + extra) / math.cos(slope)


knob = 0.0042
disc = [(turret_y + knob * math.cos(a), knob * math.sin(a)) for a in (i * math.tau / 24 for i in range(24))]
window = max(side_x(y, dz, 0.0) for y, dz in disc)
outside = min(side_x(y, dz, wall) for y, dz in disc)
assert window < outside, "the windage turret has no room in the wall"
seat = (window + outside) / 2
b.cylinder_x("WindageTurret", knob, -seat - 0.005, -seat, turret_y, AXIS_Z)
b.cylinder_x("WindageCap", knob + 0.0004, -seat - 0.0065, -seat - 0.005, turret_y, AXIS_Z)
glass_y = 0.017
b.glass("Glass", width, height, glass_y, inside(1.22, front, back, glass_y))
mark_y = glass_y - 0.0004
arm, thick, spread = 0.0036, 0.0005, math.radians(38)
for side_sign, name in ((-1, "ChevronLeft"), (1, "ChevronRight")):
    sx, sz = side_sign * math.sin(spread), -math.cos(spread)
    nx, nz = -sz * thick / 2, sx * thick / 2
    b.flat(name, [(nx, nz), (-nx, -nz), (sx * arm - nx, sz * arm - nz), (sx * arm + nx, sz * arm + nz)], mark_y)
b.dot("Dot", 0.0005, mark_y)
for i, below in enumerate((0.0040, 0.0050)):
    b.flat(f"Holdover{i}", [(x, z - below) for x, z in circle(0.00035, 10)], mark_y)
b.ring("Ring", 0.0060, 0.0055, mark_y)
b.aim_point(mark_y)
b.save("3x")
