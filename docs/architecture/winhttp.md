# Win64 WinHTTP 适配器

五项 export 由 `WINHTTP_CONTRACTS` 统一 provider/ABI/WinHttpApi。静态和动态均仅接受 winhttp.dll，PE32 保持 absent/未知 ABI。处理函数迁入 `win32::x64::winhttp`，只借 Memory、ID、参数和当前线程 LastError 地址；原 legacy runtime 模块删除。

当前没有 IE profile 或 HTTP transport。用户 proxy 查询先检查 32-byte 输出，再以错误 2 返回并保持输出；默认 proxy 查询写出原 24-byte direct profile，成功时 LastError 保持。空 profile 指针返回错误 87；Open 返回错误 50；ProxyForUrl/Close 返回错误 6，不伪造 HINTERNET。访存故障和返回帧顺序保持。

本批风险为低：使用既有 `tests/x64/network.rs::win64_winhttp_proxy_profile_and_absent_transport_preserve_fault_outputs` 验证真实输出/错误/故障/provider，所属 release 1、catalog 一致性及 fmt/strict Clippy/release build 通过，不增加重复测试或性能矩阵。基线 `1bd3adf` 已运行过包含该用例的 Win64 264 项；证据和源码/executable SHA 记录于 `wire-out/refactoring-2026-10-14/r2-winhttp-*`。累计 **792 exports**。
