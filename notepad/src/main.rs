use glutin::{
    context::{ContextApi, ContextAttributesBuilder, Version},
    display::{GetGlDisplay, GlDisplay},
    prelude::*,
};
use glutin_winit::{DisplayBuilder, GlWindow};
use raw_window_handle::HasRawWindowHandle;
use skia_safe::{
    gpu::{gl::FramebufferInfo, BackendRenderTarget, SurfaceOrigin},
    Color, ColorType, Surface,
};
use std::path::PathBuf;
use winit::{
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
    window::WindowBuilder,
};

fn main() {
    let event_loop = EventLoop::new().unwrap();
    let window_builder = WindowBuilder::new()
        .with_title("Notepad (Skia + Winit)")
        .with_inner_size(winit::dpi::LogicalSize::new(800.0, 600.0));

    let template = glutin::config::ConfigTemplateBuilder::new();
    let display_builder = DisplayBuilder::new().with_window_builder(Some(window_builder));

    let (window, gl_config) = display_builder
        .build(&event_loop, template, |configs| {
            configs
                .reduce(|accum, config| {
                    if config.num_samples() > accum.num_samples() {
                        config
                    } else {
                        accum
                    }
                })
                .unwrap()
        })
        .unwrap();

    let window = window.unwrap();

    let raw_window_handle = Some(window.raw_window_handle());
    let context_attributes = ContextAttributesBuilder::new().build(raw_window_handle);

    let fallback_context_attributes = ContextAttributesBuilder::new()
        .with_context_api(ContextApi::Gles(None))
        .build(raw_window_handle);

    let not_current_gl_context = unsafe {
        gl_config
            .display()
            .create_context(&gl_config, &context_attributes)
            .unwrap_or_else(|_| {
                gl_config
                    .display()
                    .create_context(&gl_config, &fallback_context_attributes)
                    .expect("failed to create context")
            })
    };

    let (width, height): (u32, u32) = window.inner_size().into();
    let attrs = glutin::surface::SurfaceAttributesBuilder::<glutin::surface::WindowSurface>::new()
        .build(
            window.raw_window_handle(),
            std::num::NonZeroU32::new(width).unwrap(),
            std::num::NonZeroU32::new(height).unwrap(),
        );

    let gl_surface = unsafe {
        gl_config
            .display()
            .create_window_surface(&gl_config, &attrs)
            .unwrap()
    };

    let gl_context = not_current_gl_context.make_current(&gl_surface).unwrap();

    gl::load_with(|s| {
        gl_config
            .display()
            .get_proc_address(std::ffi::CString::new(s).unwrap().as_c_str())
    });

    let mut gr_context =
        skia_safe::gpu::DirectContext::new_gl(None, None).expect("Failed to create GrContext");

    let mut create_surface =
        |window: &winit::window::Window, gr_context: &mut skia_safe::gpu::DirectContext| {
            let (width, height): (u32, u32) = window.inner_size().into();
            let mut fboid = 0;
            unsafe { gl::GetIntegerv(gl::FRAMEBUFFER_BINDING, &mut fboid) };

            let fb_info = FramebufferInfo {
                fboid: fboid.try_into().unwrap(),
                format: skia_safe::gpu::gl::Format::RGBA8.into(),
            };

            let backend_render_target =
                BackendRenderTarget::new_gl((width as i32, height as i32), Some(1), 0, fb_info);

            Surface::from_backend_render_target(
                gr_context,
                &backend_render_target,
                SurfaceOrigin::BottomLeft,
                ColorType::RGBA8888,
                None,
                None,
            )
            .unwrap()
        };

    let mut surface = create_surface(&window, &mut gr_context);

    event_loop
        .run(move |event, window_target| {
            match event {
                Event::WindowEvent { event, .. } => match event {
                    WindowEvent::CloseRequested => window_target.exit(),
                    WindowEvent::Resized(physical_size) => {
                        if physical_size.width != 0 && physical_size.height != 0 {
                            gl_surface.resize(
                                &gl_context,
                                std::num::NonZeroU32::new(physical_size.width).unwrap(),
                                std::num::NonZeroU32::new(physical_size.height).unwrap(),
                            );
                            surface = create_surface(&window, &mut gr_context);
                            window.request_redraw();
                        }
                    }
                    WindowEvent::RedrawRequested => {
                        let canvas = surface.canvas();
                        canvas.clear(Color::WHITE);
                        // Drawing logic will be migrated here

                        gr_context.flush_and_submit();
                        gl_surface.swap_buffers(&gl_context).unwrap();
                    }
                    _ => (),
                },
                Event::AboutToWait => {
                    // window.request_redraw();
                }
                _ => (),
            }
        })
        .unwrap();
}
