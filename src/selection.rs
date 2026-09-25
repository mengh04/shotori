//! # 选区状态机：覆盖层交互的纯逻辑核心
//!
//! 三态生命周期：`Idle`（无选区）→ `Dragging`（按住拖拽）→ `Selected`（松手定型）。
//! 不含任何渲染/UI 依赖 → 可以脱离合成器做单元测试；渲染层只读 [`Selection::bounds`]。
//!
//! 交互语义（v0.2 起）：
//! - 原地**点击**（拖拽 < 2px）= 清空选区，不产生 0×0 的怪选区
//! - 拖拽中 [`Selection::cancel_drag`]（Esc）= 只放弃本次拖拽

use gpui_kit::*;

/// 判定"点击而非拖拽"的阈值（逻辑像素）：任一边小于它视为无意义选区
const MIN_SIZE: f32 = 2.0;

#[derive(Clone, Copy)]
pub enum Selection {
    Idle,
    Dragging {
        start: Point<Pixels>,
        current: Point<Pixels>,
    },
    Selected {
        bounds: Bounds<Pixels>,
    },
}

impl Selection {
    /// 按下：开始新选区（Selected 状态下重新框选也是它）
    pub fn begin(&mut self, p: Point<Pixels>) {
        *self = Self::Dragging { start: p, current: p };
    }

    /// 拖动到 p；返回是否真的变了（调用方据此决定 cx.notify()）
    pub fn drag_to(&mut self, p: Point<Pixels>) -> bool {
        if let Self::Dragging { current, .. } = self
            && *current != p
        {
            *current = p;
            return true;
        }
        false
    }

    /// 松手：真拖拽 → `Selected`（角点归一化，往左上拖也对）；
    /// 原地点击（< [`MIN_SIZE`]）→ `Idle`（清空选区）
    pub fn end(&mut self, p: Point<Pixels>) {
        if let Self::Dragging { start, .. } = *self {
            let bounds = Self::normalized(start, p);
            *self = if f32::from(bounds.size.width) < MIN_SIZE
                || f32::from(bounds.size.height) < MIN_SIZE
            {
                Self::Idle
            } else {
                Self::Selected { bounds }
            };
        }
    }

    /// 拖拽中取消（Esc 第一段语义）；非拖拽状态不动
    pub fn cancel_drag(&mut self) {
        if matches!(self, Self::Dragging { .. }) {
            *self = Self::Idle;
        }
    }

    /// 当前选区（拖拽中也算，实时显示尺寸标签用）
    pub fn bounds(&self) -> Option<Bounds<Pixels>> {
        match *self {
            Self::Idle => None,
            Self::Dragging { start, current } => Some(Self::normalized(start, current)),
            Self::Selected { bounds } => Some(bounds),
        }
    }

    /// 两个角点 → 归一化 Bounds（左上=origin，宽高恒正）。
    /// 不能用 `Bounds::from_corners`：它不排序，往左上拖会得到负宽高
    /// （v0.1 潜伏 bug：左上拖出"隐形选区"，Enter 时报选区为空）
    fn normalized(a: Point<Pixels>, b: Point<Pixels>) -> Bounds<Pixels> {
        let (left, right) = if a.x <= b.x { (a.x, b.x) } else { (b.x, a.x) };
        let (top, bottom) = if a.y <= b.y { (a.y, b.y) } else { (b.y, a.y) };
        Bounds {
            origin: point(left, top),
            size: size(right - left, bottom - top),
        }
    }

    pub fn is_dragging(&self) -> bool {
        matches!(self, Self::Dragging { .. })
    }

    /// 松手定型了吗（工具条只在这个状态出现）
    pub fn is_selected(&self) -> bool {
        matches!(self, Self::Selected { .. })
    }
}

#[cfg(test)]
mod tests {
    // 注意：不用 `use super::*`——父模块的 `use gpui_kit::*` 会把 gpui 自己的
    // `test` 属性宏带进来，遮蔽内建 `#[test]`（宏展开直接爆递归限制）
    use super::Selection;
    use gpui_kit::{point, px, Point, Pixels};

    fn pt(x: f32, y: f32) -> Point<Pixels> {
        point(px(x), px(y))
    }

    fn size_of(s: &Selection) -> (f32, f32) {
        let b = s.bounds().expect("应有选区");
        (f32::from(b.size.width), f32::from(b.size.height))
    }

    #[test]
    fn 往左上拖_选区归一化() {
        let mut s = Selection::Idle;
        s.begin(pt(100., 100.));
        s.drag_to(pt(40., 60.));
        s.end(pt(40., 60.));
        let b = s.bounds().unwrap();
        assert_eq!(f32::from(b.left()), 40.);
        assert_eq!(f32::from(b.top()), 60.);
        assert_eq!(size_of(&s), (60., 40.));
        assert!(s.is_selected());
    }

    #[test]
    fn 原地点击_清空选区() {
        let mut s = Selection::Idle;
        s.begin(pt(50., 50.));
        s.end(pt(50., 50.));
        assert!(matches!(s, Selection::Idle));
    }

    #[test]
    fn 微小拖拽_视为点击() {
        let mut s = Selection::Idle;
        s.begin(pt(10., 10.));
        s.end(pt(11.5, 10.)); // 1.5px < 2px
        assert!(matches!(s, Selection::Idle));
    }

    #[test]
    fn 点击已有选区_重新开始() {
        let mut s = Selection::Idle;
        s.begin(pt(0., 0.));
        s.end(pt(100., 100.));
        assert!(s.is_selected());
        s.begin(pt(200., 200.)); // Selected 下再按下 = 新选区
        assert!(s.is_dragging());
    }

    #[test]
    fn 拖拽中取消_回到空闲_不影响已定型选区() {
        let mut s = Selection::Idle;
        s.begin(pt(0., 0.));
        s.drag_to(pt(30., 30.));
        s.cancel_drag();
        assert!(matches!(s, Selection::Idle));

        let mut s = Selection::Idle;
        s.begin(pt(0., 0.));
        s.end(pt(30., 30.));
        s.cancel_drag(); // 已定型：不受影响（Esc 走退出语义）
        assert!(s.is_selected());
    }

    #[test]
    fn drag_to_没变化时返回_false() {
        let mut s = Selection::Idle;
        s.begin(pt(5., 5.));
        assert!(s.drag_to(pt(9., 5.)));
        assert!(!s.drag_to(pt(9., 5.))); // 同位置不重复通知
        // Idle 状态下拖动是 no-op
        let mut s = Selection::Idle;
        assert!(!s.drag_to(pt(9., 5.)));
    }
}
