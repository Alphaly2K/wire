/** Sample the native host's aggregate CPU durations without losing u64 precision. */
export async function readCPUTimeSample(url='/cpu-times') {
  const response=await fetch(url);
  if(!response.ok)throw Error('host CPU sampling failed');
  const sample=await response.json();
  if(sample?.error!==0){
    if(!Number.isInteger(sample?.error) || sample.error<1 || sample.error>0xffffffff)throw Error('invalid host CPU error');
    return{error:sample.error};
  }
  if(!Array.isArray(sample.times) || sample.times.length!==3 || sample.times.some(value=>typeof value!=='string' || !/^[0-9]{1,20}$/.test(value)))throw Error('invalid host CPU durations');
  return sample.times.map(BigInt);
}
/** Fetch one immutable installation file or its exact requested byte range. */
export async function readApplicationBytes(url,range) {
  const end=range && range.offset+BigInt(range.length)-1n;
  const response=await fetch(url,range?{headers:{Range:'bytes='+range.offset+'-'+end}}:{});
  if(!response.ok || range && (response.status!==206 || response.headers.get('Content-Range')!==`bytes ${range.offset}-${end}/${range.size}`))throw Error('无法读取启动资源');
  return new Uint8Array(await response.arrayBuffer());
}

