"""Collect complete versioned vendor terms, never page metadata or models."""
from html.parser import HTMLParser
from pathlib import Path
from urllib.request import urlopen
import sys
import io
import zipfile
from xml.etree import ElementTree
import re


def cuda_terms(version, output):
    if not re.fullmatch(r'\d+\.\d+\.\d+', version):
        raise ValueError('use an explicit CUDA documentation version')
    url='https://docs.nvidia.com/cuda/archive/'+version+'/eula/index.html'
    class Article(HTMLParser):
        active=False
        parts=[]
        def handle_starttag(self, tag, attrs):
            if tag=='article' and dict(attrs).get('id')=='contents': self.active=True
        def handle_endtag(self, tag):
            if tag=='article': self.active=False
            if self.active and tag in ('p','li','h1','h2','h3','h4','pre'): self.parts.append('\n')
        def handle_data(self, data):
            if self.active:self.parts.append(data)
    with urlopen(url,timeout=30) as response: document=response.read().decode('utf-8')
    parser=Article();parser.feed(document);body=''.join(parser.parts)
    if len(body)<20000 or 'libcudart.so' not in body or 'libcublas.so' not in body:
        raise ValueError('unexpected or incomplete CUDA EULA; manual review required')
    output.write_text('Source: '+url+'\n\n'+body,encoding='utf-8')

TERMS = 'https://visualstudio.microsoft.com/wp-content/uploads/2021/09/Visual-C-Runtime-2015-2022-License-1.docx'

URL = 'https://learn.microsoft.com/en-us/visualstudio/releases/2022/redistribution'
class Visible(HTMLParser):
    def __init__(self):
        super().__init__(); self.active=False; self.parts=[]
    def handle_starttag(self, tag, attrs):
        if tag=='main': self.active=True
    def handle_endtag(self, tag):
        if tag=='main': self.active=False
        if self.active and tag in ('p','li','h1','h2','h3','tr'): self.parts.append('\n')
    def handle_data(self, data):
        if self.active: self.parts.append(data)

if __name__=='__main__':
    if len(sys.argv)==4 and sys.argv[1]=='--cuda':
        cuda_terms(sys.argv[2],Path(sys.argv[3]));raise SystemExit(0)
    with urlopen(URL, timeout=30) as response: data=response.read().decode('utf-8')
    parser=Visible(); parser.feed(data); text=''.join(parser.parts)
    if 'Visual C++ Runtime Redistributable' not in text or 'devdiv.visualstudio.com' in text:
        raise SystemExit('unexpected redistribution document; manual review required')
    output = Path(sys.argv[1])
    output.write_text('Source: '+URL+'\n\n'+text, encoding='utf-8')
    with urlopen(TERMS, timeout=30) as response: document=response.read()
    tree=ElementTree.fromstring(zipfile.ZipFile(io.BytesIO(document)).read('word/document.xml'))
    ns={'w':'http://schemas.openxmlformats.org/wordprocessingml/2006/main'}
    body='\n'.join(''.join(t.text or '' for t in p.findall('.//w:t',ns)) for p in tree.findall('.//w:p',ns))
    if 'MICROSOFT VISUAL C++' not in body: raise SystemExit('unexpected runtime terms')
    output.with_name('MSVC-RUNTIME-LICENSE.docx').write_bytes(document)
    output.with_name('MSVC-RUNTIME-LICENSE.txt').write_text('Source: '+TERMS+'\n\n'+body,encoding='utf-8')
