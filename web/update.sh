#!/bin/bash

npm run build
cp -f dist/index.html ../html/index.html
rm ../static/index.*.js ../static/index.*.css
cp dist/assets/* ../static/
