// wgpu 渲染上下文：alpha 透明 surface + sprite quad pipeline + dynamic offset uniform pool。
#![allow(non_snake_case)]

use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};
use winit::window::Window;

pub mod spritePipeline;

use crate::asset::{SpriteAsset, SpriteFactory};
use spritePipeline::{SpritePipeline, SpriteUniforms, SPRITE_UNIFORM_SIZE, UNIFORM_SLOT_BYTES};

// 每帧 sprite 上限：桌宠本体肢体(~30) + 头顶状态面板(分段进度条 ~58) + 多行气泡(~50)
// 同屏时远超旧值 64，超出部分会被 renderFrame 截断（表现为最后绘制的口渴行不显示）。
pub const MAX_SPRITES_PER_FRAME: usize = 256;

pub struct SpriteDraw<'a> {
    pub asset: &'a SpriteAsset,
    pub matrix: [[f32; 4]; 4],
    pub color: [f32; 4],
    pub uvRect: [f32; 4],
}

impl<'a> SpriteDraw<'a> {
    pub fn full(asset: &'a SpriteAsset, matrix: [[f32; 4]; 4]) -> Self {
        Self {
            asset,
            matrix,
            color: [1.0, 1.0, 1.0, 1.0],
            uvRect: [0.0, 0.0, 1.0, 1.0],
        }
    }
}

pub struct Renderer {
    pub window: Arc<Window>,
    pub surface: wgpu::Surface<'static>,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub config: wgpu::SurfaceConfiguration,
    pub spritePipeline: SpritePipeline,
    pub uniformBuffer: wgpu::Buffer,
    uniformStaging: Vec<u8>,
}

impl Renderer {
    pub fn new(window: Arc<Window>) -> Result<Self> {
        let size = window.inner_size();
        let backends = pickBackends();
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends,
            ..Default::default()
        });
        let surface = instance
            .create_surface(window.clone())
            .context("create surface")?;
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        }))
        .context("request adapter")?;
        let (device, queue) = pollster::block_on(adapter.request_device(
            &wgpu::DeviceDescriptor {
                label: Some("pet-device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::downlevel_defaults(),
                memory_hints: wgpu::MemoryHints::Performance,
            },
            None,
        ))
        .context("request device")?;

        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .unwrap_or(caps.formats[0]);
        let alpha_mode = pickAlphaMode(&caps.alpha_modes);
        let present_mode = pickPresentMode(&caps.present_modes);

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode,
            alpha_mode,
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        let spritePipeline = SpritePipeline::new(&device, format);
        let uniformBuffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("sprite-uniform-pool"),
            size: MAX_SPRITES_PER_FRAME as u64 * UNIFORM_SLOT_BYTES,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let uniformStaging = vec![0u8; MAX_SPRITES_PER_FRAME * UNIFORM_SLOT_BYTES as usize];

        Ok(Self {
            window,
            surface,
            device,
            queue,
            config,
            spritePipeline,
            uniformBuffer,
            uniformStaging,
        })
    }

    pub fn factory(&self) -> SpriteFactory<'_> {
        SpriteFactory {
            device: &self.device,
            queue: &self.queue,
            bindLayout: &self.spritePipeline.bindLayout,
            uniformBuffer: &self.uniformBuffer,
            sampler: &self.spritePipeline.nearestSampler,
        }
    }

    pub fn loadSpritesFromDir(&self, dir: &Path) -> Result<std::collections::HashMap<String, SpriteAsset>> {
        self.factory().fromDir(dir)
    }

    pub fn createSolidSprite(&self, rgba: [u8; 4]) -> Result<SpriteAsset> {
        self.factory().solid(rgba, "solid")
    }

    pub fn resize(&mut self, w: u32, h: u32) {
        if w == 0 || h == 0 {
            return;
        }
        self.config.width = w;
        self.config.height = h;
        self.surface.configure(&self.device, &self.config);
    }

    pub fn render(&mut self) -> Result<()> {
        self.renderFrame(&[])
    }

    pub fn renderFrame(&mut self, draws: &[SpriteDraw]) -> Result<()> {
        // 新窗口首帧 / resize 后 get_current_texture 常返回 Outdated。
        // 循环重配 + 重试，最多 5 次（避免无限循环），确保至少尝试渲染。
        let frame = {
            let mut f = None;
            for _ in 0..5 {
                match self.surface.get_current_texture() {
                    Ok(frame) => { f = Some(frame); break; }
                    Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                        self.surface.configure(&self.device, &self.config);
                        // 重配后立即重试
                        continue;
                    }
                    Err(e) => return Err(anyhow::anyhow!("acquire frame: {e:?}")),
                }
            }
            f.ok_or_else(|| anyhow::anyhow!("acquire frame: surface keeps returning Outdated"))?
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("pet-encoder"),
            });

        let count = draws.len().min(MAX_SPRITES_PER_FRAME);
        if count > 0 {
            let stride = UNIFORM_SLOT_BYTES as usize;
            let writeLen = count * stride;
            for i in 0..count {
                let d = &draws[i];
                let off = i * stride;
                let u = SpriteUniforms {
                    transform: d.matrix,
                    color: d.color,
                    uvRect: d.uvRect,
                };
                self.uniformStaging[off..off + SPRITE_UNIFORM_SIZE as usize]
                    .copy_from_slice(bytemuck::bytes_of(&u));
            }
            self.queue.write_buffer(&self.uniformBuffer, 0, &self.uniformStaging[..writeLen]);
        }

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("sprite-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            if count > 0 {
                pass.set_pipeline(&self.spritePipeline.pipeline);
                pass.set_vertex_buffer(0, self.spritePipeline.vertexBuffer.slice(..));
                pass.set_index_buffer(
                    self.spritePipeline.indexBuffer.slice(..),
                    wgpu::IndexFormat::Uint16,
                );
                for i in 0..count {
                    let d = &draws[i];
                    let off = (i as u32) * UNIFORM_SLOT_BYTES as u32;
                    pass.set_bind_group(0, &d.asset.bindGroup, &[off]);
                    pass.draw_indexed(0..self.spritePipeline.indexCount, 0, 0..1);
                }
            }
        }
        self.queue.submit(std::iter::once(encoder.finish()));
        frame.present();
        Ok(())
    }
}

fn pickAlphaMode(modes: &[wgpu::CompositeAlphaMode]) -> wgpu::CompositeAlphaMode {
    for &m in modes {
        if matches!(
            m,
            wgpu::CompositeAlphaMode::PreMultiplied | wgpu::CompositeAlphaMode::PostMultiplied
        ) {
            return m;
        }
    }
    modes.first().copied().unwrap_or(wgpu::CompositeAlphaMode::Auto)
}

pub fn pickPresentMode(modes: &[wgpu::PresentMode]) -> wgpu::PresentMode {
    for &m in modes {
        if matches!(m, wgpu::PresentMode::Mailbox) {
            return m;
        }
    }
    wgpu::PresentMode::Fifo
}

/// Windows 优先 Vulkan（透明窗口下 DComposition 路径可靠），DX12/GL fallback；其他平台用 PRIMARY。
fn pickBackends() -> wgpu::Backends {
    if let Ok(v) = std::env::var("PET_WGPU_BACKEND") {
        match v.to_ascii_lowercase().as_str() {
            "dx12" | "d3d12" => return wgpu::Backends::DX12,
            "vulkan" | "vk" => return wgpu::Backends::VULKAN,
            "gl" | "opengl" => return wgpu::Backends::GL,
            "metal" => return wgpu::Backends::METAL,
            _ => {}
        }
    }
    #[cfg(windows)]
    {
        wgpu::Backends::VULKAN | wgpu::Backends::DX12 | wgpu::Backends::GL
    }
    #[cfg(not(windows))]
    {
        wgpu::Backends::PRIMARY
    }
}
