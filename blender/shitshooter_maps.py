"""ShitShooter map tools for Blender: build a map in Blender, export it as
a .json file into the game's `maps` folder, and play it.

Install: Edit > Preferences > Add-ons > (top right menu) Install from
Disk..., pick this file, tick "ShitShooter map tools". It adds
File > Export > ShitShooter map (.json), and a "ShitShooter" tab in the 3D
view's sidebar (press N) with buttons to export again in one click, check
the map, and add ramps and the markers a course needs.

How a map is read (see blender/README.md for more):

- Every visible mesh is solid. Each loose part of a mesh is one solid
  shape: its convex hull (shrink-wrapped, every dent filled). Build curved
  or bent ramps from several pieces. Faces steeper than about 45 degrees
  are surfed; flatter ones are stood on.
- A mesh's material gives its colour, and, if its name has a texture's name
  in it (panels, slab, concrete, paint, plaster, brick, cinder, stone,
  rock, wood, boards, metal, corrugated, rust, asphalt, ground), that
  texture. An object whose name starts with "clip" is solid but unseen.
- Empties mark the rest, by name (".001" endings are fine):
    spawn          where players start, standing, facing its Y axis
                   (one per player you expect, up to 16)
    start          a box (Cube empty): leaving it starts a time trial's clock
    checkpoint 1   boxes to fly through, in order: 1, 2, 3...
    respawn 1      where a fall after checkpoint 1 puts you, facing its Y
                   axis, moving at its "speed" (custom property, m/s)
    finish         a box: through it and the run is over
    finish spawn   where you wait after finishing a race (else a spawn)
    kill           boxes that count as falling off (else: falling 10 m
                   below the lowest solid)
  A map with a start and a finish is a course (race and time trial); one
  without is played in free for all and team deathmatch.
- Blender's metres are the game's metres; Z is up.
"""

bl_info = {
    "name": "ShitShooter map tools",
    "author": "ShitShooter",
    "version": (1, 0, 0),
    "blender": (4, 2, 0),
    "location": "File > Export > ShitShooter map; 3D View > Sidebar > ShitShooter",
    "description": "Build maps for ShitShooter and export them for the game",
    "category": "Import-Export",
}

import json
import math
import os
import re

import bmesh
import bpy
from bpy.props import FloatProperty, StringProperty
from bpy_extras.io_utils import ExportHelper
from mathutils import Matrix, Vector

FORMAT = 1
TEXTURES = (
    "panels", "slab", "concrete", "paint", "plaster", "brick", "cinder", "stone",
    "rock", "wood", "boards", "metal", "corrugated", "rust", "asphalt", "ground",
)
# A part whose volume is less than this share of its hull's is not convex.
CONVEX = 0.98


# ---------------------------------------------------------------- reading the scene


def game_point(v):
    """Blender (x, y, z), Z up, to the game's (x, y, z), Y up."""
    return [round(v.x, 4), round(v.z, 4), round(-v.y, 4)]


def game_facing(v):
    """A horizontal direction in Blender to the game's (x, z)."""
    return [round(v.x, 4), round(-v.y, 4)]


def to_srgb(c):
    return [round(x * 12.92 if x <= 0.0031308 else 1.055 * x ** (1 / 2.4) - 0.055, 4) for x in c]


def base_name(obj):
    """Its name in lower case, without a ".001" ending, spaces squeezed."""
    name = re.sub(r"\.\d+$", "", obj.name).lower().replace("_", " ")
    return re.sub(r"\s+", " ", name).strip()


def material_look(mat):
    """A material's colour (sRGB) and the texture its name names, if any."""
    if mat is None:
        return [0.6, 0.6, 0.62], None
    color = list(mat.diffuse_color)[:3]
    if mat.node_tree:
        for node in mat.node_tree.nodes:
            if node.type == "BSDF_PRINCIPLED":
                color = list(node.inputs["Base Color"].default_value)[:3]
                break
    name = mat.name.lower()
    texture = next((t for t in TEXTURES if t in name), None)
    return to_srgb(color), texture


def loose_parts(bm):
    """The vertices of each connected piece of a mesh."""
    seen = set()
    parts = []
    for start in bm.verts:
        if start.index in seen:
            continue
        part, stack = [], [start]
        seen.add(start.index)
        while stack:
            v = stack.pop()
            part.append(v)
            for edge in v.link_edges:
                other = edge.other_vert(v)
                if other.index not in seen:
                    seen.add(other.index)
                    stack.append(other)
        parts.append(part)
    return parts


def part_volume(verts):
    """The volume a closed piece encloses (0 if it is not closed)."""
    faces = {f for v in verts for f in v.link_faces}
    volume = 0.0
    for face in faces:
        loop = [v.co for v in face.verts]
        for i in range(1, len(loop) - 1):
            volume += loop[0].dot(loop[i].cross(loop[i + 1])) / 6.0
    return abs(volume)


def hull(points):
    """The convex hull of `points`: its corners, its faces (indices round
    each, coplanar triangles merged), and its volume."""
    bm = bmesh.new()
    for p in points:
        bm.verts.new(p)
    result = bmesh.ops.convex_hull(bm, input=bm.verts[:], use_existing_faces=False)
    bmesh.ops.delete(bm, geom=result["geom_interior"] + result["geom_unused"], context="VERTS")
    bmesh.ops.dissolve_limit(bm, angle_limit=math.radians(0.5), verts=bm.verts[:], edges=bm.edges[:])
    bm.verts.index_update()
    corners = [v.co.copy() for v in bm.verts]
    faces = [[v.index for v in f.verts] for f in bm.faces]
    volume = bm.calc_volume() if bm.faces else 0.0
    bm.free()
    return corners, faces, abs(volume)


def solids(context, problems):
    """Every visible mesh's loose parts, as the game's solids."""
    out = []
    depsgraph = context.evaluated_depsgraph_get()
    for obj in context.scene.objects:
        if obj.type != "MESH" or not obj.visible_get():
            continue
        evaluated = obj.evaluated_get(depsgraph)
        mesh = evaluated.to_mesh()
        bm = bmesh.new()
        bm.from_mesh(mesh)
        bm.transform(obj.matrix_world)
        bm.verts.ensure_lookup_table()
        bm.verts.index_update()
        materials = [slot.material for slot in obj.material_slots]
        visible = not base_name(obj).startswith("clip")
        for n, part in enumerate(loose_parts(bm)):
            label = obj.name if n == 0 else f"{obj.name} (part {n + 1})"
            if len(part) < 4:
                problems.append(f"{label}: too few corners to be solid, left out")
                continue
            corners, faces, volume = hull([v.co for v in part])
            if volume < 1e-4 or len(faces) < 4:
                problems.append(f"{label}: flat, so not solid (give it thickness), left out")
                continue
            if part_volume(part) < volume * CONVEX:
                problems.append(f"{label}: not convex, so it collides as its hull (every dent filled); split it into convex pieces")
            face_mats = [f.material_index for v in part for f in v.link_faces]
            index = max(set(face_mats), key=face_mats.count) if face_mats else 0
            color, texture = material_look(materials[index] if index < len(materials) else None)
            solid = {"corners": [game_point(c) for c in corners], "faces": faces, "color": color}
            if texture:
                solid["texture"] = texture
            if not visible:
                solid["visible"] = False
            out.append(solid)
        bm.free()
        evaluated.to_mesh_clear()
    return out


def facing(obj, problems):
    """Which way an empty faces: its Y axis, flattened."""
    m = obj.matrix_world.to_3x3()
    forward, up = m @ Vector((0, 1, 0)), m @ Vector((0, 0, 1))
    if up.normalized().z < 0.99:
        problems.append(f"{obj.name}: tilted; only its turn about Z counts")
    flat = Vector((forward.x, forward.y, 0))
    if flat.length < 1e-6:
        problems.append(f"{obj.name}: its Y axis points straight up or down; facing -Y")
        flat = Vector((0, -1, 0))
    return game_facing(flat.normalized())


def marker(obj, problems):
    out = {"at": game_point(obj.matrix_world.translation), "facing": facing(obj, problems)}
    speed = obj.get("speed")
    if speed:
        out["speed"] = float(speed)
    return out


def gate(obj, problems):
    if obj.empty_display_type != "CUBE":
        problems.append(f"{obj.name}: shown as a box (Cube empty) it would show its size; its display size and scale are used")
    size = obj.empty_display_size
    scale = obj.matrix_world.to_scale()
    return {
        "center": game_point(obj.matrix_world.translation),
        # Across (X), up (Z), along (Y), in the game's order.
        "half": [round(abs(size * scale.x), 4), round(abs(size * scale.z), 4), round(abs(size * scale.y), 4)],
        "facing": facing(obj, problems),
    }


def inside(point, box):
    """Is a marker's point inside a gate (roughly: within its turned box)?"""
    c, h, f = box["center"], box["half"], box["facing"]
    d = [point[0] - c[0], point[1] - c[1], point[2] - c[2]]
    along = d[0] * f[0] + d[2] * f[1]
    across = d[0] * f[1] - d[2] * f[0]
    return abs(across) <= h[0] and abs(d[1]) <= h[1] + 2 and abs(along) <= h[2]


def read_map(context):
    """The scene as a map file, and what is wrong with it: (map, problems,
    fatal) where fatal problems stop the export."""
    # Every object's place, up to date (a script may just have moved them).
    context.view_layer.update()
    problems, fatal = [], []
    found = {"spawns": [], "checkpoints": {}, "respawns": {}, "kills": []}
    start = finish = finish_spawn = None
    for obj in context.scene.objects:
        if obj.type != "EMPTY" or not obj.visible_get():
            continue
        name = base_name(obj)
        number = re.match(r"(checkpoint|respawn) ?(\d+)$", name)
        if name == "finish spawn":
            finish_spawn = marker(obj, problems)
        elif name.startswith("spawn"):
            found["spawns"].append((obj.name, marker(obj, problems)))
        elif name == "start":
            start = gate(obj, problems)
        elif name == "finish":
            finish = gate(obj, problems)
        elif name.startswith("kill"):
            found["kills"].append(gate(obj, problems))
        elif number and number.group(1) == "checkpoint":
            found["checkpoints"][int(number.group(2))] = gate(obj, problems)
        elif number:
            found["respawns"][int(number.group(2))] = marker(obj, problems)
    found["spawns"].sort(key=lambda item: item[0])
    spawns = [m for _, m in found["spawns"]]

    shapes = solids(context, problems)
    if not shapes:
        fatal.append("no solids: add a mesh")
    if not spawns:
        fatal.append("no spawn: add one (sidebar > ShitShooter > Spawn)")
    if (start is None) != (finish is None):
        fatal.append("a course needs both a start and a finish" if start or finish else "")
    checkpoints = []
    numbers = sorted(found["checkpoints"])
    if numbers and start is None:
        problems.append("checkpoints but no start and finish: not a course, so they do nothing")
    for expected, number in enumerate(numbers, start=1):
        if number != expected:
            fatal.append(f"checkpoint {expected} is missing (they go 1, 2, 3...)")
            break
        if number not in found["respawns"]:
            fatal.append(f"checkpoint {number} has no respawn {number}: where a fall after it puts you")
            continue
        checkpoints.append({"gate": found["checkpoints"][number], "respawn": found["respawns"][number]})
    if start:
        outside = [n for n, m in found["spawns"] if not inside(m["at"], start)]
        if outside:
            problems.append(f"spawns outside the start box (a time trial starts its clock as you leave it): {', '.join(outside)}")
    if 0 < len(spawns) < 16:
        problems.append(f"{len(spawns)} spawns: players beyond that share them")

    world = context.scene.world
    sky = to_srgb(list(world.color)[:3]) if world else None
    data = {"format": FORMAT, "solids": shapes, "spawns": spawns}
    if sky:
        data["sky"] = sky
    if start and finish:
        data.update(start=start, finish=finish, checkpoints=checkpoints)
        if finish_spawn:
            data["finish_spawn"] = finish_spawn
        if found["kills"]:
            data["kills"] = found["kills"]
    return data, problems, [f for f in fatal if f]


def export(context, path):
    """Writes the map to `path`. Returns (ok, lines to tell)."""
    data, problems, fatal = read_map(context)
    if fatal:
        return False, ["Not exported:"] + fatal + problems
    os.makedirs(os.path.dirname(os.path.abspath(path)), exist_ok=True)
    with open(path, "w") as f:
        json.dump(data, f, separators=(",", ":"))
    course = "a course" if "start" in data else "a free-for-all map"
    summary = f"Exported {os.path.basename(path)}: {course}, {len(data['solids'])} solids, {len(data.get('checkpoints', []))} checkpoints"
    return True, [summary] + problems


def tell(operator, ok, lines):
    for line in lines[1:]:
        operator.report({"WARNING"}, line)
    operator.report({"INFO"} if ok else {"ERROR"}, lines[0])
    print("\n".join(lines))


# ---------------------------------------------------------------- operators


class EXPORT_OT_shitshooter_map(bpy.types.Operator, ExportHelper):
    """Export the scene as a ShitShooter map, into the game's maps folder"""

    bl_idname = "export_scene.shitshooter_map"
    bl_label = "Export ShitShooter map"
    filename_ext = ".json"
    filter_glob: StringProperty(default="*.json", options={"HIDDEN"})

    def invoke(self, context, event):
        last = context.scene.get("shitshooter_path")
        if last:
            self.filepath = last
        return ExportHelper.invoke(self, context, event)

    def execute(self, context):
        context.scene["shitshooter_path"] = self.filepath
        ok, lines = export(context, self.filepath)
        tell(self, ok, lines)
        return {"FINISHED"} if ok else {"CANCELLED"}


class SHITSHOOTER_OT_export_again(bpy.types.Operator):
    """Export again to the same file: then start a match in the game"""

    bl_idname = "shitshooter.export_again"
    bl_label = "Export again"

    def execute(self, context):
        path = context.scene.get("shitshooter_path")
        if not path:
            return bpy.ops.export_scene.shitshooter_map("INVOKE_DEFAULT")
        ok, lines = export(context, path)
        tell(self, ok, lines)
        return {"FINISHED"} if ok else {"CANCELLED"}


class SHITSHOOTER_OT_check(bpy.types.Operator):
    """Say what the game would make of this scene, without exporting"""

    bl_idname = "shitshooter.check"
    bl_label = "Check map"

    def execute(self, context):
        data, problems, fatal = read_map(context)
        if fatal:
            tell(self, False, ["Cannot be exported:"] + fatal + problems)
        else:
            course = "a course" if "start" in data else "a free-for-all map"
            tell(self, True, [f"Fine: {course}, {len(data['solids'])} solids, {len(data.get('checkpoints', []))} checkpoints"] + problems)
        return {"FINISHED"}


def new_empty(context, name, kind, size, location):
    obj = bpy.data.objects.new(name, None)
    obj.empty_display_type = kind
    obj.empty_display_size = size
    obj.location = location
    context.collection.objects.link(obj)
    for other in context.selected_objects:
        other.select_set(False)
    obj.select_set(True)
    context.view_layer.objects.active = obj
    return obj


def next_number(context, word):
    taken = {int(m.group(1)) for o in context.scene.objects if (m := re.match(word + r" ?(\d+)$", base_name(o)))}
    n = 1
    while n in taken:
        n += 1
    return n


class SHITSHOOTER_OT_add_marker(bpy.types.Operator):
    """Add a course marker at the 3D cursor"""

    bl_idname = "shitshooter.add_marker"
    bl_label = "Add marker"
    bl_options = {"REGISTER", "UNDO"}
    what: StringProperty()

    def execute(self, context):
        at = context.scene.cursor.location.copy()
        if self.what == "spawn":
            new_empty(context, "spawn", "ARROWS", 1.0, at)
        elif self.what in ("start", "finish", "kill"):
            obj = new_empty(context, self.what, "CUBE", 1.0, at)
            obj.scale = {"start": (6, 6, 2), "finish": (12, 1, 12), "kill": (200, 200, 5)}[self.what]
        elif self.what == "checkpoint":
            n = next_number(context, "checkpoint")
            obj = new_empty(context, f"checkpoint {n}", "CUBE", 1.0, at)
            obj.scale = (12, 1, 12)
            respawn = new_empty(context, f"respawn {n}", "ARROWS", 1.0, at + Vector((0, 4, 0)))
            respawn["speed"] = 0.0
            respawn.id_properties_ui("speed").update(min=0.0, max=60.0, description="How fast a fall puts you back on, m/s, facing its Y axis")
        return {"FINISHED"}


class SHITSHOOTER_OT_add_ramp(bpy.types.Operator):
    """Add a surf ramp at the 3D cursor: a ridge (two faces) or a wall
    (one face, on its -X side), running along Y"""

    bl_idname = "shitshooter.add_ramp"
    bl_label = "Add surf ramp"
    bl_options = {"REGISTER", "UNDO"}
    length: FloatProperty(name="Length", default=60.0, min=1.0, unit="LENGTH")
    height: FloatProperty(name="Face height", default=16.0, min=0.5, unit="LENGTH")
    slope: FloatProperty(name="Slope", default=math.radians(55.0), min=math.radians(46.0), max=math.radians(85.0), subtype="ANGLE",
                         description="Steeper than about 45 degrees is surfed")
    one_sided: bpy.props.BoolProperty(name="One face (a wall)", default=False)

    def execute(self, context):
        reach = self.height / math.tan(self.slope)
        if self.one_sided:
            section = [(0, self.height), (-reach, 0), (0, 0)]
        else:
            section = [(0, self.height), (-reach, 0), (reach, 0)]
        verts = [(x, y, z) for y in (0.0, self.length) for (x, z) in section]
        faces = [(0, 1, 4, 3), (1, 2, 5, 4), (2, 0, 3, 5), (0, 2, 1), (3, 4, 5)]
        mesh = bpy.data.meshes.new("ramp")
        mesh.from_pydata(verts, [], faces)
        mesh.update()
        obj = bpy.data.objects.new("wall" if self.one_sided else "ramp", mesh)
        obj.location = context.scene.cursor.location
        context.collection.objects.link(obj)
        for other in context.selected_objects:
            other.select_set(False)
        obj.select_set(True)
        context.view_layer.objects.active = obj
        return {"FINISHED"}


class SHITSHOOTER_PT_panel(bpy.types.Panel):
    bl_label = "ShitShooter map"
    bl_idname = "SHITSHOOTER_PT_panel"
    bl_space_type = "VIEW_3D"
    bl_region_type = "UI"
    bl_category = "ShitShooter"

    def draw(self, context):
        layout = self.layout
        path = context.scene.get("shitshooter_path")
        col = layout.column(align=True)
        col.operator("shitshooter.export_again", icon="EXPORT", text="Export again" if path else "Export...")
        if path:
            col.label(text=os.path.basename(path))
        col.operator("export_scene.shitshooter_map", text="Export as...")
        layout.operator("shitshooter.check", icon="CHECKMARK")
        box = layout.box()
        box.label(text="Add at the 3D cursor")
        row = box.row(align=True)
        row.operator("shitshooter.add_ramp", text="Ramp").one_sided = False
        row.operator("shitshooter.add_ramp", text="Wall").one_sided = True
        for what, label in (("spawn", "Spawn"), ("start", "Start"), ("checkpoint", "Checkpoint"), ("finish", "Finish"), ("kill", "Kill zone")):
            box.operator("shitshooter.add_marker", text=label).what = what


def menu_export(self, context):
    self.layout.operator(EXPORT_OT_shitshooter_map.bl_idname, text="ShitShooter map (.json)")


CLASSES = (
    EXPORT_OT_shitshooter_map,
    SHITSHOOTER_OT_export_again,
    SHITSHOOTER_OT_check,
    SHITSHOOTER_OT_add_marker,
    SHITSHOOTER_OT_add_ramp,
    SHITSHOOTER_PT_panel,
)


def register():
    for cls in CLASSES:
        bpy.utils.register_class(cls)
    bpy.types.TOPBAR_MT_file_export.append(menu_export)


def unregister():
    bpy.types.TOPBAR_MT_file_export.remove(menu_export)
    for cls in reversed(CLASSES):
        bpy.utils.unregister_class(cls)


if __name__ == "__main__":
    register()
