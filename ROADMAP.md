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

## v0.3.1 重构：反屎山（2026-09-26 凌晨）

- capture.rs(449行) 拆为 capture/{mod,wayland,pixels}：编排/事件状态机/纯像素
- display.rs 新建：display 匹配从 main.rs 挪出，匹配谓词单测锁定"gpui 坐标=位置÷整数scale"
- hud.rs 新建：dim_strips/selection_chrome/hint_bar 从 overlay.rs 移出
- overlay.rs 后门拆成 debug_targeted/debug_selection/spawn_debug_copy 私有函数
- **单测抓到真 bug**：rotated_size 的 _180 落进 catch-all（180° 会错误交换宽高；
  此前靠未写内存假通过）——已修 + 四角断言锁死
- pin_selection 裁剪改走自算 scale（原 scale_factor() 错报路径的漏网之鱼）
- 删除死代码 capture_first_output；测试 12 → 21

## 发布路线（2026-09-26）

- **本地安装**：`cargo install --path . --bin saccade` → ~/.cargo/bin（无阻塞）
- **crates.io**：三阻塞 ① publish=false ② [patch.crates-io] 本地 path（crates.io 禁止）
  ③ set_layer_margin 上游未合并。路线：pin 做 feature gate → 无 pin 构建去掉 patch → 可发
- **AUR / GitHub Release**：对 Arch 用户更现实；PKGBUILD 里可以打 vendor 补丁
- 用户现状：niri `Mod+Shift+S` 原绑 shotori，saccade 接班中

## v0.4.0 改名 shotori + 取消/Esc 修复（2026-09-26）

- 项目更名 saccade → shotori（用户沿用旧工具名；旧 ~/Projects/shotori 源码不动，
  cargo bin 直接覆盖）
- **修复：取消按钮/Esc 全部无反应**——真实用户鼠标+探针日志定位：
  按钮点击链路全通（容器拦截→on_click→dispatch_action→覆盖层 handler），
  但 gpui 窗口内 dispatch_action 的动作**沿焦点路径走完就停，不会冒泡到
  App::on_action**——退出逻辑押在 app 级兜底上，从工具条诞生起就是死的。
  修复：两段 Esc 就地在覆盖层 handler 处理（拖拽中=取消拖拽，否则 quit）
- 后门升级：SHOTORI_DEBUG_ACTION=copy|quit（quit 走真实 dispatch_action 管线，e2e 可测退出）

## v0.5.0 拔 pin 解锁发布（2026-09-26）

- pin（贴图）整体移到 `pin` 分支保存（含 vendor set_layer_margin 补丁依赖）
- main 移除 [patch.crates-io]（走上游原版 gpui-pre）、publish=false、
  补齐 crates.io 元数据（license/repository 待用户确认）
- `cargo publish --dry-run --allow-dirty` 通过：无 path 依赖、打包合规
- 真发布：`cargo login` → `cargo publish`

## v0.6.0 选区 OCR（2026-09-26）

### 功能
- Ctrl+O：选区 → PP-OCRv6 small（rapidocr-core + ort/ONNX Runtime）→ 文本进剪贴板
- 首次使用自动从 ModelScope 下载模型（4 文件 ~31MB）到
  `~/.local/share/shotori/ocr-models/`，二次调用秒开（引擎 OnceLock<Mutex> 常驻）
- feature gate：`--features ocr`；默认构建零增量（crates.io 发布不受重依赖拖累）
- 剪贴板 daemon 通用化：`--clipboard-daemon <MIME>`，copy_image/copy_text 共用
  分身框架；文本 offer `text/plain;charset=utf-8` + 降级 `text/plain`

### 选型记录（为什么是 rapidocr-core）
- 候选：rapidocr-core（ONNX/ort）vs rusto-rs（MNN）vs paddle-ocr-rs
- rusto-rs 的 mnn-sys 构建链三策略（vendor/prebuilt/源码）+ bindgen/cmake 偏脆
- rapidocr-core：`run_image(&RgbImage)` 直接吃内存像素；模型缓存体系完善
  （ModelCache + SHA256 校验）；ort 构建时自动下载预编译 libonnxruntime 静态链接
- 准确率：PP-OCRv6 small 中英混排可用；1080p 屏 <16px 小字会吃力
  （HiDPI 屏反而好，物理像素多）

### e2e 验证记录
- 首次：模型自动下载 ✓ → 全屏 OCR → 剪贴板文本读回（终端内容中英混排转录）✓
- 二次：秒开，日志完整 ✓
- 精确选区 1500x800 → 32 行 612 字符 ✓（行结构保留）
- 图片复制回归 600x400 ✓（daemon 改造无破坏）
- 真实用户 Ctrl+O 验收：文件侧边栏 631x328 → 14 行 ✓
- 已知小坑：stdout 重定向到文件时全缓冲，SIGTERM 杀进程丢最后几行日志
  （前台使用无影响）；行首缺字多为选区边缘切字，非模型问题

### 备注
- `opencode run` 会话里 SHOTORI_DEBUG_* 环境变量不残留（每 shell 独立）
- OCR 结果无 GUI 预览（v1 直接进剪贴板）；浮窗预览/编辑候补

## v0.6.1 OCR 实现审查（2026-09-26）

### 审查发现并修复
- **🔴 损坏模型永久堵死**：rapidocr-core 的 download_asset 直接写目标文件（无
  temp+rename），下载中断留截断文件 → 之后每次 sha256 校验失败 → 永久报错。
  修复：初始化失败时清掉模型缓存目录，下次重试从零下载（已实测：塞垃圾文件
  → 报 sha mismatch → 目录被清 → 重跑自动重新下载成功）
- **🔴 init 走 panic 行为不可控**：get_or_init + expect 的失败路径（断网首跑、
  目录不可写）会 panic 穿 gpui 后台执行器。修复：init_engine 改返 Result，
  失败不缓存（OnceLock 不 set）→ 覆盖层打错误后保持可用，下次 Ctrl+O 重试。
  实测：XDG_DATA_HOME 指向不可写路径 → 干净报错、进程存活可 Esc
- **自埋 bug**：重构时漏了 ENG.set()——下载+建引擎全成功然后把引擎扔了，
  报"初始化后丢失"。e2e 抓到（重跑第二次成功路径），已修
- **🟡 提示条/键位 feature 门控**：非 ocr 构建不再宣传/绑定 Ctrl+O
  （随后 OCR 转为默认 feature，门控变成轻构建出口）
- **🟡 文本粘贴兼容**：文本模式 offer 补齐 UTF8_STRING/STRING（xwayland
  老应用）；Send 处理改为"命中 offer 列表任意项"
- **🟢 首次下载反馈**：模型缺失时终端先打"下载 PP-OCRv6 small 模型…"
- **🟢 下载独立线程的真实理由**：reqwest::blocking 不能在异步上下文跑
  （gpui 后台执行器就是异步上下文），注释已纠正

### 设计变更：OCR 转为默认 feature
- `default = ["ocr"]`：crates.io/AUR 用户裸装即得完整功能，OCR 不再是暗桩
- 轻构建出口：`--no-default-features`
- 理由：产品身份=截图+OCR；gpui 依赖树面前 ort+reqwest 的增量是零头

### 审查方法论记录
- 失败路径（断网/坏文件/重试）是懒加载设计的必修课，成功路径 e2e 不够
- "wl-paste -l 突然少了 MIME"→ 先怀疑自己，再怀疑 compositor，最后想起
  用户也在用电脑（他们的复制会顶掉测试态）

## v0.6.2 工具条 OCR 按钮 + 代码全英文化（2026-09-26）

### 功能
- 工具条加 [OCR] 按钮：[Copy][Save][OCR][Cancel]，与键盘同管线
  dispatch_action；ocr feature 裁掉时按钮编译期消失（`.children(Option)`）
- 提示条/按钮/日志全部英文（UI 面向 crates.io/AUR 的国际用户）

### 英文化范围与原则
- src/ 全部 16 文件：doc 注释、行内注释、字符串字面量、测试函数名
- 翻译保留全部"战史"知识（wayland-rs 三坑、gpui 坑、grim 对拍校准等），
  只换语言不删内容
- ROADMAP.md 保持中文（项目活文档，不是代码）
- 两个 feature 组合 build/test/clippy 全绿；OCR/复制 e2e 回归通过

### 小坑记录
- `#[cfg]` 不能挂在方法链表达式中间（`.child()` 链里插属性不是合法
  Rust）——用 `.children(Option<E>)` 收编（children 吃 IntoIterator，
  Option 天然是）
- gpui-kit 没给 Option<impl IntoElement> 实现 IntoElement（上游 gpui 有），
  Infallible 也没有——非 ocr 桩返回 Option<&'static str> 最省事

## v0.6.3 OCR 提速：预热 + 跳过重复哈希（2026-09-26）

### 问题
- 一次性进程 × 进程内引擎缓存 = 每次 Ctrl+O 都全量冷启动
  （读盘 31MB + 建 3 个 ort session + sha256 哈希 31MB），实测 OCR 净耗时
  1464ms（release，800x400 选区 34 行）
- 诊断方法论：debug 构建的纯 Rust 前后处理慢 10-100 倍（10.8s），
  测速必须用 release 安装版（3.36s 全链路）

### 修复
- **A. 预热**：overlay 打开即后台 warmup（独立线程，仅当模型已缓存——
  首次使用不在用户只要截图时惊喜下载 31MB）。init 藏进用户框选的
  2-5 秒里
- **B. 跳过重复 sha256**：模型齐全时完全跳过 ensure（它每次调用全量
  哈希）；损坏检测改由"engine init 失败 → 清缓存"兜底
- 效果：后门最坏情况 OCR 净耗时 1464ms → 890ms；真实使用（框选 2-5s）
  Ctrl+O 只剩纯推理 ~300-500ms

### 自愈行为升级（意外收获）
- 损坏模型文件现在被 warmup 线程静默消化：warmup 撞上损坏 → init 失败
  → 清缓存；随后正式 OCR 发现缓存空 → 自动重新下载 → 用户无感修复
  （v0.6.1 是显式报错后手动重试）

### 备注
- `let _ = engine()` 触发 let_underscore_lock lint（故意放锁也不行），
  显式 `drop(engine())` 表达意图
