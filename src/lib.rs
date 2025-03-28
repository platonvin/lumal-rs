#![allow(dead_code, unused)]
#![allow(unused_parens)]
#![feature(optimize_attribute)]

// lumal is divided into files (aka modules)
// this in needed for whole thing to compile
// Rust is so good that figuring it out only took 1 hour
pub mod barriers;
pub mod blit_copy;
pub mod buffers;
pub mod descriptors;
pub mod images;
pub mod macros;
pub mod pipes;
pub mod renderer;
pub mod ring; // circular Vec
pub mod rpass;
pub mod samplers;

use anyhow::{anyhow, Ok, Result};
use ring::*;

use std::process::exit;
use std::{any::type_name, os::raw::c_void};
use std::{
    any::Any,
    mem::{size_of, size_of_val},
};
use std::{any::TypeId, ffi::CStr};
use std::{collections::HashSet, default};
use vulkanalia::{
    bytecode::Bytecode,
    loader::{LibloadingLoader, LIBRARY},
    vk::{
        DebugMarkerObjectNameInfoEXT, DebugReportObjectTypeEXT, DebugUtilsObjectNameInfoEXT,
        DescriptorSet, DescriptorSetLayout, ExtDebugMarkerExtension, ImageView,
        PFN_vkSetDebugUtilsObjectNameEXT, Pipeline,
    },
};
use vulkanalia::{prelude::v1_3::*, vk::Extent3D};
use vulkanalia::{vk::ImageAspectFlags, Version};
use vulkanalia_vma::{self as vma};
use winit::window::Window;
use winit::{
    application::ApplicationHandler,
    dpi::LogicalSize,
    event::{DeviceEvent, DeviceId, WindowEvent},
    event_loop::{ActiveEventLoop, EventLoop},
    window::{WindowAttributes, WindowId},
};
use Option as optional;

use vk::{InputChainStruct, KhrSurfaceExtension, KhrSwapchainExtension};

/// The required instance and device layer if validation is enabled.
const VALIDATION_LAYER: vk::ExtensionName =
    vk::ExtensionName::from_bytes(b"VK_LAYER_KHRONOS_validation");
const LUNARG_MONITOR_LAYER: vk::ExtensionName =
    vk::ExtensionName::from_bytes(b"VK_LAYER_LUNARG_monitor");
/// The required device extensions.
const DEVICE_EXTENSIONS: &[vk::ExtensionName] = &[
    vk::KHR_SWAPCHAIN_EXTENSION.name,
    vk::EXT_HOST_QUERY_RESET_EXTENSION.name,
    vk::KHR_PUSH_DESCRIPTOR_EXTENSION.name,
    // vk::
];

/// Vulkan SDK version that started requiring the portability subset extension for macOS.
const PORTABILITY_MACOS_VERSION: Version = Version::new(1, 3, 216);

/// number of frames that will be processed concurrently. 2 is perferct - CPU prepares frame N, GPU renders frame N-1
const MAX_FRAMES_IN_FLIGHT: usize = 2;

#[derive(Clone, Debug)]
pub struct Buffer {
    pub buffer: vk::Buffer,
    pub allocation: vma::Allocation,
    pub mapped: Option<*mut u8>, // If allocation is mapped
}
impl Default for Buffer {
    fn default() -> Self {
        Self {
            buffer: Default::default(),
            allocation: unsafe { std::mem::zeroed() },
            mapped: Default::default(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Image {
    pub image: vk::Image,
    pub allocation: vma::Allocation,
    pub view: vk::ImageView,           // Main view
    pub mip_views: Vec<vk::ImageView>, // Vec for mip views
    pub format: vk::Format,
    pub aspect: vk::ImageAspectFlags,
    pub extent: vk::Extent3D,
    pub mip_levels: u32,
}

impl Default for Image {
    fn default() -> Self {
        Self {
            image: Default::default(),
            allocation: unsafe { std::mem::zeroed() },
            view: Default::default(),
            mip_views: Default::default(),
            format: Default::default(),
            aspect: Default::default(),
            extent: Default::default(),
            mip_levels: Default::default(),
        }
    }
}

// #[derive(Default)]
#[derive(Clone, Debug)]
pub struct RasterPipe {
    pub line: vk::Pipeline,
    pub line_layout: vk::PipelineLayout,
    // WHERE IS MY FUCKING DEFAULT VALUE WHY NO ONE WRITES BINDINGS THAT JUST WORK
    pub sets: Ring<vk::DescriptorSet>,
    pub set_layout: vk::DescriptorSetLayout,
    pub render_pass: vk::RenderPass, // We don't need to store it in here but why not
    pub subpass_id: i32,
}
impl RasterPipe {
    pub fn as_mut_ptr(&self) -> *mut RasterPipe {
        self as *const RasterPipe as *mut RasterPipe
    }

    // fn as_mut(&self) -> &mut RasterPipe {
    //     unsafe { &mut *self.as_mut_ptr() }
    // }
}
impl Default for RasterPipe {
    fn default() -> Self {
        Self {
            sets: Default::default(),
            line: Default::default(),
            line_layout: Default::default(),
            set_layout: Default::default(),
            render_pass: Default::default(),
            subpass_id: Default::default(),
        }
    }
}

// Structure for ComputePipe (Compute pipeline)
// #[derive(Default)]
#[derive(Clone)]
pub struct ComputePipe {
    pub line: vk::Pipeline,
    pub line_layout: vk::PipelineLayout,
    pub sets: Ring<vk::DescriptorSet>,
    pub set_layout: vk::DescriptorSetLayout,
}
impl Default for ComputePipe {
    fn default() -> Self {
        Self {
            line: Default::default(),
            line_layout: Default::default(),
            sets: Default::default(),
            set_layout: Default::default(),
        }
    }
}

// Structure for RenderPass
#[derive(Clone, Debug)]
pub struct RenderPass {
    pub clear_colors: Vec<vk::ClearValue>,   // Colors to clear
    pub framebuffers: Ring<vk::Framebuffer>, // Framebuffers for the pass
    pub extent: vk::Extent2D,                // Extent of the render pass
    pub render_pass: vk::RenderPass,         // The actual RenderPass object
}

impl Default for RenderPass {
    fn default() -> Self {
        Self {
            clear_colors: Default::default(),
            framebuffers: Default::default(),
            extent: Default::default(),
            render_pass: Default::default(),
        }
    }
}

// Structure for Window
// #[derive(Clone, Default)]
pub struct LumalWindow {
    // pub pointer: *mut glfw::ffi::GLFWwindow, // GLFW window pointer
    pub pointer: *mut winit::window::Window,
    pub width: i32,
    pub height: i32,
}

// Structure for QueueFamilyIndices
pub struct LumalQueueFamilyIndices {
    pub graphical_and_compute: optional<u32>,
    pub present: optional<u32>,
}

impl LumalQueueFamilyIndices {
    pub fn is_complete(&self) -> bool {
        self.graphical_and_compute.is_some() && self.present.is_some()
    }
}

// Structure for SwapChainSupportDetails
pub struct SwapChainSupportDetails {
    pub capabilities: vk::SurfaceCapabilitiesKHR,
    pub formats: Vec<vk::SurfaceFormatKHR>,
    pub present_modes: Vec<vk::PresentModeKHR>,
}

impl SwapChainSupportDetails {
    pub fn is_suitable(&self) -> bool {
        !self.formats.is_empty() && !self.present_modes.is_empty()
    }
}

// Structure for Settings
#[derive(Clone, Copy, Debug, Default)]
pub struct LumalSettings {
    pub timestamp_count: i32,
    pub fif: usize,
    pub vsync: bool,
    pub fullscreen: bool,
    pub debug: bool,
    pub profile: bool,
    pub device_features: vk::PhysicalDeviceFeatures,
    pub device_features11: vk::PhysicalDeviceVulkan11Features,
    pub device_features12: vk::PhysicalDeviceVulkan12Features,
    pub physical_features2: vk::PhysicalDeviceFeatures2,
    // pub instance_layers: Vec<*const i8>,
    // pub instance_extensions: Vec<*const i8>,
    // pub device_extensions: Vec<*const i8>,
}
impl LumalSettings {
    pub fn create_default() -> LumalSettings {
        LumalSettings {
            timestamp_count: 0,
            fif: MAX_FRAMES_IN_FLIGHT,
            vsync: true,
            fullscreen: false,
            debug: false,
            profile: false,
            device_features: vk::PhysicalDeviceFeatures::default(),
            device_features11: vk::PhysicalDeviceVulkan11Features::default(),
            device_features12: vk::PhysicalDeviceVulkan12Features::default(),
            physical_features2: vk::PhysicalDeviceFeatures2::default(),
            // instance_layers: vec![],
            // instance_extensions: vec![],
            // device_extensions: vec![],
        }
    }
}

#[allow(non_snake_case)]
#[derive(Debug, Clone, Copy, Default)]
pub struct DescriptorCounter {
    pub COMBINED_IMAGE_SAMPLER: u32,
    pub INPUT_ATTACHMENT: u32,
    pub SAMPLED_IMAGE: u32,
    pub SAMPLER: u32,
    pub STORAGE_BUFFER: u32,
    pub STORAGE_BUFFER_DYNAMIC: u32,
    pub STORAGE_IMAGE: u32,
    pub STORAGE_TEXEL_BUFFER: u32,
    pub UNIFORM_BUFFER: u32,
    pub UNIFORM_BUFFER_DYNAMIC: u32,
    pub UNIFORM_TEXEL_BUFFER: u32,
}

// TODO: not copy? or Copy image?
#[derive(Default, Clone, Debug)]
pub struct ImageDeletion {
    pub image: Image,
    pub lifetime: i32,
}

// TODO: not copy? or Copy buffer
#[derive(Default, Clone, Debug)]
pub struct BufferDeletion {
    pub buffer: Buffer,
    pub lifetime: i32,
}

#[derive(Debug)]
pub struct Renderer {
    // pub custom_data: Option<T>,
    pub allocator: Option<vma::Allocator>,

    pub settings: LumalSettings,
    pub vulkan_data: VulkanData, // ok example from vulkanalia is good
    // pub event_loop: Option<EventLoop<MyUserEvent>>,
    // pub window: Window, // winit window
    pub entry: Entry,       // internal Vulkanalia entry point
    pub instance: Instance, // wrapper around vk::Instance. TODO: custom vulkan al wrapper (barebone)
    pub device: Device,     // wrapper around vk::Device. TODO: custom vulkan al wrapper (barebone)
    pub frame: i32,         // global counter of rendered frame, mostly for internal use
    pub image_index: u32,
    pub should_recreate: bool,
    pub descriptor_counter: DescriptorCounter,
    pub descriptor_sets_count: u32,

    pub main_command_buffers: Ring<vk::CommandBuffer>, // yep, copied
    pub extra_command_buffers: Ring<vk::CommandBuffer>, // yep, copied

    // Queues of defered (delayed) GPU-side deletion of resources
    // this is not immediate because there are some frames in flight
    // that might still be using resources
    pub buffer_deletion_queue: Vec<BufferDeletion>,
    pub image_deletion_queue: Vec<ImageDeletion>,
}

impl Renderer {
    #[cold]
    #[optimize(size)]
    pub fn create(settings: &LumalSettings, window: &Window) -> Result<Renderer> {
        println!("Starting app.");

        let mut vulkan_data = VulkanData {
            validation: settings.debug,
            ..Default::default()
        };

        if vulkan_data.validation {
            println!("Validation layers requested.");
        }
        unsafe {
            let loader = LibloadingLoader::new(LIBRARY)?;
            let entry = Entry::new(loader).map_err(|b| anyhow!("{}", b))?;
            let instance = Renderer::create_instance(window, &entry, &mut vulkan_data)?;
            vulkan_data.surface = vulkanalia::window::create_surface(&instance, &window, &window)?;
            pick_physical_device(&instance, &mut vulkan_data)?;
            let device = create_logical_device(&entry, &instance, &mut vulkan_data)?;

            let allocator_options =
                vma::AllocatorOptions::new(&instance, &device, vulkan_data.physical_device);
            let allocator = vma::Allocator::new(&allocator_options)?;

            create_swapchain(window, &instance, &device, &mut vulkan_data)?;
            // create_swapchain_image_views(&device, &mut vulkan_data)?;
            // these are handled by downstream user. Makes no sense to hardcode pipes in renderer
            // example.create_render_pass(&device, &mut data)?;
            // example.create_pipeline(&device, &mut data)?;
            // create_framebuffers(&device, &mut data)?;
            // create_command_buffers(&device, &mut vulkan_data)?;
            create_command_pool(&instance, &device, &mut vulkan_data)?;
            create_sync_objects(&device, &mut vulkan_data)?;

            Ok(Renderer {
                allocator: Some(allocator),
                vulkan_data,
                entry,
                instance,
                device,
                frame: 0,
                should_recreate: false,
                settings: *settings,
                descriptor_counter: DescriptorCounter::default(),
                descriptor_sets_count: 0,
                image_index: 0, // cause just init'ed, no descriptor setup deferred yet
                // delayed_descriptor_setups: vec![],
                main_command_buffers: Default::default(),
                extra_command_buffers: Default::default(),
                buffer_deletion_queue: vec![],
                image_deletion_queue: vec![],
            })
        }
    }

    #[cold]
    #[optimize(size)]
    pub unsafe fn create_instance(
        window: &Window,
        entry: &Entry,
        data: &mut VulkanData,
    ) -> Result<Instance> {
        // Application Info

        let application_info = vk::ApplicationInfo::builder()
            .application_name(b"renderer_vk\0")
            .application_version(vk::make_version(1, 3, 0))
            .engine_name(b"No Engine\0")
            .engine_version(vk::make_version(1, 3, 0))
            .api_version(vk::make_version(1, 3, 0));

        // Layers

        let available_layers = entry
            .enumerate_instance_layer_properties()?
            .iter()
            .map(|l| l.layer_name)
            .collect::<HashSet<_>>();

        if data.validation && !available_layers.contains(&VALIDATION_LAYER) {
            return Err(anyhow!("Validation layers requested but not supported."));
        }

        let mut layers = if (data.validation) {
            vec![VALIDATION_LAYER.as_ptr()]
        } else {
            Vec::new()
        };

        if available_layers.contains(&LUNARG_MONITOR_LAYER) {
            layers.push(LUNARG_MONITOR_LAYER.as_ptr());
        }

        // Extensions

        let mut extensions = vulkanalia::window::get_required_instance_extensions(window)
            .iter()
            .map(|e| e.as_ptr())
            .collect::<Vec<_>>();

        // Required by Vulkan SDK on macOS since 1.3.216.
        let flags = if cfg!(target_os = "macos") && entry.version()? >= PORTABILITY_MACOS_VERSION {
            println!("Enabling extensions for macOS portability.");
            extensions.push(vk::KHR_GET_PHYSICAL_DEVICE_PROPERTIES2_EXTENSION.name.as_ptr());
            extensions.push(vk::KHR_PORTABILITY_ENUMERATION_EXTENSION.name.as_ptr());
            vk::InstanceCreateFlags::ENUMERATE_PORTABILITY_KHR
        } else {
            vk::InstanceCreateFlags::empty()
        };

        if data.validation {
            extensions.push(vk::EXT_DEBUG_UTILS_EXTENSION.name.as_ptr());
        }

        // Create

        let mut info = vk::InstanceCreateInfo::builder()
            .application_info(&application_info)
            .enabled_layer_names(&layers)
            .enabled_extension_names(&extensions)
            .flags(flags);

        let mut debug_info = vk::DebugUtilsMessengerCreateInfoEXT::builder()
            .message_severity(vk::DebugUtilsMessageSeverityFlagsEXT::all())
            .message_type(
                vk::DebugUtilsMessageTypeFlagsEXT::GENERAL
                    | vk::DebugUtilsMessageTypeFlagsEXT::VALIDATION
                    | vk::DebugUtilsMessageTypeFlagsEXT::PERFORMANCE,
            )
            .user_callback(Some(debug_callback));

        if data.validation {
            info = info.push_next(&mut debug_info);
        }

        Ok(entry.create_instance(&info, None)?)
    }
    /// buffers, images, pipelines - everything created manually should be destroyed manually before this funcall
    pub unsafe fn destroy(&mut self) {
        self.process_deletion_queues_untill_all_done();
        self.device.destroy_descriptor_pool(self.vulkan_data.descriptor_pool, None);
        self.device.destroy_command_pool(self.vulkan_data.command_pool, None);
        self.destroy_swapchain();
        self.destroy_sync_primitives();
        // cause author of vulkanalia decided to hide it behind the drop. WHY
        if let Some(allocator) = self.allocator.take() {
            std::mem::drop(allocator);
        }
        self.device.destroy_device(None);
        self.instance.destroy_surface_khr(self.vulkan_data.surface, None);
        self.instance.destroy_instance(None);
    }

    #[cold]
    #[optimize(size)]
    unsafe fn destroy_swapchain(&self) {
        self.vulkan_data
            .swapchain_images
            .iter()
            .for_each(|v| self.device.destroy_image_view(v.view, None));
        self.device.destroy_swapchain_khr(self.vulkan_data.swapchain, None);
    }

    #[cold]
    #[optimize(size)]
    unsafe fn destroy_sync_primitives(&self) {
        self.vulkan_data
            .in_flight_fences
            .iter()
            .for_each(|f| self.device.destroy_fence(*f, None));
        self.vulkan_data
            .render_finished_semaphores
            .iter()
            .for_each(|s| self.device.destroy_semaphore(*s, None));
        self.vulkan_data
            .image_available_semaphores
            .iter()
            .for_each(|s| self.device.destroy_semaphore(*s, None));
    }

    #[cold]
    #[optimize(size)]
    pub fn begin_single_time_command_buffer(&self) -> vk::CommandBuffer {
        let alloc_info = vk::CommandBufferAllocateInfo {
            s_type: vk::StructureType::COMMAND_BUFFER_ALLOCATE_INFO,
            level: vk::CommandBufferLevel::PRIMARY,
            command_pool: self.vulkan_data.command_pool,
            command_buffer_count: 1,
            next: std::ptr::null(),
        };
        let command_buffers = unsafe { self.device.allocate_command_buffers(&alloc_info).unwrap() };
        let command_buffer = command_buffers[0];
        unsafe {
            self.device
                .begin_command_buffer(command_buffer, &vk::CommandBufferBeginInfo::default())
                .unwrap();
        }
        command_buffer
    }

    #[cold]
    #[optimize(size)]
    pub fn end_single_time_command_buffer(&self, command_buffer: vk::CommandBuffer) {
        unsafe {
            self.device.end_command_buffer(command_buffer).unwrap();
        }
        let submit_info = vk::SubmitInfo {
            s_type: vk::StructureType::SUBMIT_INFO,
            wait_semaphore_count: 0,
            wait_semaphores: std::ptr::null(),
            wait_dst_stage_mask: std::ptr::null(),
            command_buffer_count: 1,
            command_buffers: &command_buffer,
            signal_semaphore_count: 0,
            signal_semaphores: std::ptr::null(),
            next: std::ptr::null(),
        };
        unsafe {
            // grapics is also capable of compute and transfer btw
            self.device
                .queue_submit(
                    self.vulkan_data.graphics_queue,
                    &[submit_info],
                    vk::Fence::null(),
                )
                .unwrap();
            // yep unoptimal but you are not supposed to use this at all
            self.device.queue_wait_idle(self.vulkan_data.graphics_queue).unwrap();
        }
        unsafe {
            self.device
                .free_command_buffers(self.vulkan_data.command_pool, &[command_buffer]);
        }
    }

    #[cold]
    #[optimize(size)]
    pub fn bind_compute_pipe(&self, cmb: &vk::CommandBuffer, pipe: &ComputePipe) {
        unsafe {
            self.device.cmd_bind_pipeline(*cmb, vk::PipelineBindPoint::COMPUTE, pipe.line);
            self.device.cmd_bind_descriptor_sets(
                *cmb,
                vk::PipelineBindPoint::COMPUTE,
                pipe.line_layout,
                0,
                &[*pipe.sets.current()],
                &[],
            );
        }
    }

    #[cold]
    #[optimize(size)]
    pub fn bind_raster_pipe(&self, cmb: &vk::CommandBuffer, pipe: &RasterPipe) {
        unsafe {
            self.device.cmd_bind_pipeline(*cmb, vk::PipelineBindPoint::GRAPHICS, pipe.line);
            self.device.cmd_bind_descriptor_sets(
                *cmb,
                vk::PipelineBindPoint::GRAPHICS,
                pipe.line_layout,
                0,
                &[*pipe.sets.current()],
                &[],
            );
        }
    }

    // creates primary command buffer. Lumal does not interact with non-primary command buffers
    #[cold]
    #[optimize(size)]
    pub fn create_command_buffer(&self) -> Ring<vk::CommandBuffer> {
        let info = vk::CommandBufferAllocateInfo::builder()
            .command_pool(self.vulkan_data.command_pool)
            .level(vk::CommandBufferLevel::PRIMARY)
            .command_buffer_count(MAX_FRAMES_IN_FLIGHT as u32);

        Ring::from_vec(unsafe { self.device.allocate_command_buffers(&info).unwrap() })
    }

    #[cold]
    #[optimize(size)]
    pub fn destroy_command_buffer(&self, compute_command_buffers: &Ring<vk::CommandBuffer>) {
        unsafe {
            self.device.free_command_buffers(
                self.vulkan_data.command_pool,
                compute_command_buffers.as_slice(),
            )
        };
    }

    #[cold]
    #[optimize(size)]
    pub fn transition_image_layout_single_time(
        &self,
        image: &Image,
        old_layout: vk::ImageLayout,
        new_layout: vk::ImageLayout,
    ) {
        let command_buffer = self.begin_single_time_command_buffer();
        let barrier = vk::ImageMemoryBarrier::builder()
            .old_layout(old_layout)
            .new_layout(new_layout)
            .image(image.image)
            .subresource_range(vk::ImageSubresourceRange {
                aspect_mask: image.aspect,
                base_mip_level: 0,
                level_count: 1,
                base_array_layer: 0,
                layer_count: 1,
            })
            .src_access_mask(vk::AccessFlags::MEMORY_READ | vk::AccessFlags::MEMORY_WRITE)
            .dst_access_mask(vk::AccessFlags::MEMORY_READ | vk::AccessFlags::MEMORY_WRITE);
        unsafe {
            self.device.cmd_pipeline_barrier(
                command_buffer,
                vk::PipelineStageFlags::ALL_COMMANDS,
                vk::PipelineStageFlags::ALL_COMMANDS,
                vk::DependencyFlags::empty(),
                &[] as &[vk::MemoryBarrier],
                &[] as &[vk::BufferMemoryBarrier],
                &[barrier],
            );
        };
        self.end_single_time_command_buffer(command_buffer);
    }

    #[cold]
    #[optimize(size)]
    pub fn copy_buffer_to_image_single_time(
        &self,
        buffer: vk::Buffer,
        img: &Image,
        extent: vk::Extent3D,
    ) {
        let command_buffer = self.begin_single_time_command_buffer();
        let copy_region = vk::BufferImageCopy::builder()
            .image_extent(extent)
            .image_subresource(vk::ImageSubresourceLayers {
                aspect_mask: vk::ImageAspectFlags::COLOR,
                mip_level: 0,
                base_array_layer: 0,
                layer_count: 1,
            })
            .buffer_offset(0)
            .build();
        unsafe {
            self.device.cmd_copy_buffer_to_image(
                command_buffer,
                buffer,
                img.image,
                vk::ImageLayout::GENERAL,
                &[copy_region],
            );
        };
        self.end_single_time_command_buffer(command_buffer);
    }

    #[cold]
    #[optimize(size)]
    pub fn copy_buffer_to_buffer_single_time(
        &mut self,
        src_buffer: vk::Buffer,
        dst_buffer: vk::Buffer,
        size: vk::DeviceSize,
    ) {
        let command_buffer = self.begin_single_time_command_buffer();
        unsafe {
            self.device.cmd_copy_buffer(
                command_buffer,
                src_buffer,
                dst_buffer,
                &[vk::BufferCopy {
                    src_offset: 0,
                    dst_offset: 0,
                    size,
                }],
            );
        };
        self.end_single_time_command_buffer(command_buffer);
    }

    #[cold]
    #[optimize(size)]
    pub fn process_deletion_queues(&mut self) {
        let mut write_index = 0;
        for i in 0..self.buffer_deletion_queue.len() {
            let deletion = &self.buffer_deletion_queue[i];
            let should_keep = deletion.lifetime > 0;

            if (should_keep) {
                self.buffer_deletion_queue[write_index] = deletion.clone();
                self.buffer_deletion_queue[i].lifetime -= 1;
                write_index += 1;
            } else {
                if let Some(_mapped_ptr) = deletion.buffer.mapped {
                    unsafe {
                        self.allocator.as_ref().unwrap().unmap_memory(deletion.buffer.allocation)
                    };
                }
                unsafe {
                    self.allocator
                        .as_ref()
                        .unwrap()
                        .destroy_buffer(deletion.buffer.buffer, deletion.buffer.allocation)
                };
            }
            // TODO why shrink_to does not work?
        }
        self.buffer_deletion_queue.resize(write_index, BufferDeletion::default());

        write_index = 0;
        for i in 0..self.image_deletion_queue.len() {
            let deletion = &self.image_deletion_queue[i];
            let should_keep = deletion.lifetime > 0;

            if (should_keep) {
                self.image_deletion_queue[write_index] = deletion.clone();
                self.image_deletion_queue[i].lifetime -= 1;
                write_index += 1;
            } else {
                unsafe {
                    self.allocator
                        .as_ref()
                        .unwrap()
                        .destroy_image(deletion.image.image, deletion.image.allocation)
                };
                unsafe { self.device.destroy_image_view(deletion.image.view, None) };
            }
        }
        self.image_deletion_queue.resize(write_index, ImageDeletion::default());
    }

    // The only use i can imagine for this is the indented one - freing resources
    #[cold]
    #[optimize(size)]
    pub fn process_deletion_queues_untill_all_done(&mut self) {
        while !self.buffer_deletion_queue.is_empty() || !self.image_deletion_queue.is_empty() {
            self.process_deletion_queues();
        }
    }

    pub fn recreate_swapchain(&mut self, window: &Window) {
        // like catching an exception
        self.should_recreate = false;

        let size = window.inner_size();
        if size.width == 0 || size.height == 0 {
            // like throwing an exception back
            self.should_recreate = true;
            return;
        }

        unsafe {
            self.device.device_wait_idle().unwrap();

            // match self.destroy_swapchain_dependent_resources {
            //     Some(ref mut fun) => fun(window),
            //     None => { /* not fun */ }
            // }

            self.destroy_swapchain();

            self.device.device_wait_idle().unwrap();

            create_swapchain(window, &self.instance, &self.device, &mut self.vulkan_data).unwrap();
            // create_swapchain_image_views(&self.device, &mut self.vulkan_data).unwrap();
            // create_command_pool(&self.instance, &self.device, &mut self.vulkan_data).unwrap();

            // match self.create_swapchain_dependent_resources {
            //     Some(ref mut fun) => fun(window),
            //     None => { /* not fun */ }
            // }

            self.image_index = 0;
            self.should_recreate = false;
        };
    }

    pub fn name_var(&self, o_type: vk::ObjectType, o: u64, o_name: &str) {
        let name_info = DebugUtilsObjectNameInfoEXT {
            // TODO: get rid of vulkanalia, trait get_s_type & get_type_name
            object_type: o_type,
            object_handle: o,
            object_name: o_name.as_bytes().as_ptr() as *const i8,
            ..Default::default()
        };
        unsafe {
            vulkanalia::vk::ExtDebugUtilsExtension::set_debug_utils_object_name_ext(
                &self.instance,
                self.device.handle(),
                &name_info,
            );
        }
    }
}

#[rustfmt::skip]
macro_rules! elif {
    ($type_id:ident, $type_1:ident, $type_2:expr) => {
        if $type_id == TypeId::of::<$type_1>() {
            return Some($type_2);
        }
    };
}

#[rustfmt::skip]
pub fn get_vulkan_object_type<T: Any>(_object: &T) -> Option<vk::ObjectType> {
    println!("TYPE NAME {}", type_name::<T>());
    let type_id = TypeId::of::<T>();

    // use vk::ObjectType::*;
    use vk::Buffer;
    use vk::*;

    elif!(type_id, Instance, vk::ObjectType::INSTANCE);
    elif!(type_id, PhysicalDevice, vk::ObjectType::PHYSICAL_DEVICE);
    elif!(type_id, Device, vk::ObjectType::DEVICE);
    elif!(type_id, Queue, vk::ObjectType::QUEUE);
    elif!(type_id, Semaphore, vk::ObjectType::SEMAPHORE);
    elif!(type_id, CommandBuffer, vk::ObjectType::COMMAND_BUFFER);
    elif!(type_id, Fence, vk::ObjectType::FENCE);
    elif!(type_id, DeviceMemory, vk::ObjectType::DEVICE_MEMORY);
    elif!(type_id, Buffer, vk::ObjectType::BUFFER);
    elif!(type_id, Image, vk::ObjectType::IMAGE);
    elif!(type_id, Event, vk::ObjectType::EVENT);
    elif!(type_id, QueryPool, vk::ObjectType::QUERY_POOL);
    elif!(type_id, BufferView, vk::ObjectType::BUFFER_VIEW);
    elif!(type_id, ImageView, vk::ObjectType::IMAGE_VIEW);
    elif!(type_id, ShaderModule, vk::ObjectType::SHADER_MODULE);
    elif!(type_id, PipelineLayout, vk::ObjectType::PIPELINE_LAYOUT);
    elif!(type_id, RenderPass, vk::ObjectType::RENDER_PASS);
    elif!(type_id, Pipeline, vk::ObjectType::PIPELINE);
    elif!(type_id, DescriptorSetLayout, vk::ObjectType::DESCRIPTOR_SET_LAYOUT);
    elif!(type_id, Sampler, vk::ObjectType::SAMPLER);
    elif!(type_id, DescriptorPool, vk::ObjectType::DESCRIPTOR_POOL);
    elif!(type_id, DescriptorSet, vk::ObjectType::DESCRIPTOR_SET);
    elif!(type_id, Framebuffer, vk::ObjectType::FRAMEBUFFER);
    elif!(type_id, CommandPool, vk::ObjectType::COMMAND_POOL);
    elif!(type_id, SurfaceKHR, vk::ObjectType::SURFACE_KHR);
    elif!(type_id, SwapchainKHR, vk::ObjectType::SWAPCHAIN_KHR);
    elif!(type_id, DebugUtilsMessengerEXT, vk::ObjectType::DEBUG_UTILS_MESSENGER_EXT);

    return Some(vk::ObjectType::UNKNOWN);
}

#[macro_export]
macro_rules! set_debug_name {
    ($lumal:expr, $variable:expr, $debug_name:expr) => {{
        if let Some(debug_name) = $debug_name {
            let object_handle = $variable.as_raw();
            // dbg!(std::any::type_name::<$variable>());
            let object_type_option = $crate::get_vulkan_object_type($variable); // Call the function

            if let Some(object_type_vk) = object_type_option {
                let object_type_debug_report = object_type_vk;
                $lumal.name_var(object_type_debug_report, object_handle, debug_name);
            } else {
                // WTF this is printed EVERYWHERE
                // eprintln!(
                //     " Warning: Could not automatically determine ObjectType for {} ",
                //     stringify!($variable)
                // );
            }
        }
    }};
}

#[macro_export]
macro_rules! set_debug_names {
    ($renderer:expr, $base_name:expr, $( ($object:expr, $suffix:expr) ),*) => {
        #[cfg(feature = "debug_validation_names")]
        {
            if let Some(name) = $base_name {
                $(
                    let debug_name = format!("{}{}\0", name, $suffix);
                    let object_handle = $object.as_raw();

                    if let Some(object_type_vk) = $crate::get_vulkan_object_type($object); {
                        $renderer.name_var(object_type_vk, object_handle, &debug_name);
                    }
                )*
            }
        }
    };
}

/// The Vulkan handles and associated properties used by an example Vulkan app.
#[derive(Clone, Debug, Default)]
pub struct VulkanData {
    pub validation: bool,
    // Surface
    pub surface: vk::SurfaceKHR,
    // Physical Device / Logical Device
    pub physical_device: vk::PhysicalDevice,
    pub graphics_queue: vk::Queue,
    pub present_queue: vk::Queue,
    // Swapchain
    pub swapchain_format: vk::Format,
    pub swapchain_extent: vk::Extent2D,
    pub swapchain: vk::SwapchainKHR,
    pub swapchain_images: Ring<crate::Image>,
    // pub swapchain_image_views: Ring<vk::ImageView>,
    // Command Pool
    pub command_pool: vk::CommandPool,
    // Sync Objects
    pub image_available_semaphores: Ring<vk::Semaphore>,
    pub render_finished_semaphores: Ring<vk::Semaphore>,
    pub in_flight_fences: Ring<vk::Fence>,
    // pub images_in_flight: Ring<vk::Fence>,
    // Descriptor pool
    pub descriptor_pool: vk::DescriptorPool,
}

/// Logs debug messages.
#[cold]
#[optimize(speed)]
extern "system" fn debug_callback(
    severity: vk::DebugUtilsMessageSeverityFlagsEXT,
    type_: vk::DebugUtilsMessageTypeFlagsEXT,
    data: *const vk::DebugUtilsMessengerCallbackDataEXT,
    _: *mut c_void,
) -> vk::Bool32 {
    let data = unsafe { *data };
    let message = unsafe { CStr::from_ptr(data.message) }.to_string_lossy();

    if severity.contains(vk::DebugUtilsMessageSeverityFlagsEXT::ERROR) {
        println!("({:?}) {}", type_, message);
    } else if severity.contains(vk::DebugUtilsMessageSeverityFlagsEXT::WARNING) {
        println!("({:?}) {}", type_, message);
    }

    vk::FALSE
}

/// Picks a suitable physical device.
#[cold]
#[optimize(size)]
unsafe fn pick_physical_device(instance: &Instance, data: &mut VulkanData) -> Result<()> {
    for physical_device in instance.enumerate_physical_devices()? {
        let properties = instance.get_physical_device_properties(physical_device);

        if let Err(error) = check_physical_device(instance, data, physical_device) {
            println!(
                "Skipping physical device (`{}`): {}",
                properties.device_name, error
            );
        } else {
            println!("Selected physical device (`{}`).", properties.device_name);
            data.physical_device = physical_device;
            return Ok(());
        }
    }

    Err(anyhow!("Failed to find suitable physical device."))
}

/// Checks that a physical device is suitable.
#[cold]
#[optimize(size)]
unsafe fn check_physical_device(
    instance: &Instance,
    data: &VulkanData,
    physical_device: vk::PhysicalDevice,
) -> Result<()> {
    QueueFamilyIndices::get(instance, data, physical_device)?;
    check_physical_device_extensions(instance, physical_device)?;

    let support = SwapchainSupport::get(instance, data, physical_device)?;
    if support.formats.is_empty() || support.present_modes.is_empty() {
        // return Err(anyhow!(SuitabilityError("Insufficient swapchain support.")));
        println!("Insufficient swapchain support");
        exit(1);
    }

    Ok(())
}

/// Checks that a physical device supports the required device extensions.
#[cold]
#[optimize(size)]
unsafe fn check_physical_device_extensions(
    instance: &Instance,
    physical_device: vk::PhysicalDevice,
) -> Result<()> {
    let extensions = instance
        .enumerate_device_extension_properties(physical_device, None)?
        .iter()
        .map(|e| e.extension_name)
        .collect::<HashSet<_>>();
    if DEVICE_EXTENSIONS.iter().all(|e| extensions.contains(e)) {
        Ok(())
    } else {
        println!("Missing required device extensions");
        exit(1);
    }
}

/// Creates a logical device for the picked physical device.
#[allow(unused_variables)]
#[cold]
#[optimize(size)]
unsafe fn create_logical_device(
    entry: &Entry,
    instance: &Instance,
    data: &mut VulkanData,
) -> Result<Device> {
    let indices = QueueFamilyIndices::get(instance, data, data.physical_device)?;

    let mut unique_indices = HashSet::new();
    unique_indices.insert(indices.graphics);
    unique_indices.insert(indices.present);

    let queue_priorities = &[1.0];
    let queue_infos = unique_indices
        .iter()
        .map(|i| {
            vk::DeviceQueueCreateInfo::builder()
                .queue_family_index(*i)
                .queue_priorities(queue_priorities)
        })
        .collect::<Vec<_>>();

    let layers = if data.validation {
        vec![VALIDATION_LAYER.as_ptr()]
    } else {
        vec![]
    };

    let mut extensions = DEVICE_EXTENSIONS.iter().map(|n| n.as_ptr()).collect::<Vec<_>>();

    // Required by Vulkan SDK on macOS since 1.3.216.
    if cfg!(target_os = "macos") && entry.version()? >= PORTABILITY_MACOS_VERSION {
        extensions.push(vk::KHR_PORTABILITY_SUBSET_EXTENSION.name.as_ptr());
    }

    // TODO: unhardcode
    let mut features = vk::PhysicalDeviceFeatures::builder()
        .sampler_anisotropy(true)
        .shader_int16(true)
        .geometry_shader(true)
        .vertex_pipeline_stores_and_atomics(true)
        .independent_blend(true)
        .build();
    let mut features11 = vk::PhysicalDeviceVulkan11Features::builder()
        .storage_push_constant16(true)
        .build();
    let mut features12 = vk::PhysicalDeviceVulkan12Features::builder()
        .storage_push_constant8(true)
        .storage_buffer_8bit_access(true)
        .shader_int8(true)
        .build();

    features12.next = &mut features11 as *mut vk::PhysicalDeviceVulkan11Features as *mut c_void;

    let mut features2 = vk::PhysicalDeviceFeatures2::builder()
        .features(features)
        .push_next(&mut features12)
        .build();

    let info = vk::DeviceCreateInfo::builder()
        .queue_create_infos(&queue_infos)
        .enabled_layer_names(&layers)
        .enabled_extension_names(&extensions)
        .push_next(&mut features2)
        .build();

    let device = instance.create_device(data.physical_device, &info, None)?;

    data.graphics_queue = device.get_device_queue(indices.graphics, 0);
    data.present_queue = device.get_device_queue(indices.present, 0);

    Ok(device)
}

/// Creates a swapchain and swapchain images.
#[cold]
#[optimize(size)]
unsafe fn create_swapchain(
    window: &Window,
    instance: &Instance,
    device: &Device,
    data: &mut VulkanData,
) -> Result<()> {
    let indices = QueueFamilyIndices::get(instance, data, data.physical_device)?;
    let support = SwapchainSupport::get(instance, data, data.physical_device)?;

    let surface_format = get_swapchain_surface_format(&support.formats);
    let present_mode = get_swapchain_present_mode(&support.present_modes);
    let extent = get_swapchain_extent(window, support.capabilities);

    data.swapchain_format = surface_format.format;
    data.swapchain_extent = extent;

    // A max image count of 0 indicates that the surface has no upper limit on number of images.
    let max_image_count = if support.capabilities.max_image_count != 0 {
        support.capabilities.max_image_count
    } else {
        u32::MAX
    };

    let image_count = (support.capabilities.min_image_count + 1).min(max_image_count);

    let mut queue_family_indices = vec![];
    let image_sharing_mode = if indices.graphics != indices.present {
        queue_family_indices.push(indices.graphics);
        queue_family_indices.push(indices.present);
        vk::SharingMode::CONCURRENT
    } else {
        vk::SharingMode::EXCLUSIVE
    };

    let info = vk::SwapchainCreateInfoKHR::builder()
        .surface(data.surface)
        .min_image_count(image_count)
        .image_format(surface_format.format)
        .image_color_space(surface_format.color_space)
        .image_extent(extent)
        .image_array_layers(1)
        .image_usage(vk::ImageUsageFlags::COLOR_ATTACHMENT)
        .image_sharing_mode(image_sharing_mode)
        .queue_family_indices(&queue_family_indices)
        .pre_transform(support.capabilities.current_transform)
        .composite_alpha(vk::CompositeAlphaFlagsKHR::OPAQUE)
        .present_mode(present_mode)
        .clipped(true);

    data.swapchain = device.create_swapchain_khr(&info, None)?;

    data.swapchain_images = Ring::from_vec(
        device
            .get_swapchain_images_khr(data.swapchain)
            .unwrap()
            .iter()
            .enumerate()
            .map(|(i, vk_img)| {
                let components = vk::ComponentMapping::builder()
                    .r(vk::ComponentSwizzle::IDENTITY)
                    .g(vk::ComponentSwizzle::IDENTITY)
                    .b(vk::ComponentSwizzle::IDENTITY)
                    .a(vk::ComponentSwizzle::IDENTITY);

                let subresource_range = vk::ImageSubresourceRange::builder()
                    .aspect_mask(vk::ImageAspectFlags::COLOR)
                    .base_mip_level(0)
                    .level_count(1)
                    .base_array_layer(0)
                    .layer_count(1);

                let info = vk::ImageViewCreateInfo::builder()
                    .image(*vk_img)
                    .view_type(vk::ImageViewType::_2D)
                    .format(data.swapchain_format)
                    .components(components)
                    .subresource_range(subresource_range);

                let view = device.create_image_view(&info, None).unwrap();

                // manually give swapchain image views debug names
                #[cfg(feature = "debug_validation_names")]
                {
                    let debug_name = format!("Swapchain Image View {}\0", i);
                    let object_handle = (&view).as_raw();
                    let object_type_option = crate::get_vulkan_object_type((&view));
                    if let Some(object_type_vk) = object_type_option {
                        let object_type_debug_report = object_type_vk;
                        let name_info = DebugUtilsObjectNameInfoEXT {
                            // TODO: get rid of vulkanalia, trait get_s_type & get_type_name
                            object_type: object_type_vk,
                            object_handle: object_handle,
                            object_name: debug_name.as_bytes().as_ptr() as *const i8,
                            ..Default::default()
                        };
                        unsafe {
                            vulkanalia::vk::ExtDebugUtilsExtension::set_debug_utils_object_name_ext(
                                instance,
                                device.handle(),
                                &name_info,
                            );
                        }
                    }
                };

                Image {
                    image: *vk_img,
                    // fuck Vulkanalia
                    allocation: std::mem::transmute::<*const u8, vma::Allocation>(std::ptr::null()),
                    view: view,
                    mip_views: vec![],
                    format: surface_format.format,
                    aspect: ImageAspectFlags::COLOR,
                    extent: Extent3D {
                        width: extent.width,
                        height: extent.height,
                        depth: 1,
                    },
                    mip_levels: 0,
                }
            })
            .collect(),
    );

    Ok(())
}

/// Gets a suitable swapchain surface format.
#[cold]
#[optimize(size)]
fn get_swapchain_surface_format(formats: &[vk::SurfaceFormatKHR]) -> vk::SurfaceFormatKHR {
    for f in formats {
        if f.format == vk::Format::B8G8R8A8_UNORM
            && f.color_space == vk::ColorSpaceKHR::SRGB_NONLINEAR
        {
            return *f;
        }
    }
    for f in formats {
        if f.format == vk::Format::R8G8B8A8_UNORM
            && f.color_space == vk::ColorSpaceKHR::SRGB_NONLINEAR
        {
            return *f;
        }
    }
    return formats[0];
}

/// Gets a suitable swapchain present mode.
#[cold]
#[optimize(size)]
fn get_swapchain_present_mode(present_modes: &[vk::PresentModeKHR]) -> vk::PresentModeKHR {
    present_modes
        .iter()
        .cloned()
        .find(|m| *m == vk::PresentModeKHR::MAILBOX)
        .unwrap_or(vk::PresentModeKHR::FIFO)
}

/// Gets a suitable swapchain extent.
#[rustfmt::skip]
#[cold]
#[optimize(size)]
fn get_swapchain_extent(window: &Window, capabilities: vk::SurfaceCapabilitiesKHR) -> vk::Extent2D {
    if capabilities.current_extent.width != u32::MAX {
        capabilities.current_extent
    } else {
        vk::Extent2D::builder()
            .width(window.inner_size().width.clamp(
                capabilities.min_image_extent.width,
                capabilities.max_image_extent.width,
            ))
            .height(window.inner_size().height.clamp(
                capabilities.min_image_extent.height,
                capabilities.max_image_extent.height,
            ))
            .build()
    }
}

#[cold]
#[optimize(size)]
unsafe fn create_command_pool(
    instance: &Instance,
    device: &Device,
    data: &mut VulkanData,
) -> Result<()> {
    let indices = QueueFamilyIndices::get(instance, data, data.physical_device)?;

    let info = vk::CommandPoolCreateInfo::builder()
        .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER)
        .queue_family_index(indices.graphics);

    data.command_pool = device.create_command_pool(&info, None)?;

    Ok(())
}

#[cold]
#[optimize(size)]
unsafe fn create_sync_objects(device: &Device, data: &mut VulkanData) -> Result<()> {
    let semaphore_info = vk::SemaphoreCreateInfo::builder();
    let fence_info = vk::FenceCreateInfo::builder().flags(vk::FenceCreateFlags::SIGNALED);

    data.image_available_semaphores.resize(MAX_FRAMES_IN_FLIGHT, Default::default());
    data.render_finished_semaphores.resize(MAX_FRAMES_IN_FLIGHT, Default::default());
    data.in_flight_fences.resize(MAX_FRAMES_IN_FLIGHT, Default::default());

    for i in 0..MAX_FRAMES_IN_FLIGHT {
        data.image_available_semaphores[i] = (device.create_semaphore(&semaphore_info, None)?);
        data.render_finished_semaphores[i] = (device.create_semaphore(&semaphore_info, None)?);
        data.in_flight_fences[i] = (device.create_fence(&fence_info, None)?);
    }

    Ok(())
}

#[derive(Copy, Clone, Debug)]
struct QueueFamilyIndices {
    graphics: u32,
    present: u32,
}

impl QueueFamilyIndices {
    #[cold]
    #[optimize(size)]
    unsafe fn get(
        instance: &Instance,
        data: &VulkanData,
        physical_device: vk::PhysicalDevice,
    ) -> Result<Self> {
        let properties = instance.get_physical_device_queue_family_properties(physical_device);

        let graphics = properties
            .iter()
            .position(|p| p.queue_flags.contains(vk::QueueFlags::GRAPHICS))
            .map(|i| i as u32);

        let mut present = None;
        for (index, _) in properties.iter().enumerate() {
            if instance.get_physical_device_surface_support_khr(
                physical_device,
                index as u32,
                data.surface,
            )? {
                present = Some(index as u32);
                break;
            }
        }

        if let (Some(graphics), Some(present)) = (graphics, present) {
            Ok(Self { graphics, present })
        } else {
            println!("Missing required queue families");
            exit(1);
        }
    }
}

#[derive(Clone, Debug)]
struct SwapchainSupport {
    capabilities: vk::SurfaceCapabilitiesKHR,
    formats: Vec<vk::SurfaceFormatKHR>,
    present_modes: Vec<vk::PresentModeKHR>,
}

impl SwapchainSupport {
    #[cold]
    #[optimize(size)]
    unsafe fn get(
        instance: &Instance,
        data: &VulkanData,
        physical_device: vk::PhysicalDevice,
    ) -> Result<Self> {
        Ok(Self {
            capabilities: instance
                .get_physical_device_surface_capabilities_khr(physical_device, data.surface)?,
            formats: instance
                .get_physical_device_surface_formats_khr(physical_device, data.surface)?,
            present_modes: instance
                .get_physical_device_surface_present_modes_khr(physical_device, data.surface)?,
        })
    }
}

use std::fs::File;
use std::io::Read;
use std::path::Path;

#[cold]
#[optimize(size)]
fn read_file<P: AsRef<Path>>(path: P) -> Vec<u8> {
    let possible_error = "Failed to open file: ".to_owned() + path.as_ref().to_str().unwrap();
    let mut file = File::open(path).expect(&possible_error);
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer).expect("Failed to read file");

    assert!(
        buffer.len() % 4 == 0,
        "Shader file must be aligned to 4 bytes"
    );

    unsafe { std::slice::from_raw_parts(buffer.as_ptr() as *const u8, buffer.len() / 4).to_vec() }
}
