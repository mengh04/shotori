# Shotori

**Wayland 优先的截图工具，内置 OCR，UI 全部用 [gpui-kit](https://crates.io/crates/gpui-kit) 自绘。**

[English](README.md) · [简体中文](README.zh-CN.md)

冻结屏幕、拖框选区，然后复制、保存，或者把图里的文字直接读出来——全程不用碰鼠标。

![workflow](https://img.shields.io/badge/platform-Linux%20%2F%20Wayland-8892bf) ![license](https://img.shields.io/badge/license-MIT-blue)

## 功能

- **区域截图**——拖拽框选，四周变暗、选区"透视"，实时尺寸标签跟随。
- **多屏正确处理**——每块输出一个覆盖层窗口，钉在它截获的那块屏上。
  混合缩放（1.0 / 1.5 / 2.0）与旋转输出（竖屏面板）都支持；在三屏
  niri 环境实测。
- **复制**（`Enter` / `Ctrl+C`）——PNG 经驻留后台分身进 Wayland 剪贴板
  （wl-copy 同款模型），剪贴板内容活得比工具本身久。
- **保存**（`Ctrl+S`）——带时间戳的 PNG 落到 `~/Pictures/Shotori/`，
  同秒重名自动加后缀。
- **OCR**（`Ctrl+O`）——本地文字识别（PP-OCRv6 small，ONNX Runtime），
  文本直接进剪贴板，中英混排可用。你画选区时引擎在后台预热；推理期间
  选区中心显示转圈徽章。
  - 首次使用弹确认卡片、字节级进度条、可随时取消。模型（~31MB，来自
    ModelScope）下载后经 sha256 校验、原子落盘——取消的下载不留垃圾。
    缓存位置：`~/.local/share/shotori/ocr-models/`。
- **桌面通知**——复制 / 保存 / OCR 三个出口都带截图缩略图反馈
  （`org.freedesktop.Notifications`）。
- 无选区按 `Enter` = 全屏截图。从键位启动一切正常工作；有通知就永远
  不需要终端。

## 环境要求

- Linux + wlroots 系 Wayland 合成器（niri、sway、Hyprland……），需要
  `zwlr_screencopy-unstable-v1` 和 `zwlr-data-control-v1`。
- 通知 daemon 可选（没有也不影响复制/保存/OCR）。
- OCR 在构建期引入 ONNX Runtime（构建脚本自动下载预编译库）。

## 安装

```bash
cargo install shotori
```

然后绑键位，比如 niri 的 `binds.kdl`：

```kdl
Mod+Shift+S { spawn "shotori"; }
```

## 用法

```
shotori            # 冻结全部屏幕 → 框选 → 动作
```

| 按键 | 动作 |
|------|------|
| 拖拽 | 框选区域（再次按下可重选） |
| `Enter` / `Ctrl+C` | 选区（或全屏）复制到剪贴板 |
| `Ctrl+S` | 选区保存为 PNG |
| `Ctrl+O` | 选区 OCR → 文本进剪贴板 |
| `Esc`（拖拽中） | 放弃本次拖拽 |
| `Esc`（其余状态） | 退出 |

松手后选区下方会出现工具条（同款动作）。

### OCR 说明

- 引擎为 PP-OCRv6 small（检测 + 方向 + 识别），一次性下载后完全离线。
- 1080p 屏上小于 ~16px 的小字吃力；HiDPI 屏更好（物理像素多）。
- OCR 是默认 feature。想要不带 OCR 的轻量二进制：
  `cargo install shotori --no-default-features`。

## 源码构建

```bash
cargo build --release
cargo test                        # 21 个单元测试，无需合成器
cargo build --no-default-features # 不带 OCR 的轻量构建
```

附带 `screencap`：捕获代码的调试前端：

```bash
cargo run --bin screencap -- --all
```

## 设计笔记

有意思的部分——驻留 offer 剪贴板模型、混合缩放下的 display 匹配、与
协议字面相反的 transform 语义、1px 接缝 bug 的像素取证——都写在
[ROADMAP.md](ROADMAP.md)（英文）里。

## 许可

MIT——见 [LICENSE](LICENSE)。
