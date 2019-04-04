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
# if repeated build are done it can be made faster by mapping volumes to /.cargo and /.npm
docker run -it --rm -v $(pwd):/src -u $(id -u) $OPTS_ARGS audioserve-builder