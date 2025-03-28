use crate::{ring::Ring, Renderer}; // Import the LumalRenderer struct
use crate::{set_debug_names, Image};
use anyhow::*;
use std::ptr::{self};
use vulkanalia::vk::{self, DeviceV1_0, Handle};
use vulkanalia_vma::Alloc;
use vulkanalia_vma::{self as vma};

impl Renderer {
    #[cold]
    #[optimize(speed)]
    pub fn create_image(
        &self,
        image_type: vk::ImageType,
        format: vk::Format,
        usage: vk::ImageUsageFlags,
        vma_usage: vma::MemoryUsage,
        vma_flags: vma::AllocationCreateFlags,
        aspect: vk::ImageAspectFlags,
        extent: vk::Extent3D,
        mipmaps: u32,
        sample_count: vk::SampleCountFlags,
        #[cfg(feature = "debug_validation_names")] debug_name: Option<&str>,
    ) -> Result<Image> {
        let image_aspect = aspect;
        let image_format = format;
        let image_extent = extent;
        let image_mip_levels = mipmaps;

        let image_info = vk::ImageCreateInfo {
            s_type: vk::StructureType::IMAGE_CREATE_INFO,
            flags: vk::ImageCreateFlags::empty(),
            image_type,
            format,
            extent,
            mip_levels: mipmaps,
            array_layers: 1,
            samples: sample_count,
            tiling: vk::ImageTiling::OPTIMAL,
            usage,
            sharing_mode: vk::SharingMode::EXCLUSIVE,
            queue_family_index_count: 0,
            // p_queue_family_indices: ptr::null(),
            initial_layout: vk::ImageLayout::UNDEFINED,
            next: ptr::null(),
            queue_family_indices: ptr::null(),
        };

        let alloc_info = vma::AllocationOptions {
            usage: vma_usage,
            flags: vma_flags,
            ..Default::default()
        };

        let (vk_image, allocation) =
            unsafe { self.allocator.as_ref().unwrap().create_image(image_info, &alloc_info) }?;

        let image_image = vk_image;
        let image_allocation = allocation;

        let view_type = match image_type {
            vk::ImageType::_1D => vk::ImageViewType::_1D,
            vk::ImageType::_2D => vk::ImageViewType::_2D,
            vk::ImageType::_3D => vk::ImageViewType::_3D,
            _ => return Err(anyhow!("Unsupported image type")),
        };

        let mut view_info = vk::ImageViewCreateInfo {
            s_type: vk::StructureType::IMAGE_VIEW_CREATE_INFO,
            // p_next: std::ptr::null(),
            flags: vk::ImageViewCreateFlags::empty(),
            image: vk_image,
            view_type,
            format,
            components: vk::ComponentMapping::default(),
            subresource_range: vk::ImageSubresourceRange {
                aspect_mask: if (aspect.contains(vk::ImageAspectFlags::DEPTH))
                    && (aspect.contains(vk::ImageAspectFlags::STENCIL))
                {
                    vk::ImageAspectFlags::DEPTH
                } else {
                    aspect
                },
                base_mip_level: 0,
                level_count: mipmaps,
                base_array_layer: 0,
                layer_count: 1,
            },
            next: ptr::null(),
        };

        let image_view = unsafe { self.device.create_image_view(&view_info, None)? };

        let mut image_mip_views = vec![];
        if mipmaps > 1 {
            image_mip_views = (0..mipmaps)
                .map(|mip| {
                    view_info.subresource_range.base_mip_level = mip;
                    view_info.subresource_range.level_count = 1;
                    let view = unsafe { self.device.create_image_view(&view_info, None).unwrap() };
                    set_debug_names!(self, Some("Stencil View for DS"), (&view, "Image View"));
                    view
                })
                .collect::<Vec<_>>();
        }

        let image = Image {
            image: image_image,
            allocation: image_allocation,
            view: image_view,
            mip_views: image_mip_views,
            format: image_format,
            aspect: image_aspect,
            extent: image_extent,
            mip_levels: image_mip_levels,
        };

        self.transition_image_layout_single_time(
            &image,
            vk::ImageLayout::UNDEFINED,
            vk::ImageLayout::GENERAL,
        );

        set_debug_names!(
            self,
            debug_name,
            (&image.image, "Image"),
            (&image.view, "Image View")
        );

        Ok(image)
    }
    #[cold]
    #[optimize(speed)]
    pub fn create_image_ring(
        &self,
        size: usize,
        image_type: vk::ImageType,
        format: vk::Format,
        usage: vk::ImageUsageFlags,
        vma_usage: vma::MemoryUsage,
        vma_flags: vma::AllocationCreateFlags,
        aspect: vk::ImageAspectFlags,
        extent: vk::Extent3D,
        mipmaps: u32,
        sample_count: vk::SampleCountFlags,
        #[cfg(feature = "debug_validation_names")] debug_name: Option<&str>,
    ) -> Result<Ring<Image>> {
        // Create a vector to hold the images.
        let mut images = Vec::with_capacity(size);

        // Initialize each image and push to the vector.
        for _ in 0..size {
            let image = self.create_image(
                image_type,
                format,
                usage,
                vma_usage,
                vma_flags,
                aspect,
                extent,
                mipmaps,
                sample_count,
                #[cfg(feature = "debug_validation_names")]
                debug_name,
            )?;
            images.push(image);
        }

        // Return the Ring initialized with the images.
        Ok(Ring {
            data: images.into_boxed_slice(),
            index: 0,
        })
    }

    #[cold]
    #[optimize(speed)]
    pub fn destroy_image(&self, img: &Image) {
        unsafe {
            self.device.destroy_image_view(img.view, None);
            self.allocator.as_ref().unwrap().destroy_image(img.image, img.allocation);
        };
    }

    #[cold]
    #[optimize(speed)]
    pub fn destroy_image_ring(&self, images: &Ring<Image>) {
        for img in images {
            self.destroy_image(img);
        }
    }
}
// }
