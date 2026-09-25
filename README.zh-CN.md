# Shotori

[![Crates.io](https://img.shields.io/crates/v/shotori.svg)](https://crates.io/crates/shotori)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
![Platform](https://img.shields.io/badge/platform-Linux%20%7C%20Wayland-8892bf)

Wayland 原生的截图工具，内置本地 OCR——整套 UI 用
[gpui-kit](https://crates.io/crates/gpui-kit) 手绘。

冻结屏幕、拖框选区，然后复制、保存，或者把图里的文字直接读出来——
全程不用碰鼠标。

**[English](README.md)**

## 功能

- **区域选择**——屏幕冻结，选区之外变暗，实时尺寸标签跟随拖拽。
- **多屏感知**——每块输出一个覆盖层，钉在它截获的那块屏上。混合缩放
  （1× / 1.5× / 2×）与旋转（竖屏）输出都能正确处理。
- **复制到剪贴板**（`Enter` / `Ctrl+C`）——PNG 由驻留后台分身伺服
  （`wl-copy` 同款模型），剪贴板内容活得比填充它的进程久。
- **保存到磁盘**（`Ctrl+S`）——以本地时间命名，精确到毫秒，例如
  `Shotori_2026-09-26_12-34-56_789.png`，存入 `~/Pictures/Shotori/`。
  同时保存遇到重名会自动加后缀，不覆盖已有文件。
- **OCR**（`Ctrl+O`）——本地文字识别（PP-OCRv6，ONNX Runtime），文本
  直接进剪贴板。中英混排可用，一次性下载模型后完全离线。
- **桌面通知**——复制 / 保存 / OCR 每个出口都带截图缩略图反馈。
- **无需终端**——从键位启动一切正常；通知就是反馈通道。

## 环境要求

- Linux + wlroots 系 Wayland 合成器（niri、sway、Hyprland……），需要
  `zwlr_screencopy-unstable-v1` 和 `zwlr-data-control-v1`。
- 通知 daemon（dunst、mako、swaync……）可选——没有也不影响复制、
  保存和 OCR。
- 默认 feature 构建时会在编译期下载预编译的 ONNX Runtime。

## 安装

```bash
cargo install shotori
```

绑到键位上，比如 niri：

```kdl
Mod+Shift+S { spawn "shotori"; }
```

## 用法

运行 `shotori`，所有屏幕冻结并出现选区覆盖层。

| 按键              | 动作                                   |
| ----------------- | -------------------------------------- |
| 拖拽              | 框选区域                               |
| `Enter` / `Ctrl+C` | 选区（或全屏）复制到剪贴板             |
| `Ctrl+S`          | 选区保存为 PNG                         |
| `Ctrl+O`          | 选区 OCR → 文本进剪贴板                |
| `Esc`（拖拽中）   | 放弃本次拖拽                           |
| `Esc`             | 退出                                   |

松手后选区下方会出现等效按钮的工具条。

### OCR

- 引擎为 PP-OCRv6 small（检测 + 方向 + 识别），经 ONNX Runtime 本地运行。
- 首次使用弹出确认卡片，随后是带取消按钮的进度条。模型（~31MB，取自
  ModelScope）经 sha256 校验、原子安装。取消会立即关闭对话框；后台在
  当前网络操作返回或达到 10 秒超时后清理临时文件，已校验的模型保留供重试复用。
  存放于 `~/.local/share/shotori/ocr-models/`，之后一直复用。
- 你画选区的同时引擎在后台预热，按下 `Ctrl+O` 后通常几百毫秒内出结果；
  等待期间有转圈徽章提示。
- 1080p 级屏幕上小于 ~16px 的文字吃力；HiDPI 屏表现更好。
- OCR 是默认 feature。想要不带它的轻量二进制：

  ```bash
  cargo install shotori --no-default-features
  ```

## 源码构建

```bash
git clone https://github.com/mengh04/shotori
cd shotori
cargo build --release
cargo test                          # 单元测试，无需合成器
cargo build --no-default-features   # 不带 OCR 的轻量构建
```

## 设计笔记

驻留 offer 剪贴板模型、混合缩放下的 display 匹配、与协议字面相反的
输出 transform、以及一个 1px 接缝 bug 的像素级取证，都写在
[ROADMAP.md](ROADMAP.md)（英文）里。

## 许可

[MIT](LICENSE)
