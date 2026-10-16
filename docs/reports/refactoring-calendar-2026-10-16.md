# 日历、时区与 CPU 计时契约迁移

## 独立旧行为基线

运行时基线 `0044ae6`，before 为 `r2-system-profile-ordered-wire/wire`，SHA256 `fce5b7ce8275b1d46187753b12e5150a3f023127e7660a4cd08c38bb80ce00d9`。范围为 11 项 export；PE32 的 GetSystemTime/GetLocalTime/GetTimeZoneInformation 已有实现，其余八项保留 ABI-only error 120；Win64 十项已有实现，FileTimeToLocalFileTime absent。

两项独立 legacy 日历边界回归已随系统信息批次冻结。本批在迁移前重跑七项日历/时区/CPU 计时回归，固定 public facade 的精确 DLL/名称范围、PE32 172-byte 零时区 copy-out、LastError/trace、Win64 保存返回帧、CompareFileTime 的 DWORD carrier、日期校验/DST/alias 与受检输出故障。日志 `r2-calendar-baseline-calendar-tests.txt`、`r2-calendar-baseline-timezone-tests.txt`、`r2-calendar-baseline-system-times-tests.txt`。

原创可选 `gateway-calendar` 循环转换固定 1970-01-01 SYSTEMTIME，累加返回值和 FILETIME；n=max(1,work//9)，返回 `(116444736000000001*n) mod 2^64`，retired=8n+13、work=9n+13、API calls=n。单导入 inventory 见 `r2-calendar-draft-fixture-imports.txt`；旧实现 12K smoke 和 20 项工具检查通过，默认 case 集合保持。日志前缀 `r2-calendar-baseline-`。

运行时代码尚未接入。
