#!/bin/bash

set -euo pipefail

pngquant -f ./*.png
for p in *-fs8.png; do mv -f "$p" "../static/${p%-fs8.png}.png"; done
cat ./*.css > ../static/sprites.css
