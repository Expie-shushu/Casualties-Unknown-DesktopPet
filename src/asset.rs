// 资源加载：PNG bytes → wgpu Texture → SpriteAsset (含预创建 BindGroup 共享 uniform buffer)。
#![allow(non_snake_case)]

use std::collections::HashMap;
use std::path::Path;

use anyhow::{anyhow, Context, Result};

use crate::renderer::spritePipeline::SPRITE_UNIFORM_SIZE;

pub struct SpriteAsset {
    pub view: wgpu::TextureView,
    pub bindGroup: wgpu::BindGroup,
    pub width: u32,
    pub height: u32,
}

pub struct SpriteFactory<'a> {
    pub device: &'a wgpu::Device,
    pub queue: &'a wgpu::Queue,
    pub bindLayout: &'a wgpu::BindGroupLayout,
    pub uniformBuffer: &'a wgpu::Buffer,
    pub sampler: &'a wgpu::Sampler,
}

impl<'a> SpriteFactory<'a> {
    pub fn fromPng(&self, bytes: &[u8], label: &str) -> Result<SpriteAsset> {
        let img = image::load_from_memory(bytes)
            .with_context(|| format!("decode png {label}"))?
            .to_rgba8();
        let img = premultiply_rgba(&img);
        let (w, h) = (img.width(), img.height());
        if w == 0 || h == 0 {
            return Err(anyhow!("sprite {label} has zero dimension"));
        }
        self.fromRgba(&img, w, h, label)
    }

    pub fn fromRgba(&self, rgba: &[u8], w: u32, h: u32, label: &str) -> Result<SpriteAsset> {
        if w == 0 || h == 0 {
            return Err(anyhow!("sprite {label} has zero dimension"));
        }
        let size = wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        };
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        self.queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            rgba,
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(4 * w),
                rows_per_image: Some(h),
            },
            size,
        );
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bindGroup = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(label),
            layout: self.bindLayout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: self.uniformBuffer,
                        offset: 0,
                        size: std::num::NonZeroU64::new(SPRITE_UNIFORM_SIZE),
                    }),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(self.sampler),
                },
            ],
        });
        Ok(SpriteAsset {
            view,
            bindGroup,
            width: w,
            height: h,
        })
    }

    pub fn solid(&self, rgba: [u8; 4], label: &str) -> Result<SpriteAsset> {
        self.fromRgba(&rgba, 1, 1, label)
    }

    pub fn fromDir(&self, dir: &Path) -> Result<HashMap<String, SpriteAsset>> {
        if !dir.exists() {
            return Err(anyhow!("dir not found: {}", dir.display()));
        }
        let mut out = HashMap::new();
        walkAndLoad(self, dir, &mut out)?;
        Ok(out)
    }
}

fn premultiply_rgba(img: &image::RgbaImage) -> image::RgbaImage{
    let mut out = image::RgbaImage::new(img.width(), img.height());
    for (x, y, pixel) in img.enumerate_pixels() {
        let a = pixel[3] as f32 / 255.0;
        out.put_pixel(
            x,
            y,
            image::Rgba([
                (pixel[0] as f32 * a) as u8,
                (pixel[1] as f32 * a) as u8,
                (pixel[2] as f32 * a) as u8,
                pixel[3],
            ]),
        );
    }
    out
}

fn walkAndLoad(
    factory: &SpriteFactory<'_>,
    dir: &Path,
    out: &mut HashMap<String, SpriteAsset>,
) -> Result<()> {
    for entry in std::fs::read_dir(dir)?.flatten() { // 遍历目录下所有的目录
        let p = entry.path();
        if p.is_dir() {
            walkAndLoad(factory, &p, out)?;   // 递归调用子文件夹
            continue;
        }
        let ext = p.extension().and_then(|e| e.to_str()).unwrap_or(""); // 取文件扩展名
        if !ext.eq_ignore_ascii_case("png") {
            continue;
        }
        let stem = match p.file_stem().and_then(|s| s.to_str()) {
            Some(s) => s.to_string(),
            None => continue,
        };
        if out.contains_key(&stem) {
            continue;
        }
        let bytes = std::fs::read(&p)?;
        let asset = factory.fromPng(&bytes, &stem)?;
        out.insert(stem, asset);  // 以文件名stem作为key存入到hashmap里
    }
    Ok(())
}
