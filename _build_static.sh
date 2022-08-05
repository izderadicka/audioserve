#!/bin/bash
set -e -x
BUILD_DIR="_static_build"
TARGET=x86_64-unknown-linux-musl
export HOME=/root

#Workaround for static builb of rust_uci
if [[ "$FEATURES" =~ "collation" ]]; then
    # assure that collation-static is used
    FEATURES=$(echo $FEATURES | perl -pe 's/collation(?!\-static)/collation-static/g' )
    export RUST_ICU_MAJOR_VERSION_NUMBER=67
    export RUSTFLAGS=$RUSTFLAGS" -L native=/usr/lib -l static=clang -l static=icui18n -l static=icuuc -l static=icudata -l static=stdc++"
fi

echo "RUSTFLAGS: $RUSTFLAGS"

cargo build --target $TARGET --release ${CARGO_ARGS} --features static,${FEATURES}

VERSION=`grep  -m 1  "version" Cargo.toml | sed 's/.*"\(.*\)".*/\1/'`
AS_DIR="audioserve_static_v$VERSION"

if [[ -d $BUILD_DIR ]]; then
rm -r $BUILD_DIR
fi

mkdir -p $BUILD_DIR/$AS_DIR

cp target/$TARGET/release/audioserve $BUILD_DIR/$AS_DIR

cd $BUILD_DIR
tar czvf audioserve_static.tar.gz $AS_DIR

