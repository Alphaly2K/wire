# WinHTTP profile

Wasm 入口在 PE32 和 AMD64 的 `winhttp.dll` 接通五项导出：WinHttpGetIEProxyConfigForCurrentUser、WinHttpGetDefaultProxyConfiguration、WinHttpGetProxyForUrl、WinHttpOpen 和 WinHttpCloseHandle。

当前 profile 没有 IE 配置或 HTTP transport。用户 proxy 查询先检查完整的 16/32 字节输出范围，再以错误 2 返回并保持输出。默认 proxy 查询写出 12/24 字节的 direct profile，成功保持 LastError；结构内的代理与绕过列表指针均为 NULL。空 profile 指针返回错误 87。Open 返回错误 50，ProxyForUrl/Close 返回错误 6，没有成功创建的 HINTERNET。

原有处理函数已提取到 `win32::winhttp`，新旧入口共享同一逻辑，仅适配内存操作及结构指针宽度。输出和错误槽故障保持 CPU、work 和保存返回帧，调用不采样时间。检查入口为 `node crates/wire-wasm/tests/winhttp.mjs`，使用项目生成输入；旧入口可通过 `cargo test --locked --offline --test x64 win64_winhttp --target-dir target/resident` 定向复查。
