import glob
import os.path
import re

import tomlkit
import tomlkit.items


def slugify(s):
    s = re.sub(r"[^A-Za-z0-9]", "-", s)
    s = re.sub(r"([a-z])([A-Z])", "\\1-\\2", s)
    return s.lower()


def is_complete(skeleton):
    atlas = skeleton.replace(".skel", ".atlas")
    if os.path.exists(atlas):
        with open(atlas, "r") as atlas_f:
            for line in atlas_f:
                if line.strip():
                    if os.path.exists("cotl/" + line.strip()):
                        return True
                    else:
                        print(f"png missing: {line.strip()}")
    else:
        print(f"atlas missing: {atlas}")
        return False


with open("config.toml", "rb") as f:
    config = tomlkit.load(f)

existing = set(actor["skeleton"] for actor in config["actors"])

for skel in glob.glob("cotl/*.skel"):
    if skel not in existing:
        if is_complete(skel):
            print(skel)
            actor = tomlkit.table()
            actor.append("name", skel.replace(".skel", "").replace("cotl/", ""))
            actor.append("slug", slugify(skel.replace(".skel", "").replace("cotl/", "")))
            actor.append("atlas", skel.replace(".skel", ".atlas"))
            actor.append("skeleton", skel)
            actor.append("category", tomlkit.items.String(tomlkit.items.StringType.SLB, "Uncategorized", "Uncategorized", tomlkit.items.Trivia(trail="\n\n")))
            config["actors"].append(actor)
        else:
            print(skel, "(incomplete)")

with open("config.toml.new", "w") as config_f:
    tomlkit.dump(config, config_f)

print("\nWrote config.toml.new")
