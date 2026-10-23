---
name: update-translations
description: Use after adding or changing an i18n() string in src/shell, or when po/*.po have untranslated or fuzzy entries. Regenerates the gettext catalogs and has the translator agent fill them.
---

# Update the translations

1. If a new source file with `i18n()` calls was added, list it in `po/POTFILES.in`.
2. Regenerate the catalogs from the repository root (never edit `po/kuickshare.pot` by hand):
   ```sh
   build/build-env.sh po/extract.sh
   python3 .claude/scripts/po.py status
   ```
3. If anything is pending, delegate to the `translator` agent (haiku) with the Agent tool: "Translate the pending entries of po/*.po". It reads `python3 .claude/scripts/po.py todo` itself.
4. Check yourself, do not trust the report alone:
   ```sh
   python3 .claude/scripts/po.py status        # every language: 0 pending
   for l in $(cat po/LINGUAS); do msgfmt -c --statistics -o /dev/null po/$l.po; done
   ```
5. Optional smoke test of the shell: `build/build-env.sh cmake --build dist/tmp/cmake-dev && build/build-env.sh ctest --test-dir dist/tmp/cmake-dev`.
6. Tell the user that `nl` and `tr` are machine translations without a native review (the `tr` catalog came from a contributor; the strings added since are ours).

A changed msgid makes `msgmerge` mark the old translation fuzzy: the translator rewrites it, so no fuzzy entry must survive. Run the script test after touching `.claude/scripts/po.py`: `python3 -I .claude/scripts/test_po.py`.
