use crate::{ring::Ring, Buffer, Renderer}; // Import the LumalRenderer struct
use std::ptr;
use vulkanalia::vk::{self, DeviceV1_0};

use vulkanalia_vma::Alloc;
use vulkanalia_vma::{self as vma};

impl Renderer {
    #[cold]
    #[optimize(size)]
    pub fn create_sampler(&self, sampler_info: &vk::SamplerCreateInfo) -> vk::Sampler {
        let sampler = unsafe { self.device.create_sampler(sampler_info, None) }.unwrap();
        sampler
    }

    #[cold]
    #[optimize(size)]
    pub fn destroy_sampler(&self, sampler: vk::Sampler) {
        unsafe { self.device.destroy_sampler(sampler, None) };
    }
}
