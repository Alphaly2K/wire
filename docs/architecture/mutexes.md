# 互斥体

新 Wasm 入口在 PE32 与 Win64 均实现 CreateMutexA/W、CreateMutexExA/W、OpenMutexA/W 和 ReleaseMutex。provider 与参数宽度来自 `win32/catalog/mutexes.rs`；目录中的 legacy 策略仍用于旧适配器，新入口按实际能力独立绑定。

互斥体与事件、信号量、定时器共用[有类型的命名空间与句柄规则](sync-objects.md)。新建时可以授予当前线程初始所有权；同名创建只增加别名，忽略初始所有权请求。Ex 仅接受 CREATE_MUTEX_INITIAL_OWNER 标志，未知位返回错误 87。创建成功将 LastError 设为 0 或 ERROR_ALREADY_EXISTS（183）；其它成功操作保持 LastError。

等待需要 SYNCHRONIZE 权限。无 owner 或 owner 为当前线程时可以取得互斥体，重复取得增加递归深度。ReleaseMutex 需要 MUTEX_MODIFY_STATE，只允许当前 owner 释放；非 owner 返回错误 288，错误类型或无效句柄返回错误 6。递归深度归零才解除所有权。

线程完成退出通知和 TLS/栈/TEB 回收后，持有的互斥体变为 abandoned。下一次取得报告 WAIT_ABANDONED，并授予新的所有权；后续递归取得按普通成功处理。混合等待的选择、原子提交及返回值见[等待协议](waits.md)。关闭句柄本身不释放所有权，也不结束线程。

所有权使用客体线程 ID，不创建宿主锁。调用帧先完整捕获，再改变资源；成功计一 work，不计客体退休，不采样时间。
