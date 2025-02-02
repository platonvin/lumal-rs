use crate::{read_file, ring::Ring, ComputePipe, RasterPipe, RenderPass};

use crate::*;
use descriptors::*;

use std::{error, ffi::CStr, ptr::null};
use vulkanalia::prelude::v1_3::*;
use vulkanalia::vk::{self, Cast, DeviceV1_3, DynamicState, SuccessCode};

use std::result::Result::Ok;

impl Renderer {
    #[cold]
    #[optimize(speed)]
    pub fn start_frame(&mut self, command_buffers: &[vk::CommandBuffer]) {
        unsafe {
            self.device.wait_for_fences(
                &[*self.vulkan_data.in_flight_fences.current()],
                true,
                u64::MAX,
            );
            self.device.reset_fences(&[*self.vulkan_data.in_flight_fences.current()]);
        };

        let begin_info = vk::CommandBufferBeginInfo::default();

        for command_buffer in command_buffers {
            unsafe {
                self.device
                    .reset_command_buffer(*command_buffer, vk::CommandBufferResetFlags::empty())
                    .unwrap();
            }

            unsafe {
                self.device.begin_command_buffer(*command_buffer, &begin_info).unwrap();
            }
        }

        let index_code = unsafe {
            // this is index of swapchain image that we should render to
            // it is not just incremented-wrapped because driver might (and will) juggle them around for perfomance reasons
            self.device.acquire_next_image_khr(
                self.vulkan_data.swapchain,
                u64::MAX,
                *self.vulkan_data.image_available_semaphores.current(),
                vk::Fence::null(), // no fence
            )
        };

        self.process_error_code(index_code);
    }

    #[cold]
    #[optimize(speed)]
    pub fn present_frame(&mut self, window: &Window) {
        let wait_semaphores = [*self.vulkan_data.render_finished_semaphores.current()];
        let swapchains = [self.vulkan_data.swapchain];
        let image_indices = [self.image_index];
        let present_info = vk::PresentInfoKHR::builder()
            .wait_semaphores(&wait_semaphores)
            .swapchains(&swapchains)
            .image_indices(&image_indices);

        // TODO: figure out how do you make crates so unconvinient to use
        let error_code = unsafe {
            self.device.queue_present_khr(self.vulkan_data.graphics_queue, &present_info)
        };

        self.process_success_code(error_code, window);
    }

    #[cold]
    #[optimize(speed)]
    pub fn end_frame(&mut self, command_buffers: &[vk::CommandBuffer], window: &Window) {
        for command_buffer in command_buffers {
            unsafe {
                self.device.end_command_buffer(*command_buffer).unwrap();
            }
        }
        let signal_semaphores = [*self.vulkan_data.render_finished_semaphores.current()];
        let wait_semaphores = [*self.vulkan_data.image_available_semaphores.current()];
        let wait_stages = [vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT];
        let submit_info = vk::SubmitInfo::builder()
            .wait_semaphores(&wait_semaphores)
            .wait_dst_stage_mask(&wait_stages)
            .command_buffers(command_buffers)
            .signal_semaphores(&signal_semaphores);

        unsafe {
            // ask a queue to exectue the commands in command buffer
            self.device
                .queue_submit(
                    self.vulkan_data.graphics_queue,
                    &[submit_info],
                    *self.vulkan_data.in_flight_fences.current(),
                )
                .unwrap();
        }

        self.present_frame(window);

        self.vulkan_data.image_available_semaphores.move_next();
        self.vulkan_data.render_finished_semaphores.move_next();
        self.vulkan_data.in_flight_fences.move_next();
        // counter for internal purposes
        self.frame += 1;
    }

    // figure out if entire thing has to be recreated or not. Does not reacreate, only "flags" it
    // does someone know how to make this cleaner?
    #[cold]
    #[optimize(speed)]
    fn process_error_code(&mut self, index_code: Result<(u32, SuccessCode), vk::ErrorCode>) {
        // man why did you corrode vulkan. Should i make my own fn wrapper?
        match index_code {
            Ok((index, success_code)) => {
                self.image_index = index;
                match success_code {
                    SuccessCode::SUCCESS => {
                        // do nothing
                    }
                    SuccessCode::SUBOPTIMAL_KHR => {
                        // i still do not really know if suboptimal should be recreated. Works on my machine ©
                        self.should_recreate = true;
                    }
                    _ => {
                        // log cause i dont know what to do with it
                        println!("success_code on aquire_next_image_khr: {:?}", success_code);
                    }
                }
            }
            Err(error_code) => {
                match error_code {
                    vk::ErrorCode::OUT_OF_DATE_KHR => {
                        // out of date => clearly recreate
                        self.should_recreate = true;
                    }
                    _ => {
                        panic!(
                            "unknown error code on aquire_next_image_khr: {:?}",
                            error_code
                        );
                    }
                }
            }
        }
    }

    // does someone know how to make this cleaner?
    #[cold]
    #[optimize(speed)]
    fn process_success_code(&mut self, index_code: VkResult<SuccessCode>, window: &Window) {
        match index_code {
            Ok(success_code) => {
                match success_code {
                    SuccessCode::SUCCESS => { /* do nothing */ }
                    SuccessCode::SUBOPTIMAL_KHR => {
                        // i still do not really know if suboptimal should be recreated. Works on my machine ©
                        self.should_recreate = true;
                        self.recreate_swapchain(window);
                    }
                    _ => {
                        // log cause i dont know what to do with it
                        println!("success_code on aquire_next_image_khr: {:?}", success_code);
                    }
                }
            }
            Err(error_code) => {
                match error_code {
                    vk::ErrorCode::OUT_OF_DATE_KHR => {
                        // out of date => clearly recreate
                        self.should_recreate = true;
                    }
                    _ => {
                        panic!(
                            "unknown error code on aquire_next_image_khr: {:?}",
                            error_code
                        );
                    }
                }
            }
        }
    }
}
