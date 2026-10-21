"""Explicit experimental provider; no native spqx/MLX/GGML claims or fallbacks."""
import base64
import hashlib
import io
from importlib.metadata import version as package_version
import json
import math
import os
from pathlib import Path
import subprocess
import sys
import time

import numpy as np
import soundfile as sf
import torch
from safetensors.torch import load_file
from transformers import set_seed
from qwen_tts import Qwen3TTSModel
from qwen_tts.inference.qwen3_tts_model import VoiceClonePromptItem

root = Path(sys.argv[1]).resolve()
request = json.loads(sys.stdin.read(2049))
if request['base'] not in ('A','C','D') or len(request['axes']) != 2 or any(type(v) not in (float,int) or not math.isfinite(v) or abs(v)>.75 for v in request['axes']):
    raise ValueError('invalid recipe')
basis_hash = hashlib.sha256((root/'basis.npz').read_bytes()).hexdigest()
if request['space'] != basis_hash:
    raise ValueError('wrong conditioning space')
spec = json.loads((root/'experiment.json').read_text())
expected = '5d83992436eae1d760afd27aff78a71d676296fc'
if spec['models']['base']['revision'] != expected or not torch.cuda.is_available():
    raise RuntimeError('required CUDA model unavailable; no fallback')
torch.set_num_threads(2)
torch.set_num_interop_threads(1)
torch.cuda.set_per_process_memory_fraction(2.6*1024**3/torch.cuda.get_device_properties(0).total_memory)
arrays=np.load(root/'basis.npz',allow_pickle=False)
vectors=load_file(root/'conditioning.safetensors')
base=vectors['source_'+request['base']].numpy().astype(np.float64)
if base.shape != (1024,) or arrays['components'].shape != (2,1024):
    raise ValueError('wrong conditioning dimensions')
# Raw SDK space, no L2 normalization; preserve the selected base residual.
vector=base+(np.array(request['axes'])*arrays['std']) @ arrays['components']
if not np.isfinite(vector).all():
    raise ValueError('nonfinite conditioning')
model_path=json.loads((root/'evidence/model-paths.json').read_text())['base']
if expected not in model_path:
    raise ValueError('wrong cached checkpoint')
started=time.monotonic()
model=Qwen3TTSModel.from_pretrained(model_path,device_map='cuda:0',dtype=torch.bfloat16,attn_implementation='sdpa')
prompt=VoiceClonePromptItem(ref_code=None,ref_spk_embedding=torch.tensor(vector,device='cuda:0',dtype=torch.bfloat16),x_vector_only_mode=True,icl_mode=False,ref_text=None)
set_seed(spec['seed'])
with torch.inference_mode():
    waves,sr=model.generate_voice_clone(text=spec['preview_text'],language='German',voice_clone_prompt=[prompt],**spec['sampling'])
wave=np.asarray(waves[0],dtype=np.float32)
if wave.ndim != 1 or sr!=24000 or not np.isfinite(wave).all() or not 2 <= len(wave)/sr <= 18.5 or float(np.max(np.abs(wave)))>=.99999 or float(np.sqrt(np.mean(wave**2)))<1e-5:
    raise ValueError('invalid/silent/clipped/potentially capped preview')
frozen_record={'version':1,'provider':'qwen-sdk','sdk_version':package_version('qwen-tts'),'model':spec['models']['base'],'representation':'effective-bfloat16-as-f32','dimension':1024,'values':prompt.ref_spk_embedding.detach().float().cpu().tolist(),'recipe':request,'basis_sha256':basis_hash,'source_vectors_sha256':hashlib.sha256((root/'conditioning.safetensors').read_bytes()).hexdigest()}
result=subprocess.run(['nvidia-smi','--query-compute-apps=pid,used_gpu_memory','--format=csv,noheader,nounits'],capture_output=True,text=True,check=True)
owned=sum(int(line.split(',')[1]) for line in result.stdout.splitlines() if line.split(',')[0].strip()==str(os.getpid()))
if owned>3328:
    raise RuntimeError('owned GPU footprint exceeded reservation')
buffer=io.BytesIO();sf.write(buffer,wave,sr,format='WAV',subtype='FLOAT')
print('VOICE_LAB_RESULT '+json.dumps({'sdk_conditioning_json':json.dumps(frozen_record,allow_nan=False),'wave':base64.b64encode(buffer.getvalue()).decode(),'seconds':len(wave)/sr,'model':expected,'basis':basis_hash,'elapsed_seconds':time.monotonic()-started,'sampled_owned_mib':owned,'cuda_allocated_gib':torch.cuda.max_memory_allocated()/1024**3}),flush=True)
