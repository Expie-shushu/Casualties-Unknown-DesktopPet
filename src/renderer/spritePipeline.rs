// sprite 渲染管线：单位 quad + uniform(transform + color + uvRect) dynamic offset + 1 张纹理 + 1 个 sampler。
#![allow(non_snake_case)]

use std::num::NonZeroU64;

use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
pub struct SpriteVertex {
    pub position: [f32; 2],
    pub uv: [f32; 2],
}

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable, Debug)]
pub struct SpriteUniforms {
    pub transform: [[f32; 4]; 4],
    pub color: [f32; 4],
    pub uvRect: [f32; 4],
}

pub const UNIFORM_SLOT_BYTES: u64 = 256;
pub const SPRITE_UNIFORM_SIZE: u64 = std::mem::size_of::<SpriteUniforms>() as u64;

const QUAD_VERTICES: &[SpriteVertex] = &[
    SpriteVertex { position: [-0.5, -0.5], uv: [0.0, 1.0] },
    SpriteVertex { position: [ 0.5, -0.5], uv: [1.0, 1.0] },
    SpriteVertex { position: [ 0.5,  0.5], uv: [1.0, 0.0] },
    SpriteVertex { position: [-0.5,  0.5], uv: [0.0, 0.0] },
];
const QUAD_INDICES: &[u16] = &[0, 1, 2, 0, 2, 3];

pub struct SpritePipeline {
    pub pipeline: wgpu::RenderPipeline,
    pub bindLayout: wgpu::BindGroupLayout,
    pub vertexBuffer: wgpu::Buffer,
    pub indexBuffer: wgpu::Buffer,
    pub indexCount: u32,
    pub linearSampler: wgpu::Sampler,
    pub nearestSampler: wgpu::Sampler,
}

impl SpritePipeline {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("sprite-shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("sprite.wgsl").into()),
        });

        let bindLayout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("sprite-bind"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: true,
                        min_binding_size: NonZeroU64::new(SPRITE_UNIFORM_SIZE),
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let pipelineLayout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("sprite-pipeline-layout"),
            bind_group_layouts: &[&bindLayout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("sprite-pipeline"),
            layout: Some(&pipelineLayout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<SpriteVertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2],
                }],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                unclipped_depth: false,
                polygon_mode: wgpu::PolygonMode::Fill,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let vertexBuffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("sprite-vertex"),
            contents: bytemuck::cast_slice(QUAD_VERTICES),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let indexBuffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("sprite-index"),
            contents: bytemuck::cast_slice(QUAD_INDICES),
            usage: wgpu::BufferUsages::INDEX,
        });

        let linearSampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("sprite-sampler-linear"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let nearestSampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("sprite-sampler-nearest"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        Self {
            pipeline,
            bindLayout,
            vertexBuffer,
            indexBuffer,
            indexCount: QUAD_INDICES.len() as u32,
            linearSampler,
            nearestSampler,
        }
    }
}

/// 二维仿射 → 4×4 NDC 变换矩阵。screen 像素 (x_px,y_px) + size + Unity rotZ + pivot + 屏幕宽高。
/// rotDeg 即 Unity transform.localEulerAngles.z（y-up CCW 正），编辑器 imageOps::blitWithRotation 同款约定。
pub fn buildSpriteMatrix(
    screenW: f32,
    screenH: f32,
    centerXpx: f32,
    centerYpx: f32,
    sizeWpx: f32,
    sizeHpx: f32,
    rotDeg: f32,
    flipX: f32,
) -> [[f32; 4]; 4] {
    let cos = (rotDeg.to_radians()).cos();
    let sin = (rotDeg.to_radians()).sin();
    let sx = sizeWpx * flipX;
    let sy = sizeHpx;
    let nx = 2.0 / screenW;
    let ny = 2.0 / screenH;
    let tx = centerXpx;
    let ty = centerYpx;
    [
        [ cos * sx * nx,  sin * sx * ny, 0.0, 0.0],
        [-sin * sy * nx,  cos * sy * ny, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [tx * nx - 1.0, 1.0 - ty * ny, 0.0, 1.0],
    ]
}
