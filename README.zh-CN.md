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

所有屏幕共享一个选区：在另一块屏幕重新框选会替换旧选区，也可以跨屏
拖拽框选。跨屏导出按参与屏幕中最高的像素密度合成，屏幕之间的空隙
保留透明；单屏截图保留该屏幕的原始分辨率。

| 按键              | 动作                                   |
| ----------------- | -------------------------------------- |
| 拖拽              | 框选区域                               |
| `Enter` / `Ctrl+C` | 选区（或全屏）复制到剪贴板             |
| `Ctrl+S`          | 保存选区——系统"另存为"对话框          |
| `Ctrl+O`          | 选区 OCR → 文本进剪贴板                |
| `Esc`（拖拽中）   | 放弃本次拖拽                           |
| `Esc`             | 退出                                   |

### 矩形与椭圆标注

完成框选后，点击矩形图标（`R`）或椭圆图标（`E`），在选区内拖拽绘制。
按住 `Shift` 可画正方形或正圆。支持在跨屏选区内连续绘制，两个工具共享颜色、线宽和撤销历史。

- 在第二行直接点击色块选择红、橙、黄、绿、蓝、黑、白；点击圆点选择 1、3、5 个逻辑像素的线宽。
- `Ctrl+Z` 撤销，`Ctrl+Y` 或 `Ctrl+Shift+Z` 重做，不占用工具栏按钮。
- 绘制中按 `Esc` 取消当前笔画；再次按 `Esc` 或按当前工具的快捷键退出标注工具，已完成的标注保留。
- 退出标注工具后重新框选会清除旧选区的标注。
- 复制和保存包含标注；OCR 读取未标注的原图。

目前提供矩形和椭圆描边，暂不支持移动、缩放、旋转、填充和圆角等二次编辑。

### 直线与折线标注

- 点击直线图标或按 `L`，按住左键拖动，松开完成一条直线。
- 点击折线图标或按 `P`，连续点击添加节点；双击左键、右键或 `Enter` 完成整条折线。
- 按住 `Shift` 将当前线段方向约束为 45° 的倍数。折线结束时只保留已点击确认的节点。
- 两种工具都复用颜色、线宽和撤销历史，支持跨屏绘制、抗锯齿导出；整条折线作为一次撤销操作。
- `Esc` 取消正在绘制的线条，再按一次退出工具。复制或保存会先结束折线，不包含悬浮预览段。

目前提供实线、圆形端点和圆角连接；节点二次编辑、虚线及端点样式切换留待后续实现。

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
