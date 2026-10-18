#!/bin/sh
# Takes every screenshot of docs/screenshots that can be scripted. Close other windows first and
# leave the desktop on the light theme: the active window is what gets captured.
# pair.png needs a click and is taken by hand: Preferences > Pair a device, then capture the dialog.
here=$(cd "$(dirname "$0")" && pwd)
"$here/shoot.sh" send --page send
"$here/shoot.sh" send-files --page send photo-1.jpg photo-2.jpg notes.pdf
"$here/shoot.sh" transfers --page transfers
"$here/shoot.sh" preferences --page settings
