import io
import re
from math import ceil
from urllib.parse import quote

import httpx
from PIL import Image, UnidentifiedImageError

SPRITE_SIZE = 48
ATLAS_ROW_LENGTH = 16
CSS = "#%s {\n  background-position: %dpx %dpx\n}\n\n"
UNKNOWN_IMAGE_URL = "http://localhost:3000/v1/follower/Coloured%2FDeer?animation=shrug&format=png&start_time=.6"


def slugify(s):
    s = re.sub(r"[^A-Za-z0-9]", "-", s)
    s = re.sub(r"([a-z])([A-Z])", "\\1-\\2", s)
    return s.lower()


def make_spritesheet(items, url, filename, css_prefix=""):
    row_count = int(ceil(len(items) / ATLAS_ROW_LENGTH))
    image_width = 4 + (SPRITE_SIZE + 2) * ATLAS_ROW_LENGTH
    image_height = 4 + (SPRITE_SIZE + 2) * row_count

    image = Image.new("RGBA", (image_width, image_height))
    x = 2
    y = 2
    this_row = 0
    css = open(f"{filename}.css", "w")

    for i, item in enumerate(items):
        slug = slugify(item["name"])
        print(f"#{i}", item["name"], slug, f"({x}, {y})")
        resp = httpx.get(url % quote(item["name"], safe=''))
        resp.raise_for_status()

        try:
            im = Image.open(io.BytesIO(resp.content))
        except UnidentifiedImageError:
            resp = httpx.get(UNKNOWN_IMAGE_URL)
            resp.raise_for_status()
            im = Image.open(io.BytesIO(resp.content))

        im.thumbnail((SPRITE_SIZE, SPRITE_SIZE))
        assert x + SPRITE_SIZE < image_width
        assert y + SPRITE_SIZE < image_height
        image.paste(im, (x, y))
        css.write(CSS % (css_prefix + slug, -x, -y))
        this_row += 1
        if this_row == ATLAS_ROW_LENGTH:
            x = 2
            y += SPRITE_SIZE + 2
            this_row = 0
        else:
            x += SPRITE_SIZE + 2

    image.save(f"{filename}.png")


def skins(data, animation="walk"):
    skin_count = len(data["skins"])
    row_count = int(ceil(skin_count / ATLAS_ROW_LENGTH))
    image_width = 4 + (SPRITE_SIZE + 2) * ATLAS_ROW_LENGTH
    image_height = 4 + (SPRITE_SIZE + 2) * row_count

    image = Image.new("RGBA", (image_width, image_height))
    x = 2
    y = 2
    css = open(f"{SPRITE}-animations.css", "w")

    for skin in data["skins"]:
        slug = slugify(skin["name"])
        print(skin["name"], slug)
        resp = httpx.get(f"http://localhost:3000/v1/{SPRITE}/{quote(skin['name'], safe='')}",
                         params={"animation": animation, "format": "png"})
        resp.raise_for_status()
        try:
            im = Image.open(io.BytesIO(resp.content))
        except UnidentifiedImageError:
            continue
        im.thumbnail((SPRITE_SIZE, SPRITE_SIZE))
        image.paste(im, (pos, 2))
        css.write(CSS % (slug, -pos, -2))
        pos += SPRITE_SIZE + 2

    image.save(f"{SPRITE}-skins.png")


def skins_dumb(data, animation="walk"):
    for skin in data["skins"]:
        slug = slugify(skin["name"])
        print(skin["name"], slug)
        resp = httpx.get(f"http://localhost:3000/v1/{SPRITE}/{quote(skin['name'], safe='')}",
                         params={"animation": animation, "format": "png"})
        resp.raise_for_status()
        with open(f"{SPRITE}/skins/{slug}.png", "wb") as f:
            f.write(resp.content)


def animations(data, skin="Coloured/Fox", fps=6, duration=1):
    pos = 2
    css = open(f"{SPRITE}-animations.css", "w")

    frames = [Image.new("RGBA", (4 + len(data["animations"]) * (SPRITE_SIZE + 2), SPRITE_SIZE + 4)) for _ in range(round(fps * duration))]

    for animation in data["animations"]:
        slug = slugify(animation["name"])
        print(animation["name"], slug)
        resp = httpx.get(f"http://localhost:3000/v1/{SPRITE}/{quote(skin, safe='')}",
                         params={"animation": animation["name"], "format": "apng", "end_time": str(duration), "fps": str(fps), "scale": "0.5"})
        resp.raise_for_status()

        try:
            im = Image.open(io.BytesIO(resp.content))
        except UnidentifiedImageError:
            continue

        for i, frame in enumerate(frames):
            im.seek(i % im.n_frames)
            frame.paste(im, (pos, 2))

        css.write(CSS % (slug, -pos, -2))
        pos += SPRITE_SIZE + 2

    for i, frame in enumerate(frames):
        frame.save(f"{SPRITE}-animations.{i}.png")


def animations_plain(data, skin="Coloured/Fox", timestamp=0.25):
    pos = 2
    css = open(f"{SPRITE}-animations.css", "w")

    frames = [Image.new("RGBA", (4 + len(data["animations"]) * (SPRITE_SIZE + 2), SPRITE_SIZE + 4)) for _ in range(round(fps * duration))]

    for animation in data["animations"]:
        slug = slugify(animation["name"])
        print(animation["name"], slug)
        resp = httpx.get(f"http://localhost:3000/v1/{SPRITE}/{quote(skin, safe='')}",
                         params={"animation": animation["name"], "format": "apng", "end_time": str(duration), "fps": str(fps), "scale": "0.5"})
        resp.raise_for_status()

        try:
            im = Image.open(io.BytesIO(resp.content))
        except UnidentifiedImageError:
            continue

        for i, frame in enumerate(frames):
            im.seek(i % im.n_frames)
            frame.paste(im, (pos, 2))

        css.write(CSS % (slug, -pos, -2))
        pos += SPRITE_SIZE + 2

    for i, frame in enumerate(frames):
        frame.save(f"{SPRITE}-animations.{i}.png")


def animations_dumb(data, skin="Coloured/Fox", fps=6, duration=1):
    for animation in data["animations"]:
        slug = slugify(animation["name"])
        print(animation["name"], slug)
        resp = httpx.get(f"http://localhost:3000/v1/{SPRITE}/{quote(skin, safe='')}",
                         params={"animation": animation["name"], "format": "gif", "end_time": str(duration), "fps": str(fps), "scale": "0.5"})
        resp.raise_for_status()
        with open(f"{SPRITE}/animations/{slug}.gif", "wb") as f:
            f.write(resp.content)


if 0:
    sprite = "player"
    animation = "idle"
    skin = "Lamb"
elif 0:
    sprite = "follower"
    animation = "idle"
    skin = "Fox"
else:
    sprite = "ratau"
    animation = "idle"
    skin = "normal"

data = httpx.get(f"http://localhost:3000/v1/{sprite}").json()

make_spritesheet(data["animations"], f"http://localhost:3000/v1/{sprite}/{skin}?animation=%s&format=png&start_time=0.25", f"{sprite}-animations", css_prefix=sprite + "-animations-")
make_spritesheet(data["skins"], f"http://localhost:3000/v1/{sprite}/%s?animation={animation}&format=png", f"{sprite}-skins", css_prefix=sprite + "-skins-")
