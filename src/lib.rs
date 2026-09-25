//! Saccade：Wayland 优先的截图套件（区域截图 / 贴图 / OCR / 长截图）
//!
//! 模块地图：
//! - [`capture`]：wlr-screencopy 捕获库（独立 wayland 连接，同步，~15ms）
//! - [`overlay`]：截图覆盖层（layer-shell Overlay 层，冻结背景 + 选区）
//! - [`pin`]：贴图窗口（layer-shell Top 层，可拖动）
//! - [`theme`]：视觉常量（自绘设计系统种子）

pub mod capture;
pub mod overlay;
pub mod pin;
pub mod theme;
