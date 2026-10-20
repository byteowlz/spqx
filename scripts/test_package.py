"""Model-free bundle integrity/escape regressions; run with uv/unittest."""
import hashlib
import json
import os
import subprocess
import struct
import zipfile
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

    def test_binary_architecture(self):
        path=self.root/'binary';data=bytearray(128);data[:6]=b'\x7fELF\x02\x01';struct.pack_into('<H',data,18,62);path.write_bytes(data)
        package.check_binary(path,'x86_64-unknown-linux-gnu')
        with self.assertRaises(ValueError): package.check_binary(path,'aarch64-apple-darwin')
        data=bytearray(128);struct.pack_into('<II',data,0,0xfeedfacf,0x100000c);path.write_bytes(data)
        package.check_binary(path,'aarch64-apple-darwin')
        data=bytearray(128);data[:2]=b'MZ';struct.pack_into('<I',data,60,80);data[80:84]=b'PE\0\0';struct.pack_into('<H',data,84,0x8664);path.write_bytes(data)
        package.check_binary(path,'x86_64-pc-windows-msvc')
    def test_extra_file(self):
        (self.root/'unexpected.dll').write_bytes(b'not recorded')
        with self.assertRaises(ValueError): package.verify(self.root)
    def test_old_zip_timestamp(self):
        os.utime(self.root/'file',(0,0))
        archive=package.write_archive(self.root,True)
        try:
            with zipfile.ZipFile(archive) as z:
                self.assertEqual(z.read(self.root.name+'/file'),b'public synthetic fixture')
                self.assertGreaterEqual(z.getinfo(self.root.name+'/file').date_time[0],1980)
        finally: archive.unlink()
    @unittest.skipIf(os.name=='nt','Linux launcher uses GNU readlink')
    def test_installed_symlink_launcher(self):
        if subprocess.run(['readlink','-f','/'],capture_output=True).returncode != 0:
            self.skipTest('GNU readlink unavailable')
        stage=self.root/'stage';(stage/'bin').mkdir(parents=True)
        for name in ('spqx','spqx-tts-worker'):
            path=stage/'bin'/name;path.write_text('#!/usr/bin/env bash\nprintf synthetic-ok');path.chmod(0o755)
            package.unix_launcher(stage,name)
        package.installers(stage,'synthetic',False)
        prefix=self.root/'prefix with spaces'
        subprocess.run(['bash',str(stage/'install.sh'),str(prefix)],check=True,capture_output=True)
        result=subprocess.run([str(prefix/'bin/spqx')],check=True,capture_output=True,text=True)
        self.assertEqual(result.stdout,'synthetic-ok')
        again=subprocess.run(['bash',str(stage/'install.sh'),str(prefix)],capture_output=True)
        self.assertNotEqual(again.returncode,0)

if __name__=='__main__': unittest.main()
