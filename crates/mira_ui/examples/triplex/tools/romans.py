"""Dresses a scanned modern man as Romans of 49 BC and their Gallic enemies, and renders
them as sprites for a battlefield seen from above at a slant.

    python3 crates/mira_ui/examples/triplex/tools/romans.py . res/triplex/sprites [only=legate,commander] [pitch=35] [quick]

Run from the repository's root with Blender as a Python module (`pip install bpy`), or as
`blender -b --python romans.py -- <mira root> <output dir>`. A sheet takes a minute or two
on a laptop's processor; `quick` renders three directions at low quality, to look at.

The figure is the rigged Rocketbox man the Paris demo dresses (`res/paris/models/citizen.glb`),
and the horses are the Paris horses. For each kind of man this repaints the body texture by
where each texel lies on the body at rest (mail, tunic, skin, sandals), adds the kit (helmet,
shield, pilum, sword, belt, cloak, cuirass), poses him with two-bone reaching for each frame,
and renders every frame from eight directions with an orthographic camera tilted down as the
game's is. Each kind becomes one sprite sheet: a row for each direction, a column for each
frame.

  rows     direction 0 faces the viewer; each row after turns him 45 degrees to his left
           (counterclockwise seen from above): 1 faces down-right on screen, 2 right,
           3 up-right, 4 away, 5 up-left, 6 left, 7 down-left
  columns  infantry: idle, walk 0-7, fight 0-5, dead      (16)
           horsemen: idle, walk 0-7                       (9)

The figure is Microsoft Rocketbox (MIT licence); the horses are CC0 (see res/paris/CREDITS.md).
"""

import json
import math
import os
import shutil
import sys

import bpy  # noqa: I001  (bpy first: it makes bmesh and mathutils importable)
import bmesh
import numpy as np
from mathutils import Matrix, Quaternion, Vector

ARGS = [a for a in sys.argv[1:] if not a.endswith(".py")]
if "--" in sys.argv:
    ARGS = sys.argv[sys.argv.index("--") + 1:]
MIRA = os.path.abspath(ARGS[0])
OUT = os.path.abspath(ARGS[1])
ONLY = None
QUICK = "quick" in ARGS
PITCH_DEGREES = 35.0
for a in ARGS[2:]:
    if a.startswith("only="):
        ONLY = a[5:].split(",")
    if a.startswith("pitch="):
        PITCH_DEGREES = float(a[6:])
os.makedirs(OUT, exist_ok=True)

# ---- How the game sees them ----------------------------------------------------------------

# The game's camera looks down at this angle below the horizon, orthographically.
PITCH = math.radians(PITCH_DEGREES)
# Metres of the world a pixel of the sprite is (across, and up the picture).
PX = 0.0185
# Cells: (width, height, how far up from the bottom the man's feet are), pixels.
FOOT_CELL = (112, 144, 22)
HORSE_CELL = (208, 192, 50)
DIRECTIONS = 8


def srgb(hex_color):
    """A colour written as sRGB hex, in the linear values Blender's materials take."""
    c = np.array([((hex_color >> s) & 255) / 255.0 for s in (16, 8, 0)])
    return np.where(c <= 0.04045, c / 12.92, ((c + 0.055) / 1.055) ** 2.4)


def hexc(hex_color):
    """A colour written as sRGB hex, as the values a picture's pixels hold."""
    return np.array([((hex_color >> s) & 255) / 255.0 for s in (16, 8, 0)])


def update():
    bpy.context.view_layer.update()


# ---- The figure ----------------------------------------------------------------------------

bpy.ops.wm.read_factory_settings(use_empty=True)
bpy.ops.import_scene.gltf(filepath=os.path.join(MIRA, "res/paris/models/citizen.glb"))
arm = next(o for o in bpy.data.objects if o.type == "ARMATURE")
body = next(o for o in bpy.data.objects if o.type == "MESH" and o.parent == arm)
for o in list(bpy.data.objects):
    if o not in (arm, body):
        bpy.data.objects.remove(o)
if arm.animation_data:
    arm.animation_data.action = None

# The citizen's coat, cravat and hat go: a bare body to dress.
_drop = {i for i, m in enumerate(body.data.materials) if not m.name.startswith("m013_")}
bm = bmesh.new()
bm.from_mesh(body.data)
bmesh.ops.delete(bm, geom=[f for f in bm.faces if f.material_index in _drop], context="FACES")
bm.to_mesh(body.data)
bm.free()
for _m, _rough in (("m013_body", 0.9), ("m013_head", 0.55), ("m013_opacity", 0.7)):
    _bsdf = bpy.data.materials[_m].node_tree.nodes.get("Principled BSDF")
    if _bsdf:
        _bsdf.inputs["Metallic"].default_value = 0.0
        _bsdf.inputs["Roughness"].default_value = _rough
update()


def B(name):
    return arm.pose.bones["Bip01 " + name]


def J(name):
    """Where a joint is, in the world."""
    return arm.matrix_world @ B(name).head


def reset_pose():
    for pb in arm.pose.bones:
        pb.matrix_basis = Matrix.Identity(4)
    update()


reset_pose()
arm.rotation_mode = "QUATERNION"
REST_Q = arm.rotation_quaternion.copy()
REST_PELVIS = J("Pelvis").copy()
ANKLE_Z = J("L Foot").z


def rotate_bone(name, q):
    """Turns a bone about its head by a rotation given in the world."""
    pb = B(name)
    a3 = arm.matrix_world.to_3x3().normalized()
    r = (a3.inverted() @ q.to_matrix() @ a3).to_4x4()
    h = pb.head.copy()
    pb.matrix = Matrix.Translation(h) @ r @ Matrix.Translation(-h) @ pb.matrix
    update()


def aim(name, child, direction):
    """Turns a bone so that its child's joint lies along a direction from its own."""
    v = J(child) - J(name)
    rotate_bone(name, v.normalized().rotation_difference(direction.normalized()))


def turn(name, axis, angle):
    from mathutils import Quaternion
    rotate_bone(name, Quaternion(axis, angle))


def reach(upper, lower, end, target, pole):
    """Two bones bent so that the end joint reaches the target, the middle joint toward the
    pole."""
    s = J(upper)
    l1 = (J(lower) - s).length
    l2 = (J(end) - J(lower)).length
    d = target - s
    dist = min(max(d.length, 1e-3), (l1 + l2) * 0.999)
    dn = d.normalized()
    cos_a = (l1 * l1 + dist * dist - l2 * l2) / (2 * l1 * dist)
    sin_a = math.sqrt(max(0.0, 1 - cos_a * cos_a))
    p = (pole - dn * pole.dot(dn)).normalized()
    elbow = s + dn * l1 * cos_a + p * l1 * sin_a
    aim(upper, lower, elbow - s)
    aim(lower, end, (s + dn * dist) - J(lower))


def arm_to(side, target, pole):
    reach(f"{side} UpperArm", f"{side} Forearm", f"{side} Hand", target, pole)


def leg_to(side, target, pole=Vector((0, -1, 0))):
    reach(f"{side} Thigh", f"{side} Calf", f"{side} Foot", target, pole)


def pelvis_at(offset):
    """Moves the whole figure by an offset (for the bob of a step, or a lunge)."""
    arm.location = REST_PELVIS + offset
    update()


# ---- Painting -------------------------------------------------------------------------------

body_mat = bpy.data.materials["m013_body"]
_base = next(n for n in body_mat.node_tree.nodes
             if n.type == "TEX_IMAGE" and n.outputs[0].links and n.outputs[0].links[0].to_socket.name == "Base Color")
SOURCE = _base.image
W, H = SOURCE.size
ORIGINAL = np.array(SOURCE.pixels[:], dtype=np.float32).reshape(H, W, 4)[:, :, :3]
LUM = ORIGINAL @ np.array([0.2126, 0.7152, 0.0722])
SHADE = np.clip(LUM / max(np.median(LUM[LUM > 0.05]), 1e-3), 0.55, 1.45)

me = body.data
me.calc_loop_triangles()
UV = me.uv_layers.active.data
MW = body.matrix_world
NW = MW.to_3x3().inverted().transposed()
BODY_INDEX = list(me.materials).index(body_mat)
VERTS = np.array([tuple(MW @ v.co) for v in me.vertices])
NORMALS = np.array([tuple((NW @ v.normal).normalized()) for v in me.vertices])
SHOULDER = {s: np.array(tuple(J(f"{n} UpperArm"))) for s, n in ((1, "L"), (-1, "R"))}
WRIST = {s: np.array(tuple(J(f"{n} Hand"))) for s, n in ((1, "L"), (-1, "R"))}


def segment(p, a, b):
    ab = b - a
    t = np.clip(((p - a) @ ab) / (ab @ ab), 0.0, 1.0)
    return np.linalg.norm(p - (a + t[:, None] * ab), axis=1), t


def rasterize(rule):
    """Every body texel coloured by the rule, from where it lies on the body at rest."""
    tex = ORIGINAL.copy()
    for tri in me.loop_triangles:
        if tri.material_index != BODY_INDEX:
            continue
        uv = np.array([tuple(UV[l].uv) for l in tri.loops]) * [W, H]
        p3 = VERTS[list(tri.vertices)]
        n3 = NORMALS[list(tri.vertices)]
        lo = np.clip(np.floor(uv.min(0)).astype(int) - 1, 0, [W - 1, H - 1])
        hi = np.clip(np.ceil(uv.max(0)).astype(int) + 1, 0, [W - 1, H - 1])
        xs, ys = np.meshgrid(np.arange(lo[0], hi[0] + 1), np.arange(lo[1], hi[1] + 1))
        px = np.stack([xs.ravel() + 0.5, ys.ravel() + 0.5], 1)
        a, b_, c_ = uv
        v0, v1 = b_ - a, c_ - a
        den = v0[0] * v1[1] - v1[0] * v0[1]
        if abs(den) < 1e-9:
            continue
        d = px - a
        w1 = (d[:, 0] * v1[1] - v1[0] * d[:, 1]) / den
        w2 = (v0[0] * d[:, 1] - d[:, 0] * v0[1]) / den
        w0 = 1 - w1 - w2
        inside = (w0 > -0.08) & (w1 > -0.08) & (w2 > -0.08)
        if not inside.any():
            continue
        wts = np.stack([w0, w1, w2], 1)[inside]
        ty, tx = ys.ravel()[inside], xs.ravel()[inside]
        tex[ty, tx] = rule(wts @ p3, wts @ n3, (ty, tx))
    return tex


def regions(p):
    """Which part of the body each point is: arm (with how far down it), legs, feet."""
    x, y, z = p[:, 0], p[:, 1], p[:, 2]
    side = np.where(x >= 0, 1, -1)
    arm_d = np.full_like(x, 9.0)
    arm_t = np.zeros_like(x)
    for s in (1, -1):
        k = side == s
        arm_d[k], arm_t[k] = segment(p[k], SHOULDER[s], WRIST[s])
    on_arm = (arm_d < 0.11) & (np.abs(x) > 0.19)
    return x, y, z, on_arm, arm_t


def checks(p, a, b, size=0.045):
    """A woven check, for Gallic trousers."""
    u = np.floor((p[:, 0] + p[:, 1]) / size).astype(int)
    v = np.floor(p[:, 2] / size).astype(int)
    k = ((u + v) % 2 == 0)[:, None]
    return np.where(k, a, b)


def paint(kind):
    tunic = {"legionary": hexc(0xD6CDB6), "gaul": hexc(0x4A6038), "legate": hexc(0xC9BFA6),
             "commander": hexc(0xB02A25)}[kind]
    skin = hexc(0xB07A58)
    mail = hexc(0x55585B)
    leather = hexc(0x4A3020)

    def rule(p, n, texels):
        x, y, z, on_arm, arm_t = regions(p)
        ax = np.abs(x)
        s = SHADE[texels][:, None]
        out = tunic * s
        legs = z < 0.86
        if kind == "gaul":
            # Checked trousers to the ankle, a short tunic over them, bare forearms.
            trousers = legs & (z > 0.11)
            out[trousers] = checks(p[trousers], hexc(0x6B5A2E), hexc(0x2F3A4E)) * s[trousers]
            bare = on_arm & (arm_t > 0.42)
        else:
            # Bare legs below a tunic hemmed above the knee; sandals.
            bare_leg = (z < 0.6) & (z > 0.115)
            out[bare_leg] = skin * s[bare_leg] ** 0.5
            bare = on_arm & (arm_t > 0.3)
        out[bare] = skin * s[bare] ** 0.5
        feet = z <= 0.115
        if kind == "gaul":
            out[feet] = leather * s[feet]
        else:
            # Caligae: straps over the foot, skin between.
            strap = (np.floor((z[feet] + 0.6 * np.abs(y[feet])) / 0.018).astype(int) % 2 == 0)[:, None]
            out[feet] = np.where(strap, leather, skin * 0.85) * s[feet]
        # The neck above the collar is skin.
        neck = (z > 1.47) & (ax < 0.1)
        out[neck] = skin * s[neck] ** 0.5
        if kind == "legionary":
            # Mail from the neck to the hips, with short sleeves, doubled over the shoulders.
            body_mail = (z > 0.83) & (z < 1.49) & ~on_arm & (ax < 0.25)
            sleeve = on_arm & (arm_t < 0.22)
            m = body_mail | sleeve
            grain = 0.85 + 0.3 * ((np.floor(p[m, 0] / 0.006) + np.floor(p[m, 2] / 0.006)) % 2)[:, None]
            out[m] = mail * s[m] * grain
            cape = m & (z > 1.36) & (ax > 0.05)
            out[cape] *= 0.75
            edge = m & (np.abs(z - 1.36) < 0.008) & (ax > 0.05)
            out[edge] = leather * 0.8
            out[neck] = skin * s[neck] ** 0.5
        if kind in ("legate", "commander"):
            boots = (z < 0.32) & (z > 0.0)
            out[boots] = leather * 0.9 * s[boots]
        return out

    tex = rasterize(rule)
    img = bpy.data.images.new(f"{kind}_body", W, H, alpha=False)
    img.pixels = np.concatenate([tex, np.ones((H, W, 1), np.float32)], 2).ravel()
    img.pack()
    mat = body_mat.copy()
    mat.name = f"{kind}_body"
    node = next(n for n in mat.node_tree.nodes if n.type == "TEX_IMAGE" and n.image == SOURCE)
    node.image = img
    return mat


# ---- Kit --------------------------------------------------------------------------------------

def material(name, color, roughness=0.75, metallic=0.0):
    m = bpy.data.materials.get(name)
    if m:
        return m
    m = bpy.data.materials.new(name)
    m.use_nodes = True
    b = m.node_tree.nodes["Principled BSDF"]
    b.inputs["Base Color"].default_value = (*color, 1.0)
    b.inputs["Roughness"].default_value = roughness
    b.inputs["Metallic"].default_value = metallic
    return m


BRONZE = material("bronze", srgb(0x8A6A3A), 0.45, 1.0)
GILT = material("gilt", srgb(0xC9A040), 0.35, 1.0)
IRON = material("iron", srgb(0x6E7074), 0.4, 1.0)
STEEL = material("steel", srgb(0xA9ADB2), 0.3, 1.0)
WOOD = material("wood", srgb(0x6B4A2E), 0.7)
LEATHER = material("leather", srgb(0x4A3020), 0.6)
WOOL_BROWN = material("wool_brown", srgb(0x6A4A30), 0.95)


def mesh_object(name, bm, mat, smooth=True):
    me = bpy.data.meshes.new(name)
    bm.to_mesh(me)
    bm.free()
    if smooth:
        for p in me.polygons:
            p.use_smooth = True
    me.materials.append(mat)
    o = bpy.data.objects.new(name, me)
    bpy.context.scene.collection.objects.link(o)
    return o


def cylinder(r1, r2, depth, segs, matrix, mat, name="part"):
    bm = bmesh.new()
    bmesh.ops.create_cone(bm, cap_ends=True, cap_tris=False, segments=segs, radius1=r1, radius2=r2, depth=depth, matrix=matrix)
    return mesh_object(name, bm, mat)


def box(size, matrix, mat, name="part"):
    bm = bmesh.new()
    bmesh.ops.create_cube(bm, size=1.0, matrix=matrix @ Matrix.Diagonal((*size, 1.0)))
    return mesh_object(name, bm, mat, smooth=False)


def sphere(radius, matrix, mat, name="part"):
    bm = bmesh.new()
    bmesh.ops.create_uvsphere(bm, u_segments=12, v_segments=8, radius=radius, matrix=matrix)
    return mesh_object(name, bm, mat)


def along(a, b):
    """A matrix at the middle of ab with its z along ab."""
    d = (b - a)
    q = Vector((0, 0, 1)).rotation_difference(d.normalized())
    return Matrix.Translation((a + b) / 2) @ q.to_matrix().to_4x4()


def join(objs, name):
    bpy.ops.object.select_all(action="DESELECT")
    for o in objs:
        o.select_set(True)
    bpy.context.view_layer.objects.active = objs[0]
    bpy.ops.object.join()
    objs[0].name = name
    return objs[0]


def on_bone(obj, bone_name):
    """Carries an object built in the rest pose on a bone."""
    m = obj.matrix_world.copy()
    obj.parent = arm
    obj.parent_type = "BONE"
    obj.parent_bone = "Bip01 " + bone_name
    update()
    obj.matrix_world = m
    update()
    return obj


def head_frame():
    """The head's crown, centre and size at rest, from its vertices."""
    idx = [v for p in me.polygons if me.materials[p.material_index].name == "m013_head" for v in p.vertices]
    pts = VERTS[sorted(set(idx))]
    top = pts[:, 2].max()
    upper = pts[pts[:, 2] > top - 0.1]
    centre = upper.mean(0)
    return top, Vector((centre[0], centre[1], 0)), np.ptp(upper[:, 0]) / 2, np.ptp(upper[:, 1]) / 2


HEAD_TOP, HEAD_C, HEAD_RX, HEAD_RY = head_frame()


def bowl(rim_z, height, rx, ry, mat, name, knob=True):
    """A helmet bowl: the Montefortino's profile, r = R (1 - (z/H)^3.22)^(1/1.16), revolved
    about the head and stretched front to back."""
    bm = bmesh.new()
    rings = []
    seg = 24
    for i in range(9):
        t = i / 8 * 0.97
        r = (1 - t ** 3.22) ** (1 / 1.16)
        ring = [bm.verts.new((HEAD_C.x + rx * r * math.cos(a), HEAD_C.y + ry * r * math.sin(a), rim_z + height * t))
                for a in (k / seg * math.tau for k in range(seg))]
        rings.append(ring)
    for a, b in zip(rings, rings[1:]):
        for k in range(seg):
            bm.faces.new((a[k], a[(k + 1) % seg], b[(k + 1) % seg], b[k]))
    bm.faces.new(rings[-1])
    parts = [mesh_object(name, bm, mat)]
    if knob:
        parts.append(sphere(0.016, Matrix.Translation((HEAD_C.x, HEAD_C.y, rim_z + height + 0.008)), mat))
    return parts


def montefortino(mat):
    rim = HEAD_TOP - 0.135
    rx, ry = HEAD_RX + 0.012, HEAD_RY + 0.014
    parts = bowl(rim, 0.15, rx, ry, mat, "helmet")
    # A short neck guard flaring at the back.
    bm = bmesh.new()
    inner, outer = [], []
    for k in range(9):
        a = math.radians(40 + k * 12.5)
        c, s = math.cos(a), math.sin(a)
        inner.append(bm.verts.new((HEAD_C.x + rx * c, HEAD_C.y + ry * s, rim + 0.004)))
        outer.append(bm.verts.new((HEAD_C.x + (rx + 0.035) * c, HEAD_C.y + (ry + 0.045) * s, rim - 0.012)))
    for k in range(8):
        bm.faces.new((inner[k], inner[k + 1], outer[k + 1], outer[k]))
    parts.append(mesh_object("neckguard", bm, mat))
    # Hinged cheek guards hanging at the sides of the face.
    for side in (1, -1):
        top = Vector((HEAD_C.x + side * (rx - 0.004), HEAD_C.y - ry * 0.35, rim))
        g = Matrix.Translation(top + Vector((side * 0.006, -0.01, -0.055))) @ Matrix.Rotation(side * -0.12, 4, "Y")
        parts.append(box((0.008, 0.075, 0.11), g, mat))
    return parts


def attic(mat, crest_mat):
    rim = HEAD_TOP - 0.13
    rx, ry = HEAD_RX + 0.012, HEAD_RY + 0.014
    parts = bowl(rim, 0.15, rx, ry, mat, "helmet", knob=False)
    # A brow band rising to a point over the forehead.
    bm = bmesh.new()
    lo, hi = [], []
    for k in range(13):
        a = math.radians(-160 + k * 23.3)
        c, s = math.cos(a), math.sin(a)
        peak = max(0.0, 1 - abs(math.degrees(a) + 90) / 90)
        lo.append(bm.verts.new((HEAD_C.x + (rx + 0.004) * c, HEAD_C.y + (ry + 0.004) * s, rim)))
        hi.append(bm.verts.new((HEAD_C.x + (rx + 0.004) * c, HEAD_C.y + (ry + 0.004) * s, rim + 0.025 + 0.035 * peak)))
    for k in range(12):
        bm.faces.new((lo[k], lo[k + 1], hi[k + 1], hi[k]))
    parts.append(mesh_object("brow", bm, mat))
    # A crest from brow to nape.
    crest = Matrix.Translation((HEAD_C.x, HEAD_C.y + 0.01, rim + 0.17))
    parts.append(box((0.03, 0.26, 0.07), crest, crest_mat))
    for side in (1, -1):
        g = Matrix.Translation((HEAD_C.x + side * (rx - 0.002), HEAD_C.y - ry * 0.35, rim - 0.05)) @ Matrix.Rotation(side * -0.12, 4, "Y")
        parts.append(box((0.008, 0.08, 0.1), g, mat))
    return parts


def ring_at(z, grow, mat, height=0.045, name="belt", within=0.24):
    """A band round the body at a height, just outside it."""
    band = VERTS[(np.abs(VERTS[:, 2] - z) < 0.02) & (np.abs(VERTS[:, 0]) < within)]
    cx, cy = band[:, 0].mean(), band[:, 1].mean()
    rx, ry = np.ptp(band[:, 0]) / 2 + grow, np.ptp(band[:, 1]) / 2 + grow
    bm = bmesh.new()
    seg = 24
    lo = [bm.verts.new((cx + rx * math.cos(a), cy + ry * math.sin(a), z - height / 2)) for a in (k / seg * math.tau for k in range(seg))]
    hi = [bm.verts.new((cx + rx * math.cos(a), cy + ry * math.sin(a), z + height / 2)) for a in (k / seg * math.tau for k in range(seg))]
    for k in range(seg):
        bm.faces.new((lo[k], lo[(k + 1) % seg], hi[(k + 1) % seg], hi[k]))
    return mesh_object(name, bm, mat), (cx, cy, rx, ry)


def skirt(z_top, z_hem, grow_top, flare, mat, name, uv_stripes=False):
    """A hanging skirt from the waist, bent by the hips and thighs as they move."""
    top_band = VERTS[(np.abs(VERTS[:, 2] - z_top) < 0.02) & (np.abs(VERTS[:, 0]) < 0.24)]
    cx, cy = top_band[:, 0].mean(), top_band[:, 1].mean()
    rx, ry = np.ptp(top_band[:, 0]) / 2 + grow_top, np.ptp(top_band[:, 1]) / 2 + grow_top
    bm = bmesh.new()
    seg, rows = 28, 6
    uv_layer = bm.loops.layers.uv.new()
    grid = []
    for i in range(rows + 1):
        t = i / rows
        z = z_top + (z_hem - z_top) * t
        f = 1 + flare * t
        grid.append([bm.verts.new((cx + rx * f * math.cos(a), cy + ry * f * math.sin(a), z)) for a in (k / seg * math.tau for k in range(seg))])
    for i in range(rows):
        for k in range(seg):
            f = bm.faces.new((grid[i][k], grid[i][(k + 1) % seg], grid[i + 1][(k + 1) % seg], grid[i + 1][k]))
            for loop, (u, v) in zip(f.loops, ((k, i), (k + 1, i), (k + 1, i + 1), (k, i + 1))):
                loop[uv_layer].uv = (u / seg, 1 - v / rows)
    o = mesh_object(name, bm, mat)
    # Weights: all to the pelvis at the waist, toward each thigh lower down.
    groups = {n: o.vertex_groups.new(name="Bip01 " + n) for n in ("Pelvis", "L Thigh", "R Thigh")}
    for v in o.data.vertices:
        t = (z_top - v.co.z) / (z_top - z_hem)
        wp = 1.0 - 0.75 * t
        left = min(max((v.co.x - cx) / 0.1 + 0.5, 0.0), 1.0)
        groups["Pelvis"].add([v.index], wp, "REPLACE")
        groups["L Thigh"].add([v.index], (1 - wp) * left, "REPLACE")
        groups["R Thigh"].add([v.index], (1 - wp) * (1 - left), "REPLACE")
    mod = o.modifiers.new("arm", "ARMATURE")
    mod.object = arm
    o.parent = arm
    o.matrix_parent_inverse = arm.matrix_world.inverted()
    return o


def stripes_material(name, a, b, fringe):
    """Leather strips with gaps between, for pteruges."""
    w, h = 256, 32
    img = bpy.data.images.new(name, w, h, alpha=True)
    px = np.zeros((h, w, 4), np.float32)
    for x in range(w):
        strip = (x // 8) % 2 == 0
        px[:, x, :3] = a if strip else b
        px[:, x, 3] = 1.0
    px[:3, :, :3] = fringe
    img.pixels = px.ravel()
    img.pack()
    m = bpy.data.materials.new(name)
    m.use_nodes = True
    nt = m.node_tree
    tex = nt.nodes.new("ShaderNodeTexImage")
    tex.image = img
    bsdf = nt.nodes["Principled BSDF"]
    nt.links.new(tex.outputs["Color"], bsdf.inputs["Base Color"])
    bsdf.inputs["Roughness"].default_value = 0.7
    return m


def shell(name, keep, offset, mat):
    """A layer over part of the body, such as a cuirass, bent with it as it moves."""
    o = body.copy()
    o.data = body.data.copy()
    o.name = name
    bpy.context.scene.collection.objects.link(o)
    bm = bmesh.new()
    bm.from_mesh(o.data)
    bm.normal_update()
    mw = body.matrix_world
    gone = [f for f in bm.faces if not keep(mw @ f.calc_center_median(), f.material_index)]
    bmesh.ops.delete(bm, geom=gone, context="FACES")
    scale = mw.to_scale().x
    for v in bm.verts:
        v.co += v.normal * (offset / scale)
    bm.to_mesh(o.data)
    bm.free()
    o.data.materials.clear()
    o.data.materials.append(mat)
    return o


def cloak_sheet(rows, mat, name="cloak"):
    """A sheet through rows of points, shoulders first."""
    bm = bmesh.new()
    grid = [[bm.verts.new(p) for p in row] for row in rows]
    for a, b in zip(grid, grid[1:]):
        for k in range(len(a) - 1):
            bm.faces.new((a[k], a[k + 1], b[k + 1], b[k]))
    o = mesh_object(name, bm, mat)
    mod = o.modifiers.new("sub", "SUBSURF")
    mod.levels = mod.render_levels = 2
    return o


# ---- Held things, placed afresh for each frame ---------------------------------------------

HELD = []


def held(obj):
    HELD.append(obj)
    return obj


def clear_held():
    for o in HELD:
        bpy.data.objects.remove(o)
    HELD.clear()


def scutum(center, facing, paint_mat):
    """A tall curved oval shield, its face toward `facing` (a direction in the ground plane),
    with a bronze boss and a wooden spine."""
    height, width, radius = 1.12, 0.64, 0.42
    bm = bmesh.new()
    rows, cols = 12, 10
    verts = []
    for i in range(rows + 1):
        v = -1 + 2 * i / rows
        half = math.sqrt(max(1 - v * v, 0.0)) ** 0.6
        row = []
        for j in range(cols + 1):
            u = (-1 + 2 * j / cols) * half
            a = u * (width / 2) / radius
            row.append(bm.verts.new((radius * math.sin(a), -radius * (math.cos(a) - 1) - 0.0, v * height / 2)))
        verts.append(row)
    for i in range(rows):
        for j in range(cols):
            bm.faces.new((verts[i][j], verts[i][j + 1], verts[i + 1][j + 1], verts[i + 1][j]))
    face = mesh_object("scutum", bm, paint_mat)
    sol = face.modifiers.new("thick", "SOLIDIFY")
    sol.thickness = 0.018
    parts = [face]
    parts.append(sphere(0.075, Matrix.Translation((0, -0.012, 0)) @ Matrix.Diagonal((1, 0.55, 1, 1)), BRONZE))
    parts.append(box((0.035, 0.012, height * 0.92), Matrix.Translation((0, -0.012, 0)), WOOD))
    # Turn it so that its face (local -Y) faces the way asked, and put it in place.
    yaw = math.atan2(facing.x, -facing.y)
    m = Matrix.Translation(center) @ Matrix.Rotation(yaw, 4, "Z")
    for p in parts:
        p.matrix_world = m @ p.matrix_world
        held(p)
    return parts


def gallic_shield(center, facing, paint_mat):
    height, width = 1.05, 0.56
    bm = bmesh.new()
    ring = [bm.verts.new((width / 2 * math.cos(a), 0, height / 2 * math.sin(a))) for a in (k / 24 * math.tau for k in range(24))]
    bm.faces.new(ring)
    face = mesh_object("gallic_shield", bm, paint_mat, smooth=False)
    sol = face.modifiers.new("thick", "SOLIDIFY")
    sol.thickness = 0.015
    parts = [face]
    parts.append(box((0.07, 0.03, 0.2), Matrix.Translation((0, -0.02, 0)), IRON))
    parts.append(box((0.03, 0.01, height * 0.85), Matrix.Translation((0, -0.012, 0)), WOOD))
    yaw = math.atan2(facing.x, -facing.y)
    m = Matrix.Translation(center) @ Matrix.Rotation(yaw, 4, "Z")
    for p in parts:
        p.matrix_world = m @ p.matrix_world
        held(p)
    return parts


def pilum(hand, direction, grip=0.95):
    """A heavy javelin through the hand: a wooden shaft and a long iron shank with a small
    pyramidal point."""
    d = direction.normalized()
    butt = hand - d * grip
    join_at = butt + d * 1.3
    tip = join_at + d * 0.6
    held(cylinder(0.016, 0.016, (join_at - butt).length, 8, along(butt, join_at), WOOD))
    held(cylinder(0.006, 0.006, (tip - join_at).length, 6, along(join_at, tip), IRON))
    held(box((0.04, 0.04, 0.07), along(join_at - d * 0.035, join_at + d * 0.035), IRON))
    held(cylinder(0.012, 0.0, 0.05, 4, along(tip, tip + d * 0.05), IRON))


def sword(hand, direction, blade=0.55, width=0.05):
    d = direction.normalized()
    tip = hand + d * (blade + 0.06)
    held(box((width, 0.008, blade), along(hand + d * 0.06, tip), STEEL))
    held(box((0.1, 0.03, 0.025), along(hand + d * 0.04, hand + d * 0.065), BRONZE))
    held(sphere(0.022, Matrix.Translation(hand - d * 0.07), BRONZE))


# ---- Kinds of men ---------------------------------------------------------------------------

def dress(kind):
    """The body painted and the kit that rides on bones, for one kind of man. Returns what was
    added, to be taken away after."""
    body.data.materials[BODY_INDEX] = paint(kind)
    worn = []
    if kind == "legionary":
        worn += [on_bone(o, "Head") for o in montefortino(BRONZE)]
        belt, (cx, cy, rx, ry) = ring_at(1.0, 0.035, LEATHER)
        worn.append(on_bone(belt, "Pelvis"))
        # The belt's apron of studded strips in front.
        for k in range(4):
            x = cx - 0.045 + k * 0.03
            g = Matrix.Translation((x, cy - ry - 0.01, 0.9))
            worn.append(on_bone(box((0.018, 0.006, 0.2), g, LEATHER), "Pelvis"))
        focale, _ = ring_at(1.47, 0.03, WOOL_BROWN, height=0.05, name="focale", within=0.1)
        worn.append(on_bone(focale, "Spine2"))
        # Mail hanging to mid-thigh over a tunic to above the knee.
        mail = material("mail", srgb(0x4E5154), 0.65, 0.6)
        worn.append(skirt(1.02, 0.6, 0.012, 0.22, material("tunic", srgb(0xCFC5AC), 0.95), "tunic"))
        worn.append(skirt(1.02, 0.78, 0.022, 0.14, mail, "mail_skirt"))
        # The scabbard on the right hip.
        g = along(Vector((-0.2, -0.02, 1.0)), Vector((-0.24, 0.12, 0.5)))
        worn.append(on_bone(box((0.06, 0.03, 0.55), g, LEATHER), "Pelvis"))
    elif kind == "gaul":
        worn.append(skirt(1.02, 0.72, 0.025, 0.35, material("gaul_tunic", srgb(0x4A6038), 0.95), "tunic"))
        belt, _ = ring_at(1.0, 0.035, LEATHER)
        worn.append(on_bone(belt, "Pelvis"))
        torc, _ = ring_at(1.49, 0.02, GILT, height=0.02, name="torc", within=0.1)
        worn.append(on_bone(torc, "Spine2"))
    else:
        officer = kind == "legate"
        metal = BRONZE if officer else GILT
        worn.append(shell("cuirass", lambda c, m: m == BODY_INDEX and 0.93 < c.z < 1.49 and abs(c.x) < 0.21, 0.025, metal))
        worn.append(skirt(0.97, 0.66, 0.04, 0.35, stripes_material(f"{kind}_pteruges", hexc(0x5A3A22), hexc(0xE0D8C4), hexc(0xC9A040)), "pteruges"))
        sash, _ = ring_at(1.02, 0.05, material(f"{kind}_sash", srgb(0xE0D8C4) if officer else srgb(0xB02A25), 0.9), height=0.04, name="sash")
        worn.append(on_bone(sash, "Spine"))
        if officer:
            worn += [on_bone(o, "Head") for o in attic(BRONZE, material("crest", srgb(0xC0302A), 0.95))]
    return worn


SHIELD_PAINT = material("shield_paint", srgb(0x8E2A22), 0.7)
GALLIC_PAINT = material("gallic_paint", srgb(0x6E5A2C), 0.75)
FWD = Vector((0, -1, 0))


def stand():
    pelvis_at(Vector((0, 0, 0)))
    for side, x in (("L", 0.11), ("R", -0.11)):
        leg_to(side, Vector((x, 0.0, ANKLE_Z)))


def stride(step, length=0.3):
    """Legs at one of eight moments of a walking step; returns the arm swing."""
    phase = step / 8.0 * math.tau
    pelvis_at(Vector((0, 0, -0.025 * abs(math.cos(phase)))))
    for side, x, offset in (("L", 0.1, 0.0), ("R", -0.1, math.pi)):
        p = phase + offset
        forward = length * math.sin(p)
        lift = 0.08 * max(0.0, math.cos(p)) ** 2
        leg_to(side, Vector((x, -forward, ANKLE_Z + lift)))
    return math.sin(phase)


def carry_legionary(swing=0.0):
    """Scutum on the left arm in front, pilum upright in the right hand."""
    arm_to("L", Vector((0.17, -0.3, 1.0)), Vector((1.0, 0.6, -0.2)))
    shield_c = J("L Hand") + Vector((0.0, -0.09, 0.02))
    scutum(shield_c, Vector((0.25, -1, 0)), SHIELD_PAINT)
    hand = Vector((-0.27, -0.14 - 0.06 * swing, 1.0))
    arm_to("R", hand, Vector((-1.0, 0.8, -0.2)))
    pilum(J("R Hand") + Vector((0, -0.03, 0)), Vector((0.0, -0.18, 1.0)))


def fight_legionary(k):
    """Six moments of the stab from behind the shield: cocked, cocked, thrust, out, back,
    cocked."""
    out = [0.0, 0.1, 0.6, 1.0, 0.45, 0.05][k]
    pelvis_at(Vector((0, -0.06 * out, -0.07)))
    leg_to("L", Vector((0.13, -0.28, ANKLE_Z)), Vector((0.3, -1, 0)))
    leg_to("R", Vector((-0.15, 0.25, ANKLE_Z)), Vector((-0.3, -1, 0)))
    turn("Spine1", Vector((0, 0, 1)), -0.25 + 0.3 * out)
    arm_to("L", Vector((0.12, -0.38, 1.05)), Vector((1.0, 0.4, -0.3)))
    scutum(J("L Hand") + Vector((0.0, -0.1, 0.0)), Vector((0.1, -1, 0)), SHIELD_PAINT)
    hand = Vector((-0.24, 0.05, 1.02)).lerp(Vector((-0.1, -0.68, 1.12)), out)
    arm_to("R", hand, Vector((-1.0, 0.6, -0.4)))
    sword(J("R Hand"), Vector((0.05, -1.0, 0.12 - 0.1 * out)))


def carry_gaul(swing=0.0):
    arm_to("L", Vector((0.2, -0.26, 1.0)), Vector((1.0, 0.6, -0.2)))
    gallic_shield(J("L Hand") + Vector((0.0, -0.08, 0.0)), Vector((0.3, -1, 0)), GALLIC_PAINT)
    arm_to("R", Vector((-0.3, -0.12 - 0.08 * swing, 0.98)), Vector((-1.0, 0.8, -0.2)))
    sword(J("R Hand"), Vector((-0.1, -0.5, -1.0)), blade=0.8, width=0.045)


def fight_gaul(k):
    """A long sword swung down from overhead."""
    up = [0.0, 0.5, 1.0, 1.0, 0.4, 0.0][k]
    down = [0.0, 0.0, 0.0, 0.3, 1.0, 0.6][k]
    pelvis_at(Vector((0, -0.05 * down, -0.05)))
    leg_to("L", Vector((0.14, -0.26, ANKLE_Z)), Vector((0.3, -1, 0)))
    leg_to("R", Vector((-0.15, 0.22, ANKLE_Z)), Vector((-0.3, -1, 0)))
    arm_to("L", Vector((0.22, -0.3, 1.05)), Vector((1.0, 0.4, -0.3)))
    gallic_shield(J("L Hand") + Vector((0.0, -0.09, 0.0)), Vector((0.2, -1, 0)), GALLIC_PAINT)
    raised = Vector((-0.18, 0.05, 1.85))
    struck = Vector((-0.12, -0.6, 1.0))
    rest = Vector((-0.3, -0.15, 1.05))
    hand = rest.lerp(raised, up) if down == 0 else raised.lerp(struck, down)
    arm_to("R", hand, Vector((-1.0, 0.5, -0.3)))
    blade_dir = Vector((0.05, 0.4, 1.0)).lerp(Vector((0, -1, -0.5)), down) if up > 0.3 or down > 0 else Vector((-0.1, -0.5, -1))
    sword(J("R Hand"), blade_dir, blade=0.8, width=0.045)


def fallen(kind):
    """On his back, arms out, his shield beside him."""
    reset_pose()
    arm.rotation_quaternion = Quaternion((1, 0, 0), -math.pi / 2) @ REST_Q
    arm.location = Vector((0, 0.0, 0.12))
    update()
    for side, x in (("L", 1), ("R", -1)):
        arm_to(side, J(f"{side} UpperArm") + Vector((x * 0.45, 0.05, -0.1)), Vector((0, 0, 1)))
    if kind == "legionary":
        scutum(Vector((0.55, 0.0, 0.03)), Vector((0, 0, 1)), SHIELD_PAINT)
        for p in HELD[-3:]:
            p.rotation_euler = (-math.pi / 2, 0, 0.3)
            p.location = (0.6, -0.1, 0.06)
    else:
        gallic_shield(Vector((0.55, 0.0, 0.03)), Vector((0, 0, 1)), GALLIC_PAINT)
        for p in HELD[-3:]:
            p.rotation_euler = (-math.pi / 2, 0, 0.3)
            p.location = (0.6, -0.1, 0.05)


def upright():
    arm.rotation_quaternion = REST_Q
    arm.location = REST_PELVIS
    reset_pose()


# ---- Horses -----------------------------------------------------------------------------------

def load_horse(coat):
    before = set(bpy.data.objects)
    actions_before = set(bpy.data.actions)
    bpy.ops.import_scene.gltf(filepath=os.path.join(MIRA, f"res/paris/models/animals/horse_{'chestnut' if coat == 'grey' else 'bay'}.glb"))
    new = [o for o in bpy.data.objects if o not in before]
    h_arm = next(o for o in new if o.type == "ARMATURE")
    h_body = next(o for o in new if o.type == "MESH" and o.parent == h_arm)
    for o in new:
        if o not in (h_arm, h_body):
            bpy.data.objects.remove(o)
    walk = next(a for a in bpy.data.actions if a not in actions_before and a.name.startswith("walk"))
    for m in h_body.data.materials:
        b = m.node_tree.nodes.get("Principled BSDF")
        if b:
            b.inputs["Metallic"].default_value = 0.0
    if coat == "grey":
        m = h_body.data.materials[0]
        node = next(n for n in m.node_tree.nodes if n.type == "TEX_IMAGE" and n.outputs[0].links
                    and n.outputs[0].links[0].to_socket.name == "Base Color")
        img = node.image
        w, h = img.size
        px = np.array(img.pixels[:], np.float32).reshape(h, w, 4)
        lum = px[..., :3] @ np.array([0.2126, 0.7152, 0.0722])
        grey = hexc(0xA29E95) * np.clip(lum / max(lum.mean(), 1e-3), 0.45, 1.3)[..., None] ** 0.8
        new_img = bpy.data.images.new("horse_grey", w, h)
        new_img.pixels = np.concatenate([grey, np.ones((h, w, 1), np.float32)], 2).ravel()
        new_img.pack()
        node.image = new_img
    return h_arm, h_body, walk




def horse_pose(h_arm, walk, frame):
    """The horse at a moment of its walk (or standing), kept over the origin."""
    if h_arm.animation_data is None:
        h_arm.animation_data_create()
    h_arm.animation_data.action = walk
    if hasattr(h_arm.animation_data, "action_slot") and walk.slots:
        h_arm.animation_data.action_slot = walk.slots[0]
    lo, hi = walk.frame_range
    if frame is None:
        bpy.context.scene.frame_set(int(lo))
    else:
        f = lo + (hi - lo) * frame / 8.0
        bpy.context.scene.frame_set(int(math.floor(f)), subframe=f - math.floor(f))
    update()
    pelvis = h_arm.matrix_world @ h_arm.pose.bones["horse Pelvis"].head
    return pelvis


def seat_rider(h_arm, saddle_z_above_spine):
    spine = h_arm.matrix_world @ h_arm.pose.bones["horse Spine1"].head
    seat = spine + Vector((0, 0.05, saddle_z_above_spine))
    reset_pose()
    arm.location = seat + Vector((0, 0, 0.1))
    update()
    for side, x in (("L", 1), ("R", -1)):
        leg_to(side, Vector((x * 0.36, seat.y - 0.12, seat.z - 0.62)), Vector((x * 0.5, -1.0, 0.1)))
        arm_to(side, Vector((x * 0.13, seat.y - 0.38, seat.z + 0.22)), Vector((x * 0.6, 1.0, -0.5)))
    return seat


def rider_cloak(mat, seat, sway):
    l, r = J("L UpperArm"), J("R UpperArm")
    neck = J("Neck")
    rows = []
    for depth, drop, width in ((0.04, 0.0, 1.0), (0.16, 0.3, 1.35), (0.34, 0.55, 1.6), (0.52, 0.72, 1.75)):
        row = []
        for k in range(5):
            t = k / 4
            p = l.lerp(r, t)
            mid = (l + r) / 2
            p = mid + (p - mid) * width
            p.y += depth + 0.03 * math.sin(t * math.pi) * (1 if drop > 0 else 0)
            p.z = neck.z - 0.05 - drop
            p.x += sway * drop * 0.05
            row.append(p)
        rows.append(row)
    rows[0] = [p + Vector((0, 0.02, 0.02)) for p in rows[0]]
    return held(cloak_sheet(rows, mat))


def saddle(seat, cloth_mat):
    held(box((0.62, 0.95, 0.02), Matrix.Translation(seat + Vector((0, 0.05, -0.02))), cloth_mat))
    for side in (1, -1):
        drape = Matrix.Translation(seat + Vector((side * 0.32, 0.06, -0.2))) @ Matrix.Rotation(side * 0.12, 4, "Y")
        held(box((0.012, 0.85, 0.36), drape, cloth_mat))
    held(box((0.38, 0.5, 0.08), Matrix.Translation(seat + Vector((0, 0.03, 0.03))), LEATHER))
    for x, y in ((0.13, -0.2), (-0.13, -0.2), (0.13, 0.25), (-0.13, 0.25)):
        held(sphere(0.04, Matrix.Translation(seat + Vector((x, y, 0.09))), LEATHER))


# ---- Rendering --------------------------------------------------------------------------------

scene = bpy.context.scene
scene.render.engine = "CYCLES"
scene.cycles.samples = 8 if QUICK else 28
scene.cycles.use_denoising = True
scene.cycles.device = "CPU"
scene.cycles.max_bounces = 4
scene.render.film_transparent = True
scene.render.filter_size = 1.2
scene.view_settings.view_transform = "Standard"
scene.view_settings.look = "None"
scene.view_settings.exposure = 0.0
scene.render.image_settings.file_format = "PNG"
scene.render.image_settings.color_mode = "RGBA"
world = bpy.data.worlds.new("sky")
world.use_nodes = True
world.node_tree.nodes["Background"].inputs[0].default_value = (0.55, 0.62, 0.72, 1)
world.node_tree.nodes["Background"].inputs[1].default_value = 0.9
scene.world = world

pivot = bpy.data.objects.new("pivot", None)
scene.collection.objects.link(pivot)
cam_data = bpy.data.cameras.new("cam")
cam_data.type = "ORTHO"
cam_data.sensor_fit = "VERTICAL"
cam = bpy.data.objects.new("cam", cam_data)
scene.collection.objects.link(cam)
scene.camera = cam
sun_data = bpy.data.lights.new("sun", "SUN")
sun_data.energy = 4.2
sun_data.angle = math.radians(8)
sun_data.color = (1.0, 0.96, 0.9)
sun = bpy.data.objects.new("sun", sun_data)
scene.collection.objects.link(sun)
for o in (cam, sun):
    o.parent = pivot
# Light from over the viewer's left shoulder: the same on every sprite, whichever way he faces.
sun.rotation_euler = Vector((0.55, 0.75, -0.9)).to_track_quat("-Z", "Y").to_euler()


def frame_camera(cell):
    w, h, foot = cell
    scene.render.resolution_x, scene.render.resolution_y = w, h
    scene.render.resolution_percentage = 100
    cam_data.ortho_scale = h * PX
    view = Vector((0, math.cos(PITCH), -math.sin(PITCH)))
    up = Vector((0, math.sin(PITCH), math.cos(PITCH)))
    centre = up * ((h / 2 - foot) * PX)
    cam.location = centre - view * 30
    cam.rotation_euler = view.to_track_quat("-Z", "Y").to_euler()
    cam_data.clip_end = 100


def shoot(path, direction):
    # The man faces direction * 45 degrees turned to his left; the camera goes the other way.
    pivot.rotation_euler = (0, 0, -direction * math.tau / DIRECTIONS)
    update()
    scene.render.filepath = path
    bpy.ops.render.render(write_still=True)


def render_kind(kind, frames, cell):
    """frames: (name, pose function) pairs; each pose builds the held things it needs."""
    frame_camera(cell)
    tmp = os.path.join(OUT, f"_{kind}")
    os.makedirs(tmp, exist_ok=True)
    names = []
    for name, pose in frames:
        clear_held()
        pose()
        update()
        names.append(name)
        for d in range(DIRECTIONS):
            if QUICK and d not in (0, 2, 3):
                continue
            shoot(os.path.join(tmp, f"{name}_{d}.png"), d)
        print("RENDERED", kind, name, flush=True)
    clear_held()
    upright()
    # One sheet: a row for each direction, a column for each frame.
    from PIL import Image
    w, h, foot = cell
    sheet = Image.new("RGBA", (w * len(names), h * DIRECTIONS))
    for c, name in enumerate(names):
        for d in range(DIRECTIONS):
            p = os.path.join(tmp, f"{name}_{d}.png")
            if os.path.exists(p):
                sheet.paste(Image.open(p), (c * w, d * h))
    sheet.save(os.path.join(OUT, f"{kind}.png"), optimize=True)
    shutil.rmtree(tmp, ignore_errors=True)
    return {"cell": [w, h], "foot": foot, "frames": names}


def infantry_frames(kind):
    carry = carry_legionary if kind == "legionary" else carry_gaul
    fight = fight_legionary if kind == "legionary" else fight_gaul
    frames = [("idle", lambda: (upright(), stand(), carry(0.0)))]
    for k in range(8):
        frames.append((f"walk{k}", lambda k=k: (upright(), carry(stride(k)))))
    for k in range(6):
        frames.append((f"fight{k}", lambda k=k: (upright(), fight(k))))
    frames.append(("dead", lambda: fallen(kind)))
    return frames


meta = {"pitch_degrees": math.degrees(PITCH), "metres_per_pixel": PX, "directions": DIRECTIONS, "kinds": {}}
kinds = ONLY or ["legionary", "gaul", "legate", "commander"]
for kind in kinds:
    if kind in ("legionary", "gaul"):
        worn = dress(kind)
        meta["kinds"][kind] = render_kind(kind, infantry_frames(kind), FOOT_CELL)
        for o in worn:
            bpy.data.objects.remove(o)
        continue
    worn = dress(kind)
    coat = "bay" if kind == "legate" else "grey"
    h_arm, h_body, walk = load_horse(coat)
    horse_pose(h_arm, walk, None)
    # The saddle sits on the horse's back above its middle spine bone.
    spine = h_arm.matrix_world @ h_arm.pose.bones["horse Spine1"].head
    hv = np.array([tuple(h_body.matrix_world @ v.co) for v in h_body.data.vertices])
    near = hv[(np.abs(hv[:, 1] - spine.y) < 0.12) & (np.abs(hv[:, 0]) < 0.1)]
    above = float(near[:, 2].max() - spine.z) if len(near) else 0.15
    cloak = material(f"{kind}_cloak", srgb(0x7A1E22) if kind == "legate" else srgb(0xB3221C), 0.9)
    cloth = material(f"{kind}_saddlecloth", srgb(0x3A2A20) if kind == "legate" else srgb(0x8E1E1A), 0.9)
    # The horse's own walk drifts forward; keep it over the origin.
    base = horse_pose(h_arm, walk, None).copy()
    home = h_arm.location.copy()

    def mounted(frame, h_arm=h_arm, walk=walk, base=base, above=above, cloak=cloak, cloth=cloth, home=home):
        h_arm.location = home
        pelvis = horse_pose(h_arm, walk, frame)
        h_arm.location = home + Vector((base.x - pelvis.x, base.y - pelvis.y, 0))
        update()
        seat = seat_rider(h_arm, above)
        saddle(seat, cloth)
        rider_cloak(cloak, seat, 0.0 if frame is None else math.sin(frame / 8 * math.tau))
        if kind == "commander":
            # The right hand raised, pointing the way.
            arm_to("R", J("R UpperArm") + Vector((-0.3, -0.4, 0.25)), Vector((-1, 0.3, -0.5)))

    frames = [("idle", lambda: mounted(None))] + [(f"walk{k}", lambda k=k: mounted(k)) for k in range(8)]
    meta["kinds"][kind] = render_kind(kind, frames, HORSE_CELL)
    upright()
    arm.location = REST_PELVIS
    for o in worn + [h_arm, h_body]:
        bpy.data.objects.remove(o)

old = {}
path = os.path.join(OUT, "sprites.json")
if os.path.exists(path) and ONLY:
    old = json.load(open(path))
    old.setdefault("kinds", {}).update(meta["kinds"])
    meta = old
json.dump(meta, open(path, "w"), indent=2)
print("DONE", list(meta["kinds"]))
