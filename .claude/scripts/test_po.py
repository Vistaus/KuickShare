"""Run with: python3 .claude/scripts/test_po.py"""

import importlib.util
import tempfile
import unittest
from pathlib import Path

spec = importlib.util.spec_from_file_location("po", Path(__file__).with_name("po.py"))
po = importlib.util.module_from_spec(spec)
spec.loader.exec_module(po)

CATALOG = '''msgid ""
msgstr ""
"Language: it\\n"

#: a.qml:1
msgid "Done"
msgstr "Fatto"

#: a.qml:2
#, fuzzy
msgid "Send"
msgstr "Vecchio"

#: a.qml:3
msgid ""
"Two lines "
"joined"
msgstr ""

#: a.qml:4
msgid "Say \\"hi\\""
msgstr ""'''


class PoTest(unittest.TestCase):
    def setUp(self) -> None:
        self.dir = tempfile.TemporaryDirectory()
        self.addCleanup(self.dir.cleanup)
        root = Path(self.dir.name)
        (root / "LINGUAS").write_text("it\n")
        (root / "it.po").write_text(CATALOG, encoding="utf-8")
        self.original = po.PO_DIR
        po.PO_DIR = root
        self.addCleanup(setattr, po, "PO_DIR", self.original)

    def test_pending_lists_fuzzy_and_untranslated_but_not_the_header(self) -> None:
        self.assertEqual(po.pending("it"), ["Send", "Two lines joined", 'Say "hi"'])

    def test_apply_fills_entries_clears_fuzzy_and_keeps_the_rest(self) -> None:
        result = po.apply({"it": {"Send": "Invia", "Two lines joined": "Due righe", 'Say "hi"': 'Di\' "ciao"'}})
        self.assertEqual(result, {"it": 3})
        text = (po.PO_DIR / "it.po").read_text(encoding="utf-8")
        self.assertNotIn("fuzzy", text)
        self.assertIn('msgstr "Fatto"', text)
        self.assertIn('msgstr "Invia"', text)
        self.assertIn('msgstr "Di\' \\"ciao\\""', text)
        self.assertEqual(po.pending("it"), [])


if __name__ == "__main__":
    unittest.main()
