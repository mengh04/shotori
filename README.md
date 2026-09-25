# Saccade

**Linux（Wayland 优先）截图套件** —— 区域截图、窗口识别截图、滚动长截图、OCR、贴图钉屏。
目标是做出 PixPin 级别的功能完整度，原生 Wayland，不将就 X11。

名字来自 **saccade（眼跳）**：你阅读这行字时，眼球正以每秒 3–4 次的频率快速跳动——
滚动长截图与 OCR 阅读扫描，在神经科学里是同一个动作。

## 技术栈

- **UI**：[gpui-kit](https://github.com/longbridge/gpui-kit)（Zed GPUI 的应用框架封装）
- **平台**：Wayland（`zwlr_layer_shell_v1` 覆盖层 / `zwlr_screencopy` 与
  `ext-image-copy-capture` 捕获 / portal 兜底）
- 语言：Rust

## 当前状态：spike 阶段

- [x] spike #1 — gpui `WindowKind::LayerShell` 覆盖层链路验证（本仓库第一个可运行目标）
- [ ] spike #2 — 手写 `wayland-client` 裸连 screencopy，60 行最小截图（自制迷你 grim）
- [ ] 选区交互（拖框、十字线、尺寸提示）
- [ ] `CaptureBackend` trait + wlr-screencopy 后端
- [ ] 窗口识别（foreign-toplevel × 双协议）
- [ ] 工具条（gpui-kit 组件，长在覆盖层内部）
- [ ] OCR / 长截图 / 贴图（Top 层 layer-shell 窗口）

## 运行

```bash
cargo run   # 需要在 Wayland 图形会话中（niri/sway/Hyprland 等）
```

spike #1 的预期表现：全屏盖上半透明黑暗幕、中央提示卡片、Esc 退出。
背景不透明 / Esc 无效 / 尺寸不对 → 都是有效发现，记进 ROADMAP 的"spike 结论"。

## 架构决策记录

见 [ROADMAP.md](./ROADMAP.md) —— 含 Wayland 三条捕获通道的选型矩阵（源自开发前的原理调研）。
