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

use ring::*;

pub use ash::vk;
use ash::{
    ext::debug_utils,
    khr::{push_descriptor, surface, swapchain},
    prelude::VkResult,
    vk::{
        ConformanceVersion, DebugUtilsObjectNameInfoEXT, ImageAspectFlags, EXT_DEBUG_UTILS_NAME,
        KHR_GET_PHYSICAL_DEVICE_PROPERTIES2_NAME, KHR_PORTABILITY_ENUMERATION_NAME,
    },
    Device, Entry, Instance,
};
use core::error;
use gpu_allocator::vulkan::{self as vma, Allocator, AllocatorCreateDesc};
use std::{any::type_name, os::raw::c_void};
use std::{
    any::Any,
    mem::{size_of, size_of_val},
};
use std::{any::TypeId, ffi::CStr};
use std::{collections::HashSet, default};
use std::{ffi::c_char, process::exit};
use winit::{
    application::ApplicationHandler,
    dpi::LogicalSize,
    event::{DeviceEvent, DeviceId, WindowEvent},
    event_loop::{ActiveEventLoop, EventLoop},
    raw_window_handle::HasWindowHandle,
    window::{WindowAttributes, WindowId},
};
use winit::{raw_window_handle::HasDisplayHandle, window::Window};

const VALIDATION_LAYERS: &CStr = c"VK_LAYER_KHRONOS_validation";
const LUNARG_MONITOR_LAYER: &CStr = c"VK_LAYER_LUNARG_monitor";

/// The required device extensions.
const DEVICE_EXTENSIONS: &[&CStr] = &[
    vk::KHR_SWAPCHAIN_NAME,
    vk::EXT_HOST_QUERY_RESET_NAME,
    vk::KHR_PUSH_DESCRIPTOR_NAME,
];

/// Vulkan SDK version that started requiring the portability subset extension for macOS.
const PORTABILITY_MACOS_VERSION: ConformanceVersion = ConformanceVersion {
    major: 1,
    minor: 3,
    patch: 216,
    subminor: 0,
};

/// number of frames that will be processed concurrently. 2 is perferct - CPU prepares frame N, GPU renders frame N-1
const MAX_FRAMES_IN_FLIGHT: usize = 2;

// unsafe fn unsafe_clone_on_stack<T>(value: &T) -> T {
//     const SIZE: usize = std::mem::size_of::<T>();
//     // Create an uninitialized array on the stack
//     let mut buffer: std::mem::MaybeUninit<[u8; SIZE]> = std::mem::MaybeUninit::uninit();

//     // Get a mutable pointer to the start of the buffer
//     let buffer_ptr = buffer.as_mut_ptr() as *mut u8;

//     // Copy the bytes from the value to the buffer
//     std::ptr::copy_nonoverlapping(value as *const T as *const u8, buffer_ptr, SIZE);

//     // Transmute the buffer to the target type T
//     let cloned_value: T = buffer.assume_init_ref().as_ptr().cast::<T>().read_unaligned();
//     cloned_value
// }

#[derive(Debug)]
pub struct Buffer {
    pub buffer: vk::Buffer,
    pub allocation: vma::Allocation,
    // pub mapped: Option<*mut c_void>, // If allocation is mapped
}
// impl Clone for Buffer {
//     fn clone(&self) -> Self {
//         let clone_allocation = std::mem::transmute();
//         Self {
//             buffer: self.buffer,
//             allocation: clone_allocation,
//             mapped: self.mapped,
//         }
//     }
// }
impl Default for Buffer {
    fn default() -> Self {
        Self {
            buffer: Default::default(),
            allocation: unsafe { std::mem::zeroed() },
            // mapped: Default::default(),
        }
    }
}

#[derive(Debug)]
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
    pub graphical_and_compute: Option<u32>,
    pub present: Option<u32>,
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
    // pub device_features: vk::PhysicalDeviceFeatures,
    // pub device_features11: vk::PhysicalDeviceVulkan11Features,
    // pub device_features12: vk::PhysicalDeviceVulkan12Features,
    // pub physical_features2: vk::PhysicalDeviceFeatures2,
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
            // device_features: vk::PhysicalDeviceFeatures::default(),
            // device_features11: vk::PhysicalDeviceVulkan11Features::default(),
            // device_features12: vk::PhysicalDeviceVulkan12Features::default(),
            // physical_features2: vk::PhysicalDeviceFeatures2::default(),
        }
    }
}

#[allow(non_snake_case)]
#[derive(Debug, Default)]
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
#[derive(Default, Debug)]
pub struct ImageDeletion {
    pub image: vk::Image,
    pub allocation: vma::Allocation,
    pub view: vk::ImageView,           // Main view
    pub mip_views: Vec<vk::ImageView>, // Vec for mip views
    pub lifetime: i32,
}

impl Clone for ImageDeletion {
    fn clone(&self) -> Self {
        Self {
            image: self.image.clone(),
            view: self.view.clone(),
            mip_views: self.mip_views.clone(),
            lifetime: self.lifetime,
            allocation: vma::Allocation::default(),
        }
    }
}

// TODO: not copy? or Copy buffer
#[derive(Default, Debug)]
pub struct BufferDeletion {
    pub buffer: Buffer,
    pub lifetime: i32,
}

pub struct Renderer {
    pub allocator: vma::Allocator,
    pub settings: LumalSettings,
    pub vulkan_data: VulkanData, // ok example from vk is good
    pub entry: Entry,            // internal vk entry point
    pub instance: Instance, // wrapper around vk::Instance. TODO: custom vulkan al wrapper (barebone)
    pub device: Device,     // wrapper around vk::Device. TODO: custom vulkan al wrapper (barebone)
    pub surface_loader: surface::Instance,
    pub swapchain_loader: swapchain::Device,
    pub debug_utils_loader: debug_utils::Instance,
    pub debug_utils_device_loader: debug_utils::Device,
    pub push_descriptors_loader: push_descriptor::Device,
    pub frame: i32, // global counter of rendered frame, mostly for internal use
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
    pub fn create(settings: &LumalSettings, window: &Window) -> Renderer {
        println!("Starting app.");

        let mut vulkan_data = VulkanData {
            validation: settings.debug,
            ..Default::default()
        };

        if vulkan_data.validation {
            println!("Validation layers requested.");
        }
        unsafe {
            atrace!();
            let entry = Entry::load().expect("Failed to load Vulkan entry point");
            let instance = Renderer::create_instance(window, &entry, &mut vulkan_data);
            atrace!();
            vulkan_data.surface = ash_window::create_surface(
                &entry,
                &instance,
                window.display_handle().unwrap().as_raw(),
                window.window_handle().unwrap().as_raw(),
                None,
            )
            .unwrap();
            atrace!();
            pick_physical_device(&instance, &entry, &mut vulkan_data);
            atrace!();
            let device = create_logical_device(&entry, &instance, &mut vulkan_data);
            atrace!();

            let mut allocator = Allocator::new(&AllocatorCreateDesc {
                instance: instance.clone(),
                device: device.clone(),
                physical_device: vulkan_data.physical_device,
                debug_settings: Default::default(),
                buffer_device_address: true, // Ideally, check the BufferDeviceAddressFeatures struct.
                allocation_sizes: Default::default(),
            })
            .unwrap();
            atrace!();

            create_swapchain(window, &instance, &entry, &device, &mut vulkan_data);
            atrace!();
            // create_swapchain_image_views(&device, &mut vulkan_data);
            // these are handled by downstream user. Makes no sense to hardcode pipes in renderer
            // example.create_render_pass(&device, &mut data);
            // example.create_pipeline(&device, &mut data);
            // create_framebuffers(&device, &mut data);
            // create_command_buffers(&device, &mut vulkan_data);
            atrace!();
            create_command_pool(&instance, &entry, &device, &mut vulkan_data);
            atrace!();
            create_sync_objects(&device, &mut vulkan_data);
            atrace!();

            atrace!();
            let surface_loader = surface::Instance::new(&entry, &instance);
            atrace!();
            let swapchain_loader = swapchain::Device::new(&instance, &device);
            atrace!();
            let debug_utils_loader = debug_utils::Instance::new(&entry, &instance);
            atrace!();
            let debug_utils_device_loader = debug_utils::Device::new(&instance, &device);
            atrace!();
            let push_descriptors_loader = push_descriptor::Device::new(&instance, &device);
            atrace!();

            Renderer {
                allocator,
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
                surface_loader,
                swapchain_loader,
                debug_utils_loader,
                debug_utils_device_loader,
                push_descriptors_loader,
            }
        }
    }

    #[cold]
    #[optimize(size)]
    pub unsafe fn create_instance(
        window: &Window,
        entry: &Entry,
        data: &mut VulkanData,
    ) -> Instance {
        // Application Info
        let application_info = vk::ApplicationInfo {
            p_application_name: c"renderer_vk".as_ptr(),
            application_version: vk::make_api_version(0, 1, 3, 0),
            p_engine_name: c"No Engine".as_ptr(),
            engine_version: vk::make_api_version(0, 1, 3, 0),
            api_version: vk::make_api_version(0, 1, 3, 0),
            ..Default::default()
        };

        // Layers
        let available_layers = entry
            .enumerate_instance_layer_properties()
            .unwrap()
            .iter()
            .map(|l| l.layer_name)
            .collect::<HashSet<[i8; 256]>>();

        let mut _validation_layers = VALIDATION_LAYERS
            .to_bytes_with_nul()
            .iter()
            .map(|c| *c as i8)
            .collect::<Vec<_>>();
        _validation_layers.resize(256, 0);
        let _validation_layers: [i8; 256] = _validation_layers.try_into().unwrap();

        if data.validation && !available_layers.contains(&_validation_layers) {
            return panic!("Validation layers requested but not supported");
        }

        let mut layers = if (data.validation) {
            vec![VALIDATION_LAYERS.as_ptr()]
        } else {
            Vec::new()
        };

        let mut lunarg_monitor_layer = LUNARG_MONITOR_LAYER
            .to_bytes_with_nul()
            .iter()
            .map(|c| *c as i8)
            .collect::<Vec<_>>();
        lunarg_monitor_layer.resize(256, 0);
        let lunarg_monitor_layer: [i8; 256] = lunarg_monitor_layer.try_into().unwrap();

        if available_layers.contains(&lunarg_monitor_layer) {
            layers.push(LUNARG_MONITOR_LAYER.as_ptr());
        }

        // Extensions
        let mut extensions =
            ash_window::enumerate_required_extensions(window.display_handle().unwrap().as_raw())
                .unwrap()
                .to_vec();

        // Required by Vulkan SDK on macOS since 1.3.216.
        let flags = if cfg!(target_os = "macos")
            && entry.try_enumerate_instance_version().unwrap().unwrap()
                >= PORTABILITY_MACOS_VERSION.major as u32
        {
            println!("Enabling extensions for macOS portability.");

            extensions.push(KHR_GET_PHYSICAL_DEVICE_PROPERTIES2_NAME.as_ptr());
            extensions.push(KHR_PORTABILITY_ENUMERATION_NAME.as_ptr());
            vk::InstanceCreateFlags::ENUMERATE_PORTABILITY_KHR
        } else {
            vk::InstanceCreateFlags::empty()
        };

        if data.validation {
            extensions.push(EXT_DEBUG_UTILS_NAME.as_ptr());
        }

        // Create
        let mut info = vk::InstanceCreateInfo {
            p_application_info: &application_info,
            enabled_layer_count: layers.len() as u32,
            pp_enabled_layer_names: layers.as_ptr(),
            enabled_extension_count: extensions.len() as u32,
            pp_enabled_extension_names: extensions.as_ptr(),
            flags,
            ..Default::default()
        };

        let mut debug_info = vk::DebugUtilsMessengerCreateInfoEXT {
            message_severity: vk::DebugUtilsMessageSeverityFlagsEXT::WARNING
                | vk::DebugUtilsMessageSeverityFlagsEXT::ERROR,
            message_type: vk::DebugUtilsMessageTypeFlagsEXT::GENERAL
                | vk::DebugUtilsMessageTypeFlagsEXT::VALIDATION
                | vk::DebugUtilsMessageTypeFlagsEXT::PERFORMANCE,
            pfn_user_callback: Some(debug_callback),
            ..Default::default()
        };

        if data.validation {
            info.p_next = &mut debug_info as *mut _ as *mut c_void;
        }

        entry.create_instance(&info, None).unwrap()
    }
    /// buffers, images, pipelines - everything created manually should be destroyed manually before this funcall
    pub unsafe fn destroy(&mut self) {
        self.process_deletion_queues_untill_all_done();
        {
            self.device.destroy_descriptor_pool(self.vulkan_data.descriptor_pool, None);
            self.vulkan_data.descriptor_pool = vk::DescriptorPool::null();
        }
        self.device.destroy_command_pool(self.vulkan_data.command_pool, None);
        self.destroy_swapchain();
        self.destroy_sync_primitives();

        // let allocator = std::mem::replace(
        //     &mut self.allocator,
        //     std::mem::MaybeUninit::zeroed().assume_init(),
        // );

        // std::mem::drop(allocator);

        self.device.destroy_device(None);
        unsafe {
            self.surface_loader.destroy_surface(self.vulkan_data.surface, None);
        }
        self.instance.destroy_instance(None);
    }

    #[cold]
    #[optimize(size)]
    unsafe fn destroy_swapchain(&self) {
        self.vulkan_data
            .swapchain_images
            .iter()
            .for_each(|v| self.device.destroy_image_view(v.view, None));
        unsafe {
            self.swapchain_loader.destroy_swapchain(self.vulkan_data.swapchain, None);
        }
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
            level: vk::CommandBufferLevel::PRIMARY,
            command_pool: self.vulkan_data.command_pool,
            command_buffer_count: 1,
            ..Default::default()
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
            wait_semaphore_count: 0,
            signal_semaphore_count: 0,
            command_buffer_count: 1,
            p_command_buffers: &command_buffer,
            ..Default::default()
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
        let info = vk::CommandBufferAllocateInfo {
            command_pool: self.vulkan_data.command_pool,
            level: vk::CommandBufferLevel::PRIMARY,
            command_buffer_count: MAX_FRAMES_IN_FLIGHT as u32,
            ..Default::default()
        };

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
        let barrier = vk::ImageMemoryBarrier {
            old_layout,
            new_layout,
            image: image.image,
            subresource_range: vk::ImageSubresourceRange {
                aspect_mask: image.aspect,
                base_mip_level: 0,
                level_count: 1,
                base_array_layer: 0,
                layer_count: 1,
            },
            src_access_mask: vk::AccessFlags::MEMORY_READ | vk::AccessFlags::MEMORY_WRITE,
            dst_access_mask: vk::AccessFlags::MEMORY_READ | vk::AccessFlags::MEMORY_WRITE,
            ..Default::default()
        };
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
        let copy_region = vk::BufferImageCopy {
            image_extent: extent,
            image_subresource: vk::ImageSubresourceLayers {
                aspect_mask: vk::ImageAspectFlags::COLOR,
                mip_level: 0,
                base_array_layer: 0,
                layer_count: 1,
            },
            buffer_offset: 0,
            ..Default::default()
        };
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
        let len = self.buffer_deletion_queue.len();
        let mut i = 0;

        while i < len {
            if self.buffer_deletion_queue[i].lifetime > 0 {
                // If keeping the buffer, swap it to write_index if needed
                if i != write_index {
                    self.buffer_deletion_queue.swap(i, write_index);
                }
                self.buffer_deletion_queue[write_index].lifetime -= 1;
                write_index += 1;
            } else {
                // Destroy the buffer before overwriting
                let buffer =
                    std::mem::replace(&mut self.buffer_deletion_queue[i].buffer, Buffer::default());
                self.allocator.free(buffer.allocation);
                unsafe { self.device.destroy_buffer(buffer.buffer, None) };
            }
            i += 1;
        }
        self.buffer_deletion_queue.truncate(write_index);

        let mut write_index = 0;
        let len = self.image_deletion_queue.len();
        let mut i = 0;

        while i < len {
            if self.image_deletion_queue[i].lifetime > 0 {
                // If keeping the image, swap it to write_index if needed
                if i != write_index {
                    self.image_deletion_queue.swap(i, write_index);
                }
                self.image_deletion_queue[write_index].lifetime -= 1;
                write_index += 1;
            } else {
                // Destroy the image and view before overwriting
                let image = self.image_deletion_queue[i].image;
                let view = self.image_deletion_queue[i].view;
                let mip_views = std::mem::take(&mut self.image_deletion_queue[i].mip_views);

                unsafe {
                    self.device.destroy_image(image, None);
                    self.device.destroy_image_view(view, None);
                    for mip_view in mip_views {
                        self.device.destroy_image_view(mip_view, None);
                    }
                    // TODO: Handle allocation cleanup
                }
            }
            i += 1;
        }
        self.image_deletion_queue.truncate(write_index);
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

            create_swapchain(
                window,
                &self.instance,
                &self.entry,
                &self.device,
                &mut self.vulkan_data,
            );
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
            // TODO: get rid of vk, trait get_s_type & get_type_name
            object_type: o_type,
            object_handle: o,
            p_object_name: o_name.as_bytes().as_ptr() as *const i8,
            ..Default::default()
        };
        unsafe {
            self.debug_utils_device_loader.set_debug_utils_object_name(&name_info);
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

                    if let Some(object_type_vk) = $crate::get_vulkan_object_type($object) {
                        $renderer.name_var(object_type_vk, object_handle, &debug_name);
                    }
                )*
            }
        }
    };
}

/// The Vulkan handles and associated properties used by an example Vulkan app.
#[derive(Debug, Default)]
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
    let message = unsafe { CStr::from_ptr(data.p_message) }.to_string_lossy();

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
unsafe fn pick_physical_device(instance: &Instance, entry: &Entry, data: &mut VulkanData) {
    for physical_device in instance.enumerate_physical_devices().unwrap() {
        atrace!();
        let properties = instance.get_physical_device_properties(physical_device);
        atrace!();
        if let Err(error) = check_physical_device(instance, entry, data, physical_device) {
            atrace!();
            //TODO:
            println!(
                "Skipping physical device (`{}`): {}",
                properties.device_name_as_c_str().unwrap().to_string_lossy(),
                error
            );
        } else {
            atrace!();
            println!(
                "Selected physical device (`{}`).",
                properties.device_name_as_c_str().unwrap().to_string_lossy()
            );
            data.physical_device = physical_device;
            return;
        }
    }

    panic!("Failed to find suitable physical device")
}

/// Checks that a physical device is suitable.
#[cold]
#[optimize(size)]
unsafe fn check_physical_device(
    instance: &Instance,
    entry: &Entry,
    data: &VulkanData,
    physical_device: vk::PhysicalDevice,
) -> VkResult<()> {
    atrace!();
    QueueFamilyIndices::get(instance, entry, data, physical_device)?;
    atrace!();
    check_physical_device_extensions(instance, physical_device)?;
    atrace!();
    let support = SwapchainSupport::get(instance, entry, data, physical_device)?;
    atrace!();
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
) -> VkResult<()> {
    atrace!();
    let extensions = instance
        .enumerate_device_extension_properties(physical_device)?
        .iter()
        .map(|e| e.extension_name)
        .collect::<HashSet<_>>();
    atrace!();

    // Check if all required extensions are supported
    for required_ext in DEVICE_EXTENSIONS {
        let required_bytes = required_ext.to_bytes();
        let required_len = required_bytes.len();

        // Check if any extension in the set matches our required extension
        let extension_found = extensions.iter().any(|ext| {
            // Compare characters for the length of required_bytes
            for i in 0..required_len {
                if ext[i] != required_bytes[i] as i8 {
                    return false;
                }
            }
            // Check if the extension name ends with a null terminator
            ext[required_len] == 0
        });

        if !extension_found {
            println!(
                "Missing required device extension: {:?}",
                required_ext.to_bytes()
            );
            println!("all extensions: {:?}", extensions);
            exit(34);
        }
    }

    Ok(())
}

/// Creates a logical device for the picked physical device.
#[allow(unused_variables)]
#[cold]
#[optimize(size)]
unsafe fn create_logical_device(
    entry: &Entry,
    instance: &Instance,
    data: &mut VulkanData,
) -> Device {
    let indices = QueueFamilyIndices::get(instance, entry, data, data.physical_device).unwrap();

    let mut unique_indices = HashSet::new();
    unique_indices.insert(indices.graphics);
    unique_indices.insert(indices.present);

    let queue_priorities = &[1.0];
    let queue_infos = unique_indices
        .iter()
        .map(|i| vk::DeviceQueueCreateInfo {
            queue_family_index: *i,
            queue_count: 1,
            p_queue_priorities: queue_priorities.as_ptr(),
            ..Default::default()
        })
        .collect::<Vec<_>>();

    let layers = if data.validation {
        vec![VALIDATION_LAYERS.as_ptr()]
    } else {
        vec![]
    };

    let mut extensions = DEVICE_EXTENSIONS.iter().map(|n| n.as_ptr()).collect::<Vec<_>>();

    // Required by Vulkan SDK on macOS since 1.3.216.
    if cfg!(target_os = "macos")
        && entry.try_enumerate_instance_version().unwrap().unwrap()
            >= PORTABILITY_MACOS_VERSION.major as u32
    {
        extensions.push(vk::KHR_PORTABILITY_SUBSET_NAME.as_ptr());
    }

    // TODO: unhardcode
    let mut features = vk::PhysicalDeviceFeatures {
        sampler_anisotropy: vk::TRUE,
        shader_int16: vk::TRUE,
        geometry_shader: vk::TRUE,
        vertex_pipeline_stores_and_atomics: vk::TRUE,
        independent_blend: vk::TRUE,
        ..Default::default()
    };

    let mut features11 = vk::PhysicalDeviceVulkan11Features {
        storage_push_constant16: vk::TRUE,
        ..Default::default()
    };

    let mut features12 = vk::PhysicalDeviceVulkan12Features {
        storage_push_constant8: vk::TRUE,
        storage_buffer8_bit_access: vk::TRUE,
        shader_int8: vk::TRUE,
        ..Default::default()
    };

    features12.p_next = &mut features11 as *mut vk::PhysicalDeviceVulkan11Features as *mut c_void;

    let mut features2 = vk::PhysicalDeviceFeatures2 {
        features,
        p_next: &mut features12 as *mut vk::PhysicalDeviceVulkan12Features as *mut c_void,
        ..Default::default()
    };

    let info = vk::DeviceCreateInfo {
        queue_create_info_count: queue_infos.len() as u32,
        p_queue_create_infos: queue_infos.as_ptr(),
        // enabled_layer_count: layers.len() as u32,
        // pp_enabled_layer_names: layers.as_ptr(),
        enabled_extension_count: extensions.len() as u32,
        pp_enabled_extension_names: extensions.as_ptr(),
        p_next: &mut features2 as *mut vk::PhysicalDeviceFeatures2 as *mut c_void,
        ..Default::default()
    };

    let device = instance.create_device(data.physical_device, &info, None).unwrap();

    data.graphics_queue = device.get_device_queue(indices.graphics, 0);
    data.present_queue = device.get_device_queue(indices.present, 0);

    device
}

/// Creates a swapchain and swapchain images.
#[cold]
#[optimize(size)]
unsafe fn create_swapchain(
    window: &Window,
    instance: &Instance,
    entry: &Entry,
    device: &Device,
    data: &mut VulkanData,
) {
    let indices = QueueFamilyIndices::get(instance, entry, data, data.physical_device).unwrap();
    let support = SwapchainSupport::get(instance, entry, data, data.physical_device).unwrap();
    let surface_format = get_swapchain_surface_format(&support.formats);
    let present_mode = get_swapchain_present_mode(&support.present_modes);
    let extent = get_swapchain_extent(window, support.capabilities);
    data.swapchain_format = surface_format.format;
    data.swapchain_extent = extent;
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

    let info = vk::SwapchainCreateInfoKHR {
        surface: data.surface,
        min_image_count: image_count,
        image_format: surface_format.format,
        image_color_space: surface_format.color_space,
        image_extent: extent,
        image_array_layers: 1,
        image_usage: vk::ImageUsageFlags::COLOR_ATTACHMENT,
        image_sharing_mode,
        queue_family_index_count: queue_family_indices.len() as u32,
        p_queue_family_indices: queue_family_indices.as_ptr(),
        pre_transform: support.capabilities.current_transform,
        composite_alpha: vk::CompositeAlphaFlagsKHR::OPAQUE,
        present_mode,
        clipped: vk::TRUE,
        ..Default::default()
    };

    let swapchain_loader = swapchain::Device::new(instance, device);
    data.swapchain = swapchain_loader.create_swapchain(&info, None).unwrap();

    let swapchain_images = swapchain_loader.get_swapchain_images(data.swapchain).unwrap();

    data.swapchain_images = Ring::from_vec(
        swapchain_images
            .iter()
            .enumerate()
            .map(|(i, vk_img)| {
                let components = vk::ComponentMapping {
                    r: vk::ComponentSwizzle::IDENTITY,
                    g: vk::ComponentSwizzle::IDENTITY,
                    b: vk::ComponentSwizzle::IDENTITY,
                    a: vk::ComponentSwizzle::IDENTITY,
                };

                let subresource_range = vk::ImageSubresourceRange {
                    aspect_mask: vk::ImageAspectFlags::COLOR,
                    base_mip_level: 0,
                    level_count: 1,
                    base_array_layer: 0,
                    layer_count: 1,
                };

                let info = vk::ImageViewCreateInfo {
                    image: *vk_img,
                    view_type: vk::ImageViewType::TYPE_2D,
                    format: data.swapchain_format,
                    components,
                    subresource_range,
                    ..Default::default()
                };

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
                            // TODO: get rid of vk, trait get_s_type & get_type_name
                            object_type: object_type_vk,
                            object_handle: object_handle,
                            object_name: debug_name.as_bytes().as_ptr() as *const i8,
                            ..Default::default()
                        };
                        unsafe {
                            vk::vk::ExtDebugUtilsExtension::set_debug_utils_object_name_ext(
                                instance,
                                device.handle(),
                                &name_info,
                            );
                        }
                    }
                };

                Image {
                    image: *vk_img,
                    // fuck vk
                    allocation: vma::Allocation::default(),
                    view: view,
                    mip_views: vec![],
                    format: surface_format.format,
                    aspect: ImageAspectFlags::COLOR,
                    extent: vk::Extent3D {
                        width: extent.width,
                        height: extent.height,
                        depth: 1,
                    },
                    mip_levels: 0,
                }
            })
            .collect(),
    );
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
        vk::Extent2D {
            width: window.inner_size().width.clamp(
                capabilities.min_image_extent.width,
                capabilities.max_image_extent.width,
            ),
            height: window.inner_size().height.clamp(
                capabilities.min_image_extent.height,
                capabilities.max_image_extent.height,
            ),
        }
    }
}

#[cold]
#[optimize(size)]
unsafe fn create_command_pool(
    instance: &Instance,
    entry: &Entry,
    device: &Device,
    data: &mut VulkanData,
) {
    let indices = QueueFamilyIndices::get(instance, entry, data, data.physical_device).unwrap();
    let info = vk::CommandPoolCreateInfo {
        flags: vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER,
        queue_family_index: indices.graphics,
        ..Default::default()
    };
    data.command_pool = device.create_command_pool(&info, None).unwrap();
}

#[cold]
#[optimize(size)]
unsafe fn create_sync_objects(device: &Device, data: &mut VulkanData) {
    let semaphore_info = vk::SemaphoreCreateInfo::default();
    let fence_info = vk::FenceCreateInfo {
        flags: vk::FenceCreateFlags::SIGNALED,
        ..Default::default()
    };
    data.image_available_semaphores.resize(MAX_FRAMES_IN_FLIGHT);
    data.render_finished_semaphores.resize(MAX_FRAMES_IN_FLIGHT);
    data.in_flight_fences.resize(MAX_FRAMES_IN_FLIGHT);
    for i in 0..MAX_FRAMES_IN_FLIGHT {
        data.image_available_semaphores[i] =
            (device.create_semaphore(&semaphore_info, None).unwrap());
        data.render_finished_semaphores[i] =
            (device.create_semaphore(&semaphore_info, None).unwrap());
        data.in_flight_fences[i] = (device.create_fence(&fence_info, None).unwrap());
    }
}

#[derive(Clone, Debug)]
struct QueueFamilyIndices {
    graphics: u32,
    present: u32,
}

impl QueueFamilyIndices {
    #[cold]
    #[optimize(size)]
    unsafe fn get(
        instance: &Instance,
        entry: &Entry,
        data: &VulkanData,
        physical_device: vk::PhysicalDevice,
    ) -> VkResult<Self> {
        let properties = instance.get_physical_device_queue_family_properties(physical_device);
        let surface_loader = surface::Instance::new(entry, instance);

        let graphics = properties
            .iter()
            .position(|p| p.queue_flags.contains(vk::QueueFlags::GRAPHICS))
            .map(|i| i as u32);

        let mut present = None;
        for (index, _) in properties.iter().enumerate() {
            if surface_loader.get_physical_device_surface_support(
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
        entry: &Entry,
        data: &VulkanData,
        physical_device: vk::PhysicalDevice,
    ) -> VkResult<SwapchainSupport> {
        let surface_loader = surface::Instance::new(entry, instance);

        Ok(SwapchainSupport {
            capabilities: surface_loader
                .get_physical_device_surface_capabilities(physical_device, data.surface)?,
            formats: surface_loader
                .get_physical_device_surface_formats(physical_device, data.surface)?,
            present_modes: surface_loader
                .get_physical_device_surface_present_modes(physical_device, data.surface)?,
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
