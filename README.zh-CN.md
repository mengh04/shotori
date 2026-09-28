# Shotori

[![Crates.io](https://img.shields.io/crates/v/shotori.svg)](https://crates.io/crates/shotori)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
![Platform](https://img.shields.io/badge/platform-Linux-8892bf)
![Status](https://img.shields.io/badge/status-early%20development-orange)

Wayland 原生的截图工具，内置本地 OCR——整套 UI 用
[gpui-kit](https://crates.io/crates/gpui-kit) 手绘。

冻结屏幕、拖框选区，然后复制、保存、标注，或者直接把图里的文字读出来——
全程不用碰鼠标。

**[English](README.md)**

## 状态

Shotori 处于**早期开发阶段**（pre-1.0）。截图、标注、复制、保存、OCR、
贴图这些核心流程已可日常使用，但仍会有毛边：功能可能在版本之间无预警地
调整或移除，命令行参数、键位与主题格式尚未稳定。已在 niri、sway、
Hyprland 上测试，其他 Wayland 合成器表现可能不同。欢迎到
[issue 区](https://github.com/mengh04/shotori/issues)报告问题与反馈。

## 功能

- 区域选择，画完可再调整；多屏感知，混合缩放与旋转输出、跨屏选区都能正确处理
- 标注：矩形、椭圆、直线、折线、箭头、序号、画笔、荧光笔、马赛克/模糊、橡皮、文字
- 双击已有文字可原位编辑，实时换行，可调整字号和颜色，整次编辑可一步撤销；
  文本框与选区四边保留 2px，超出底部的输入、粘贴或字号调整不会生效，避免保存被截断的内容
- 贴图：把选区裁剪成置顶浮动小窗，截图界面关闭后依然保留——可跨屏拖动、滚轮缩放
- 复制到剪贴板、系统"另存为"对话框保存，或 OCR 成文字（首次下载 ~31MB 模型后完全离线）
- CLI 全屏静默截图
- 可选托盘图标；主题跟随系统深浅色

## 环境要求

- wlroots 系 Wayland 合成器（niri、sway、Hyprland……）
- 保存对话框依赖 `xdg-desktop-portal`（绝大多数桌面发行版默认就有）
- 通知 daemon（dunst、mako、swaync……）可选

## 安装

```bash
cargo install shotori        # crates.io
paru -S shotori              # AUR（预编译二进制）
```

也可以从 [GitHub Releases](https://github.com/mengh04/shotori/releases)
直接下载二进制。

绑到键位上，比如 niri：

```kdl
Mod+Shift+S { spawn "shotori"; }
```

`shotori tray` 常驻系统托盘（StatusNotifierItem；waybar、KDE Plasma、
GNOME appindicator 扩展均可用）。

## 使用

运行 `shotori`（或 `shotori gui`）：所有屏幕冻结并出现选区浮层，松开后
选区下方出现等价按钮的工具栏。

| 按键               | 功能                                     |
| ------------------ | ---------------------------------------- |
| 拖动               | 选择区域                                 |
| `Ctrl+A`           | 全选当前屏幕；再按一次 → 所有屏幕        |
| `Enter` / `Ctrl+C` | 复制选区到剪贴板                         |
| `Ctrl+S`           | 保存选区——系统"另存为"对话框            |
| `Ctrl+O`           | OCR 选区 → 文字进剪贴板                  |
| `Ctrl+P`           | 把选区钉成贴图                           |
| `Esc`              | 放弃当前拖动 / 退出                      |

非交互截图（不出现浮层）：

```sh
shotori full                 # 全部屏幕 → 剪贴板
shotori full -p ~/Pictures   # → 目录下带时间戳的 PNG
shotori full -d 2            # 先等 2 秒
```

主题：`shotori --theme light`（也支持 `dark`、`high_contrast`；`auto`
跟随系统）。自定义配色：把
[`docs/theme.example.toml`](docs/theme.example.toml) 复制到
`~/.config/shotori/theme.toml`。

完整命令行参数见 `shotori --help`。

## 开发

```bash
git clone https://github.com/mengh04/shotori
cd shotori
cargo build --release
cargo test    # 单元测试，无需合成器
```

CI 强制 `cargo fmt --all --check` 和
`cargo clippy --all-targets -- -D warnings`，推送前请先本地跑一遍。

- 模块结构：[`src/lib.rs`](src/lib.rs) 文件头
- 决策与踩坑记录：[ROADMAP.md](ROADMAP.md)

## License

[MIT](LICENSE)
