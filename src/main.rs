use std::{
    array,
    borrow::Cow,
    sync::{
        Arc, RwLock,
        atomic::{AtomicU8, AtomicU64, Ordering},
        mpsc::{Sender, channel},
    },
    time::{Duration, Instant},
};

use encase::ShaderType;
use wgpu::util::DeviceExt as _;
use winit::{
    application::ApplicationHandler,
    dpi::PhysicalSize,
    event::{ElementState, MouseButton, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    window::{Window, WindowId},
};

#[derive(Copy, Clone, ShaderType)]
struct Uniforms {
    width: u32,
    height: u32,
}

impl Uniforms {
    fn to_bytes(self) -> encase::internal::Result<Vec<u8>> {
        let mut buffer = encase::UniformBuffer::new(Vec::new());
        buffer.write(&self)?;
        Ok(buffer.into_inner())
    }
}

const SIM_RATE: f32 = 800.0;

fn main() {
    let event_loop = EventLoop::new().unwrap();
    event_loop.set_control_flow(ControlFlow::Poll);
    let (tx, rx) = channel();
    let mut app = App {
        state: None,
        send_state: tx,
        real_size: PhysicalSize::new(0, 0),
    };
    std::thread::spawn(move || {
        let state = rx.recv().unwrap();
        let delay_interval = Duration::from_secs_f32(1. / SIM_RATE);
        let mut last_iter = Instant::now();
        let mut last_second = last_iter;
        let mut count = 0;
        loop {
            let state = state.read().unwrap();
            let pressed = state.mouse_pressed.load(Ordering::Relaxed);
            let input = match pressed {
                0b00 => None,
                0b01 => {
                    let combined = state.mouse_pos.load(Ordering::Relaxed).to_le_bytes();
                    let x = f32::from_le_bytes(array::from_fn(|i| combined[i]));
                    let y = f32::from_le_bytes(array::from_fn(|i| combined[4 + i]));
                    Some(DrawInput { x, y, size: 50. })
                }
                0b10 => {
                    let combined = state.mouse_pos.load(Ordering::Relaxed).to_le_bytes();
                    let x = f32::from_le_bytes(array::from_fn(|i| combined[i]));
                    let y = f32::from_le_bytes(array::from_fn(|i| combined[4 + i]));
                    Some(DrawInput { x, y, size: -50. })
                }
                0b11 => None,
                _ => unreachable!(),
            };
            let mut encoder = state.state.create_encoder();
            state.state.simulate_step(&mut encoder, input);
            state.state.submit(encoder);
            let elapsed = last_iter.elapsed();
            if elapsed < delay_interval {
                std::thread::sleep(delay_interval - elapsed);
                last_iter += delay_interval;
            } else {
                last_iter = Instant::now();
            }

            count += 1;
            if last_second.elapsed() > Duration::from_secs(1) {
                eprint!("\r{count}");
                std::io::Write::flush(&mut std::io::stderr()).unwrap();
                count = 0;
                last_second = last_iter;
            }
        }
    });
    event_loop.run_app(&mut app).unwrap();
}

macro_rules! pipeline {
    ($name: ident) => {
        #[allow(unused)]
        struct $name {
            bind_group_layout: wgpu::BindGroupLayout,
            bind_group: Option<wgpu::BindGroup>,
            pipeline_layout: wgpu::PipelineLayout,
            pipeline: wgpu::$name,
        }
    };
}

pipeline!(ComputePipeline);
pipeline!(RenderPipeline);

struct DrawInput {
    x: f32,
    y: f32,
    size: f32,
}

struct State {
    window: Arc<Window>,
    size: PhysicalSize<u32>,
    surface_format: wgpu::TextureFormat,
    uniform_buffer: wgpu::Buffer,
    snad_buffers: [wgpu::Buffer; 2],
    render_shader: wgpu::ShaderModule,
    compute_shader: wgpu::ShaderModule,
    input_pipeline: ComputePipeline,
    simulate_pipeline: ComputePipeline,
    render_pipeline: RenderPipeline,
    queue: wgpu::Queue,
    device: wgpu::Device,
    surface: wgpu::Surface<'static>,
}

impl State {
    async fn new(window: Arc<Window>) -> Self {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions::default())
            .await
            .unwrap();
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                required_features: wgpu::Features::IMMEDIATES,
                required_limits: wgpu::Limits {
                    max_immediate_size: 12,
                    ..Default::default()
                },
                ..Default::default()
            })
            .await
            .unwrap();

        let size = window.inner_size();

        let surface = instance.create_surface(window.clone()).unwrap();
        let cap = surface.get_capabilities(&adapter);
        let surface_format = cap.formats[0];
        let uniforms_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Uniforms buffer"),
            contents: &Uniforms {
                width: size.width,
                height: size.height,
            }
            .to_bytes()
            .unwrap(),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::UNIFORM,
        });
        let render_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("render shader"),
            source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(include_str!("render.wgsl"))),
        });
        let compute_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("compute shader"),
            source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(include_str!("compute.wgsl"))),
        });
        let input_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Input bind group layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage { read_only: false },
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                ],
            });
        let input_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Input compute pipeline layout"),
                bind_group_layouts: &[&input_bind_group_layout],
                immediate_size: 12,
            });
        let input_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("Compute input pipeline"),
            layout: Some(&input_pipeline_layout),
            module: &compute_shader,
            entry_point: Some("input"),
            compilation_options: Default::default(),
            cache: None,
        });
        let input_pipeline = ComputePipeline {
            bind_group_layout: input_bind_group_layout,
            bind_group: None,
            pipeline_layout: input_pipeline_layout,
            pipeline: input_pipeline,
        };

        let simulate_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Simulate bind group layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage { read_only: false },
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage { read_only: false },
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                ],
            });
        let simulate_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Simulate compute pipeline layout"),
                bind_group_layouts: &[&simulate_bind_group_layout],
                immediate_size: 0,
            });
        let simulate_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("Compute simulate pipeline"),
            layout: Some(&simulate_pipeline_layout),
            module: &compute_shader,
            entry_point: Some("simulate"),
            compilation_options: Default::default(),
            cache: None,
        });
        let simulate_pipeline = ComputePipeline {
            bind_group_layout: simulate_bind_group_layout,
            bind_group: None,
            pipeline_layout: simulate_pipeline_layout,
            pipeline: simulate_pipeline,
        };

        let render_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Render bind group layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage { read_only: true },
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                ],
            });
        let render_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Render pipeline layout"),
                bind_group_layouts: &[&render_bind_group_layout],
                immediate_size: 0,
            });
        let render_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Render pipeline"),
            layout: Some(&render_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &render_shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &render_shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(surface_format.into())],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        let render_pipeline = RenderPipeline {
            bind_group_layout: render_bind_group_layout,
            bind_group: None,
            pipeline_layout: render_pipeline_layout,
            pipeline: render_pipeline,
        };

        let snad_buffers = Self::create_snad_buffers(&device, size);
        let mut state = State {
            window,
            size,
            surface_format,
            uniform_buffer: uniforms_buffer,
            snad_buffers,
            compute_shader,
            render_shader,
            input_pipeline,
            simulate_pipeline,
            render_pipeline,
            queue,
            device,
            surface,
        };

        state.create_bind_groups();
        state.configure_surface();

        state
    }

    fn get_window(&self) -> &Window {
        &self.window
    }

    fn configure_surface(&self) {
        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: self.surface_format,
            view_formats: vec![self.surface_format.add_srgb_suffix()],
            alpha_mode: Default::default(),
            width: self.size.width,
            height: self.size.height,
            desired_maximum_frame_latency: 2,
            present_mode: wgpu::PresentMode::AutoVsync,
        };
        self.surface.configure(&self.device, &surface_config);
    }

    fn create_encoder(&self) -> wgpu::CommandEncoder {
        self.device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None })
    }

    fn create_texture(&self) -> (wgpu::SurfaceTexture, wgpu::TextureView) {
        let surface_texture = self
            .surface
            .get_current_texture()
            .expect("failed to acquire next swapchain texture");
        let texture_view = surface_texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor {
                format: Some(self.surface_format.add_srgb_suffix()),
                ..Default::default()
            });
        (surface_texture, texture_view)
    }

    fn simulate_step(&self, encoder: &mut wgpu::CommandEncoder, input: Option<DrawInput>) {
        let mut compute_pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("Compute pass"),
            timestamp_writes: None,
        });
        if let Some(input) = input {
            compute_pass.set_pipeline(&self.input_pipeline.pipeline);
            compute_pass.set_bind_group(0, self.input_pipeline.bind_group.as_ref().unwrap(), &[]);
            let immediate_data =
                [input.x, self.size.height as f32 - input.y, input.size].map(f32::to_le_bytes);
            compute_pass.set_immediates(0, immediate_data.as_flattened());
            compute_pass.dispatch_workgroups(self.size.width.div_ceil(64), self.size.height, 1);
        }
        compute_pass.set_pipeline(&self.simulate_pipeline.pipeline);
        compute_pass.set_bind_group(0, self.simulate_pipeline.bind_group.as_ref().unwrap(), &[]);
        compute_pass.dispatch_workgroups(self.size.width.div_ceil(64), self.size.height - 1, 1);

        drop(compute_pass);

        encoder.copy_buffer_to_buffer(
            &self.snad_buffers[1],
            0,
            &self.snad_buffers[0],
            0,
            (self.size.width as usize * self.size.height as usize * size_of::<u32>()) as u64,
        );
    }

    fn render(&self, encoder: &mut wgpu::CommandEncoder, texture_view: &wgpu::TextureView) {
        let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Render pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: texture_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::GREEN),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });

        render_pass.set_pipeline(&self.render_pipeline.pipeline);
        render_pass.set_bind_group(0, self.render_pipeline.bind_group.as_ref().unwrap(), &[]);
        render_pass.draw(0..6, 0..1);

        drop(render_pass);
    }

    fn submit(&self, encoder: wgpu::CommandEncoder) {
        self.queue.submit([encoder.finish()]);
    }

    fn present(&self, surface_texture: wgpu::SurfaceTexture) {
        self.window.pre_present_notify();
        surface_texture.present();
    }

    fn create_snad_buffers(device: &wgpu::Device, size: PhysicalSize<u32>) -> [wgpu::Buffer; 2] {
        array::from_fn(|i| {
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(&format!("Snad buffer {i}")),
                contents: &vec![0; size.width as usize * size.height as usize * size_of::<u32>()],
                usage: wgpu::BufferUsages::STORAGE
                    | wgpu::BufferUsages::COPY_SRC
                    | wgpu::BufferUsages::COPY_DST,
            })
        })
    }

    fn create_bind_groups(&mut self) {
        let entries = &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: self.uniform_buffer.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: self.snad_buffers[0].as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: self.snad_buffers[1].as_entire_binding(),
            },
        ];
        self.input_pipeline.bind_group =
            Some(self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Input bind group"),
                layout: &self.input_pipeline.bind_group_layout,
                entries: &entries[..=1],
            }));
        self.simulate_pipeline.bind_group =
            Some(self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Simulate bind group"),
                layout: &self.simulate_pipeline.bind_group_layout,
                entries,
            }));
        self.render_pipeline.bind_group =
            Some(self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Render bind group"),
                layout: &self.render_pipeline.bind_group_layout,
                entries: &entries[..=1],
            }));
    }

    fn resize(&mut self, new_size: PhysicalSize<u32>) {
        self.size = new_size;
        self.configure_surface();
        self.snad_buffers = Self::create_snad_buffers(&self.device, new_size);
        self.queue.write_buffer(
            &self.uniform_buffer,
            0,
            &Uniforms {
                width: new_size.width,
                height: new_size.height,
            }
            .to_bytes()
            .expect("Error serializing new uniforms"),
        );
        self.create_bind_groups();
    }
}

struct UiState {
    state: State,
    mouse_pressed: AtomicU8,
    mouse_pos: AtomicU64,
}

struct App {
    state: Option<Arc<RwLock<UiState>>>,
    send_state: Sender<Arc<RwLock<UiState>>>,
    real_size: PhysicalSize<u32>,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let window = Arc::new(
            event_loop
                .create_window(Window::default_attributes())
                .unwrap(),
        );
        let state = pollster::block_on(State::new(Arc::clone(&window)));
        self.real_size = state.size;
        let state = Arc::new(RwLock::new(UiState {
            state,
            mouse_pressed: AtomicU8::new(0),
            mouse_pos: AtomicU64::new(0),
        }));
        self.send_state.send(Arc::clone(&state)).unwrap();
        self.state = Some(state);

        window.request_redraw();
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let lock = self.state.as_ref().unwrap().read().unwrap();
        let UiState {
            state,
            mouse_pressed,
            mouse_pos,
        } = &*lock;
        match event {
            WindowEvent::CloseRequested => {
                println!("Close requested, stopping");
                event_loop.exit();
            }
            WindowEvent::RedrawRequested => {
                let mut encoder = state.create_encoder();
                let (surface_texture, texture_view) = state.create_texture();
                state.render(&mut encoder, &texture_view);
                state.submit(encoder);
                state.present(surface_texture);
                state.get_window().request_redraw();
            }
            WindowEvent::Resized(size) => {
                self.real_size = size;
                drop(lock);
                let mut lock = self.state.as_ref().unwrap().write().unwrap();
                lock.state.resize(size);
            }
            WindowEvent::MouseInput { button, state, .. } => {
                let curr_val = mouse_pressed.load(Ordering::Relaxed);
                mouse_pressed.store(
                    match (button, state) {
                        (MouseButton::Left, ElementState::Pressed) => curr_val | 0b1,
                        (MouseButton::Left, ElementState::Released) => curr_val & !0b1,
                        (MouseButton::Right, ElementState::Pressed) => curr_val | 0b10,
                        (MouseButton::Right, ElementState::Released) => curr_val & !0b10,
                        _ => return,
                    },
                    Ordering::Relaxed,
                );
            }
            WindowEvent::CursorMoved { position, .. } => {
                let pct_x = position.x as f32 / self.real_size.width as f32;
                let pct_y = position.y as f32 / self.real_size.height as f32;
                let mut combined = [0; 8];
                combined[..4].copy_from_slice(&(pct_x * state.size.width as f32).to_le_bytes());
                combined[4..].copy_from_slice(&(pct_y * state.size.height as f32).to_le_bytes());
                let combined = u64::from_le_bytes(combined);
                mouse_pos.store(combined, Ordering::Relaxed);
            }
            _ => {
                // eprintln!("{event:?}")
            }
        }
    }
}
