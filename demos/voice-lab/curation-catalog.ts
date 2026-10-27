import { CurationBank, CurationError } from './curation';

/** Only operator-configured bank keys, never client-provided filesystem paths. */
export class CurationCatalog {
  private banks=new Map<string,CurationBank>();
  constructor(primary?:CurationBank,extra:Map<string,CurationBank>=new Map()){
    if(primary)this.banks.set('default',primary);
    for(const [key,bank] of extra){
      if(!/^[a-z][a-z0-9_-]{0,31}$/.test(key)||key==='default')throw new CurationError('invalid_bank_key','Configure distinct short bank keys; default is reserved.',503);
      this.banks.set(key,bank);
    }
  }
  select(key:string|null=null){
    if(!this.banks.size)throw new CurationError('curation_unavailable','No curation bank is configured.',503);
    const selected=key??this.banks.keys().next().value!;
    const bank=this.banks.get(selected);
    if(!bank)throw new CurationError('bank_not_found','No such configured audition bank; no substitute was selected.',404);
    return {key:selected,bank};
  }
  async state(key:string|null=null){
    const selected=this.select(key),state=await selected.bank.state();
    const banks=await Promise.all([...this.banks].map(async([key,bank])=>({key,...await bank.summary()})));
    const query='?bank='+encodeURIComponent(selected.key);
    return {...state,selected_bank:selected.key,banks,candidates:state.candidates.map(c=>({...c,media:c.media+query,...(c.source_media?{source_media:c.source_media+query}:{})}))};
  }
}
