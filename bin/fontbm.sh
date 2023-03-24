#!/bin/sh

set -e

# Absolute path to this script, e.g. /home/user/bin/foo.sh
SCRIPT=$(readlink -f "$0")

# Absolute path this script is in, thus /home/user/bin
SCRIPT_DIR=$(dirname "$SCRIPT")

$SCRIPT_DIR/fontbm/$1/fontbm \
    --font-file $SCRIPT_DIR/../art/font/kenney_mini_square_mono.ttf \
    --font-size 8 \
    --color 255,0,0 \
    --monochrome \
    --output $SCRIPT_DIR/../art/font/kenney_mini_square_mono
