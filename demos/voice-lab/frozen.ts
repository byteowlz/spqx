// Preserve the provider's numeric document bytes: parsing/re-encoding can lose -0 or f64 bits.
export type CapturedConditioning = { kind:'native'|'sdk'; raw:string; document:any };
function sameRecipe(value:any, recipe:any, native:boolean) {
 const axes=native?value?.offsets:value?.axes;
 return value?.space===recipe.space && value?.base===recipe.base && Array.isArray(axes) && axes.length===2 && axes.every((v:any,i:number)=>v===recipe.axes[i]);
}
function vector(values:any, dimension:number) {
 return Array.isArray(values)&&values.length===dimension&&values.every((v:any)=>typeof v==='number'&&Number.isFinite(v)&&Math.abs(v)<=1e4)&&values.some((v:number)=>v!==0);
}
export function captureConditioning(result:any,recipe:any,info:any,waveHash:string):CapturedConditioning|null {
 if(result.native_voice_json!==undefined && result.sdk_conditioning_json!==undefined)throw new Error('Ambiguous conditioning provider');
 const native=result.native_voice_json!==undefined;
 const raw=native?result.native_voice_json:result.sdk_conditioning_json;
 if(raw===undefined)return null;
 if(typeof raw!=='string'||raw.length>16000000)throw new Error('Invalid frozen document');
 const document=JSON.parse(raw);
 if(native){
  const frozen=document.conditioning,scope=frozen?.scope;
  if(document.version!==1||frozen?.version!==1||scope?.backend!=='ggml'||scope.model_sha256!==info.model.revision||scope.dimension!==info.dimension||scope.representation!=='raw-f32'||document.reference_sha256!==waveHash||document.text!==info.text||!sameRecipe(frozen.recipe,recipe,true)||!vector(frozen.values,info.dimension))throw new Error('Incompatible native frozen audition');
 }else{
  if(document.version!==1||document.provider!=='qwen-sdk'||document.model?.revision!==info.model.revision||document.basis_sha256!==info.space||document.source_vectors_sha256!==info.source_vectors_sha256||document.representation!=='effective-bfloat16-as-f32'||!sameRecipe(document.recipe,recipe,false)||!vector(document.values,info.dimension))throw new Error('Incompatible SDK frozen audition');
 }
 return {kind:native?'native':'sdk',raw,document};
}
