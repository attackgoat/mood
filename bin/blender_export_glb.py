import argparse
import bpy
import os
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

# See: https://docs.blender.org/api/current/bpy.ops.export_scene.html#bpy.ops.export_scene.gltf
bpy.ops.export_scene.gltf(filepath=stem + '.glb',
    check_existing=False,
    export_format='GLB',
    ui_tab='GENERAL',
    export_copyright='',
    export_image_format='AUTO',
    export_texture_dir='',
    export_keep_originals=False,
    export_texcoords=True,
    export_normals=True,
    export_draco_mesh_compression_enable=False,
    export_draco_mesh_compression_level=6,
    export_draco_position_quantization=14,
    export_draco_normal_quantization=10,
    export_draco_texcoord_quantization=12,
    export_draco_color_quantization=10,
    export_draco_generic_quantization=12,
    export_tangents=True,
    export_materials='PLACEHOLDER',
    export_colors=True,
    use_mesh_edges=False,
    use_mesh_vertices=False,
    export_cameras=False,
    use_selection=False,
    use_visible=False,
    use_renderable=False,
    use_active_collection=False,
    export_extras=False,
    export_yup=True,
    export_apply=True,
    export_animations=True,
    export_frame_range=True,
    export_frame_step=1,
    export_force_sampling=True,
    export_nla_strips=True,
    export_def_bones=False,
    export_current_frame=False,
    export_skins=True,
    export_all_influences=False,
    export_morph=True,
    export_morph_normal=True,
    export_morph_tangent=False,
    export_lights=False,
    will_save_settings=False,
    filter_glob='*.glb;*.gltf')
