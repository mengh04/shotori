//! Saccade：Wayland 优先的截图套件（区域截图 / 贴图 / OCR / 长截图）
//!
//! 模块地图（装配在 `main.rs`：键位分域绑定 + 开窗）：
//! - [`capture`]：wlr-screencopy 捕获（多输出；子模块 pixels 纯像素处理 + 测试、
//!   wayland 事件状态机）
//! - [`display`]：捕获 ↔ gpui display 匹配（位置制；含 zed#46378 的异步等待）
//! - [`clipboard`]：复制到剪贴板（zwlr_data_control + 后台分身驻留）
//! - [`selection`]：选区状态机（纯逻辑 + 单元测试）
//! - [`export`]：裁剪 → PNG 编码 → 剪贴板/落盘（纯函数 + 单元测试）
//! - [`image_util`]：RGBA → RenderImage（BGRA 契约集中地）
//! - [`overlay`]：覆盖层装配（layer-shell Overlay 层，每屏一个）
//! - [`hud`]：覆盖层视觉件（变暗边带/选区边框/提示条）
//! - [`toolbar`]：选区工具条（按钮与键盘同管线 dispatch_action）
//! - [`pin`]：贴图窗口（layer-shell Top 层）——**挂起中**，拖动到边缘有 bug
//! - [`theme`]：视觉常量（自绘设计系统种子）

pub mod capture;
pub mod clipboard;
pub mod display;
pub mod export;
pub mod hud;
pub mod image_util;
pub mod overlay;
pub mod pin;
pub mod selection;
pub mod theme;
pub mod toolbar;
