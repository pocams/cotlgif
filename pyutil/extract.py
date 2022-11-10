import glob
import json
import os

import UnityPy

PATH = "/Users/mark/Library/Application Support/Steam/steamapps/common/Cult of the Lamb/Cult Of The Lamb.app/Contents/Resources/Data"


texture2d_locations = {}


if 0:
    print("Extracting TextAssets")
    for root, dirs, files in os.walk(PATH):
        for filename in files:
            path = os.path.join(root, filename)
            env = UnityPy.load(path)

            for obj in env.objects:
                if obj.type.name == "TextAsset":
                    data = obj.read()
                    dest = f"extracted/{data.name}"
                    print(dest)
                    with open(dest, "wb") as out:
                        out.write(data.m_Script)
                elif obj.type.name == "Texture2D":
                    data = obj.read()
                    texture2d_locations[data.name] = path


    with open("texture2d_locations.json", "w") as j:
        json.dump(texture2d_locations, j)
else:
    with open("texture2d_locations.json") as j:
        texture2d_locations = json.load(j)


print("Finding wanted textures")
wanted_textures = set()
files_to_read = set()

for atlas in glob.glob("extracted/*.atlas"):
    for line in open(atlas, "rt"):
        line = line.strip()
        if line:
            texture_name = line.replace(".png", "")
            print(texture_name)
            wanted_textures.add(texture_name)
            try:
                files_to_read.add(texture2d_locations[texture_name])
            except KeyError:
                print(f"!!! No texture found for {texture_name}!")
            break


print(f"Extracting Texture2Ds from {len(files_to_read)} files")
for file_to_read in files_to_read:
    env = UnityPy.load(file_to_read)

    for obj in env.objects:
        if obj.type.name == "Texture2D":
            data = obj.read()
            if data.name in wanted_textures:
                dest = f"extracted/{data.name}.png"
                data.image.save(dest)
