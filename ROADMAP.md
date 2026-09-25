# Saccade 架构决策与路线图

调研期（写代码之前）的结论存档。原则：**以 wayland-info 实测为准，不信文档、不信记忆。**

## 环境（2026-09-25 实测）

- Arch Linux + niri 26.04（zwlr_layer_shell_v1 v5、zwlr_screencopy_manager_v1 v3、
  foreign-toplevel 新旧双协议、virtual-pointer、data-control 全齐）
- 双屏混合缩放：HDMI-A-1 1920×1080@1.0 (0,0) + eDP-1 逻辑 1536×960@1.25 (1920,0)
- `ext-image-copy-capture`：niri main 分支 2026-09-13 才合入（且暂无窗口捕获），
  26.04 **没有** —— 官方 wiki 写的是 main，别被骗

## 捕获后端矩阵（CaptureBackend trait 的由来）

| 路 | 协议 | 适用 | 备注 |
|----|------|------|------|
| ① portal | xdg-desktop-portal ScreenCast + PipeWire | 所有桌面（GNOME 唯一解） | 授权弹窗、视频流抽帧 |
| ② wlr | zwlr_screencopy-unstable-v1 | niri/sway/Hyprland 等 | grim 同款，今日主力 |
| ③ ext | ext-image-copy-capture 全家桶 | 标准化的未来 | 等发行版铺开，含窗口捕获 |

窗口几何：foreign-toplevel 只给名单不给坐标 → 原型期用 `niri msg --json windows` 后门。

## 覆盖层（第 2 讲定稿）

- 普通 xdg 窗口做覆盖层在 tiling compositor 下是灾难 → 只能 layer-shell
- **gpui-pre 0.3.6 有 `WindowKind::LayerShell(LayerShellOptions)`**（gpui-kit 生态无人用过，
  我们打头阵；spike #1 负责验证）
- 工具条必须长在覆盖层窗口内部（层级死锁：普通窗口会被自己的暗幕压住）
- 贴图 = `Layer::Top` 的 layer-shell 窗口（Wayland 没有"普通窗口置顶"协议）

## spike #1 验证点

1. layer-shell 窗口能否在 niri 上开出（协议握手）
2. `WindowBackgroundAppearance::Transparent` 是否真透明（EGL alpha）
3. `KeyboardInteractivity::Exclusive` 下 Esc 焦点链路是否通
4. `exclusive_zone: Some(px(-1.))` 负值哨兵是否正确映射协议的 -1
5. 四边锚定 + configure 是否覆盖全输出（含 1.25 缩放屏）

## 结论记录

### 已结案件（2026-09-25）
- **Root/CSD 毒害案**：base::Root 的 WindowState 插件（component）对 layer-shell 窗口刷主题背景
  （白墙→灰雾 76=0.3×255）+ WindowBorder 调 set_client_inset(20)（窗口膨胀+40）+ padding 内缩。
  覆盖层永远裸 cx.open_window；正常窗口可用 Root。
- **240Hz 隐形案**：HDMI 在 240Hz 下 layer-shell 覆盖层渲染一帧后不再合成（60Hz 正常，
  同一二进制）。wgpu/niri 高刷新率帧节奏问题，待深挖+上报。开发验证 UI 期间临时切 60Hz：
  `niri msg output HDMI-A-1 mode 1920x1080@60`
- **RenderImage 契约**：BGRA 字节（Vulkan 后端），内存直喂必须 swap(0,2)；PNG 路径是 RGBA。
- **wl_shm format 是序号**（xrgb8888=1）不是 DRM fourcc；格式名描述"字"的位序，小端内存反序。
- **多屏 output 选择**：上游 zed#46378（displays() 启动为空），修复 PR #61578 久未 review；
  可 vendor 时顺手带 roundtrip 补丁。

### 开发后门
- `SACCADE_DEBUG_SELECTION=x,y,w,h`：注入现成选区（自动化验证选区 UI 用）
