#!/bin/bash
set -e -x
OPTS_ARGS=""
if [[ -n "$FEATURES" ]]; then 
    OPTS_ARGS=$OPTS_ARGS" -e FEATURES=$FEATURES" 
fi

if [[ -n "$CARGO_ARGS" ]]; then
    OPTS_ARGS=$OPTS_ARGS" -e CARGO_ARGS=$CARGO_ARGS" 

fi

docker build --tag audioserve-builder -f Dockerfile.static .
# if repeated build are done it can be made faster by mapping volumes to root/.cargo and root/.npm
docker run -i --rm -v $(pwd):/src -u $(id -u) --mount type=volume,src=audioserve_static_build_cargo,dst=/root/.cargo \
$OPTS_ARGS audioserve-builder