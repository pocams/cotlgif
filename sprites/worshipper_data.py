import os
import struct
import json


class WorshipperData:
    def __init__(self, filename):
        self.f = open(filename, "rb")
        self.f.seek(0, os.SEEK_END)
        self.length = self.f.tell()
        self.f.seek(0, os.SEEK_SET)

    def at_eof(self):
        return self.f.tell() == self.length

    def read_u32(self):
        return struct.unpack("<L", self.f.read(4))[0]

    def read_f32(self):
        return struct.unpack("<f", self.f.read(4))[0]

    def read_string(self):
        length = self.read_u32()
        s = self.f.read(length)
        if length % 4 != 0:
            padding = self.f.read(4 - (length % 4))
            for p in padding:
                if p != 0:
                    raise ValueError("String {} padded with {:r}", s, padding)
        return s.decode("utf-8")

    def read_set(self):
        colors = {}
        slot_count = self.read_u32()
        for _ in range(slot_count):
            slot = self.read_string()
            r = self.read_f32()
            g = self.read_f32()
            b = self.read_f32()
            a = self.read_f32()
            colors[slot] = {
                "r": r,
                "g": g,
                "b": b,
                "a": a,
            }
        colors["last"] = self.read_f32(), self.read_f32(), self.read_f32(), self.read_f32()
        return colors

    def read_skin(self):
        name = self.read_string()
        zone = self.read_u32()
        is_blocked = self.read_u32()
        is_toww = self.read_u32()
        is_boss = self.read_u32()

        skins = []
        skin_count = self.read_u32()
        for i in range(skin_count):
            skins.append(self.read_string())

        sets = []
        set_count = w.read_u32()
        for s in range(set_count):
            sets.append(self.read_set())

        last = self.read_u32()
        return {
            "name": name,
            "zone": zone,
            "is_blocked": is_blocked,
            "is_toww": is_toww,
            "is_boss": is_boss,
            "skins": skins,
            "sets": sets,
            "last": last
        }


w = WorshipperData("/Users/mark/dev/cotlgif/cotl/Worshipper Data.dat")

unk = []
for i in range(11):
    unk.append(w.read_u32())

initial_sets = []
set_count = w.read_u32()
for s in range(set_count):
    initial_sets.append(w.read_set())
last = w.read_u32()

skins = []
while True:
    if w.at_eof():
        break
    skins.append(w.read_skin())

with open("worshipper_data.json", "w") as data:
    json.dump({
        "global": initial_sets,
        "skins": skins
    }, data)
