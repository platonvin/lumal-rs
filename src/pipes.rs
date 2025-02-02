use crate::{read_file, ring::Ring, ComputePipe, RasterPipe, RenderPass};

use crate::*;
use descriptors::*;

use std::{error, ffi::CStr, ptr::slice_from_raw_parts};
use vulkanalia::prelude::v1_3::*;
use vulkanalia::vk::{self, Cast, CompareOp, DeviceV1_3, DynamicState, StencilOp};

const PREFIXES: &[&str] = &["../shaders/", "../../shaders/", "shaders/compiled/"];

impl Renderer {
    #[cold]
    #[optimize(size)]
    pub fn destroy_compute_pipe(&mut self, pipe: &mut ComputePipe) {
        assert!(pipe.line != vk::Pipeline::null());
        assert!(pipe.line_layout != vk::PipelineLayout::null());
        assert!(pipe.set_layout != vk::DescriptorSetLayout::null());
        unsafe {
            self.device.destroy_pipeline(pipe.line, None);
            self.device.destroy_pipeline_layout(pipe.line_layout, None);
            self.device.destroy_descriptor_set_layout(pipe.set_layout, None);
        }
        // reset the whole thing. Its like raii but explicit
        *pipe = ComputePipe {
            line: vk::Pipeline::null(),
            line_layout: vk::PipelineLayout::null(),
            sets: Ring::new(0, vk::DescriptorSet::null()),
            set_layout: vk::DescriptorSetLayout::null(),
        };
    }
    #[cold]
    #[optimize(size)]
    pub fn destroy_raster_pipe(&mut self, pipe: RasterPipe) {
        assert!(pipe.line != vk::Pipeline::null());
        assert!(pipe.line_layout != vk::PipelineLayout::null());
        assert!(pipe.set_layout != vk::DescriptorSetLayout::null());
        unsafe {
            self.device.destroy_pipeline(pipe.line, None);
            self.device.destroy_pipeline_layout(pipe.line_layout, None);
            self.device.destroy_descriptor_set_layout(pipe.set_layout, None);
        }
        // reset the whole thing. Its like raii but explicit
        // *pipe = RasterPipe {
        //     line: vk::Pipeline::null(),
        //     line_layout: vk::PipelineLayout::null(),
        //     sets: Ring::new(0, vk::DescriptorSet::null()),
        //     set_layout: vk::DescriptorSetLayout::null(),
        //     render_pass: vk::RenderPass::null(),
        //     subpass_id: 0,
        // };
    }

    #[cold]
    #[optimize(size)]
    pub fn create_compute_pipeline(
        &self,
        pipe: &mut ComputePipe,
        extra_dynamic_layout: Option<vk::DescriptorSetLayout>,
        src: String,
        push_size: u32,
        create_flags: vk::PipelineCreateFlags,
    ) {
        assert!(!src.is_empty());

        // Read the shader source file
        // let comp_shader_code = read_file(src);

        // Shader stage info
        let (module, comp_shader_stage_info) = {
            let resolved_path =
                Self::resolve_shader_path(PREFIXES, src).expect("Failed to resolve shader path");
            // Create Vulkan compute shader module
            let module = Self::load_shader_module(&self.device, &resolved_path);
            assert!(module != vk::ShaderModule::null());

            (
                module,
                vk::PipelineShaderStageCreateInfo {
                    s_type: vk::StructureType::PIPELINE_SHADER_STAGE_CREATE_INFO,
                    next: std::ptr::null(),
                    flags: vk::PipelineShaderStageCreateFlags::empty(),
                    stage: vk::ShaderStageFlags::COMPUTE,
                    module,
                    name: b"main\0".as_ptr() as *const i8,
                    specialization_info: std::ptr::null(),
                },
            )
        };

        // Push constant range
        let push_constant_range = if push_size > 0 {
            Some(
                vk::PushConstantRange::builder()
                    .stage_flags(vk::ShaderStageFlags::COMPUTE)
                    .offset(0)
                    .size(push_size)
                    .build(),
            )
        } else {
            None
        };

        // Descriptor set layouts
        let mut used_dset_layouts = vec![pipe.set_layout];
        if let Some(dynamic_layout) = extra_dynamic_layout {
            used_dset_layouts.push(dynamic_layout);
        }

        // Pipeline layout
        let pipeline_layout_info = vk::PipelineLayoutCreateInfo::builder()
            .set_layouts(&used_dset_layouts)
            .push_constant_ranges(if let Some(range_ref) = &push_constant_range {
                std::slice::from_ref(range_ref)
            } else {
                &[]
            });

        let line_layout = unsafe {
            self.device
                .create_pipeline_layout(&pipeline_layout_info, None)
                .expect("Failed to create pipeline layout")
        };

        // Compute pipeline
        let pipeline_info = vk::ComputePipelineCreateInfo::builder()
            .stage(comp_shader_stage_info)
            .layout(line_layout)
            .flags(create_flags);

        let line = unsafe {
            self.device
                .create_compute_pipelines(vk::PipelineCache::null(), &[*pipeline_info], None)
                .expect("Failed to create compute pipeline")
                .0[0]
        };

        // Clean up shader module
        unsafe {
            self.device.destroy_shader_module(module, None);
        }

        assert!(line != vk::Pipeline::null());
        assert!(line_layout != vk::PipelineLayout::null());

        // Update the pipeline
        pipe.line = line;
        pipe.line_layout = line_layout;
    }

    #[cold]
    #[optimize(size)]
    pub fn create_raster_pipeline(
        &self,
        pipe: &mut RasterPipe,
        extra_dynamic_layout: Option<vk::DescriptorSetLayout>,
        shader_stages: &[ShaderStage],
        attr_desc: &[AttrFormOffs],
        stride: u32,
        input_rate: vk::VertexInputRate,
        topology: vk::PrimitiveTopology,
        extent: vk::Extent2D,
        blends: &[BlendAttachment],
        push_size: u32,
        depth_test: DepthTesting,
        depth_compare_op: vk::CompareOp,
        culling: vk::CullModeFlags,
        stencil: vk::StencilOpState,
    ) {
        assert!(pipe.render_pass != vk::RenderPass::null());
        // Create Vulkan shader stages
        let mut modules_to_destroy = vec![];

        let pipeline_shader_stages: Vec<vk::PipelineShaderStageCreateInfo> = shader_stages
            .iter()
            .map(|stage| {
                let resolved_path = Self::resolve_shader_path(PREFIXES, stage.src.clone())
                    .expect("Failed to resolve shader path");
                let module = Self::load_shader_module(&self.device, &resolved_path);
                modules_to_destroy.push(module);

                vk::PipelineShaderStageCreateInfo {
                    s_type: vk::StructureType::PIPELINE_SHADER_STAGE_CREATE_INFO,
                    next: std::ptr::null(),
                    flags: vk::PipelineShaderStageCreateFlags::empty(),
                    stage: stage.stage,
                    module,
                    name: b"main\0".as_ptr() as *const i8,
                    specialization_info: std::ptr::null(),
                }
            })
            .collect();

        // Create color blend state
        let color_blend_attachments: Vec<vk::PipelineColorBlendAttachmentState> = blends
            .iter()
            .map(|blend_attach| {
                let mut vk_blend = vk::PipelineColorBlendAttachmentState {
                    blend_enable: vk::FALSE,
                    color_write_mask: vk::ColorComponentFlags::all(),
                    src_color_blend_factor: vk::BlendFactor::SRC_ALPHA,
                    dst_color_blend_factor: vk::BlendFactor::ONE_MINUS_SRC_ALPHA,
                    src_alpha_blend_factor: vk::BlendFactor::SRC_ALPHA,
                    dst_alpha_blend_factor: vk::BlendFactor::ONE_MINUS_SRC_ALPHA,
                    ..Default::default()
                };

                if *blend_attach == BlendAttachment::NoBlend {
                    vk_blend.blend_enable = vk::FALSE;
                } else {
                    vk_blend.blend_enable = vk::TRUE;
                }

                match blend_attach {
                    BlendAttachment::BlendMix => {
                        vk_blend.alpha_blend_op = vk::BlendOp::ADD;
                        vk_blend.color_blend_op = vk::BlendOp::ADD;
                    }
                    BlendAttachment::BlendSub => {
                        vk_blend.src_color_blend_factor = vk::BlendFactor::ONE;
                        vk_blend.dst_color_blend_factor = vk::BlendFactor::ONE;
                        vk_blend.src_alpha_blend_factor = vk::BlendFactor::SRC_ALPHA;
                        vk_blend.dst_alpha_blend_factor = vk::BlendFactor::ONE_MINUS_SRC_ALPHA;
                        vk_blend.color_blend_op = vk::BlendOp::SUBTRACT;
                        vk_blend.alpha_blend_op = vk::BlendOp::ADD;
                    }
                    BlendAttachment::BlendReplaceIfGreater => {
                        vk_blend.src_color_blend_factor = vk::BlendFactor::ONE;
                        vk_blend.dst_color_blend_factor = vk::BlendFactor::ONE;
                        vk_blend.color_blend_op = vk::BlendOp::MAX;
                        vk_blend.src_alpha_blend_factor = vk::BlendFactor::ONE;
                        vk_blend.dst_alpha_blend_factor = vk::BlendFactor::ZERO;
                        vk_blend.alpha_blend_op = vk::BlendOp::ADD;
                    }
                    BlendAttachment::BlendReplaceIfLess => {
                        vk_blend.src_color_blend_factor = vk::BlendFactor::ONE;
                        vk_blend.dst_color_blend_factor = vk::BlendFactor::ONE;
                        vk_blend.color_blend_op = vk::BlendOp::MIN;
                        vk_blend.src_alpha_blend_factor = vk::BlendFactor::ONE;
                        vk_blend.dst_alpha_blend_factor = vk::BlendFactor::ZERO;
                        vk_blend.alpha_blend_op = vk::BlendOp::ADD;
                    }
                    BlendAttachment::NoBlend => {}
                };

                vk_blend
            })
            .collect();

        let color_blend_state = vk::PipelineColorBlendStateCreateInfo {
            s_type: vk::StructureType::PIPELINE_COLOR_BLEND_STATE_CREATE_INFO,
            next: std::ptr::null(),
            flags: vk::PipelineColorBlendStateCreateFlags::empty(),
            logic_op_enable: vk::FALSE,
            logic_op: vk::LogicOp::COPY,
            attachment_count: color_blend_attachments.len() as u32,
            attachments: color_blend_attachments.as_ptr(),
            blend_constants: [0.0; 4],
        };

        // Just vec of enabled dynamic states
        let dynamic_states: Vec<vk::DynamicState> =
            vec![vk::DynamicState::VIEWPORT, vk::DynamicState::SCISSOR];

        // Setup dynamic states
        let dynamic_state = vk::PipelineDynamicStateCreateInfo {
            s_type: vk::StructureType::PIPELINE_DYNAMIC_STATE_CREATE_INFO,
            next: std::ptr::null(),
            flags: vk::PipelineDynamicStateCreateFlags::empty(),
            dynamic_state_count: dynamic_states.len() as u32,
            dynamic_states: dynamic_states.as_ptr(),
        };

        let used_dset_layouts: &[vk::DescriptorSetLayout] = match extra_dynamic_layout {
            Some(layout) => &[pipe.set_layout, layout],
            None => &[pipe.set_layout],
        };

        let mut push_range = vk::PushConstantRange {
            stage_flags: vk::ShaderStageFlags::empty(),
            offset: 0,
            size: push_size,
        };
        for shader_stage in shader_stages {
            push_range.stage_flags |= shader_stage.stage;
        }

        // Setup pipeline layout
        let pipeline_layout_create_info = vk::PipelineLayoutCreateInfo {
            s_type: vk::StructureType::PIPELINE_LAYOUT_CREATE_INFO,
            next: std::ptr::null(),
            flags: vk::PipelineLayoutCreateFlags::empty(),
            set_layout_count: used_dset_layouts.len() as u32,
            set_layouts: used_dset_layouts.as_ptr(),
            push_constant_range_count: (push_size > 0) as u32,
            push_constant_ranges: if (push_size > 0) {
                &push_range
            } else {
                std::ptr::null()
            },
        };

        let pipeline_layout = unsafe {
            self.device.create_pipeline_layout(&pipeline_layout_create_info, None).unwrap()
        };

        let binding_description = vk::VertexInputBindingDescription {
            binding: 0,
            stride,
            input_rate,
        };

        let actual_attr_desc: Vec<vk::VertexInputAttributeDescription> = attr_desc
            .iter()
            .enumerate()
            .map(|(i, desc)| vk::VertexInputAttributeDescription {
                location: i as u32,
                binding: 0,
                format: desc.format,
                offset: desc.offset as u32,
            })
            .collect();

        let vertex_input_info = match attr_desc.len() {
            0 => vk::PipelineVertexInputStateCreateInfo::default(),
            _ => vk::PipelineVertexInputStateCreateInfo {
                vertex_binding_description_count: 1,
                vertex_binding_descriptions: &binding_description,

                vertex_attribute_description_count: attr_desc.len() as u32,
                vertex_attribute_descriptions: actual_attr_desc.as_ptr(),
                ..Default::default()
            },
        };

        let input_assembly_state = vk::PipelineInputAssemblyStateCreateInfo {
            topology,
            primitive_restart_enable: vk::FALSE,
            ..Default::default()
        };

        let viewport_state = vk::PipelineViewportStateCreateInfo {
            viewport_count: 1,
            scissor_count: 1,
            ..Default::default()
        };

        let rasterizer = vk::PipelineRasterizationStateCreateInfo {
            depth_clamp_enable: vk::FALSE,
            rasterizer_discard_enable: vk::FALSE,
            polygon_mode: vk::PolygonMode::FILL,
            cull_mode: culling,
            front_face: vk::FrontFace::COUNTER_CLOCKWISE,
            depth_bias_enable: vk::FALSE,
            depth_bias_constant_factor: 0.0,
            depth_bias_clamp: 0.0,
            depth_bias_slope_factor: 0.0,
            line_width: 1.0,
            ..Default::default()
        };

        let multisample_state = vk::PipelineMultisampleStateCreateInfo {
            rasterization_samples: vk::SampleCountFlags::_1,
            sample_shading_enable: vk::FALSE,
            min_sample_shading: 0.0,
            ..Default::default()
        };

        let depth_stencil = vk::PipelineDepthStencilStateCreateInfo {
            depth_test_enable: (depth_test == DepthTesting::DT_Read
                || depth_test == DepthTesting::DT_ReadWrite) as u32,
            depth_write_enable: (depth_test == DepthTesting::DT_Write
                || depth_test == DepthTesting::DT_ReadWrite) as u32,
            depth_compare_op,
            depth_bounds_test_enable: vk::FALSE,
            stencil_test_enable: !Self::stencil_is_empty(stencil) as u32,
            front: stencil,
            back: stencil,
            max_depth_bounds: 1.0,
            min_depth_bounds: 0.0,
            ..Default::default()
        };

        let color_blend_state = vk::PipelineColorBlendStateCreateInfo {
            logic_op_enable: vk::FALSE,
            logic_op: vk::LogicOp::COPY,
            attachment_count: color_blend_attachments.len() as u32,
            attachments: color_blend_attachments.as_ptr(),
            blend_constants: [0.0; 4],
            ..Default::default()
        };

        // Finalize pipeline creation
        let pipeline_create_info = vk::GraphicsPipelineCreateInfo {
            s_type: vk::StructureType::GRAPHICS_PIPELINE_CREATE_INFO,
            next: std::ptr::null(),
            flags: vk::PipelineCreateFlags::empty(),
            stage_count: pipeline_shader_stages.len() as u32,
            stages: pipeline_shader_stages.as_ptr(),
            vertex_input_state: &vertex_input_info,
            input_assembly_state: &input_assembly_state,
            tessellation_state: std::ptr::null(),
            viewport_state: &viewport_state,
            rasterization_state: &rasterizer,
            multisample_state: &multisample_state,
            depth_stencil_state: {
                if (depth_test == DepthTesting::DT_None && Self::stencil_is_empty(stencil)) {
                    std::ptr::null()
                } else {
                    &depth_stencil
                }
            },
            color_blend_state: &color_blend_state,
            dynamic_state: &dynamic_state,
            layout: pipeline_layout,
            render_pass: pipe.render_pass, // you HAVE TO set id in advance
            subpass: pipe.subpass_id as u32, // you HAVE TO set it in advance
            base_pipeline_handle: vk::Pipeline::null(),
            base_pipeline_index: -1,
        };

        let pipeline = unsafe {
            self.device
                .create_graphics_pipelines(vk::PipelineCache::null(), &[pipeline_create_info], None)
                .unwrap()
        }
        .0[0];

        modules_to_destroy
            .iter()
            .for_each(|m| unsafe { self.device.destroy_shader_module(*m, None) });

        // dots never meant anything]
        pipe.line = pipeline;
        pipe.line_layout = pipeline_layout;
    }

    #[cold]
    #[optimize(size)]
    fn stencil_is_empty(stencil: vk::StencilOpState) -> bool {
        (stencil.fail_op == StencilOp::KEEP)
            && (stencil.pass_op == StencilOp::KEEP)
            && (stencil.depth_fail_op == StencilOp::KEEP)
            && (stencil.compare_op == CompareOp::NEVER)
            && (stencil.compare_mask == 0)
            && (stencil.write_mask == 0)
            && (stencil.reference == 0)
    }

    #[cold]
    #[optimize(size)]
    fn create_shader_module(&self, code: &[u8]) -> vk::ShaderModule {
        let code_u32 =
            unsafe { std::slice::from_raw_parts(code.as_ptr() as *const u32, code.len() / 4) };

        let create_info = vk::ShaderModuleCreateInfo::builder().code(code_u32);

        unsafe {
            self.device
                .create_shader_module(&create_info, None)
                .expect("Failed to create shader module")
        }
    }

    // Helper function for resolving shader paths
    #[cold]
    #[optimize(size)]
    fn resolve_shader_path(prefixes: &[&str], file_name: String) -> Option<std::path::PathBuf> {
        for prefix in prefixes {
            let candidate = std::path::Path::new(prefix).join(file_name.as_str());
            if candidate.exists() {
                return Some(candidate);
            }
        }
        None
    }

    // Helper function for loading SPIR-V shader modules
    #[cold]
    #[optimize(size)]
    fn load_shader_module(device: &Device, path: &std::path::Path) -> vk::ShaderModule {
        use std::fs::File;
        use std::io::Read;

        let mut file = File::open(path).expect("Failed to open shader file");
        let mut spirv_code = Vec::new();
        file.read_to_end(&mut spirv_code).expect("Failed to read shader file");

        let create_info = vk::ShaderModuleCreateInfo {
            s_type: vk::StructureType::SHADER_MODULE_CREATE_INFO,
            next: std::ptr::null(),
            flags: vk::ShaderModuleCreateFlags::empty(),
            code_size: spirv_code.len(),
            code: spirv_code.as_ptr() as *const u32,
        };

        unsafe {
            device
                .create_shader_module(&create_info, None)
                .expect("Failed to create shader module")
        }
    }
}
