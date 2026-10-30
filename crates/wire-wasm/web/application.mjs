import {WasmMachine,Exit} from './runtime.mjs';
import {BrowserPresenter} from './presenter.mjs';

const encoder=new TextEncoder();
function installationPath(path) {
  if(typeof path!=='string' || !/^C:\\/i.test(path) || encoder.encode(path).length>32768)throw Error('invalid installation path');
  const parts=path.slice(3).split('\\');
  if([...path].some(c=>c.codePointAt(0)>=0xd800 && c.codePointAt(0)<=0xdfff) || parts.some(part=>!part || /[\x00-\x1f<>:"/|?*]/.test(part) || /[. ]$/.test(part) || /^(con|prn|aux|nul|com[1-9]|lpt[1-9])(?:\.|$)/i.test(part)))throw Error('invalid installation path');
  return path.toLowerCase();
}

export function executableMode(bytes) {
  if(!(bytes instanceof Uint8Array) || bytes.length<64)throw Error('expected a PE executable');
  const view=new DataView(bytes.buffer,bytes.byteOffset,bytes.byteLength),at=view.getUint32(60,true);
  if(view.getUint16(0,true)!==0x5a4d || at>bytes.length-24 || view.getUint32(at,true)!==0x4550)throw Error('invalid PE executable header');
  const machine=view.getUint16(at+4,true);
  if(machine===0x14c)return 32;if(machine===0x8664)return 64;
  throw Error('unsupported executable architecture');
}

export function validateInstallation(manifest) {
  if(!manifest || typeof manifest!=='object' || !Array.isArray(manifest.files) || manifest.files.length>4095)throw Error('invalid installation manifest');
  if(!manifest.program || typeof manifest.program.url!=='string' || !manifest.program.url || !Number.isSafeInteger(manifest.program.size) || manifest.program.size<1 || manifest.program.size>256*1024*1024)throw Error('invalid executable size');
  if(typeof manifest.command!=='string' || manifest.command.includes('\0'))throw Error('invalid application command line');
  if(manifest.timeZone!==undefined && (!Array.isArray(manifest.timeZone) || manifest.timeZone.length!==172 || Array.from(manifest.timeZone).some(value=>!Number.isInteger(value) || value<0 || value>255)))throw Error('invalid installation timezone');
  const program=installationPath(manifest.program.path),first=manifest.command.startsWith('"')?manifest.command.slice(1).split('"',1)[0]:manifest.command.split(/[ \t]/,1)[0];
  if(installationPath(first)!==program)throw Error('command executable differs from installation path');
  const directories=manifest.directories ?? [];
  if(!Array.isArray(directories) || directories.length>4096)throw Error('invalid installation directories');
  let bytes=manifest.program.size;
  const names=new Set([program]),dirs=new Set();
  for(const path of directories){const key=installationPath(path);if(dirs.has(key))throw Error('duplicate installation path');dirs.add(key);}
  for(const file of manifest.files){
    if(!file || typeof file.url!=='string' || !file.url || file.external!==undefined && typeof file.external!=='boolean')throw Error('invalid installation file');
    if(file.external){if(typeof file.size!=='string' || !/^[0-9]{1,19}$/.test(file.size) || BigInt(file.size)>0x7fffffffffffffffn)throw Error('invalid external file size');}
    else if(!Number.isSafeInteger(file.size) || file.size<0)throw Error('invalid installation file');
    const key=installationPath(file.path);if(names.has(key))throw Error('duplicate installation path');names.add(key);
    if(!file.external)bytes+=file.size;if(!Number.isSafeInteger(bytes))throw Error('installation size overflow');
  }
  for(const key of [...names,...dirs])for(let at=key.indexOf('\\',3);at>=0;at=key.indexOf('\\',at+1))dirs.add(key.slice(0,at));
  if([...names].some(key=>dirs.has(key)))throw Error('installation file conflicts with a directory');
  if(dirs.size>4096 || [...names,...dirs].reduce((sum,key)=>sum+encoder.encode(key).length*2,0)>4*1024*1024)throw Error('installation path capacity exceeded');
  if(bytes>256*1024*1024)throw Error('installation exceeds the current 256 MiB file capacity');
  if(manifest.input && (typeof manifest.input.url!=='string' || !Number.isSafeInteger(manifest.input.size) || manifest.input.size<0 || manifest.input.size>8*1024*1024))throw Error('invalid initial input');
  return bytes;
}

/** An owned VM with explicit work slices; the caller controls rendering and waits. */
export class WasmApplication {
  #closed=false;
  constructor(machine,presenter){this.machine=machine;this.presenter=presenter;}
  static async load({core,manifest,read,container,runtime={},graphics=null,audio=null}) {
    validateInstallation(manifest);
    if(typeof read!=='function')throw new TypeError('expected an installation file reader');
    const load=async entry=>{const bytes=await read(entry.url);if(!(bytes instanceof Uint8Array) || bytes.length!==entry.size)throw Error('installation input size changed');return bytes;};
    const program=await load(manifest.program),mode=executableMode(program);
    const result=core instanceof WebAssembly.Module?new WebAssembly.Instance(core):await WebAssembly.instantiate(core),instance=result.instance ?? result;
    const machine=new WasmMachine(instance,{...runtime,mode,...(manifest.timeZone===undefined?{}:{timeZone:Uint8Array.from(manifest.timeZone)})});let presenter;
    try {
      for(const path of manifest.directories ?? [])machine.supplyDirectory(path);
      for(const entry of manifest.files){
        if(entry.external)machine.supplyFileSource(entry.path,BigInt(entry.size),(offset,length)=>read(entry.url,{offset,length,size:BigInt(entry.size)}));
        else machine.supplyFile(entry.path,await load(entry));
      }
      await machine.loadAsync(program,{command:manifest.command});
      presenter=new BrowserPresenter(machine,container);
      machine.configureOutputs({graphics:graphics && {...graphics,getCanvas:presenter.getCanvas},audio});
      if(manifest.input)machine.writeInput(await load(manifest.input),{end:true});
      presenter.sync();return new WasmApplication(machine,presenter);
    }catch(error){presenter?.close();machine.dispose();throw error;}
  }
  async step(budget) {
    if(this.#closed)throw Error('application has been closed');
    this.presenter.flushInput();const result=await this.machine.run(budget);
    if(result.reason===Exit.Returned || result.reason===Exit.ThreadReturned){this.machine.finishProcessEntry();return {...result,reason:Exit.Dispatch};}
    return result;
  }
  present(){if(this.#closed)throw Error('application has been closed');this.presenter.sync();}
  readOutput(endpoint=1,limit=65536,processId){if(this.#closed)throw Error('application has been closed');return this.machine.readOutput(endpoint,limit,processId);}
  writeInput(bytes,options={},processId){if(this.#closed)throw Error('application has been closed');this.machine.writeInput(bytes,options,processId);}
  close(){if(this.#closed)return;this.#closed=true;this.machine.dispose();this.presenter.close();}
}

export {Exit};
