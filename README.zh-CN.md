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

- 区域选择，实时尺寸标签跟随，选区之外变暗
- 多屏感知，混合缩放与旋转（竖屏）输出都能正确处理
- 复制到剪贴板（`Enter` / `Ctrl+C`）
- 保存到磁盘（`Ctrl+S`）——系统"另存为"对话框，任选目录
- OCR（`Ctrl+O`）——本地识别，中英混排可用
- 桌面通知，带结果缩略图

## 环境要求

- Linux + wlroots 系 Wayland 合成器（niri、sway、Hyprland……）
- 保存对话框依赖 `xdg-desktop-portal`（绝大多数桌面发行版默认就有）
- 通知 daemon（dunst、mako、swaync……）可选

## 安装

```bash
cargo install shotori        # crates.io
paru -S shotori              # AUR（预编译二进制）
```

也可以从 [GitHub Releases](https://github.com/mengh04/shotori/releases)
直接下载二进制。绑到键位上，比如 niri：

```kdl
Mod+Shift+S { spawn "shotori"; }
```

## 用法

运行 `shotori`，所有屏幕冻结并出现选区覆盖层。松手后选区下方会出现
等效按钮的工具条。

| 按键              | 动作                                   |
| ----------------- | -------------------------------------- |
| 拖拽              | 框选区域                               |
| `Enter` / `Ctrl+C` | 选区（或全屏）复制到剪贴板             |
| `Ctrl+S`          | 保存选区——系统"另存为"对话框          |
| `Ctrl+O`          | 选区 OCR → 文本进剪贴板                |
| `Esc`（拖拽中）   | 放弃本次拖拽                           |
| `Esc`             | 退出                                   |

### OCR

首次按 `Ctrl+O` 会先询问再下载模型（~31MB，仅一次），之后完全离线。
模型缓存在 `~/.local/share/shotori/ocr-models/`。画选区的同时引擎在
后台预热，通常几百毫秒内出结果。过小的文字识别吃力，HiDPI 屏表现
更好。

## 源码构建

```bash
git clone https://github.com/mengh04/shotori
cd shotori
cargo build --release
cargo test    # 单元测试，无需合成器
```

## 设计笔记

实现细节的记录（驻留 offer 剪贴板模型、混合缩放下的 display 匹配、
一个 1px 接缝 bug 的像素级取证……）都在 [ROADMAP.md](ROADMAP.md)（英文）。

## 许可

[MIT](LICENSE)
