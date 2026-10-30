# Portable shader compiler

Compile DXBC shader bytecode to WGSL in Wasm. The compiler uses dxbc-spirv
(MIT) and Naga; pinned sources and license notices are retained in the
repository. Runtime integration and component boundaries are described in
[the runtime architecture](../../docs/architecture/wasm-runtime.md).

Build with an Emscripten SDK containing `emscripten/`, `llvm/`, `binaryen/`,
`node/` and a populated read-only cache:

```sh
python3 tools/build_wasm_shaders.py --emscripten /absolute/sdk/path --out /absolute/output/path
cargo test --manifest-path crates/wire-shader/Cargo.toml --offline
```

Outputs are `dxbc.js`, `dxbc.wasm`, `naga.wasm` and `identity.json`.
`--reuse-dxbc` reuses verified identical C++ build outputs.

Load `dxbc.js` and `web/compiler.js`, then create a compiler owner with the
matching Wasm bytes and the target device's uniform limits:

```js
const compiler = await WireShaderCompiler.create({
    dxbcFactory: WireDXBCModule, dxbcWasm, nagaWasm, limits
});
try {
    const vertex = await compiler.compileDXBC(vertexBytes, 1);
    const pixel = await compiler.compileDXBC(pixelBytes, 2);
    const linked = await compiler.linkVertex(vertex, pixel);
} finally {
    compiler.close();
}
```

`compileDXBC(bytes, stage)` accepts vertex (1), pixel (2) and compute (4)
shaders. Results contain WGSL, entry point, resource bindings, typed stage
interfaces, workgroup size and owned SPIR-V bytes. `linkVertex(vertex, pixel)`
returns the vertex package for the pixel interface; incompatible interfaces
fail. Geometry and tessellation stages are unsupported.

DXBC input is limited to 1 MiB; SPIR-V and WGSL are limited to 16 MiB.
Compiler memory is bounded. Concurrent transactions are rejected; closing
cancels publication, and traps require closing the failed owner. Callers own
the compiler and close it after all borrowing GPU owners finish. When reusing
it with another device, its configured uniform limits must fit that device.
