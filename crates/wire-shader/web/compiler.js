// Two fixed compiler arenas, reused across shaders and guest process loads.
// They contain compiler IR only; guest memory remains owned by WireResident.
globalThis.WireShaderCompiler = class WireShaderCompiler {
    static async create(options) {
        if (!options || typeof options.dxbcFactory !== "function" || !(options.dxbcWasm instanceof Uint8Array) || !(options.nagaWasm instanceof Uint8Array))
            throw Error("Portable shader compiler modules/factory required");
        if (options.dxbcWasm.length > 32*1024*1024 || options.nagaWasm.length > 32*1024*1024)
            throw Error("Portable shader compiler module capacity");
        const state={fatal:null};
        const dxbc=await options.dxbcFactory({wasmBinary:options.dxbcWasm,print:()=>{},printErr:()=>{},onAbort:reason=>{state.fatal=String(reason);}});
        let compiler;
        try {
            const module=new WebAssembly.Module(options.nagaWasm);
            if(WebAssembly.Module.imports(module).length)throw Error("Unexpected Naga compiler import");
            const naga=new WebAssembly.Instance(module,{}).exports;
            if(dxbc._wire_dxbc_abi()!==1 || naga.wire_shader_abi()!==2 || dxbc.HEAPU8.byteLength!==67108864 || naga.memory.buffer.byteLength!==67108864)
                throw Error("Portable shader compiler ABI/arena mismatch");
            compiler=new WireShaderCompiler(dxbc,naga,state,options.limits||{});
            return compiler;
        } catch(error) {try{dxbc._wire_dxbc_reset();}catch(_){}throw error;}
    }
    constructor(dxbc,naga,state,limits) {
        this.dxbc=dxbc;this.naga=naga;this.state=state;
        this.maxCbvBytes=Math.min(65536,limits.maxUniformBufferBindingSize??65536);
        this.maxCbvCount=Math.min(64,limits.maxUniformBuffersPerShaderStage??12);
        if(!Number.isInteger(this.maxCbvBytes)||this.maxCbvBytes<16||!Number.isInteger(this.maxCbvCount)||this.maxCbvCount<1)
            throw Error("Invalid shader compiler device limits");
        this.closed=false;this.busy=false;this.generation=0;
        this.compilations=0;this.linkages=0;this.failures=0;this.peakNagaBytes=0;
        this.dxbcBaseline=dxbc._wire_dxbc_heap_used();this.nagaBaseline=naga.wire_shader_stat(0);
        this.decoder=new TextDecoder("utf-8",{fatal:true});
    }
    diagnostic(component) {
        if(component==="dxbc")return this.decoder.decode(this.dxbc.HEAPU8.subarray(this.dxbc._wire_dxbc_error(),this.dxbc._wire_dxbc_error()+this.dxbc._wire_dxbc_error_size()));
        const n=this.naga;return this.decoder.decode(new Uint8Array(n.memory.buffer,n.wire_shader_error(),n.wire_shader_error_size()));
    }
    reset() {
        this.dxbc._wire_dxbc_reset();this.naga.wire_shader_reset();
        this.peakNagaBytes=Math.max(this.peakNagaBytes,this.naga.wire_shader_stat(1));
        if(this.dxbc.HEAPU8.byteLength!==67108864 || this.naga.memory.buffer.byteLength!==67108864 || this.dxbc._wire_dxbc_owned_bytes()!==0)
            throw Error("Shader compiler arena/ownership invariant");
    }
    package(stage,spirv) {
        const n=this.naga,length=n.wire_shader_output_size();if(!length||length>20*1024*1024)throw Error("Shader compiler package capacity");
        const result=JSON.parse(this.decoder.decode(new Uint8Array(n.memory.buffer,n.wire_shader_output(),length)));
        if(typeof result.code!=="string"||result.code.length>16*1024*1024||result.stage!==stage||!Array.isArray(result.bindings)||result.bindings.length>128||!Array.isArray(result.inputs)||result.inputs.length>32||!Array.isArray(result.internalBindings)||result.internalBindings.length>2)
            throw Error("Shader compiler reflection contract");
        // Retain the original compiler SPIR-V so another pixel stage can
        // link independently. The linked WGSL/reflection describe this pair.
        result.spirv=spirv.slice();return result;
    }
    async compileDXBC(source,stage) {
        if(this.closed)throw Error("Shader compiler closed");
        if(this.state.fatal)throw Error("Shader compiler unavailable after trap: "+this.state.fatal);
        if(this.busy)throw Error("Shader compiler already has an active transaction");
        if(!(source instanceof Uint8Array)||source.length<32||source.length>1024*1024||![1,2,4].includes(stage))
            throw Error("Shader compiler source capacity/stage");
        const saved=source.slice();
        this.busy=true;const generation=this.generation;
        try {
            // Yield within WebContent before the synchronous Wasm compilation.
            await Promise.resolve();
            if(this.closed||generation!==this.generation)throw Error("Shader compilation cancelled");
            const d=this.dxbc,n=this.naga;
            const input=d._wire_dxbc_input(saved.length);if(!input)throw Error(this.diagnostic("dxbc"));
            d.HEAPU8.set(saved,input);
            if(d._wire_dxbc_compile(stage,this.maxCbvBytes,this.maxCbvCount)!==0)throw Error(this.diagnostic("dxbc"));
            const size=d._wire_dxbc_output_size(),pointer=d._wire_dxbc_output();
            if(size<20||size>16*1024*1024)throw Error("Shader compiler SPIR-V output capacity");
            const target=n.wire_shader_input(size);if(!target)throw Error(this.diagnostic("naga"));
            const spirv=new Uint8Array(d.HEAPU8.buffer,pointer,size);
            new Uint8Array(n.memory.buffer,target,size).set(spirv);
            if(n.wire_shader_compile(stage)!==0)throw Error(this.diagnostic("naga"));
            const result=this.package(stage,spirv);
            this.compilations++;
            return result;
        } catch(error) {
            this.failures++;
            if(error instanceof WebAssembly.RuntimeError || this.state.fatal)this.state.fatal=this.state.fatal||String(error);
            throw error;
        } finally {
            if(!this.closed&&!this.state.fatal) {
                try{this.reset();}catch(error){this.state.fatal=String(error);this.busy=false;throw error;}
            }
            this.busy=false;
        }
    }
    async linkVertex(vertex,pixel) {
        if(this.closed)throw Error("Shader compiler closed");
        if(this.state.fatal)throw Error("Shader compiler unavailable after trap: "+this.state.fatal);
        if(this.busy)throw Error("Shader compiler already has an active transaction");
        if(vertex?.stage!==1||pixel?.stage!==2||[vertex,pixel].some(s=>!(s.spirv instanceof Uint8Array)||s.spirv.length<20||s.spirv.length>16*1024*1024||s.spirv.length%4))throw Error("Shader link source capacity/stage");
        const vs=vertex.spirv.slice(),ps=pixel.spirv.slice(),generation=this.generation;
        this.busy=true;
        try {
            await Promise.resolve();
            if(this.closed||generation!==this.generation)throw Error("Shader linkage cancelled");
            const n=this.naga,at=n.wire_shader_link_input(vs.length,ps.length);if(!at)throw Error(this.diagnostic("naga"));
            new Uint8Array(n.memory.buffer,at,vs.length).set(vs);new Uint8Array(n.memory.buffer,at+vs.length,ps.length).set(ps);
            if(n.wire_shader_link(vs.length)!==0)throw Error(this.diagnostic("naga"));
            const result=this.package(1,vs);this.linkages++;return result;
        } catch(error) {
            this.failures++;if(error instanceof WebAssembly.RuntimeError)this.state.fatal=String(error);throw error;
        } finally {
            if(!this.closed&&!this.state.fatal){try{this.reset();}catch(error){this.state.fatal=String(error);this.busy=false;throw error;}}
            this.busy=false;
        }
    }
    stats() {
        return {closed:this.closed,busy:this.busy,poisoned:!!this.state.fatal,compilations:this.compilations,linkages:this.linkages,failures:this.failures,owned_arena_bytes:this.closed?0:134217728,
            dxbc_heap_used:this.closed?0:this.dxbc._wire_dxbc_heap_used(),naga_heap_used:this.closed?0:this.naga.wire_shader_stat(0),peak_naga_bytes:this.peakNagaBytes};
    }
    close() {
        if(this.closed)return;
        this.generation++;this.closed=true;
        try{if(!this.state.fatal){this.dxbc._wire_dxbc_reset();this.naga.wire_shader_reset();}}
        finally{this.dxbc=null;this.naga=null;}
    }
};
