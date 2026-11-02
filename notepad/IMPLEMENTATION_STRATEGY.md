# Implementation Strategy for Winit + Skia Rendering Migration

## Overview
This document outlines the strategy for migrating the Win32 GDI/DirectWrite rendering and event handling to a cross-platform system using Winit, Glutin (OpenGL context), and Skia.

## Rendering Engine (Skia)
1. **Canvas Setup**: The `skia_safe::Surface` provides a `Canvas`. During the `RedrawRequested` event, we get this canvas (`surface.canvas()`) to perform all drawing operations.
2. **Font Handling**: Skia's `textlayout` module (`skia_safe::textlayout`) should be used for text rendering. We will use `FontMgr::new()` to load custom fonts (e.g., Cascadia Code) and system fonts.
3. **Text Buffer & Syntax Highlighting**:
   * The text content (from `tabs.rs` / `cursor.rs`) needs to be drawn line-by-line or using Skia's `Paragraph` or `TextBlob` for performance.
   * `ParagraphBuilder` allows building text with different styles (colors, font weights) based on the syntax highlighting ranges provided by `syntect` and `tree-sitter`.
4. **Drawing Cursors and Selections**:
   * Cursors: Use `canvas.draw_rect()` or `canvas.draw_line()` for blinking text cursors based on coordinates calculated from the current font metrics and cursor positions.
   * Block Selection / Multi-cursors: Iterate over all active cursor/selection ranges. Use `canvas.draw_rect()` with a semi-transparent selection color behind the text.
5. **UI Elements (Gutter, Scrollbar, Sidebar)**:
   * **Gutter**: Draw a background rect, then draw line numbers using `Paragraph` or `Canvas::draw_str()`.
   * **Scrollbar**: Draw rects mapped to the current scroll position and document length.
6. **DPI Scaling**: Winit provides scale factors. Text sizes and UI coordinates must be multiplied by `window.scale_factor()` to ensure crisp rendering on high-DPI displays.

## Event Handling (Winit)
1. **Keyboard Events**: Map `winit::event::KeyEvent` (physical keys, logical characters) to text input and cursor movement commands. Map modifier keys (Ctrl/Cmd, Shift, Alt) for shortcuts and multi-cursor spawning.
2. **Mouse Events**: Map `CursorMoved`, `MouseInput`, and `MouseWheel` to calculate cursor positioning (using font width/height metrics to map pixel coordinates to text lines/columns) and scrolling.
3. **Window Events**:
   * `Resized`: Update Glutin surface size, recreate Skia surface, update document bounds for word-wrap and scroll calculations.
   * `ScaleFactorChanged`: Adjust font sizes and recalculate metrics.

## File I/O & Theming
* File loading and saving can be delegated to existing asynchronous or background thread logic to keep the UI thread responsive, using `rfd` for dialogs instead of Win32 `GetOpenFileNameW`.
* Themes (Monokai, Mariana) map directly to Skia `Color` structs applied to `TextStyle` during rendering.
