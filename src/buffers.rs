use crate::{ring::Ring, Buffer, Renderer}; // Import the LumalRenderer struct
use std::ptr::{self, copy_nonoverlapping};
use vulkanalia::vk::{self, BufferUsageFlags};

use vulkanalia_vma::Alloc;
use vulkanalia_vma::{self as vma};

impl Renderer {
    // creates a GPU buffer
    #[cold]
    #[optimize(size)]
    pub fn create_buffer(&self, usage: vk::BufferUsageFlags, size: usize, host: bool) -> Buffer {
        // buffers.allocate(self.vulkan_data.settings.fif as usize);
        // buffers = Ring::new(self.vulkan_data.settings.fif as usize, Buffer::default());

        let buffer_info = vk::BufferCreateInfo {
            s_type: vk::StructureType::BUFFER_CREATE_INFO,
            // p_next: std::ptr::null(),
            flags: vk::BufferCreateFlags::empty(),
            size: size as vk::DeviceSize,
            usage,
            sharing_mode: vk::SharingMode::EXCLUSIVE,
            queue_family_index_count: 0,
            // p_queue_family_indices: std::ptr::null(),
            next: ptr::null(),
            queue_family_indices: ptr::null(),
        };

        let alloc_info = vma::AllocationOptions {
            flags: if host {
                vma::AllocationCreateFlags::HOST_ACCESS_SEQUENTIAL_WRITE
            } else {
                vma::AllocationCreateFlags::empty()
            },
            required_flags: if host {
                vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT
            } else {
                vk::MemoryPropertyFlags::empty()
            },
            usage: vma::MemoryUsage::Auto,
            ..Default::default()
        };

        let (vk_buffer, allocation) =
            unsafe { self.allocator.as_ref().unwrap().create_buffer(buffer_info, &alloc_info) }
                .unwrap();

        // TODO: Integrated CPU memory utilization
        // TODO: what if it fails? Different set of flags?
        let mut mapped = None;
        if host {
            // basically make so CPU can read&write buffer memory
            // this is very complicated under the hood cause memory is literally on GPU and is accessed via PCI-E bus
            mapped =
                Some(unsafe { self.allocator.as_ref().unwrap().map_memory(allocation).unwrap() });
        }
        Buffer {
            buffer: vk_buffer,
            allocation,
            mapped,
        }
    }

    // creates ring of vulkan buffers. Optionally maps
    #[cold]
    #[optimize(size)]
    pub fn create_buffer_rings(
        &self,
        ring_size: usize,
        usage: vk::BufferUsageFlags,
        biffer_size: usize,
        host: bool,
    ) -> Ring<Buffer> {
        // Create a vector to hold the images.
        let mut buffers = Vec::with_capacity(ring_size);

        // Initialize each image and push to the vector.
        for _ in 0..ring_size {
            let buffer = self.create_buffer(usage, biffer_size, host);
            buffers.push(buffer);
        }

        // Return the Ring initialized with the images.
        Ring {
            data: buffers.into_boxed_slice(),
            index: 0,
        }
    }

    #[cold]
    #[optimize(size)]
    pub fn destroy_buffer(&self, buf: Buffer) {
        unsafe {
            // unmap if mapped
            match buf.mapped {
                Some(_) => self.allocator.as_ref().unwrap().unmap_memory(buf.allocation),
                None => {} // do nothing
            }
            self.allocator.as_ref().unwrap().destroy_buffer(buf.buffer, buf.allocation);
        };
    }

    #[cold]
    #[optimize(size)]
    pub fn destroy_buffer_ring(&self, buffers: Ring<Buffer>) {
        for buf in buffers.data {
            self.destroy_buffer(buf);
        }
    }

    // creates a GPU buffer and copies elements into it
    // does buffer_usage |= TRANSFER_DST automatically
    #[cold]
    #[optimize(size)]
    pub fn create_and_upload_buffer<T>(
        &mut self,
        elements: &[T],
        mut buffer_usage: vk::BufferUsageFlags,
    ) -> Buffer {
        buffer_usage |= vk::BufferUsageFlags::TRANSFER_DST;

        let count = elements.len();
        let size = std::mem::size_of_val(elements);
        let buffer = self.create_buffer(
            buffer_usage,
            size,
            false, // TODO: bool -> Enum
        );

        let staging_buffer = self.create_buffer(BufferUsageFlags::TRANSFER_SRC, size, true);

        unsafe {
            copy_nonoverlapping(
                elements.as_ptr(),
                staging_buffer.mapped.unwrap() as *mut T,
                count,
            );
        }

        self.copy_buffer_to_buffer_single_time(
            staging_buffer.buffer,
            buffer.buffer,
            size as vk::DeviceSize,
        );

        self.destroy_buffer(staging_buffer);

        buffer
    }
    // create elem ring not implemented.
}
