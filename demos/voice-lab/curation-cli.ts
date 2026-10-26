import { parseArgs } from 'node:util';
import { readFile } from 'node:fs/promises';
import { CurationBank, CurationError } from './curation';
try {
  const {values,positionals}=parseArgs({args:Bun.argv.slice(2),allowPositionals:true,options:{bank:{type:'string'},candidate:{type:'string'},input:{type:'string'},help:{type:'boolean'}}});
  if(values.help){console.log('Usage: bun curation-cli.ts read --bank DIR\n       bun curation-cli.ts save --bank DIR --candidate c001 --input FILE|-\nRead is read-only JSON. Save consumes curation-request.schema.json, checks bank/audio/vector/revision, creates an atomic private review and prior-revision backup; never infers, renders or enrolls. Unknown save outcome: retry the identical operation_id/body, or read and reconcile first. Persistent locks require operator inspection.');}
  else {
    if(!values.bank||positionals.length!==1||!['read','save'].includes(positionals[0]))throw new CurationError('invalid_command','Use read or save with --bank; --help describes effects and recovery.');
    const bank=new CurationBank(values.bank);
    let result;
    if(positionals[0]==='read')result=await bank.state();
    else {
      if(!values.candidate||!values.input)throw new CurationError('invalid_command','Save requires --candidate and --input FILE|-; stdin/stdout compose.');
      let input;try{input=JSON.parse(values.input==='-'?await new Response(Bun.stdin.stream()).text():await readFile(values.input,'utf8'));}catch{throw new CurationError('invalid_json','Send a valid JSON save intent.');}
      result=await bank.save(values.candidate,input);
    }
    console.log(JSON.stringify(result,null,2));
  }
}catch(error){const known=error instanceof CurationError;console.log(JSON.stringify({error:known?error.code:'curation_failed',message:known?error.message:'The operation failed safely; inspect the bank and saved reviews.'}));process.exitCode=1;}
