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
- **240Hz 隐形案（终审 2026-09-25 深夜）**：与刷新率**完全无关**——真凶是 **layer surface 落点抽签**。
  gpui 不给 layer surface 指定 output 时 niri 自选（焦点所在屏），覆盖层有时落在 DP-2（720×1280），
  grim 只拍 HDMI 自然"隐形"；恰好 60Hz 测试时段覆盖层落在 HDMI，造成"刷新率相关"的假象。
  修复：开窗时带 `display_id` 钉在捕获的屏上（上游管道本来就有：WindowOptions.display_id →
  wl_outputs 匹配 → get_layer_surface(output)）。**教训：三屏异缩放环境，任何"某屏拍不到"
  先查落点再查渲染。**
- **displays() 启动为空（zed#46378）的真实形态**：同步启动阶段恒为空，但事件循环首圈后
  （cx.spawn 的第一次 update）就有 3 屏。workaround：开窗挪进 spawn 的异步任务，首拍即得。
  display_id 匹配：bounds 尺寸 == 捕获尺寸（scale=1 精确；异缩放匹配需 vendor 暴露 output 名，backlog）。
- **scale_factor() 多屏错报**：窗口钉在 HDMI（渲染按 1.0），`window.scale_factor()` 却报 1.5
  （DP-2 的）。裁剪改用"捕获物理宽 ÷ 窗口逻辑宽"自算，与渲染天然自洽。
- **niri 的 zwlr_virtual_pointer 疑似死的**：motion_absolute/motion/button 全部石沉大海
  （WAYLAND_DEBUG 确认请求已上电线，客户端零事件；with_output/不带 output、绝对/相对都一样）。
  lab 的 vinput 测试台因此不可用，GUI 自动化点击暂无手段（键盘侧未测）。可报 niri 上游。
- **RenderImage 契约**：BGRA 字节（Vulkan 后端），内存直喂必须 swap(0,2)；PNG 路径是 RGBA。
- **wl_shm format 是序号**（xrgb8888=1）不是 DRM fourcc；格式名描述"字"的位序，小端内存反序。
- **多屏 output 选择**：上游 zed#46378（displays() 启动为空），修复 PR #61578 久未 review；
  可 vendor 时顺手带 roundtrip 补丁。

### 开发后门
- `SACCADE_DEBUG_SELECTION=x,y,w,h`：注入现成选区（自动化验证选区 UI 用）
- `SACCADE_DEBUG_ACTION=copy`：启动 1.5s 后自动触发复制动作——无头 e2e 的唯一入口
  （验证套路：`SACCADE_DEBUG_TARGET=HDMI-A-1 SACCADE_DEBUG_SELECTION=... SACCADE_DEBUG_ACTION=copy ./saccade & sleep 4;
  wl-paste --type image/png | 尺寸断言`）
- `SACCADE_DEBUG_TARGET=<输出名>`：多屏下限定后门只作用于目标屏（全开会打架）

## v0.3 多屏支持（2026-09-26 凌晨）

### 功能
- capture_all_outputs()：一条连接捕获全部输出（三屏 ~350ms 含编码），每屏一个 Capture
- **每块输出一个覆盖层窗口**（display_id 钉屏），选区/裁剪/复制独立，
  Enter/Esc 作用于"你交互的那块屏"（niri 的 exclusive layer 键盘焦点跟随焦点输出——**待用户实测**）
- 裁剪的自算 scale 天然兼容各屏不同 scale（eDP 2.0 / DP-2 1.5 / HDMI 1.0）
- screencap --all：每屏一张调试图

### 结案记录（多屏篇）
- **gpui 的 display bounds 坐标 = 输出逻辑位置 ÷ wl_output 整数 scale**（backend 自己除的，
  对拍实测：eDP 1920,0→960,0；DP-2 -720,-100→-360,-50）。匹配 display 时必须用同一套算法
- **wl_output.scale 是整数**：1.5x 屏报 2（ceil），真值要走 fractional 协议（输出级拿不到）。
  所以尺寸匹配不可行，改用位置匹配（多屏布局 origin 唯一）
- **transform 语义实测**：niri "90° counter-clockwise"（Transform::_90）= 把 buffer **顺时针**转
  90° 填进面板，与协议字面相反。rotate_rgba 按 grim 对拍校准；Flipped 系罕见未处理
- **教训：壁纸轮播毁对拍**——DP-2 的照片壁纸会换方向，跨时间的 grim 对比相关性可乱到 0.54；
  验证姿势：grim→screencap→grim 一秒窗口内三方对比
- 剩余限制：选区不能跨屏；旋转+翻转组合（Flipped90 等）未实现；非 niri 合成器多 Exclusive
  覆盖层的键盘行为未知

## v0.2.1 剪贴板复制（2026-09-25 深夜）

### 功能
- Enter / Ctrl+C / 工具条[复制]：选区 → PNG → 剪贴板 → 退出；无选区 = 全屏
- Ctrl+S / 工具条[保存]：落盘（原 Enter 行为）；工具条改 [复制][保存][取消]（贴图按钮候补）
- **驻留 offer 模型**（wl-copy 同款）：复制 = re-exec 自身 `--clipboard-daemon` 分身，
  stdin 传 PNG 字节；分身挂 `zwlr_data_control` 源服务粘贴，被覆盖时收 cancelled 退场
- wayland-rs 坑：compositor 在 data_offer 事件里替客户端建新对象，父接口必须特化
  `event_created_child`（默认实现 panic，`event_created_child!` 宏一行解决）
- 全自动 e2e 已验证：字节级读回一致 / 重复粘贴 / 新旧分身替换 / 全链路（钉屏后 600×400 精确）

### 顺带修复（详见上方结案记录）
- 覆盖层/贴图 display_id 钉屏（落点抽签 bug，即"240Hz 隐形"真凶）
- 裁剪 scale 自算（scale_factor() 多屏错报）
- 工具条按钮点击仍未被真实鼠标验证（vinput 死亡）——传播链分析认为没问题，待日常使用确认

## v0.2 基础功能整理（2026-09-25）

### 模块化
overlay.rs（408 行）拆为：`selection.rs`（状态机，纯逻辑+测试）、`export.rs`（裁剪/PNG/落盘，
纯函数+测试）、`toolbar.rs`（工具条）、`image_util.rs`（BGRA 契约收口，pin 同步去重）。
overlay 只剩装配。首批 12 个单元测试（不需要合成器）。

### 行为修正（v0.1 → v0.2）
- 往左上拖产生负宽高"隐形选区"（`Bounds::from_corners` 不归一化）——测试炸出的潜伏 bug，已修
- 原地点击（<2px）= 清空选区，不再出现 0×0 选区+工具条
- 两段 Esc：拖拽中=放弃本次拖拽；松手后=退出
- 无选区 Enter = 全屏保存
- 保存失败不再 panic：打印错误、留在覆盖层可重试
- 文件名 `Saccade_年-月-日_时-分-秒.png`，同秒冲突自动 `_2`/`_3`
- 工具条只在松手定型后出现（拖拽中不闪）

### 挂起：贴图（pin）
拖动到边缘有 bug（嫌疑人：① 窗口相对坐标的运动参考系问题 ② niri 对越界 margin 的钳制
③ 光标跑出小窗后隐式抓取是否继续投递）。测试台已备：lab 的 `vinput`（zwlr_virtual_pointer
虚拟指针，可全自动驱动拖拽）。恢复侦查时的注意：用户远程鼠标与虚拟鼠标会打架。

### 待人工验收（用户回到屏幕后）
- 两段 Esc 手感、点击清空、无选区 Enter 全屏、新文件名
- 工具条按钮点击（此前虚拟指针点"贴图"未触发，坐标已修正为按钮文字簇中心 x≈298——待复核）
