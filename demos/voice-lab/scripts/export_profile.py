import hashlib,json,sys
from pathlib import Path
import numpy as np
r=Path(sys.argv[1]);spec=json.loads((r/'experiment.json').read_text());summary=json.loads((r/'summary.json').read_text())
a=np.load(r/'basis.npz',allow_pickle=False)
print(json.dumps({'title':'Voice Lab','source_vectors_sha256':hashlib.sha256((r/'conditioning.safetensors').read_bytes()).hexdigest(),'space':hashlib.sha256((r/'basis.npz').read_bytes()).hexdigest(),'model':spec['models']['base'],'text':spec['preview_text'],'seed':spec['seed'],'dimension':1024,'voices':[{'id':v['id'],'note':v['review'],'position':(a['scores'][i]/a['std']).tolist()} for i,v in enumerate(spec['bank'])],'clips':[{'id':c['id'],'seconds':c['seconds']} for c in summary['clips']],'capabilities':{'frozen_conditioning':True,'live_preview':True,'registry_promote':False,'pca_calibrated':False}}))
