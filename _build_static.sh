#!/bin/bash
set -e
BUILD_DIR="_static_build"

cargo build --target x86_64-alpine-linux-musl --release ${CARGO_ARGS} --features static,${FEATURES}
cd client
npm install
npm run build

cd ..

VERSION=`grep  -m 1  "version" Cargo.toml | sed 's/.*"\(.*\)".*/\1/'`
AS_DIR="audioserve_static_v$VERSION"

if [[ -d $BUILD_DIR ]]; then
rm -r $BUILD_DIR
fi

mkdir -p $BUILD_DIR/$AS_DIR/client

cp target/x86_64-alpine-linux-musl/release/audioserve $BUILD_DIR/$AS_DIR
cp -r client/dist $BUILD_DIR/$AS_DIR/client

cd $BUILD_DIR
tar czvf audioserve_static.tar.gz $AS_DIR

