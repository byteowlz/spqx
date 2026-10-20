"""Model-free bundle integrity/escape regressions; run with uv/unittest."""
import hashlib
import json
from pathlib import Path
import tempfile
import unittest
import package

class Integrity(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        (self.root/'file').write_bytes(b'public synthetic fixture')
        self.record = {'file': {'sha256': hashlib.sha256(b'public synthetic fixture').hexdigest()}}
        self.save()
    def tearDown(self):
        self.temp.cleanup()
    def save(self):
        (self.root/'manifest.json').write_text(json.dumps({'files':self.record}))
    def test_good(self):
        self.assertEqual(package.verify(self.root),1)
    def test_changed(self):
        (self.root/'file').write_bytes(b'changed')
        with self.assertRaises(ValueError): package.verify(self.root)
    def test_missing(self):
        (self.root/'file').unlink()
        with self.assertRaises(ValueError): package.verify(self.root)
    def test_parent_path(self):
        self.record['../outside']={'sha256':'bad'};self.save()
        with self.assertRaises(ValueError): package.verify(self.root)
    def test_absolute_path(self):
        self.record[str(self.root/'absolute')]={'sha256':'bad'};self.save()
        with self.assertRaises(ValueError): package.verify(self.root)
    def test_escaping_link(self):
        (self.root/'link').symlink_to(self.root.parent)
        self.record['link']={'symlink':str(self.root.parent)};self.save()
        with self.assertRaises(ValueError): package.verify(self.root)

if __name__=='__main__': unittest.main()
