#!/bin/bash

npm run build
sed -i 's#assets/##' dist/index.html
cp -f dist/index.html ../html/index.html
rm ../static/index.*.js ../static/index.*.css
cp dist/assets/* ../static/
