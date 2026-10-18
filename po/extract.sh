#!/bin/sh
# Regenerates po/kuickshare.pot from the files in po/POTFILES.in and merges it into the
# translations listed in po/LINGUAS (one po/<lang>.po each):
#   build/build-env.sh po/extract.sh
set -eu
cd "$(dirname "$0")/.."
xgettext --from-code=UTF-8 --language=C++ --add-comments=TRANSLATORS \
    --keyword=i18n:1 --keyword=i18nc:1c,2 --keyword=i18np:1,2 --keyword=i18ncp:1c,2,3 \
    --package-name=kuickshare --package-version=1.0.0 --copyright-holder="the-beper" \
    --msgid-bugs-address= \
    --files-from=po/POTFILES.in -o po/kuickshare.pot
# xgettext leaves placeholders in the first lines of the template: fill in the ones that are ours.
sed -i \
    -e 's/^# SOME DESCRIPTIVE TITLE\./# Translation template for KuickShare./' \
    -e 's/^# Copyright (C) YEAR /# Copyright (C) 2026 /' \
    -e 's/^# FIRST AUTHOR <EMAIL@ADDRESS>, YEAR\./# the-beper <193650145+the-beper@users.noreply.github.com>, 2026./' \
    po/kuickshare.pot
for lang in $(cat po/LINGUAS); do
    msgmerge --update --backup=none --quiet "po/$lang.po" po/kuickshare.pot
    # Drop the entries of strings that are gone, instead of keeping them as obsolete (#~) ones.
    msgattrib --no-obsolete --output-file="po/$lang.po" "po/$lang.po"
done
