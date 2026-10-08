//! The layout seam (T-21, D-045): a [`Layout`] draws the view model and the
//! cat's frame into a framework-free [`Canvas`], and [`choose`] picks the
//! layout for the terminal's size. Two adapters make it a seam: the full
//! layout (112 × 38, `full.rs`) and the widget (32 × 16, `widget.rs`, which
//! also holds the status-only [`widget::OneLine`] fallback for anything
//! smaller).
#![cfg_attr(not(test), allow(dead_code))]

pub(crate) mod browse;
pub(crate) mod full;
pub(crate) mod modals;
pub(crate) mod widget;

use super::canvas::Canvas;
use super::director::CatFrame;
use super::theme::Theme;
use super::view::ViewModel;
use crate::config;

/// One way of drawing the main screen.
pub trait Layout {
    /// The smallest terminal, in columns × rows, it draws in full.
    fn min_size(&self) -> (u16, u16);

    /// Draws `vm` and the cat's frame into `c`, inside the top-left
    /// [`Layout::min_size`] cells (the canvas clips anything past its edge;
    /// a layout on a larger canvas keeps to its area with
    /// [`Canvas::clipped`]).
    fn draw(&self, c: &mut Canvas, vm: &ViewModel, cat: &CatFrame, theme: &Theme);

    /// Whether drops, pastes and the picker work in this layout. Only the
    /// one-line fallback says no (D-064): the input seam asks the layout
    /// rather than re-deriving the size rule.
    fn accepts_input(&self) -> bool {
        true
    }
}

/// The `[ui] layout` setting: pick by size, or pin one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum LayoutPin {
    #[default]
    Auto,
    Full,
    Widget,
}

impl From<config::Layout> for LayoutPin {
    fn from(l: config::Layout) -> LayoutPin {
        match l {
            config::Layout::Auto => LayoutPin::Auto,
            config::Layout::Full => LayoutPin::Full,
            config::Layout::Widget => LayoutPin::Widget,
        }
    }
}

/// Which layout draws the screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayoutKind {
    /// 112 × 38.
    Full,
    /// 32 × 16.
    Widget,
    /// `=^..^= 3/7 ‼` on one row; accepts no input (D-064).
    OneLine,
}

/// The full layout's size.
pub const FULL_SIZE: (u16, u16) = (112, 38);
/// The widget's size.
pub const WIDGET_SIZE: (u16, u16) = (32, 16);

/// The layout for a `size` (columns, rows) terminal: the full layout from
/// 112 × 38, the widget from 32 × 16, the one-line fallback below that. A pin
/// overrides the choice between the full layout and the widget, never the
/// fallback: below 32 × 16 there is no face to drop onto, so no pin brings
/// input back there (D-064). Re-run on every resize.
pub fn choose(size: (u16, u16), pin: LayoutPin) -> LayoutKind {
    let fits = |(w, h): (u16, u16)| size.0 >= w && size.1 >= h;
    if !fits(WIDGET_SIZE) {
        return LayoutKind::OneLine;
    }
    match pin {
        LayoutPin::Full => LayoutKind::Full,
        LayoutPin::Widget => LayoutKind::Widget,
        LayoutPin::Auto if fits(FULL_SIZE) => LayoutKind::Full,
        LayoutPin::Auto => LayoutKind::Widget,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn choose_table() {
        use LayoutKind::{Full, OneLine, Widget};
        use LayoutPin as P;
        for (size, pin, want) in [
            ((112, 38), P::Auto, Full),
            ((200, 60), P::Auto, Full),
            ((111, 38), P::Auto, Widget),
            ((112, 37), P::Auto, Widget),
            ((80, 24), P::Auto, Widget),
            ((32, 16), P::Auto, Widget),
            ((31, 16), P::Auto, OneLine),
            ((32, 15), P::Auto, OneLine),
            ((20, 1), P::Auto, OneLine),
            ((0, 0), P::Auto, OneLine),
            // pins
            ((112, 38), P::Widget, Widget),
            ((200, 60), P::Widget, Widget),
            ((80, 24), P::Full, Full),
            ((32, 16), P::Full, Full),
            ((111, 38), P::Full, Full),
            ((112, 38), P::Full, Full),
            ((31, 16), P::Full, OneLine),
            ((31, 16), P::Widget, OneLine),
            ((20, 1), P::Full, OneLine),
            ((20, 1), P::Widget, OneLine),
        ] {
            assert_eq!(choose(size, pin), want, "{size:?} {pin:?}");
        }
    }

    #[test]
    fn the_pin_comes_from_the_config() {
        assert_eq!(LayoutPin::from(config::Layout::Auto), LayoutPin::Auto);
        assert_eq!(LayoutPin::from(config::Layout::Full), LayoutPin::Full);
        assert_eq!(LayoutPin::from(config::Layout::Widget), LayoutPin::Widget);
        assert_eq!(LayoutPin::default(), LayoutPin::Auto);
    }
}
