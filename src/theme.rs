//! Shotori 的视觉常量——自绘设计系统的种子。
//! 走 gpui-base 自绘路线（见 ROADMAP），颜色/间距集中在这里，长成主题系统。

/// 选区外变暗层：55% 黑
pub const DIM: u32 = 0x0000008C;
/// 强调色：橙（选框、贴图聚焦边框）
pub const ACCENT: u32 = 0xFF6A00FF;
/// 贴图未聚焦时的边框
pub const PIN_BORDER: u32 = 0xFF6A0099;
/// 提示条/卡片底色：深灰蓝 90%
pub const CHIP_BG: u32 = 0x16161DE6;
/// 提示条文字：浅灰
pub const HINT_TEXT: u32 = 0xAAAAAAFF;
/// 工具条按钮文字：近白
pub const BTN_TEXT: u32 = 0xEEEEEEFF;
/// 工具条按钮 hover 底色：15% 白
pub const BTN_HOVER_BG: u32 = 0xFFFFFF26;
