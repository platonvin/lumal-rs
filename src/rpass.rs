use std::{collections::HashMap, f64::consts::E, ptr::null};

use ash::vk;

use crate::Renderer;
use crate::{
    atrace,
    descriptors::{AttachmentDescription, LoadStoreOp, SubpassAttachmentRefs, SubpassDescription},
    ring::Ring,
    trace, Buffer, DescriptorCounter, Image, LumalSettings, RasterPipe, RenderPass,
};

use crate::function;

impl Renderer {
    #[cold]
    #[optimize(size)]
    pub fn destroy_renderpass(&mut self, rpass: RenderPass) {
        assert!(rpass.render_pass != vk::RenderPass::null());
        assert!(!rpass.framebuffers.is_empty());
        for framebuffer in rpass.framebuffers.into_iter() {
            assert!(*framebuffer != vk::Framebuffer::null());
            unsafe {
                self.device.destroy_framebuffer(*framebuffer, None);
            }
        }

        unsafe {
            self.device.destroy_render_pass(rpass.render_pass, None);
        }
    }

    #[cold]
    #[optimize(size)]
    pub fn create_renderpass(
        &self,
        attachments: &[AttachmentDescription],
        spass_attachs: &mut [SubpassDescription],
    ) -> RenderPass {
        let mut rpass = RenderPass::default();

        assert!(!attachments.is_empty());
        assert!(!spass_attachs.is_empty());

        let mut adescs = vec![vk::AttachmentDescription::default(); attachments.len()];
        let mut arefs = vec![vk::AttachmentReference::default(); attachments.len()];
        let mut img2ref = HashMap::new();
        let mut clears = Vec::new();

        for (i, attachment) in attachments.iter().enumerate() {
            let images = attachment.images;

            // reference to first image in the Ring of images given by pointer
            // Why do i work with pointers? To trick borrow checker and have a nicer syntax
            let first_image = &unsafe { &*images }[0]; //

            adescs[i] = vk::AttachmentDescription {
                format: first_image.format,
                samples: vk::SampleCountFlags::TYPE_1,
                load_op: attachment.load.to_vk_load(),
                store_op: attachment.store.to_vk_store(),
                stencil_load_op: attachment.sload.to_vk_load(),
                stencil_store_op: attachment.sstore.to_vk_store(),
                initial_layout: if (attachment.load == LoadStoreOp::DontCare
                    || attachment.load == LoadStoreOp::Clear)
                    && (attachment.sload == LoadStoreOp::DontCare
                        || attachment.sload == LoadStoreOp::Clear)
                {
                    vk::ImageLayout::UNDEFINED
                } else {
                    vk::ImageLayout::GENERAL
                },
                final_layout: attachment.final_layout,
                flags: vk::AttachmentDescriptionFlags::empty(),
            };
            // so i'th attachment reference references to i'th attachment. Very convenient
            arefs[i] = vk::AttachmentReference {
                attachment: i as u32,
                layout: vk::ImageLayout::GENERAL,
            };

            img2ref.insert(images, i);

            clears.push(attachment.clear);
        }

        rpass.clear_colors = clears;

        // this vec's are used to figure out vulkan stuff from what user supplied
        // i just feel like passing references is more convenient than manually recomputing indices every time
        let mut subpasses = vec![vk::SubpassDescription::default(); spass_attachs.len()];
        let mut sas_refs = vec![SubpassAttachmentRefs::default(); spass_attachs.len()];

        for (i, spass_attach) in spass_attachs.iter().enumerate() {
            if let Some(depth) = spass_attach.a_depth {
                let index = *img2ref.get(&depth).unwrap();
                sas_refs[i].a_depth = Some(arefs[index])
            } else {
                sas_refs[i].a_depth = None;
            };
            for color in spass_attach.a_color {
                let index = *img2ref.get(&color).unwrap();
                sas_refs[i].a_color.push(arefs[index]);
            }
            for input in spass_attach.a_input {
                let index = *img2ref.get(&input).unwrap();
                sas_refs[i].a_input.push(arefs[index]);
            }
        }

        assert!(subpasses.len() == sas_refs.len());
        for (i, sas) in sas_refs.iter_mut().enumerate() {
            subpasses[i].color_attachment_count = sas.a_color.len() as u32;
            subpasses[i].p_color_attachments = sas.a_color.as_ptr();
            subpasses[i].input_attachment_count = sas.a_input.len() as u32;
            subpasses[i].p_input_attachments = sas.a_input.as_ptr();
            // we cant just reference attachment hidden in Option because its literally not what we want
            // aka we want *a_depth, not *Option<a_depth> cause there is (might be) more bits (from enum)
            subpasses[i].p_depth_stencil_attachment = match sas.a_depth {
                Some(_) => sas.a_depth.as_mut().unwrap(),
                None => null(),
            }
        }

        for i in 0..spass_attachs.len() {
            for pipe in &mut *spass_attachs[i].pipes {
                pipe.subpass_id = i as i32;
            }
        }

        // not real vulkan struct, just barriers inside a subpass (currently, dummy barriers)
        let dependencies = Self::create_subpass_dependencies(spass_attachs);

        // typical Vulkan createinfo struct

        let create_info = vk::RenderPassCreateInfo {
            s_type: vk::StructureType::RENDER_PASS_CREATE_INFO,
            attachment_count: adescs.len() as u32,
            p_attachments: adescs.as_ptr(),
            subpass_count: subpasses.len() as u32,
            p_subpasses: subpasses.as_ptr(),
            dependency_count: dependencies.len() as u32,
            p_dependencies: dependencies.as_ptr(),
            ..Default::default()
        };

        // call Vulkan function to actually create the render pass
        let render_pass = unsafe {
            self.device
                .create_render_pass(&create_info, None)
                .expect("Failed to create render pass")
        };
        assert!(render_pass != vk::RenderPass::null());

        // Pipes (which are abstractions of Vulkan pipelines) need to know the render pass
        for spass_attach in spass_attachs {
            for pipe in &mut *spass_attach.pipes {
                pipe.render_pass = render_pass;
            }
        }

        // This is the metadata i store in my render pass abstraction. It helps (me).
        rpass.render_pass = render_pass;
        rpass.extent = vk::Extent2D {
            width: (unsafe { attachments[0].images.as_ref().unwrap() })[0].extent.width,
            height: (unsafe { attachments[0].images.as_ref().unwrap() })[0].extent.height,
        };

        let binding: Vec<&Ring<Image>> =
            attachments.iter().filter_map(|desc| Some(unsafe { &*desc.images })).collect();
        let fb_images: &[&Ring<Image>] = binding.as_slice();

        rpass.framebuffers = self.create_framebuffers(
            render_pass,
            fb_images,
            rpass.extent.width,
            rpass.extent.height,
        );

        rpass
    }

    // Function to create subpass dependencies
    #[cold]
    #[optimize(size)]
    fn create_subpass_dependencies(
        spass_attachs: &[SubpassDescription],
    ) -> Vec<vk::SubpassDependency> {
        let mut dependencies = Vec::new();

        // Initial external to first subpass dependency
        dependencies.push(vk::SubpassDependency {
            src_subpass: vk::SUBPASS_EXTERNAL,
            dst_subpass: 0,
            src_stage_mask: vk::PipelineStageFlags::ALL_GRAPHICS
                | vk::PipelineStageFlags::ALL_COMMANDS,
            dst_stage_mask: vk::PipelineStageFlags::ALL_GRAPHICS,
            src_access_mask: vk::AccessFlags::MEMORY_READ | vk::AccessFlags::MEMORY_WRITE,
            dst_access_mask: vk::AccessFlags::MEMORY_READ | vk::AccessFlags::MEMORY_WRITE,
            dependency_flags: vk::DependencyFlags::empty(),
        });

        // Full wait dependencies between all subpasses
        for i in 0..spass_attachs.len() {
            for j in (i + 1)..spass_attachs.len() {
                dependencies.push(vk::SubpassDependency {
                    src_subpass: i as u32,
                    dst_subpass: j as u32,
                    src_stage_mask: vk::PipelineStageFlags::ALL_GRAPHICS,
                    dst_stage_mask: vk::PipelineStageFlags::ALL_GRAPHICS,
                    src_access_mask: vk::AccessFlags::MEMORY_READ | vk::AccessFlags::MEMORY_WRITE,
                    dst_access_mask: vk::AccessFlags::MEMORY_READ | vk::AccessFlags::MEMORY_WRITE,
                    dependency_flags: vk::DependencyFlags::BY_REGION,
                });
            }
        }

        // Final dependency from last subpass to external
        dependencies.push(vk::SubpassDependency {
            src_subpass: (spass_attachs.len() - 1) as u32,
            dst_subpass: vk::SUBPASS_EXTERNAL,
            src_stage_mask: vk::PipelineStageFlags::ALL_GRAPHICS,
            dst_stage_mask: vk::PipelineStageFlags::ALL_GRAPHICS
                | vk::PipelineStageFlags::ALL_COMMANDS,
            src_access_mask: vk::AccessFlags::MEMORY_READ | vk::AccessFlags::MEMORY_WRITE,
            dst_access_mask: vk::AccessFlags::MEMORY_READ | vk::AccessFlags::MEMORY_WRITE,
            dependency_flags: vk::DependencyFlags::empty(),
        });

        dependencies
    }

    // Function to create framebuffers
    #[cold]
    #[optimize(size)]
    fn create_framebuffers(
        &self,
        // device: &vulkanalia::Device,
        render_pass: vk::RenderPass,
        imgs4views: &[&Ring<Image>],
        width: u32,
        height: u32,
    ) -> Ring<vk::Framebuffer> {
        // Calculate Least Common Multiple (LCM) of the sizes of the image view rings
        let lcm = imgs4views.iter().map(|v| (unsafe { (**v).len() }).clone()).fold(1, lcm_custom);
        assert!(lcm != 0);

        let mut framebuffers = Ring::new(lcm);

        for i in 0..lcm {
            let mut attachment_views = Vec::new();

            for imgs in imgs4views {
                let internal_iter = i % unsafe { (**imgs).len() };
                attachment_views.push((unsafe { (**imgs)[internal_iter].view.clone() }));
            }

            let framebuffer_info = vk::FramebufferCreateInfo {
                render_pass,
                attachment_count: attachment_views.len() as u32,
                p_attachments: attachment_views.as_ptr(),
                width,
                height,
                layers: 1,
                ..Default::default()
            };

            let framebuffer = unsafe {
                self.device
                    .create_framebuffer(&framebuffer_info, None)
                    .expect("Failed to create framebuffer")
            };

            framebuffers[i] = framebuffer;
        }

        framebuffers
    }

    #[cold]
    #[optimize(size)]
    pub fn cmd_begin_renderpass(
        &self,
        command_buffer: &vk::CommandBuffer,
        render_pass: &RenderPass,
        inline: vk::SubpassContents,
    ) {
        let begin_info = vk::RenderPassBeginInfo {
            render_pass: render_pass.render_pass,
            framebuffer: *render_pass.framebuffers.current(),
            render_area: vk::Rect2D {
                offset: vk::Offset2D { x: 0, y: 0 },
                extent: render_pass.extent,
            },
            clear_value_count: render_pass.clear_colors.len() as u32,
            p_clear_values: render_pass.clear_colors.as_slice().as_ptr(),
            ..Default::default()
        };

        unsafe {
            self.device.cmd_begin_render_pass(*command_buffer, &begin_info, inline);
            self.cmd_set_viewport(
                *command_buffer,
                render_pass.extent.width,
                render_pass.extent.height,
            );
        }
    }

    #[cold]
    #[optimize(size)]
    pub fn cmd_end_renderpass(
        &self,
        command_buffer: &vk::CommandBuffer,
        render_pass: &mut RenderPass,
    ) {
        unsafe {
            self.device.cmd_end_render_pass(*command_buffer);
        }
        render_pass.framebuffers.move_next();
    }
}

fn gcd(a: usize, b: usize) -> usize {
    let mut a_copy = a;
    let mut b_copy = b;
    while b_copy != 0 {
        let temp = b_copy;
        b_copy = a_copy % b_copy;
        a_copy = temp;
    }
    a_copy
}

fn lcm_custom(a: usize, b: usize) -> usize {
    if a == 0 || b == 0 {
        return 0;
    }
    (a * b) / gcd(a, b)
}
