import itertools

import httpx

PARAMS = "&ARM_LEFT_SKIN=%23ff0000&ARM_RIGHT_SKIN=%2300ff00&HEAD_SKIN_BTM=%230000ff&HEAD_SKIN_TOP=%23ffff00&LEG_LEFT_SKIN=%23ff00ff&LEG_RIGHT_SKIN=%2300ffff&MARKINGS=%23ff0055"
DOWN = "idle"
UP = "idle-up"

skins = httpx.get("https://cotl-spoilers.xl0.org/v1/follower/colours").json()["skins"]
all_skins = itertools.chain(*(f["skins"] for f in skins))

print("<body>")
for skin in all_skins:
    print(f"<p>{skin}")
    print(f'<img src="https://cotl-spoilers.xl0.org/v1/follower/{skin}?animation={DOWN}&{PARAMS}">')
    print(f'<img src="https://cotl-spoilers.xl0.org/v1/follower/{skin}?animation={UP}&{PARAMS}">')
    print("</p>")
print("</body>")
