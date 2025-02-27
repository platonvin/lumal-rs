use crate::Renderer;
use crate::{
    ring::Ring, Buffer, DescriptorCounter, Image, LumalSettings, RasterPipe, MAX_FRAMES_IN_FLIGHT,
};
use anyhow::*;
use std::cell::UnsafeCell;
use std::{option, ptr::null};
use vulkanalia::vk::{self, DeviceV1_3};

use vulkanalia::prelude::v1_3::*;

#[derive(PartialEq, Eq, Clone)]
pub enum BlendAttachment {
    NoBlend,
    BlendMix,
    BlendSub,
    BlendReplaceIfGreater, // Basically max
    BlendReplaceIfLess,    // Basically min
}

#[allow(non_camel_case_types)]
#[derive(PartialEq, Clone, Copy)]
pub enum DepthTesting {
    DT_None,
    DT_Read,
    DT_Write,
    DT_ReadWrite,
}

pub enum Discard {
    NoDiscard,
    DoDiscard,
}

pub enum LoadStoreOp {
    DontCare,
    Clear,
    Store,
    Load,
}
impl LoadStoreOp {
    pub(crate) fn to_vk_load(&self) -> vk::AttachmentLoadOp {
        match self {
            LoadStoreOp::DontCare => vk::AttachmentLoadOp::DONT_CARE,
            LoadStoreOp::Clear => vk::AttachmentLoadOp::CLEAR,
            LoadStoreOp::Load => vk::AttachmentLoadOp::LOAD,
            LoadStoreOp::Store => panic!(),
        }
    }
    pub(crate) fn to_vk_store(&self) -> vk::AttachmentStoreOp {
        match self {
            LoadStoreOp::DontCare => vk::AttachmentStoreOp::DONT_CARE,
            LoadStoreOp::Store => vk::AttachmentStoreOp::STORE,
            LoadStoreOp::Clear => panic!(),
            LoadStoreOp::Load => panic!(),
        }
    }
}

impl PartialEq for LoadStoreOp {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (LoadStoreOp::DontCare, LoadStoreOp::DontCare) => true,
            (LoadStoreOp::Clear, LoadStoreOp::Clear) => true,
            (LoadStoreOp::Load, LoadStoreOp::Load) => true,
            (LoadStoreOp::Store, LoadStoreOp::Store) => true,
            _ => false,
        }
    }
}

pub struct AttachmentDescription {
    pub images: *const Ring<Image>,
    pub load: LoadStoreOp,
    pub store: LoadStoreOp,
    pub sload: LoadStoreOp,
    pub sstore: LoadStoreOp,
    pub clear: vk::ClearValue,
    pub final_layout: vk::ImageLayout, // Default value is GENERAL
}

// everything is a a pointer to be able to compare them later
pub struct SubpassDescription<'lt> {
    pub pipes: &'lt mut [&'lt mut RasterPipe],
    pub a_input: &'lt [*const Ring<Image>], // Input images for the subpass
    pub a_color: &'lt [*const Ring<Image>], // Color images for the subpass
    pub a_depth: Option<*const Ring<Image>>, // Depth image for the subpass
}

#[derive(Clone, Default, Debug)]
pub struct SubpassAttachmentRefs {
    pub a_input: Vec<vk::AttachmentReference>,
    pub a_color: Vec<vk::AttachmentReference>,
    // using Option is unconvenient because we need to point'er it afterwards. But still
    pub a_depth: Option<vk::AttachmentReference>,
}

#[derive(Clone, Copy, Debug, Default)]
pub enum RelativeDescriptorPos {
    #[default]
    NotPresented, // What?
    Previous, // Relative Descriptor position previous - for accumulators
    Current,  // Relative Descriptor position matching - common CPU-paired
    First,    // Relative Descriptor position first - for GPU-only
}

#[derive(Clone, Debug)]
pub struct ShaderStage {
    pub src: String,
    pub stage: vk::ShaderStageFlags,
}

#[derive(Clone, Debug)]
pub struct AttrFormOffs {
    pub format: vk::Format,
    pub binding: u32,
    pub offset: usize,
}

#[derive(Clone, Debug, Default)]
pub struct DescriptorInfo {
    pub descriptor_type: vk::DescriptorType,
    pub relative_pos: RelativeDescriptorPos,
    pub buffers: Option<Ring<Buffer>>,
    pub images: Option<Ring<Image>>,
    pub image_sampler: vk::Sampler,
    pub image_layout: vk::ImageLayout, // Image layout for use (not current)
    pub specified_stages: vk::ShaderStageFlags,
}

impl DescriptorInfo {
    pub fn make_new(
        descriptor_type: vk::DescriptorType,
        relative_pos: RelativeDescriptorPos,
        buffers: Option<Ring<Buffer>>,
        images: Option<Ring<Image>>,
        image_sampler: vk::Sampler,
        image_layout: vk::ImageLayout,
        stages: vk::ShaderStageFlags,
    ) -> Self {
        Self {
            descriptor_type,
            relative_pos,
            buffers,
            images,
            image_sampler,
            image_layout,
            specified_stages: stages,
        }
    }
}

pub struct ShortDescriptorInfo {
    pub descriptor_type: vk::DescriptorType,
    pub stages: vk::ShaderStageFlags,
}

impl Renderer {
    /// immediately creates vulkan descriptor set layout
    #[cold]
    #[optimize(size)]
    pub fn create_descriptor_set_layout(
        &mut self,
        descriptor_infos: &[ShortDescriptorInfo],
        layout: &mut vk::DescriptorSetLayout,
        flags: vk::DescriptorSetLayoutCreateFlags,
    ) {
        let bindings: Vec<vk::DescriptorSetLayoutBinding> = descriptor_infos
            .iter()
            .enumerate()
            .map(|(i, info)| {
                macro_rules! make_descriptor_type {
                    ($name:ident) => {
                        self.descriptor_counter.$name += 1
                    };
                }
                match info.descriptor_type {
                    vk::DescriptorType::SAMPLER => make_descriptor_type!(SAMPLER),
                    vk::DescriptorType::COMBINED_IMAGE_SAMPLER => {
                        make_descriptor_type!(COMBINED_IMAGE_SAMPLER)
                    }
                    vk::DescriptorType::SAMPLED_IMAGE => make_descriptor_type!(SAMPLED_IMAGE),
                    vk::DescriptorType::STORAGE_IMAGE => make_descriptor_type!(STORAGE_IMAGE),
                    vk::DescriptorType::UNIFORM_TEXEL_BUFFER => {
                        make_descriptor_type!(UNIFORM_TEXEL_BUFFER)
                    }
                    vk::DescriptorType::STORAGE_TEXEL_BUFFER => {
                        make_descriptor_type!(STORAGE_TEXEL_BUFFER)
                    }
                    vk::DescriptorType::UNIFORM_BUFFER => make_descriptor_type!(UNIFORM_BUFFER),
                    vk::DescriptorType::STORAGE_BUFFER => make_descriptor_type!(STORAGE_BUFFER),
                    vk::DescriptorType::UNIFORM_BUFFER_DYNAMIC => {
                        make_descriptor_type!(UNIFORM_BUFFER_DYNAMIC)
                    }
                    vk::DescriptorType::STORAGE_BUFFER_DYNAMIC => {
                        make_descriptor_type!(STORAGE_BUFFER_DYNAMIC)
                    }
                    vk::DescriptorType::INPUT_ATTACHMENT => make_descriptor_type!(INPUT_ATTACHMENT),
                    _ => {
                        panic!("Unknown descriptor type");
                    }
                }

                vk::DescriptorSetLayoutBinding {
                    binding: i as u32,
                    descriptor_type: info.descriptor_type,
                    descriptor_count: 1,
                    stage_flags: info.stages,
                    ..Default::default()
                }
            })
            .collect();

        let layout_info = vk::DescriptorSetLayoutCreateInfo {
            s_type: vk::StructureType::DESCRIPTOR_SET_LAYOUT_CREATE_INFO,
            flags,
            binding_count: bindings.len() as u32,
            bindings: bindings.as_ptr(),
            ..Default::default()
        };

        // actually create layout and write it to ref
        *layout = unsafe {
            self.device
                .create_descriptor_set_layout(&layout_info, None)
                .expect("Failed to create descriptor set layout")
        };
    }

    #[cold]
    #[optimize(size)]
    pub unsafe fn create_descriptor_pool(&self) -> Result<vk::DescriptorPool> {
        let mut pool_sizes = Vec::new();

        macro_rules! make_descriptor_type {
            ($name:ident) => {
                if self.descriptor_counter.$name != 0 {
                    pool_sizes.push(vk::DescriptorPoolSize {
                        type_: vk::DescriptorType::$name,
                        descriptor_count: self.descriptor_counter.$name,
                    });
                }
            };
        }
        make_descriptor_type!(SAMPLER);
        make_descriptor_type!(COMBINED_IMAGE_SAMPLER);
        make_descriptor_type!(SAMPLED_IMAGE);
        make_descriptor_type!(STORAGE_IMAGE);
        make_descriptor_type!(UNIFORM_TEXEL_BUFFER);
        make_descriptor_type!(STORAGE_TEXEL_BUFFER);
        make_descriptor_type!(UNIFORM_BUFFER);
        make_descriptor_type!(STORAGE_BUFFER);
        make_descriptor_type!(UNIFORM_BUFFER_DYNAMIC);
        make_descriptor_type!(STORAGE_BUFFER_DYNAMIC);
        make_descriptor_type!(INPUT_ATTACHMENT);

        let pool_info = vk::DescriptorPoolCreateInfo {
            s_type: vk::StructureType::DESCRIPTOR_POOL_CREATE_INFO,
            pool_size_count: pool_sizes.len() as u32,
            pool_sizes: pool_sizes.as_ptr(),
            max_sets: self.descriptor_sets_count * self.settings.fif as u32,
            ..Default::default()
        };

        Ok(self.device.create_descriptor_pool(&pool_info, None)?)
    }

    #[cold]
    #[optimize(size)]
    pub unsafe fn allocate_descriptor(
        device: Device,
        layout: vk::DescriptorSetLayout,
        pool: vk::DescriptorPool,
        count: usize,
    ) -> Ring<vk::DescriptorSet> {
        let layouts = vec![layout; count];
        let alloc_info = vk::DescriptorSetAllocateInfo {
            s_type: vk::StructureType::DESCRIPTOR_SET_ALLOCATE_INFO,
            descriptor_pool: pool,
            descriptor_set_count: layouts.len() as u32,
            set_layouts: layouts.as_ptr(),
            ..Default::default()
        };

        let mut ring = Ring::new(count, vk::DescriptorSet::null());
        // return
        let vec = device
            .allocate_descriptor_sets(&alloc_info)
            .expect("Failed to allocate descriptor sets");
        for (i, v) in vec.iter().enumerate() {
            ring[i] = *v;
        }
        ring
    }

    // Tell the LumalRenderer that such descriptor will be setup
    // basically counts needed resources to then allocate them
    #[cold]
    #[optimize(size)]
    pub fn anounce_descriptor_setup(
        &mut self,
        dset_layout: &mut vk::DescriptorSetLayout,
        descriptor_sets: &mut Ring<vk::DescriptorSet>, // Ring to setup into (some setup happens immediately on anounce)
        descriptions: &[DescriptorInfo],
        default_stages: vk::ShaderStageFlags,
        create_flags: vk::DescriptorSetLayoutCreateFlags,
    ) {
        if *dset_layout == vk::DescriptorSetLayout::null() {
            let descriptor_infos: Vec<ShortDescriptorInfo> = descriptions
                .iter()
                .map(|desc| ShortDescriptorInfo {
                    descriptor_type: desc.descriptor_type,
                    stages: if desc.specified_stages.is_empty() {
                        default_stages
                    } else {
                        desc.specified_stages
                    },
                })
                .collect();
            unsafe {
                // actually create layout and write it to ptr
                self.create_descriptor_set_layout(&descriptor_infos, dset_layout, create_flags);
            }
        }

        self.descriptor_sets_count += (MAX_FRAMES_IN_FLIGHT as u32); // cuase dset per fif
    }
}

impl Renderer {
    // anounce is just a request, this is an actual logic
    #[cold]
    #[optimize(size)]
    pub unsafe fn actually_setup_descriptor_impl(
        descriptor_pool: &vk::DescriptorPool,
        settings: &LumalSettings,
        device: &Device,
        dset_layout: &vk::DescriptorSetLayout,
        descriptor_sets: &mut Ring<vk::DescriptorSet>,
        descriptions: &[DescriptorInfo],
        stages: vk::ShaderStageFlags,
    ) {
        *descriptor_sets = Ring::new(MAX_FRAMES_IN_FLIGHT, vk::DescriptorSet::null());
        let dset_layouts = [*dset_layout; MAX_FRAMES_IN_FLIGHT];
        for frame_i in 0..MAX_FRAMES_IN_FLIGHT {
            descriptor_sets[frame_i] = device
                .allocate_descriptor_sets(&vk::DescriptorSetAllocateInfo {
                    s_type: vk::StructureType::DESCRIPTOR_SET_ALLOCATE_INFO,
                    descriptor_pool: *descriptor_pool,
                    descriptor_set_count: MAX_FRAMES_IN_FLIGHT as u32,
                    set_layouts: dset_layouts.as_ptr(),
                    next: null(),
                })
                .unwrap()[0];
        }
        assert!(descriptor_sets.len() == MAX_FRAMES_IN_FLIGHT);
        for frame_i in 0..descriptor_sets.len() {
            let previous_frame_i = if frame_i == 0 {
                settings.fif - 1
            } else {
                frame_i - 1
            };

            let mut image_infos = vec![vk::DescriptorImageInfo::default(); descriptions.len()];
            let mut buffer_infos = vec![vk::DescriptorBufferInfo::default(); descriptions.len()];
            let mut writes = vec![vk::WriteDescriptorSet::default(); descriptions.len()];

            for (i, desc) in descriptions.iter().enumerate() {
                writes[i] = vk::WriteDescriptorSet {
                    s_type: vk::StructureType::WRITE_DESCRIPTOR_SET,
                    dst_set: descriptor_sets[frame_i],
                    dst_binding: i as u32,
                    dst_array_element: 0,
                    descriptor_count: 1,
                    descriptor_type: desc.descriptor_type,
                    ..Default::default()
                };

                let descriptor_frame_id = match desc.relative_pos {
                    RelativeDescriptorPos::Current => frame_i,
                    RelativeDescriptorPos::Previous => previous_frame_i,
                    RelativeDescriptorPos::First => 0,
                    RelativeDescriptorPos::NotPresented => {
                        writes[i].descriptor_count = 0;
                        continue;
                    }
                };

                if let Some(images) = &desc.images {
                    assert!(images[descriptor_frame_id].view != vk::ImageView::null());
                    image_infos[i] = vk::DescriptorImageInfo {
                        image_view: images[descriptor_frame_id].view,
                        image_layout: desc.image_layout,
                        sampler: desc.image_sampler,
                    };
                    writes[i].image_info = &image_infos[i];

                    assert!(desc.buffers.is_none());
                    if desc.image_sampler != vk::Sampler::null()
                        && desc.descriptor_type != vk::DescriptorType::COMBINED_IMAGE_SAMPLER
                    {
                        panic!("Descriptor has sampler but type is not for sampler");
                    }
                } else if let Some(buffers) = &desc.buffers {
                    buffer_infos[i] = vk::DescriptorBufferInfo {
                        buffer: buffers[descriptor_frame_id].buffer,
                        offset: 0,
                        range: vk::WHOLE_SIZE as u64,
                    };
                    writes[i].buffer_info = &buffer_infos[i];
                } else {
                    panic!("Unknown descriptor type");
                }
            }

            device.update_descriptor_sets(&writes, &[] as &[vk::CopyDescriptorSet]);
        }
    }

    #[cold]
    #[optimize(size)]
    pub fn flush_descriptor_setup(&mut self) {
        // (actually) create Vulkan descriptor pool
        self.vulkan_data.descriptor_pool = unsafe { self.create_descriptor_pool() }.unwrap();
    }

    #[cold]
    #[optimize(size)]
    pub fn acutally_setup_descriptor(
        &mut self,
        dset_layout: &mut vk::DescriptorSetLayout,
        descriptor_sets: &mut Ring<vk::DescriptorSet>, // Ring to setup into
        descriptions: &[DescriptorInfo],
        default_stages: vk::ShaderStageFlags,
        create_flags: vk::DescriptorSetLayoutCreateFlags,
    ) {
        // actually setup descriptor
        unsafe {
            Self::actually_setup_descriptor_impl(
                &self.vulkan_data.descriptor_pool,
                &self.settings,
                &self.device,
                &mut *dset_layout,
                descriptor_sets,
                descriptions,
                default_stages,
            );
        }
    }
}
