import argparse
import bpy
import math
import mathutils
import os
from re import sub
import sys

if (3, 4, 1) > bpy.app.version:
    raise Exception('Blender version must be >= 3.4.1')

if "--" not in sys.argv:
    argv = []  # as if no args are passed
else:
    argv = sys.argv[sys.argv.index("--") + 1:]  # get all args after "--"

parser = argparse.ArgumentParser(description='Export a .blend scene as .glb')
parser.add_argument('filepath', metavar='FILEPATH', help='a .blend file')
args = parser.parse_args(argv)

stem, ext = os.path.splitext(args.filepath)

def snake_case(s):
    return '_'.join(
        sub('([A-Z][a-z]+)', r' \1',
        sub('([A-Z]+)', r' \1',
        s.replace('-', ' '))).split()).lower()

def write_id(f, obj):
    if obj.name != 'Object' and not obj.data.library:
        f.write(f'id = \'{obj.name}\'\n')
    elif obj.data.library:
        obj_id = obj.get('id')
        if obj_id:
            f.write(f'id = \'{obj_id}\'\n')


def write_tags(f, obj):
    tags = obj.get('tags')
    if tags:
        f.write('\ntags = [\n')

        for item in tags.split(','):
            f.write(f'    \'{item}\',\n')

        f.write(']')

def write_transform(f, obj):
    f.write(f'position = [{obj.location.x}, {obj.location.z}, {-obj.location.y}]\n')

    rot = obj.rotation_euler.to_quaternion()

    f.write(f'rotation = [{rot.x}, {rot.z}, {-rot.y}, {rot.w}]')

def write_geometry(f, obj):
    mesh = obj.to_mesh()
    if mesh:
        f.write('\n\n[[scene.geometry]]\n')

        write_id(f, obj)

        f.write('indices = [\n')
        for triangle in mesh.loop_triangles:
            f.write(f'    {triangle.vertices[0]}, {triangle.vertices[1]}, {triangle.vertices[2]},\n')
        f.write(']\n')

        f.write('vertices = [\n')
        for vertex in mesh.vertices:
            f.write(f'    {vertex.co.x}, {vertex.co.z}, {-vertex.co.y},\n')
        f.write(']\n')

        write_transform(f, obj)
        write_tags(f, obj)

def write_scene_refs(f, obj):
    f.write('\n\n[[scene.ref]]\n')

    write_id(f, obj)

    if obj.data.library:
        toml_stem, _ = os.path.splitext(obj.data.library.filepath.removeprefix('//'))
        toml_stem = toml_stem.replace('\\', '/')
        f.write(f'model = \'{toml_stem}.toml\'\n')

        f.write('materials = [\n')

        # If materials are stored as a custom string property, we'll use that
        materials = obj.get('materials')
        if materials:
            for item in materials.split(','):
                f.write(f'    \'{item}\',\n')
        else:
            # Hacky way of connecting material names to hard-coded material folder
            for item in obj.data.materials:
                f.write(f'    \'../material/{snake_case(item.name)}.toml\',\n')

        f.write(']\n')

    write_transform(f, obj)
    write_tags(f, obj)

with open(args.filepath, 'w') as f:
    f.write('[scene]')

    geometry = bpy.data.collections['Geometry']
    if geometry:
        for obj in geometry.objects:
            write_geometry(f, obj)

        for collection in geometry.children:
            for obj in collection.objects:
                write_geometry(f, obj)

    scene = bpy.data.collections['Scene']
    if scene:
        for obj in scene.objects:
            write_scene_refs(f, obj)

        for collection in scene.children:
            for obj in collection.objects:
                write_scene_refs(f, obj)