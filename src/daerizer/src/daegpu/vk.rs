// SAFETY, once for the file. Every handle here came from a Vulkan call whose VkResult was checked
// and is destroyed once; structs passed by pointer are locals that outlive their call and are the
// size vulkan.rs asserts; and the two-call idiom is always run as Vulkan defines it – once for the
// count, once to fill a buffer of exactly that count. Only what does not follow is noted below.

use alloc::string::String;
use alloc::vec::Vec;
use core::ffi::c_char;

use super::backend::SurfaceFormat;
pub use super::backend::SurfaceFormat as Format;
use super::{GpuBatch, GlyphInstance, SubpixelParams};
use super::vulkan as vk;

#[repr(C, align(4))]
struct SpirV<const N: usize>([u8; N]);

static VERTEX_SPV: &SpirV<{ include_bytes!("shaders/daegun.vertex.spv").len() }> =
    &SpirV(*include_bytes!("shaders/daegun.vertex.spv"));
static FRAGMENT_SPV: &SpirV<{ include_bytes!("shaders/daegun.fragment.spv").len() }> =
    &SpirV(*include_bytes!("shaders/daegun.fragment.spv"));
static SUBPIXEL_SPV: &SpirV<{ include_bytes!("shaders/daegun.subpixel.spv").len() }> =
    &SpirV(*include_bytes!("shaders/daegun.subpixel.spv"));

const SPIRV_MAGIC: u32 = 0x0723_0203;

fn words(bytes: &[u8]) -> Result<&[u32], Error> {
    if bytes.len() < 4 || !bytes.len().is_multiple_of(4) {
        return Err(Error::Unsupported("a SPIR-V module of a whole number of words"));
    }
    let w = unsafe { core::slice::from_raw_parts(bytes.as_ptr().cast::<u32>(), bytes.len() / 4) };
    if w[0] != SPIRV_MAGIC {
        return Err(Error::Unsupported("a SPIR-V module with the right magic number"));
    }
    Ok(w)
}

fn shader_words(mode: Mode) -> Result<(&'static [u32], &'static [u32]), Error> {
    let frag = match mode {
        Mode::Grayscale => &FRAGMENT_SPV.0[..],
        Mode::Subpixel => &SUBPIXEL_SPV.0[..],
    };
    Ok((words(&VERTEX_SPV.0)?, words(frag)?))
}

pub use super::Mode;

#[derive(Debug)]
pub enum Error {
    NoDevice,
    Call {
        what: &'static str,
        code: &'static str,
    },
    Unsupported(&'static str),
    MissingEntryPoint(&'static str),
    BadTarget,
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Error::NoDevice => write!(f, "no Vulkan device"),
            Error::Call { what, code } => write!(f, "{what} failed: {code}"),
            Error::Unsupported(what) => write!(f, "device does not support {what}"),
            Error::MissingEntryPoint(name) => write!(f, "entry point {name} is missing"),
            Error::BadTarget => write!(f, "target does not belong to this device, or has no area"),
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for Error {}

fn check(what: &'static str, r: vk::Result_) -> Result<(), Error> {
    if r == vk::SUCCESS {
        Ok(())
    } else {
        Err(Error::Call { what, code: vk::result_name(r) })
    }
}

struct InstanceFns {
    destroy_instance: vk::PfnDestroyInstance,
    enumerate_physical_devices: vk::PfnEnumeratePhysicalDevices,
    get_physical_device_properties: vk::PfnGetPhysicalDeviceProperties,
    get_physical_device_features: vk::PfnGetPhysicalDeviceFeatures,
    get_physical_device_queue_family_properties: vk::PfnGetPhysicalDeviceQueueFamilyProperties,
    enumerate_device_extension_properties: vk::PfnEnumerateDeviceExtensionProperties,
    get_physical_device_memory_properties: vk::PfnGetPhysicalDeviceMemoryProperties,
    create_device: vk::PfnCreateDevice,
}

struct Suitable {
    pd: vk::PhysicalDevice,
    family: u32,
    name: String,
    device_type: i32,
    dual_src_blend: bool,
    max_target: [u32; 2],
}

struct Built {
    device: vk::Device,
    physical: vk::PhysicalDevice,
    queue_family: u32,
    max_target: [u32; 2],
    queue: vk::Queue,
    name: String,
    device_type: i32,
    rgba: Formatted,
    bgra: Formatted,
    command: OneShot,
    memory: vk::PhysicalDeviceMemoryProperties,
    set_layout: vk::DescriptorSetLayout,
    pipeline_layout: vk::PipelineLayout,
    descriptor_pool: vk::DescriptorPool,
    frames: [Frame; FRAMES_IN_FLIGHT],
    ifns: InstanceFns,
    dfns: DeviceFns,
    owns_device: bool,
}

#[derive(Clone, Copy)]
struct DeviceFns {
    destroy_device: vk::PfnDestroyDevice,
    get_device_queue: vk::PfnGetDeviceQueue,
    device_wait_idle: vk::PfnDeviceWaitIdle,
    create_image: vk::PfnCreateImage,
    destroy_image: vk::PfnDestroyImage,
    get_image_memory_requirements: vk::PfnGetImageMemoryRequirements,
    allocate_memory: vk::PfnAllocateMemory,
    free_memory: vk::PfnFreeMemory,
    bind_image_memory: vk::PfnBindImageMemory,
    create_image_view: vk::PfnCreateImageView,
    destroy_image_view: vk::PfnDestroyImageView,
    create_buffer: vk::PfnCreateBuffer,
    destroy_buffer: vk::PfnDestroyBuffer,
    get_buffer_memory_requirements: vk::PfnGetBufferMemoryRequirements,
    bind_buffer_memory: vk::PfnBindBufferMemory,
    map_memory: vk::PfnMapMemory,
    unmap_memory: vk::PfnUnmapMemory,
    create_render_pass: vk::PfnCreateRenderPass,
    destroy_render_pass: vk::PfnDestroyRenderPass,
    create_framebuffer: vk::PfnCreateFramebuffer,
    destroy_framebuffer: vk::PfnDestroyFramebuffer,
    create_command_pool: vk::PfnCreateCommandPool,
    destroy_command_pool: vk::PfnDestroyCommandPool,
    allocate_command_buffers: vk::PfnAllocateCommandBuffers,
    begin_command_buffer: vk::PfnBeginCommandBuffer,
    end_command_buffer: vk::PfnEndCommandBuffer,
    cmd_pipeline_barrier: vk::PfnCmdPipelineBarrier,
    cmd_copy_image_to_buffer: vk::PfnCmdCopyImageToBuffer,
    cmd_clear_color_image: vk::PfnCmdClearColorImage,
    queue_submit: vk::PfnQueueSubmit,
    create_fence: vk::PfnCreateFence,
    destroy_fence: vk::PfnDestroyFence,
    wait_for_fences: vk::PfnWaitForFences,
    reset_fences: vk::PfnResetFences,
    create_shader_module: vk::PfnCreateShaderModule,
    destroy_shader_module: vk::PfnDestroyShaderModule,
    create_descriptor_set_layout: vk::PfnCreateDescriptorSetLayout,
    destroy_descriptor_set_layout: vk::PfnDestroyDescriptorSetLayout,
    create_pipeline_layout: vk::PfnCreatePipelineLayout,
    destroy_pipeline_layout: vk::PfnDestroyPipelineLayout,
    create_graphics_pipelines: vk::PfnCreateGraphicsPipelines,
    destroy_pipeline: vk::PfnDestroyPipeline,
    create_descriptor_pool: vk::PfnCreateDescriptorPool,
    destroy_descriptor_pool: vk::PfnDestroyDescriptorPool,
    allocate_descriptor_sets: vk::PfnAllocateDescriptorSets,
    update_descriptor_sets: vk::PfnUpdateDescriptorSets,
    cmd_begin_render_pass: vk::PfnCmdBeginRenderPass,
    cmd_end_render_pass: vk::PfnCmdEndRenderPass,
    cmd_bind_pipeline: vk::PfnCmdBindPipeline,
    cmd_bind_descriptor_sets: vk::PfnCmdBindDescriptorSets,
    cmd_set_viewport: vk::PfnCmdSetViewport,
    cmd_set_scissor: vk::PfnCmdSetScissor,
    cmd_draw: vk::PfnCmdDraw,
    reset_command_buffer: vk::PfnResetCommandBuffer,
}

pub struct Renderer {
    _library: vk::Library,
    instance: vk::Instance,
    physical: vk::PhysicalDevice,
    queue_family: u32,
    device: vk::Device,
    queue: vk::Queue,
    device_name: String,
    device_type: i32,
    max_target: [u32; 2],
    rgba: Formatted,
    bgra: Formatted,
    command: core::cell::RefCell<OneShot>,
    memory: vk::PhysicalDeviceMemoryProperties,
    set_layout: vk::DescriptorSetLayout,
    pipeline_layout: vk::PipelineLayout,
    descriptor_pool: vk::DescriptorPool,
    // Per-draw data cannot be shared between slots: a draw still running reads its own instance
    // buffer and descriptor set, so writing either for the next draw changes what it sees.
    frames: core::cell::RefCell<[Frame; FRAMES_IN_FLIGHT]>,
    next_frame: core::cell::Cell<usize>,
    ifns: InstanceFns,
    dfns: DeviceFns,
    owns_device: bool,
}

// Two passes because the load op is baked into a `VkRenderPass`, and one pipeline pair because
// pipeline compatibility turns on attachment format and sample count, never on load ops.
struct Formatted {
    clear_pass: vk::RenderPass,
    load_pass: vk::RenderPass,
    grayscale: vk::Pipeline,
    subpixel: Option<vk::Pipeline>,
}

impl Formatted {
    fn pass_for(&self, clear: Option<crate::daerizer::Rgba>) -> vk::RenderPass {
        if clear.is_some() { self.clear_pass } else { self.load_pass }
    }
}

impl Formatted {
    unsafe fn destroy(&self, d: &DeviceFns, device: vk::Device) {
        unsafe {
            if let Some(p) = self.subpixel {
                (d.destroy_pipeline)(device, p, core::ptr::null());
            }
            (d.destroy_pipeline)(device, self.grayscale, core::ptr::null());
            (d.destroy_render_pass)(device, self.load_pass, core::ptr::null());
            (d.destroy_render_pass)(device, self.clear_pass, core::ptr::null());
        }
    }
}

const FRAMES_IN_FLIGHT: usize = 3;

const TRANSPARENT: crate::daerizer::Rgba = crate::daerizer::Rgba { r: 0, g: 0, b: 0, a: 0 };

struct Frame {
    command: vk::CommandBuffer,
    fence: vk::Fence,
    set: vk::DescriptorSet,
    instances: Buffer,
    subpixel: Buffer,
    projection: Buffer,
    used: bool,
}

struct OneShot {
    pool: vk::CommandPool,
    buffer: vk::CommandBuffer,
    fence: vk::Fence,
}

fn dual_src_disabled() -> bool {
    // A test hook in shipping code that earns its place: the degraded path – grayscale only,
    // Subpixel refused – is what Mali-G52, G68 and G615 take, and nothing to hand lacks
    // dualSrcBlend, so without this the branch mattering most to those users would ship untested.
    unsafe { !getenv(c"DAEGUN_VK_NO_DUAL_SRC".as_ptr()).is_null() }
}

unsafe extern "C" {
    fn getenv(name: *const core::ffi::c_char) -> *const core::ffi::c_char;
}

impl Renderer {
    pub fn new() -> Result<Renderer, Error> {
        shader_words(Mode::Grayscale)?;
        shader_words(Mode::Subpixel)?;

        let library = vk::Library::open().ok_or(Error::NoDevice)?;
        let get_proc = library.get_instance_proc_addr().ok_or(Error::NoDevice)?;

        let create_instance: vk::PfnCreateInstance =
            unsafe { vk::load(get_proc, core::ptr::null_mut(), c"vkCreateInstance") }
                .ok_or(Error::MissingEntryPoint("vkCreateInstance"))?;
        let enumerate_instance_extensions: vk::PfnEnumerateInstanceExtensionProperties = unsafe {
            vk::load(get_proc, core::ptr::null_mut(), c"vkEnumerateInstanceExtensionProperties")
        }
        .ok_or(Error::MissingEntryPoint("vkEnumerateInstanceExtensionProperties"))?;

        let instance_extensions = enumerate(|count, data| {
            unsafe { enumerate_instance_extensions(core::ptr::null(), count, data) }
        })
        .unwrap_or_default();
        // MoltenVK is not conformant, so the loader hides it unless the instance opts into
        // portability drivers – which is a flag *and* an extension, and setting the flag without
        // the extension is itself invalid. Probed rather than assumed; a real driver uses neither.
        let portability = vk::has_extension(&instance_extensions, c"VK_KHR_portability_enumeration")
            && vk::has_extension(&instance_extensions, c"VK_KHR_get_physical_device_properties2");

        let app = vk::ApplicationInfo {
            s_type: vk::STRUCTURE_TYPE_APPLICATION_INFO,
            p_next: core::ptr::null(),
            p_application_name: c"daegun".as_ptr(),
            application_version: 1,
            p_engine_name: c"daegun".as_ptr(),
            engine_version: 1,
            api_version: vk::api_version(1, 0, 0),
        };
        let portability_exts: [*const c_char; 2] = [
            c"VK_KHR_portability_enumeration".as_ptr(),
            c"VK_KHR_get_physical_device_properties2".as_ptr(),
        ];
        let instance_exts: &[*const c_char] = if portability { &portability_exts } else { &[] };
        let info = vk::InstanceCreateInfo {
            s_type: vk::STRUCTURE_TYPE_INSTANCE_CREATE_INFO,
            p_next: core::ptr::null(),
            flags: if portability {
                vk::INSTANCE_CREATE_ENUMERATE_PORTABILITY_BIT_KHR
            } else {
                0
            },
            p_application_info: &app,
            enabled_layer_count: 0,
            pp_enabled_layer_names: core::ptr::null(),
            enabled_extension_count: instance_exts.len() as u32,
            pp_enabled_extension_names: if instance_exts.is_empty() {
                core::ptr::null()
            } else {
                instance_exts.as_ptr()
            },
        };
        let mut instance: vk::Instance = core::ptr::null_mut();
        let created = unsafe { create_instance(&info, core::ptr::null(), &mut instance) };
        if created == vk::ERROR_INCOMPATIBLE_DRIVER {
            return Err(Error::NoDevice);
        }
        check("vkCreateInstance", created)?;

        match Self::finish(&library, get_proc, instance) {
            Ok(b) => Ok(Self::wrap(library, instance, b)),
            Err(e) => {
                if let Some(destroy) =
                    unsafe { vk::load::<vk::PfnDestroyInstance>(get_proc, instance, c"vkDestroyInstance") }
                {
                    unsafe { destroy(instance, core::ptr::null()) };
                }
                Err(e)
            }
        }
    }

    fn wrap(library: vk::Library, instance: vk::Instance, b: Built) -> Renderer {
        Renderer {
            descriptor_pool: b.descriptor_pool,
            frames: core::cell::RefCell::new(b.frames),
            next_frame: core::cell::Cell::new(0),
            _library: library,
            instance,
            physical: b.physical,
            queue_family: b.queue_family,
            device: b.device,
            queue: b.queue,
            device_name: b.name,
            device_type: b.device_type,
            max_target: b.max_target,
            rgba: b.rgba,
            bgra: b.bgra,
            command: core::cell::RefCell::new(b.command),
            memory: b.memory,
            set_layout: b.set_layout,
            pipeline_layout: b.pipeline_layout,
            ifns: b.ifns,
            dfns: b.dfns,
            owns_device: b.owns_device,
        }
    }

    // The handles behind this renderer, for building a swapchain on the device daegun made. They
    // belong to the renderer and die with it, so nothing built on them may outlive it.
    pub unsafe fn handles(&self) -> (vk::Instance, vk::PhysicalDevice, vk::Device, u32) {
        (self.instance, self.physical, self.device, self.queue_family)
    }

    // Every handle must be live and belong together, and daegun destroys neither device nor
    // instance, so the caller outlives the renderer. `dual_src_blend` is what the caller enabled,
    // not what the hardware supports: daegun cannot tell them apart, and without it there is no
    // subpixel pipeline.
    pub unsafe fn from_device(
        instance: vk::Instance,
        physical: vk::PhysicalDevice,
        device: vk::Device,
        queue_family: u32,
        dual_src_blend: bool,
    ) -> Result<Renderer, Error> {
        shader_words(Mode::Grayscale)?;
        shader_words(Mode::Subpixel)?;
        if instance.is_null() || device.is_null() {
            return Err(Error::NoDevice);
        }
        let library = vk::Library::open().ok_or(Error::NoDevice)?;
        let get_proc = library.get_instance_proc_addr().ok_or(Error::NoDevice)?;
        let ifns = Self::instance_fns(get_proc, instance)?;

        let mut found = Self::suitable(&ifns, physical).ok_or(Error::NoDevice)?;
        found.family = queue_family;
        found.dual_src_blend = dual_src_blend && found.dual_src_blend;

        let b = Self::build_on(get_proc, instance, ifns, found, device, false)?;
        Ok(Self::wrap(library, instance, b))
    }

    // Lifted out so an adopted instance loads the same entry points as one daegun made.
    fn instance_fns(
        get_proc: vk::PfnGetInstanceProcAddr,
        instance: vk::Instance,
    ) -> Result<InstanceFns, Error> {
        macro_rules! ifn {
            ($t:ty, $name:literal) => {
                unsafe { vk::load::<$t>(get_proc, instance, $name) }
                    .ok_or(Error::MissingEntryPoint(stringify!($name)))?
            };
        }
        Ok(InstanceFns {
            destroy_instance: ifn!(vk::PfnDestroyInstance, c"vkDestroyInstance"),
            enumerate_physical_devices: ifn!(
                vk::PfnEnumeratePhysicalDevices,
                c"vkEnumeratePhysicalDevices"
            ),
            get_physical_device_properties: ifn!(
                vk::PfnGetPhysicalDeviceProperties,
                c"vkGetPhysicalDeviceProperties"
            ),
            get_physical_device_features: ifn!(
                vk::PfnGetPhysicalDeviceFeatures,
                c"vkGetPhysicalDeviceFeatures"
            ),
            get_physical_device_queue_family_properties: ifn!(
                vk::PfnGetPhysicalDeviceQueueFamilyProperties,
                c"vkGetPhysicalDeviceQueueFamilyProperties"
            ),
            enumerate_device_extension_properties: ifn!(
                vk::PfnEnumerateDeviceExtensionProperties,
                c"vkEnumerateDeviceExtensionProperties"
            ),
            get_physical_device_memory_properties: ifn!(
                vk::PfnGetPhysicalDeviceMemoryProperties,
                c"vkGetPhysicalDeviceMemoryProperties"
            ),
            create_device: ifn!(vk::PfnCreateDevice, c"vkCreateDevice"),
        })
    }

    fn finish(
        _library: &vk::Library,
        get_proc: vk::PfnGetInstanceProcAddr,
        instance: vk::Instance,
    ) -> Result<Built, Error> {
        let ifns = Self::instance_fns(get_proc, instance)?;

        let devices = enumerate(|count, data| {
            unsafe { (ifns.enumerate_physical_devices)(instance, count, data) }
        })
        .ok_or(Error::NoDevice)?;

        let Suitable { pd: physical, family: queue_family, name, device_type, dual_src_blend, max_target } =
            devices.iter().find_map(|&pd| Self::suitable(&ifns, pd)).ok_or(Error::NoDevice)?;

        let device_extensions = enumerate(|count, data| {
            unsafe {
                (ifns.enumerate_device_extension_properties)(
                    physical,
                    core::ptr::null(),
                    count,
                    data,
                )
            }
        })
        .unwrap_or_default();
        let subset = c"VK_KHR_portability_subset";
        let device_exts: &[*const c_char] = if vk::has_extension(&device_extensions, subset) {
            &[subset.as_ptr()]
        } else {
            &[]
        };

        let priority = 1.0f32;
        let queue_info = vk::DeviceQueueCreateInfo {
            s_type: vk::STRUCTURE_TYPE_DEVICE_QUEUE_CREATE_INFO,
            p_next: core::ptr::null(),
            flags: 0,
            queue_family_index: queue_family,
            queue_count: 1,
            p_queue_priorities: &priority,
        };
        let features = vk::PhysicalDeviceFeatures {
            dual_src_blend: if dual_src_blend { vk::TRUE } else { vk::FALSE },
            ..Default::default()
        };
        let device_info = vk::DeviceCreateInfo {
            s_type: vk::STRUCTURE_TYPE_DEVICE_CREATE_INFO,
            p_next: core::ptr::null(),
            flags: 0,
            queue_create_info_count: 1,
            p_queue_create_infos: &queue_info,
            enabled_layer_count: 0,
            pp_enabled_layer_names: core::ptr::null(),
            enabled_extension_count: device_exts.len() as u32,
            pp_enabled_extension_names: if device_exts.is_empty() {
                core::ptr::null()
            } else {
                device_exts.as_ptr()
            },
            p_enabled_features: &features,
        };
        let mut device: vk::Device = core::ptr::null_mut();
        check("vkCreateDevice", unsafe {
            (ifns.create_device)(physical, &device_info, core::ptr::null(), &mut device)
        })?;

        Self::build_on(
            get_proc,
            instance,
            ifns,
            Suitable { pd: physical, family: queue_family, name, device_type, dual_src_blend, max_target },
            device,
            true,
        )
    }

    // Everything past device creation, shared by the device daegun makes and one it is handed.
    fn build_on(
        get_proc: vk::PfnGetInstanceProcAddr,
        instance: vk::Instance,
        ifns: InstanceFns,
        found: Suitable,
        device: vk::Device,
        owns_device: bool,
    ) -> Result<Built, Error> {
        let Suitable { pd: physical, family: queue_family, name, device_type, dual_src_blend, max_target } =
            found;
        macro_rules! dfn {
            ($t:ty, $name:literal) => {
                unsafe { vk::load::<$t>(get_proc, instance, $name) }
                    .ok_or(Error::MissingEntryPoint(stringify!($name)))?
            };
        }
        let dfns = DeviceFns {
            destroy_device: dfn!(vk::PfnDestroyDevice, c"vkDestroyDevice"),
            get_device_queue: dfn!(vk::PfnGetDeviceQueue, c"vkGetDeviceQueue"),
            device_wait_idle: dfn!(vk::PfnDeviceWaitIdle, c"vkDeviceWaitIdle"),
            create_image: dfn!(vk::PfnCreateImage, c"vkCreateImage"),
            destroy_image: dfn!(vk::PfnDestroyImage, c"vkDestroyImage"),
            get_image_memory_requirements: dfn!(vk::PfnGetImageMemoryRequirements, c"vkGetImageMemoryRequirements"),
            allocate_memory: dfn!(vk::PfnAllocateMemory, c"vkAllocateMemory"),
            free_memory: dfn!(vk::PfnFreeMemory, c"vkFreeMemory"),
            bind_image_memory: dfn!(vk::PfnBindImageMemory, c"vkBindImageMemory"),
            create_image_view: dfn!(vk::PfnCreateImageView, c"vkCreateImageView"),
            destroy_image_view: dfn!(vk::PfnDestroyImageView, c"vkDestroyImageView"),
            create_buffer: dfn!(vk::PfnCreateBuffer, c"vkCreateBuffer"),
            destroy_buffer: dfn!(vk::PfnDestroyBuffer, c"vkDestroyBuffer"),
            get_buffer_memory_requirements: dfn!(vk::PfnGetBufferMemoryRequirements, c"vkGetBufferMemoryRequirements"),
            bind_buffer_memory: dfn!(vk::PfnBindBufferMemory, c"vkBindBufferMemory"),
            map_memory: dfn!(vk::PfnMapMemory, c"vkMapMemory"),
            unmap_memory: dfn!(vk::PfnUnmapMemory, c"vkUnmapMemory"),
            create_render_pass: dfn!(vk::PfnCreateRenderPass, c"vkCreateRenderPass"),
            destroy_render_pass: dfn!(vk::PfnDestroyRenderPass, c"vkDestroyRenderPass"),
            create_framebuffer: dfn!(vk::PfnCreateFramebuffer, c"vkCreateFramebuffer"),
            destroy_framebuffer: dfn!(vk::PfnDestroyFramebuffer, c"vkDestroyFramebuffer"),
            create_command_pool: dfn!(vk::PfnCreateCommandPool, c"vkCreateCommandPool"),
            destroy_command_pool: dfn!(vk::PfnDestroyCommandPool, c"vkDestroyCommandPool"),
            allocate_command_buffers: dfn!(vk::PfnAllocateCommandBuffers, c"vkAllocateCommandBuffers"),
            begin_command_buffer: dfn!(vk::PfnBeginCommandBuffer, c"vkBeginCommandBuffer"),
            end_command_buffer: dfn!(vk::PfnEndCommandBuffer, c"vkEndCommandBuffer"),
            cmd_pipeline_barrier: dfn!(vk::PfnCmdPipelineBarrier, c"vkCmdPipelineBarrier"),
            cmd_copy_image_to_buffer: dfn!(vk::PfnCmdCopyImageToBuffer, c"vkCmdCopyImageToBuffer"),
            cmd_clear_color_image: dfn!(vk::PfnCmdClearColorImage, c"vkCmdClearColorImage"),
            queue_submit: dfn!(vk::PfnQueueSubmit, c"vkQueueSubmit"),
            create_fence: dfn!(vk::PfnCreateFence, c"vkCreateFence"),
            destroy_fence: dfn!(vk::PfnDestroyFence, c"vkDestroyFence"),
            wait_for_fences: dfn!(vk::PfnWaitForFences, c"vkWaitForFences"),
            reset_fences: dfn!(vk::PfnResetFences, c"vkResetFences"),
            create_shader_module: dfn!(vk::PfnCreateShaderModule, c"vkCreateShaderModule"),
            destroy_shader_module: dfn!(vk::PfnDestroyShaderModule, c"vkDestroyShaderModule"),
            create_descriptor_set_layout: dfn!(vk::PfnCreateDescriptorSetLayout, c"vkCreateDescriptorSetLayout"),
            destroy_descriptor_set_layout: dfn!(vk::PfnDestroyDescriptorSetLayout, c"vkDestroyDescriptorSetLayout"),
            create_pipeline_layout: dfn!(vk::PfnCreatePipelineLayout, c"vkCreatePipelineLayout"),
            destroy_pipeline_layout: dfn!(vk::PfnDestroyPipelineLayout, c"vkDestroyPipelineLayout"),
            create_graphics_pipelines: dfn!(vk::PfnCreateGraphicsPipelines, c"vkCreateGraphicsPipelines"),
            destroy_pipeline: dfn!(vk::PfnDestroyPipeline, c"vkDestroyPipeline"),
            create_descriptor_pool: dfn!(vk::PfnCreateDescriptorPool, c"vkCreateDescriptorPool"),
            destroy_descriptor_pool: dfn!(vk::PfnDestroyDescriptorPool, c"vkDestroyDescriptorPool"),
            allocate_descriptor_sets: dfn!(vk::PfnAllocateDescriptorSets, c"vkAllocateDescriptorSets"),
            update_descriptor_sets: dfn!(vk::PfnUpdateDescriptorSets, c"vkUpdateDescriptorSets"),
            cmd_begin_render_pass: dfn!(vk::PfnCmdBeginRenderPass, c"vkCmdBeginRenderPass"),
            cmd_end_render_pass: dfn!(vk::PfnCmdEndRenderPass, c"vkCmdEndRenderPass"),
            cmd_bind_pipeline: dfn!(vk::PfnCmdBindPipeline, c"vkCmdBindPipeline"),
            cmd_bind_descriptor_sets: dfn!(vk::PfnCmdBindDescriptorSets, c"vkCmdBindDescriptorSets"),
            cmd_set_viewport: dfn!(vk::PfnCmdSetViewport, c"vkCmdSetViewport"),
            cmd_set_scissor: dfn!(vk::PfnCmdSetScissor, c"vkCmdSetScissor"),
            cmd_draw: dfn!(vk::PfnCmdDraw, c"vkCmdDraw"),
            reset_command_buffer: dfn!(vk::PfnResetCommandBuffer, c"vkResetCommandBuffer"),
        };

        let mut queue: vk::Queue = core::ptr::null_mut();
        unsafe { (dfns.get_device_queue)(device, queue_family, 0, &mut queue) };

        let command = match Self::one_shot_objects(&dfns, device, queue_family) {
            Ok(v) => v,
            Err(e) => {
                unsafe { Self::drop_device(&dfns, device, owns_device) };
                return Err(e);
            }
        };

        let mut memory: vk::PhysicalDeviceMemoryProperties = unsafe { core::mem::zeroed() };
        unsafe { (ifns.get_physical_device_memory_properties)(physical, &mut memory) };

        let (set_layout, pipeline_layout) = match Self::layouts(&dfns, device) {
            Ok(v) => v,
            Err(e) => {
                unsafe {
                    (dfns.destroy_fence)(device, command.fence, core::ptr::null());
                    (dfns.destroy_command_pool)(device, command.pool, core::ptr::null());
                    Self::drop_device(&dfns, device, owns_device);
                }
                return Err(e);
            }
        };

        let mut made: alloc::vec::Vec<Formatted> = alloc::vec::Vec::new();
        let mut failure = None;
        for format in [vk::FORMAT_R8G8B8A8_UNORM, vk::FORMAT_B8G8R8A8_UNORM] {
            match Self::formatted_for(&dfns, device, pipeline_layout, format, dual_src_blend) {
                Ok(f) => made.push(f),
                Err(e) => {
                    failure = Some(e);
                    break;
                }
            }
        }
        let frames = match failure {
            None => Self::frames(&dfns, device, &memory, set_layout, command.pool),
            Some(e) => Err(e),
        };
        let (descriptor_pool, frames) = match frames {
            Ok(v) => v,
            Err(e) => {
                unsafe {
                    for f in &made {
                        f.destroy(&dfns, device);
                    }
                    (dfns.destroy_pipeline_layout)(device, pipeline_layout, core::ptr::null());
                    (dfns.destroy_descriptor_set_layout)(device, set_layout, core::ptr::null());
                    (dfns.destroy_fence)(device, command.fence, core::ptr::null());
                    (dfns.destroy_command_pool)(device, command.pool, core::ptr::null());
                    Self::drop_device(&dfns, device, owns_device);
                }
                return Err(e);
            }
        };
        let mut made = made.into_iter();
        let (rgba, bgra) = match (made.next(), made.next()) {
            (Some(a), Some(b)) => (a, b),
            _ => unreachable!("both formats are built or the loop breaks"),
        };

        Ok(Built {
            device, physical, queue_family, queue, name, device_type, rgba, bgra, command, memory, max_target,
            set_layout, pipeline_layout, descriptor_pool, frames, ifns, dfns, owns_device,
        })
    }

    unsafe fn drop_device(dfns: &DeviceFns, device: vk::Device, owns_device: bool) {
        if owns_device {
            unsafe { (dfns.destroy_device)(device, core::ptr::null()) };
        }
    }


    fn frames(
        d: &DeviceFns,
        device: vk::Device,
        memory: &vk::PhysicalDeviceMemoryProperties,
        set_layout: vk::DescriptorSetLayout,
        pool: vk::CommandPool,
    ) -> Result<(vk::DescriptorPool, [Frame; FRAMES_IN_FLIGHT]), Error> {
        let sizes = [
            vk::DescriptorPoolSize {
                descriptor_type: vk::DESCRIPTOR_TYPE_STORAGE_BUFFER,
                descriptor_count: (6 * FRAMES_IN_FLIGHT) as u32,
            },
            vk::DescriptorPoolSize {
                descriptor_type: vk::DESCRIPTOR_TYPE_UNIFORM_BUFFER,
                descriptor_count: FRAMES_IN_FLIGHT as u32,
            },
        ];
        let pool_info = vk::DescriptorPoolCreateInfo {
            s_type: vk::STRUCTURE_TYPE_DESCRIPTOR_POOL_CREATE_INFO,
            p_next: core::ptr::null(),
            flags: 0,
            max_sets: FRAMES_IN_FLIGHT as u32,
            pool_size_count: sizes.len() as u32,
            p_pool_sizes: sizes.as_ptr(),
        };
        let mut descriptor_pool = vk::NULL_HANDLE;
        check("vkCreateDescriptorPool", unsafe {
            (d.create_descriptor_pool)(device, &pool_info, core::ptr::null(), &mut descriptor_pool)
        })?;

        let layouts = [set_layout; FRAMES_IN_FLIGHT];
        let alloc = vk::DescriptorSetAllocateInfo {
            s_type: vk::STRUCTURE_TYPE_DESCRIPTOR_SET_ALLOCATE_INFO,
            p_next: core::ptr::null(),
            descriptor_pool,
            descriptor_set_count: FRAMES_IN_FLIGHT as u32,
            p_set_layouts: layouts.as_ptr(),
        };
        let mut sets = [vk::NULL_HANDLE; FRAMES_IN_FLIGHT];
        check("vkAllocateDescriptorSets", unsafe {
            (d.allocate_descriptor_sets)(device, &alloc, sets.as_mut_ptr())
        })?;

        let cmd_alloc = vk::CommandBufferAllocateInfo {
            s_type: vk::STRUCTURE_TYPE_COMMAND_BUFFER_ALLOCATE_INFO,
            p_next: core::ptr::null(),
            command_pool: pool,
            level: vk::COMMAND_BUFFER_LEVEL_PRIMARY,
            command_buffer_count: FRAMES_IN_FLIGHT as u32,
        };
        let mut buffers = [core::ptr::null_mut(); FRAMES_IN_FLIGHT];
        check("vkAllocateCommandBuffers", unsafe {
            (d.allocate_command_buffers)(device, &cmd_alloc, buffers.as_mut_ptr())
        })?;

        let mut out: [Option<Frame>; FRAMES_IN_FLIGHT] = [const { None }; FRAMES_IN_FLIGHT];
        for i in 0..FRAMES_IN_FLIGHT {
            let fence_info = vk::FenceCreateInfo {
                s_type: vk::STRUCTURE_TYPE_FENCE_CREATE_INFO,
                p_next: core::ptr::null(),
                flags: 0,
            };
            let mut fence = vk::NULL_HANDLE;
            let made = check("vkCreateFence", unsafe {
                (d.create_fence)(device, &fence_info, core::ptr::null(), &mut fence)
            })
            .and_then(|()| {
                let instances = alloc_buffer(d, device, memory, 64, vk::BUFFER_USAGE_STORAGE_BUFFER_BIT)?;
                let subpixel = match alloc_buffer(
                    d, device, memory,
                    core::mem::size_of::<SubpixelParams>() as u64,
                    vk::BUFFER_USAGE_STORAGE_BUFFER_BIT,
                ) {
                    Ok(b) => b,
                    Err(e) => {
                        free_buffer(d, device, &instances);
                        return Err(e);
                    }
                };
                match alloc_buffer(d, device, memory, 80, vk::BUFFER_USAGE_UNIFORM_BUFFER_BIT) {
                    Ok(projection) => Ok(Frame {
                        command: buffers[i],
                        fence,
                        set: sets[i],
                        instances,
                        subpixel,
                        projection,
                        used: false,
                    }),
                    Err(e) => {
                        free_buffer(d, device, &subpixel);
                        free_buffer(d, device, &instances);
                        Err(e)
                    }
                }
            });
            match made {
                Ok(frame) => out[i] = Some(frame),
                Err(e) => {
                    unsafe {
                        if fence != vk::NULL_HANDLE {
                            (d.destroy_fence)(device, fence, core::ptr::null());
                        }
                        for f in out.iter().flatten() {
                            (d.destroy_fence)(device, f.fence, core::ptr::null());
                        }
                    }
                    for f in out.iter().flatten() {
                        for b in [&f.instances, &f.subpixel, &f.projection] {
                            free_buffer(d, device, b);
                        }
                    }
                    unsafe {
                        (d.destroy_descriptor_pool)(device, descriptor_pool, core::ptr::null());
                    }
                    return Err(e);
                }
            }
        }
        #[expect(clippy::expect_used, reason = "the merge brought `daegun`'s no-panic lint over this crate, and it is the right lint: a font is untrusted input and a panic is a denial of service. This is not that. Every slot was filled by the loop directly above, which returns early on failure, so the `None` arm is unreachable rather than merely unlikely")]
        let frames = out.map(|f| f.expect("every slot was filled"));
        Ok((descriptor_pool, frames))
    }

    // The descriptor and pipeline layouts describe bindings, not attachments, so one pair serves
    // every surface format.
    fn layouts(
        d: &DeviceFns,
        device: vk::Device,
    ) -> Result<(vk::DescriptorSetLayout, vk::PipelineLayout), Error> {
        let mut bindings = [vk::DescriptorSetLayoutBinding {
            binding: 0,
            descriptor_type: vk::DESCRIPTOR_TYPE_STORAGE_BUFFER,
            descriptor_count: 1,
            stage_flags: vk::SHADER_STAGE_VERTEX_BIT | vk::SHADER_STAGE_FRAGMENT_BIT,
            p_immutable_samplers: core::ptr::null(),
        }; 7];
        for (i, b) in bindings.iter_mut().enumerate() {
            b.binding = i as u32;
        }
        bindings[6].descriptor_type = vk::DESCRIPTOR_TYPE_UNIFORM_BUFFER;

        let layout_info = vk::DescriptorSetLayoutCreateInfo {
            s_type: vk::STRUCTURE_TYPE_DESCRIPTOR_SET_LAYOUT_CREATE_INFO,
            p_next: core::ptr::null(),
            flags: 0,
            binding_count: bindings.len() as u32,
            p_bindings: bindings.as_ptr(),
        };
        let mut set_layout = vk::NULL_HANDLE;
        check("vkCreateDescriptorSetLayout", unsafe {
            (d.create_descriptor_set_layout)(device, &layout_info, core::ptr::null(), &mut set_layout)
        })?;

        let pl_info = vk::PipelineLayoutCreateInfo {
            s_type: vk::STRUCTURE_TYPE_PIPELINE_LAYOUT_CREATE_INFO,
            p_next: core::ptr::null(),
            flags: 0,
            set_layout_count: 1,
            p_set_layouts: &set_layout,
            push_constant_range_count: 0,
            p_push_constant_ranges: core::ptr::null(),
        };
        let mut pipeline_layout = vk::NULL_HANDLE;
        let r = unsafe {
            (d.create_pipeline_layout)(device, &pl_info, core::ptr::null(), &mut pipeline_layout)
        };
        if let Err(e) = check("vkCreatePipelineLayout", r) {
            unsafe { (d.destroy_descriptor_set_layout)(device, set_layout, core::ptr::null()) };
            return Err(e);
        }

        Ok((set_layout, pipeline_layout))
    }

    // A pipeline is tied to its render pass only through attachment format and sample count, so one
    // pair per format covers every pass compatible with it.
    fn pipelines_for(
        d: &DeviceFns,
        device: vk::Device,
        render_pass: vk::RenderPass,
        pipeline_layout: vk::PipelineLayout,
        dual_src_blend: bool,
    ) -> Result<(vk::Pipeline, Option<vk::Pipeline>), Error> {
        let build = |mode: Mode| -> Result<vk::Pipeline, Error> {
            Self::one_pipeline(d, device, render_pass, pipeline_layout, mode)
        };
        let grayscale = build(Mode::Grayscale)?;
        let subpixel = if dual_src_blend {
            match build(Mode::Subpixel) {
                Ok(p) => Some(p),
                Err(e) => {
                    unsafe { (d.destroy_pipeline)(device, grayscale, core::ptr::null()) };
                    return Err(e);
                }
            }
        } else {
            None
        };
        Ok((grayscale, subpixel))
    }

    fn formatted_for(
        d: &DeviceFns,
        device: vk::Device,
        pipeline_layout: vk::PipelineLayout,
        format: i32,
        dual_src_blend: bool,
    ) -> Result<Formatted, Error> {
        let clear_pass = Self::render_pass_for(d, device, format, vk::ATTACHMENT_LOAD_OP_CLEAR)?;
        let load_pass = match Self::render_pass_for(d, device, format, vk::ATTACHMENT_LOAD_OP_LOAD) {
            Ok(p) => p,
            Err(e) => {
                unsafe { (d.destroy_render_pass)(device, clear_pass, core::ptr::null()) };
                return Err(e);
            }
        };
        match Self::pipelines_for(d, device, clear_pass, pipeline_layout, dual_src_blend) {
            Ok((grayscale, subpixel)) => {
                Ok(Formatted { clear_pass, load_pass, grayscale, subpixel })
            }
            Err(e) => {
                unsafe {
                    (d.destroy_render_pass)(device, load_pass, core::ptr::null());
                    (d.destroy_render_pass)(device, clear_pass, core::ptr::null());
                }
                Err(e)
            }
        }
    }

    fn one_pipeline(
        d: &DeviceFns,
        device: vk::Device,
        render_pass: vk::RenderPass,
        layout: vk::PipelineLayout,
        mode: Mode,
    ) -> Result<vk::Pipeline, Error> {
        let (vert_words, frag_words) = shader_words(mode)?;
        let make = |words: &[u32]| -> Result<vk::ShaderModule, Error> {
            let info = vk::ShaderModuleCreateInfo {
                s_type: vk::STRUCTURE_TYPE_SHADER_MODULE_CREATE_INFO,
                p_next: core::ptr::null(),
                flags: 0,
                code_size: core::mem::size_of_val(words),
                p_code: words.as_ptr(),
            };
            let mut m = vk::NULL_HANDLE;
            check("vkCreateShaderModule", unsafe {
                (d.create_shader_module)(device, &info, core::ptr::null(), &mut m)
            })?;
            Ok(m)
        };
        let vert = make(vert_words)?;
        let frag = match make(frag_words) {
            Ok(m) => m,
            Err(e) => {
                unsafe { (d.destroy_shader_module)(device, vert, core::ptr::null()) };
                return Err(e);
            }
        };

        let stages = [
            vk::PipelineShaderStageCreateInfo {
                s_type: vk::STRUCTURE_TYPE_PIPELINE_SHADER_STAGE_CREATE_INFO,
                p_next: core::ptr::null(),
                flags: 0,
                stage: vk::SHADER_STAGE_VERTEX_BIT,
                module: vert,
                p_name: c"main".as_ptr(),
                p_specialization_info: core::ptr::null(),
            },
            vk::PipelineShaderStageCreateInfo {
                s_type: vk::STRUCTURE_TYPE_PIPELINE_SHADER_STAGE_CREATE_INFO,
                p_next: core::ptr::null(),
                flags: 0,
                stage: vk::SHADER_STAGE_FRAGMENT_BIT,
                module: frag,
                p_name: c"main".as_ptr(),
                p_specialization_info: core::ptr::null(),
            },
        ];

        let vertex_input = vk::PipelineVertexInputStateCreateInfo {
            s_type: vk::STRUCTURE_TYPE_PIPELINE_VERTEX_INPUT_STATE_CREATE_INFO,
            p_next: core::ptr::null(),
            flags: 0,
            vertex_binding_description_count: 0,
            p_vertex_binding_descriptions: core::ptr::null(),
            vertex_attribute_description_count: 0,
            p_vertex_attribute_descriptions: core::ptr::null(),
        };
        let assembly = vk::PipelineInputAssemblyStateCreateInfo {
            s_type: vk::STRUCTURE_TYPE_PIPELINE_INPUT_ASSEMBLY_STATE_CREATE_INFO,
            p_next: core::ptr::null(),
            flags: 0,
            topology: vk::PRIMITIVE_TOPOLOGY_TRIANGLE_STRIP,
            primitive_restart_enable: vk::FALSE,
        };
        let viewport = vk::PipelineViewportStateCreateInfo {
            s_type: vk::STRUCTURE_TYPE_PIPELINE_VIEWPORT_STATE_CREATE_INFO,
            p_next: core::ptr::null(),
            flags: 0,
            viewport_count: 1,
            p_viewports: core::ptr::null(),
            scissor_count: 1,
            p_scissors: core::ptr::null(),
        };
        let raster = vk::PipelineRasterizationStateCreateInfo {
            s_type: vk::STRUCTURE_TYPE_PIPELINE_RASTERIZATION_STATE_CREATE_INFO,
            p_next: core::ptr::null(),
            flags: 0,
            depth_clamp_enable: vk::FALSE,
            rasterizer_discard_enable: vk::FALSE,
            polygon_mode: vk::POLYGON_MODE_FILL,
            cull_mode: vk::CULL_MODE_NONE,
            front_face: vk::FRONT_FACE_COUNTER_CLOCKWISE,
            depth_bias_enable: vk::FALSE,
            depth_bias_constant_factor: 0.0,
            depth_bias_clamp: 0.0,
            depth_bias_slope_factor: 0.0,
            line_width: 1.0,
        };
        let multisample = vk::PipelineMultisampleStateCreateInfo {
            s_type: vk::STRUCTURE_TYPE_PIPELINE_MULTISAMPLE_STATE_CREATE_INFO,
            p_next: core::ptr::null(),
            flags: 0,
            rasterization_samples: vk::SAMPLE_COUNT_1_BIT,
            sample_shading_enable: vk::FALSE,
            min_sample_shading: 0.0,
            p_sample_mask: core::ptr::null(),
            alpha_to_coverage_enable: vk::FALSE,
            alpha_to_one_enable: vk::FALSE,
        };

        let blend_attachment = match mode {
            Mode::Grayscale => vk::PipelineColorBlendAttachmentState {
                blend_enable: vk::FALSE,
                src_color_blend_factor: vk::BLEND_FACTOR_ONE,
                dst_color_blend_factor: vk::BLEND_FACTOR_ZERO,
                color_blend_op: vk::BLEND_OP_ADD,
                src_alpha_blend_factor: vk::BLEND_FACTOR_ONE,
                dst_alpha_blend_factor: vk::BLEND_FACTOR_ZERO,
                alpha_blend_op: vk::BLEND_OP_ADD,
                color_write_mask: vk::COLOR_COMPONENT_RGBA,
            },
            Mode::Subpixel => vk::PipelineColorBlendAttachmentState {
                blend_enable: vk::TRUE,
                src_color_blend_factor: vk::BLEND_FACTOR_SRC1_COLOR,
                dst_color_blend_factor: vk::BLEND_FACTOR_ONE_MINUS_SRC1_COLOR,
                color_blend_op: vk::BLEND_OP_ADD,
                src_alpha_blend_factor: vk::BLEND_FACTOR_SRC1_ALPHA,
                dst_alpha_blend_factor: vk::BLEND_FACTOR_ONE_MINUS_SRC1_ALPHA,
                alpha_blend_op: vk::BLEND_OP_ADD,
                color_write_mask: vk::COLOR_COMPONENT_RGBA,
            },
        };
        let blend = vk::PipelineColorBlendStateCreateInfo {
            s_type: vk::STRUCTURE_TYPE_PIPELINE_COLOR_BLEND_STATE_CREATE_INFO,
            p_next: core::ptr::null(),
            flags: 0,
            logic_op_enable: vk::FALSE,
            logic_op: 0,
            attachment_count: 1,
            p_attachments: &blend_attachment,
            blend_constants: [0.0; 4],
        };
        let dynamic_states = [vk::DYNAMIC_STATE_VIEWPORT, vk::DYNAMIC_STATE_SCISSOR];
        let dynamic = vk::PipelineDynamicStateCreateInfo {
            s_type: vk::STRUCTURE_TYPE_PIPELINE_DYNAMIC_STATE_CREATE_INFO,
            p_next: core::ptr::null(),
            flags: 0,
            dynamic_state_count: dynamic_states.len() as u32,
            p_dynamic_states: dynamic_states.as_ptr(),
        };

        let info = vk::GraphicsPipelineCreateInfo {
            s_type: vk::STRUCTURE_TYPE_GRAPHICS_PIPELINE_CREATE_INFO,
            p_next: core::ptr::null(),
            flags: 0,
            stage_count: stages.len() as u32,
            p_stages: stages.as_ptr(),
            p_vertex_input_state: &vertex_input,
            p_input_assembly_state: &assembly,
            p_tessellation_state: core::ptr::null(),
            p_viewport_state: &viewport,
            p_rasterization_state: &raster,
            p_multisample_state: &multisample,
            p_depth_stencil_state: core::ptr::null(),
            p_color_blend_state: &blend,
            p_dynamic_state: &dynamic,
            layout,
            render_pass,
            subpass: 0,
            base_pipeline_handle: vk::NULL_HANDLE,
            base_pipeline_index: -1,
        };
        let mut pipeline = vk::NULL_HANDLE;
        let r = unsafe {
            (d.create_graphics_pipelines)(
                device, vk::NULL_HANDLE, 1, &info, core::ptr::null(), &mut pipeline,
            )
        };
        unsafe {
            (d.destroy_shader_module)(device, frag, core::ptr::null());
            (d.destroy_shader_module)(device, vert, core::ptr::null());
        }
        check("vkCreateGraphicsPipelines", r)?;
        Ok(pipeline)
    }

    fn render_pass_for(
        dfns: &DeviceFns,
        device: vk::Device,
        format: i32,
        load_op: i32,
    ) -> Result<vk::RenderPass, Error> {
        let attachment = vk::AttachmentDescription {
            flags: 0,
            format,
            samples: vk::SAMPLE_COUNT_1_BIT,
            load_op,
            store_op: vk::ATTACHMENT_STORE_OP_STORE,
            stencil_load_op: vk::ATTACHMENT_LOAD_OP_DONT_CARE,
            stencil_store_op: vk::ATTACHMENT_STORE_OP_DONT_CARE,
            initial_layout: if load_op == vk::ATTACHMENT_LOAD_OP_CLEAR {
                vk::IMAGE_LAYOUT_UNDEFINED
            } else {
                vk::IMAGE_LAYOUT_COLOR_ATTACHMENT_OPTIMAL
            },
            final_layout: vk::IMAGE_LAYOUT_COLOR_ATTACHMENT_OPTIMAL,
        };
        let color = vk::AttachmentReference {
            attachment: 0,
            layout: vk::IMAGE_LAYOUT_COLOR_ATTACHMENT_OPTIMAL,
        };
        let subpass = vk::SubpassDescription {
            flags: 0,
            pipeline_bind_point: vk::PIPELINE_BIND_POINT_GRAPHICS,
            input_attachment_count: 0,
            p_input_attachments: core::ptr::null(),
            color_attachment_count: 1,
            p_color_attachments: &color,
            p_resolve_attachments: core::ptr::null(),
            p_depth_stencil_attachment: core::ptr::null(),
            preserve_attachment_count: 0,
            p_preserve_attachments: core::ptr::null(),
        };
        let rp_info = vk::RenderPassCreateInfo {
            s_type: vk::STRUCTURE_TYPE_RENDER_PASS_CREATE_INFO,
            p_next: core::ptr::null(),
            flags: 0,
            attachment_count: 1,
            p_attachments: &attachment,
            subpass_count: 1,
            p_subpasses: &subpass,
            dependency_count: 0,
            p_dependencies: core::ptr::null(),
        };
        let mut render_pass = vk::NULL_HANDLE;
        check("vkCreateRenderPass", unsafe {
            (dfns.create_render_pass)(device, &rp_info, core::ptr::null(), &mut render_pass)
        })?;

        Ok(render_pass)
    }

    fn one_shot_objects(
        dfns: &DeviceFns,
        device: vk::Device,
        queue_family: u32,
    ) -> Result<OneShot, Error> {
        let pool_info = vk::CommandPoolCreateInfo {
            s_type: vk::STRUCTURE_TYPE_COMMAND_POOL_CREATE_INFO,
            p_next: core::ptr::null(),
            flags: vk::COMMAND_POOL_CREATE_RESET_COMMAND_BUFFER_BIT,
            queue_family_index: queue_family,
        };
        let mut pool = vk::NULL_HANDLE;
        check("vkCreateCommandPool", unsafe {
            (dfns.create_command_pool)(device, &pool_info, core::ptr::null(), &mut pool)
        })?;

        let alloc = vk::CommandBufferAllocateInfo {
            s_type: vk::STRUCTURE_TYPE_COMMAND_BUFFER_ALLOCATE_INFO,
            p_next: core::ptr::null(),
            command_pool: pool,
            level: vk::COMMAND_BUFFER_LEVEL_PRIMARY,
            command_buffer_count: 1,
        };
        let mut buffer: vk::CommandBuffer = core::ptr::null_mut();
        let mut fence = vk::NULL_HANDLE;
        let fence_info = vk::FenceCreateInfo {
            s_type: vk::STRUCTURE_TYPE_FENCE_CREATE_INFO,
            p_next: core::ptr::null(),
            flags: 0,
        };
        let r = unsafe { (dfns.allocate_command_buffers)(device, &alloc, &mut buffer) }
            .max(unsafe { (dfns.create_fence)(device, &fence_info, core::ptr::null(), &mut fence) });
        if let Err(e) = check("vkAllocateCommandBuffers or vkCreateFence", r) {
            unsafe { (dfns.destroy_command_pool)(device, pool, core::ptr::null()) };
            return Err(e);
        }

        Ok(OneShot { pool, buffer, fence })
    }

    fn suitable(ifns: &InstanceFns, pd: vk::PhysicalDevice) -> Option<Suitable> {
        let mut features = vk::PhysicalDeviceFeatures::default();
        unsafe { (ifns.get_physical_device_features)(pd, &mut features) };
        let dual_src_blend = features.dual_src_blend == vk::TRUE && !dual_src_disabled();

        let families = enumerate_void(|count, data| {
            unsafe { (ifns.get_physical_device_queue_family_properties)(pd, count, data) }
        });
        let family = families
            .iter()
            .position(|f| f.queue_flags & vk::QUEUE_GRAPHICS_BIT != 0 && f.queue_count > 0)?
            as u32;

        let mut props: vk::PhysicalDeviceProperties = unsafe { core::mem::zeroed() };
        unsafe { (ifns.get_physical_device_properties)(pd, &mut props) };
        Some(Suitable {
            pd,
            family,
            name: vk::c_array_to_string(&props.device_name),
            device_type: props.device_type,
            dual_src_blend,
            max_target: [
                props.limits.max_framebuffer_width().min(props.limits.max_image_dimension_2d()),
                props.limits.max_framebuffer_height().min(props.limits.max_image_dimension_2d()),
            ],
        })
    }

    pub fn device_name(&self) -> String {
        self.device_name.clone()
    }

    pub fn supports_subpixel(&self) -> bool {
        self.rgba.subpixel.is_some()
    }

    pub fn profile(&self) -> crate::daerizer::draw::DeviceProfile {
        crate::daerizer::draw::DeviceProfile::from_vulkan(self.device_type, self.device_name.clone())
    }

    fn one_shot<F>(&self, f: F) -> Result<(), Error>
    where
        F: FnOnce(vk::CommandBuffer),
    {
        let cmd = self.command.borrow();
        let begin = vk::CommandBufferBeginInfo {
            s_type: vk::STRUCTURE_TYPE_COMMAND_BUFFER_BEGIN_INFO,
            p_next: core::ptr::null(),
            flags: vk::COMMAND_BUFFER_USAGE_ONE_TIME_SUBMIT_BIT,
            p_inheritance_info: core::ptr::null(),
        };
        unsafe {
            check("vkBeginCommandBuffer", (self.dfns.begin_command_buffer)(cmd.buffer, &begin))?;
        }
        f(cmd.buffer);
        unsafe {
            check("vkEndCommandBuffer", (self.dfns.end_command_buffer)(cmd.buffer))?;
        }

        let submit = vk::SubmitInfo {
            s_type: vk::STRUCTURE_TYPE_SUBMIT_INFO,
            p_next: core::ptr::null(),
            wait_semaphore_count: 0,
            p_wait_semaphores: core::ptr::null(),
            p_wait_dst_stage_mask: core::ptr::null(),
            command_buffer_count: 1,
            p_command_buffers: &cmd.buffer,
            signal_semaphore_count: 0,
            p_signal_semaphores: core::ptr::null(),
        };
        unsafe {
            check("vkResetFences", (self.dfns.reset_fences)(self.device, 1, &cmd.fence))?;
            check("vkQueueSubmit", (self.dfns.queue_submit)(self.queue, 1, &submit, cmd.fence))?;
            check(
                "vkWaitForFences",
                (self.dfns.wait_for_fences)(self.device, 1, &cmd.fence, vk::TRUE, vk::TIMEOUT_INFINITE),
            )?;
        }
        Ok(())
    }

    fn formatted(&self, format: SurfaceFormat) -> &Formatted {
        match format {
            SurfaceFormat::Rgba8Unorm => &self.rgba,
            SurfaceFormat::Bgra8Unorm => &self.bgra,
        }
    }

    // `image` must be live on this device with color-attachment usage and the given format. daegun
    // builds a view and framebuffer over it and destroys only those. A clearing draw takes any
    // layout; loading needs `COLOR_ATTACHMENT_OPTIMAL`, and daegun leaves it there, so transitioning
    // a swapchain image before presenting stays the caller's.
    pub unsafe fn target_from_image(
        &self,
        image: vk::Image,
        width: u32,
        height: u32,
        format: SurfaceFormat,
    ) -> Result<Target<'_>, Error> {
        if image == vk::NULL_HANDLE || width == 0 || height == 0 {
            return Err(Error::BadTarget);
        }
        let vk_format = match format {
            SurfaceFormat::Rgba8Unorm => vk::FORMAT_R8G8B8A8_UNORM,
            SurfaceFormat::Bgra8Unorm => vk::FORMAT_B8G8R8A8_UNORM,
        };
        let d = &self.dfns;

        let mut view = vk::NULL_HANDLE;
        let view_info = vk::ImageViewCreateInfo {
            s_type: vk::STRUCTURE_TYPE_IMAGE_VIEW_CREATE_INFO,
            p_next: core::ptr::null(),
            flags: 0,
            image,
            view_type: vk::IMAGE_VIEW_TYPE_2D,
            format: vk_format,
            components: vk::ComponentMapping {
                r: vk::COMPONENT_SWIZZLE_IDENTITY,
                g: vk::COMPONENT_SWIZZLE_IDENTITY,
                b: vk::COMPONENT_SWIZZLE_IDENTITY,
                a: vk::COMPONENT_SWIZZLE_IDENTITY,
            },
            subresource_range: COLOR_RANGE,
        };
        check("vkCreateImageView", unsafe {
            (d.create_image_view)(self.device, &view_info, core::ptr::null(), &mut view)
        })?;

        let mut framebuffer = vk::NULL_HANDLE;
        let fb_info = vk::FramebufferCreateInfo {
            s_type: vk::STRUCTURE_TYPE_FRAMEBUFFER_CREATE_INFO,
            p_next: core::ptr::null(),
            flags: 0,
            render_pass: self.formatted(format).clear_pass,
            attachment_count: 1,
            p_attachments: &view,
            width,
            height,
            layers: 1,
        };
        if let Err(e) = check("vkCreateFramebuffer", unsafe {
            (d.create_framebuffer)(self.device, &fb_info, core::ptr::null(), &mut framebuffer)
        }) {
            unsafe { (d.destroy_image_view)(self.device, view, core::ptr::null()) };
            return Err(e);
        }

        Ok(Target {
            borrowed: true,
            format,
            clear: Some(TRANSPARENT),
            image,
            image_memory: vk::NULL_HANDLE,
            view,
            framebuffer,
            staging: vk::NULL_HANDLE,
            staging_memory: vk::NULL_HANDLE,
            mapped: core::ptr::null_mut(),
            width,
            height,
            device: self.device,
            dfns: self.dfns,
            pending: None,
            renderer: core::marker::PhantomData,
        })
    }

    pub fn target(&self, width: u32, height: u32) -> Result<Target<'_>, Error> {
        self.target_with_format(width, height, SurfaceFormat::Rgba8Unorm)
    }

    // An offscreen target in the caller's byte order, for compositing into a surface that is not
    // daegun's without a swizzle on the way out.
    pub fn target_with_format(
        &self,
        width: u32,
        height: u32,
        format: SurfaceFormat,
    ) -> Result<Target<'_>, Error> {
        let vk_format = match format {
            SurfaceFormat::Rgba8Unorm => vk::FORMAT_R8G8B8A8_UNORM,
            SurfaceFormat::Bgra8Unorm => vk::FORMAT_B8G8R8A8_UNORM,
        };
        if width == 0
            || height == 0
            || width > self.max_target[0]
            || height > self.max_target[1]
        {
            return Err(Error::BadTarget);
        }
        let d = &self.dfns;
        let image_info = vk::ImageCreateInfo {
            s_type: vk::STRUCTURE_TYPE_IMAGE_CREATE_INFO,
            p_next: core::ptr::null(),
            flags: 0,
            image_type: vk::IMAGE_TYPE_2D,
            format: vk_format,
            extent: vk::Extent3D { width, height, depth: 1 },
            mip_levels: 1,
            array_layers: 1,
            samples: vk::SAMPLE_COUNT_1_BIT,
            tiling: vk::IMAGE_TILING_OPTIMAL,
            usage: vk::IMAGE_USAGE_COLOR_ATTACHMENT_BIT
                | vk::IMAGE_USAGE_TRANSFER_SRC_BIT
                | vk::IMAGE_USAGE_TRANSFER_DST_BIT,
            sharing_mode: vk::SHARING_MODE_EXCLUSIVE,
            queue_family_index_count: 0,
            p_queue_family_indices: core::ptr::null(),
            initial_layout: vk::IMAGE_LAYOUT_UNDEFINED,
        };
        let mut image = vk::NULL_HANDLE;
        check("vkCreateImage", unsafe {
            (d.create_image)(self.device, &image_info, core::ptr::null(), &mut image)
        })?;

        let (mut image_memory, mut view, mut framebuffer) =
            (vk::NULL_HANDLE, vk::NULL_HANDLE, vk::NULL_HANDLE);
        let (mut staging, mut staging_memory) = (vk::NULL_HANDLE, vk::NULL_HANDLE);
        macro_rules! bail {
            ($e:expr) => {{
                undo_target(d, self.device, framebuffer, view, image, image_memory, staging, staging_memory);
                return Err($e);
            }};
        }

        let mut req = vk::MemoryRequirements::default();
        unsafe { (d.get_image_memory_requirements)(self.device, image, &mut req) };
        image_memory = match self.allocate(&req, vk::MEMORY_PROPERTY_DEVICE_LOCAL_BIT) {
            Ok(m) => m,
            Err(e) => bail!(e),
        };
        if let Err(e) = check("vkBindImageMemory", unsafe {
            (d.bind_image_memory)(self.device, image, image_memory, 0)
        }) {
            bail!(e);
        }

        let view_info = vk::ImageViewCreateInfo {
            s_type: vk::STRUCTURE_TYPE_IMAGE_VIEW_CREATE_INFO,
            p_next: core::ptr::null(),
            flags: 0,
            image,
            view_type: vk::IMAGE_VIEW_TYPE_2D,
            format: vk_format,
            components: vk::ComponentMapping {
                r: vk::COMPONENT_SWIZZLE_IDENTITY,
                g: vk::COMPONENT_SWIZZLE_IDENTITY,
                b: vk::COMPONENT_SWIZZLE_IDENTITY,
                a: vk::COMPONENT_SWIZZLE_IDENTITY,
            },
            subresource_range: COLOR_RANGE,
        };
        if let Err(e) = check("vkCreateImageView", unsafe {
            (d.create_image_view)(self.device, &view_info, core::ptr::null(), &mut view)
        }) {
            bail!(e);
        }

        let fb_info = vk::FramebufferCreateInfo {
            s_type: vk::STRUCTURE_TYPE_FRAMEBUFFER_CREATE_INFO,
            p_next: core::ptr::null(),
            flags: 0,
            render_pass: self.formatted(format).clear_pass,
            attachment_count: 1,
            p_attachments: &view,
            width,
            height,
            layers: 1,
        };
        if let Err(e) = check("vkCreateFramebuffer", unsafe {
            (d.create_framebuffer)(self.device, &fb_info, core::ptr::null(), &mut framebuffer)
        }) {
            bail!(e);
        }

        let bytes = width as u64 * height as u64 * 4;
        let buf_info = vk::BufferCreateInfo {
            s_type: vk::STRUCTURE_TYPE_BUFFER_CREATE_INFO,
            p_next: core::ptr::null(),
            flags: 0,
            size: bytes,
            usage: vk::BUFFER_USAGE_TRANSFER_DST_BIT,
            sharing_mode: vk::SHARING_MODE_EXCLUSIVE,
            queue_family_index_count: 0,
            p_queue_family_indices: core::ptr::null(),
        };
        if let Err(e) = check("vkCreateBuffer", unsafe {
            (d.create_buffer)(self.device, &buf_info, core::ptr::null(), &mut staging)
        }) {
            bail!(e);
        }
        let mut breq = vk::MemoryRequirements::default();
        unsafe { (d.get_buffer_memory_requirements)(self.device, staging, &mut breq) };
        staging_memory = match self.allocate(
            &breq,
            vk::MEMORY_PROPERTY_HOST_VISIBLE_BIT | vk::MEMORY_PROPERTY_HOST_COHERENT_BIT,
        ) {
            Ok(m) => m,
            Err(e) => bail!(e),
        };
        if let Err(e) = check("vkBindBufferMemory", unsafe {
            (d.bind_buffer_memory)(self.device, staging, staging_memory, 0)
        }) {
            bail!(e);
        }

        let mut mapped: *mut core::ffi::c_void = core::ptr::null_mut();
        if let Err(e) = check("vkMapMemory", unsafe {
            (d.map_memory)(self.device, staging_memory, 0, bytes, 0, &mut mapped)
        }) {
            bail!(e);
        }
        unsafe { core::ptr::write_bytes(mapped.cast::<u8>(), 0, bytes as usize) };

        let target = Target {
            borrowed: false,
            format,
            clear: Some(TRANSPARENT),
            image,
            image_memory,
            view,
            framebuffer,
            staging,
            staging_memory,
            mapped: mapped.cast::<u8>(),
            width,
            height,
            device: self.device,
            dfns: self.dfns,
            pending: None,
            renderer: core::marker::PhantomData,
        };

        self.one_shot(|cmd| {
            self.barrier(
                cmd,
                image,
                vk::IMAGE_LAYOUT_UNDEFINED,
                vk::IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL,
                0,
                vk::ACCESS_TRANSFER_WRITE_BIT,
                vk::PIPELINE_STAGE_TOP_OF_PIPE_BIT,
                vk::PIPELINE_STAGE_TRANSFER_BIT,
            );
            let clear = vk::ClearColorValue { float32: [0.0, 0.0, 0.0, 0.0] };
            unsafe {
                (d.cmd_clear_color_image)(
                    cmd,
                    image,
                    vk::IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL,
                    &clear,
                    1,
                    &COLOR_RANGE,
                );
            }
            self.barrier(
                cmd,
                image,
                vk::IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL,
                vk::IMAGE_LAYOUT_COLOR_ATTACHMENT_OPTIMAL,
                vk::ACCESS_TRANSFER_WRITE_BIT,
                vk::ACCESS_COLOR_ATTACHMENT_WRITE_BIT,
                vk::PIPELINE_STAGE_TRANSFER_BIT,
                vk::PIPELINE_STAGE_COLOR_ATTACHMENT_OUTPUT_BIT,
            );
        })?;

        Ok(target)
    }

    pub fn draw(
        &self,
        target: &mut Target<'_>,
        geometry: &Geometry<'_>,
        instances: &[GlyphInstance],
        subpixel: &SubpixelParams,
        mode: Mode,
    ) -> Result<(), Error> {
        let projection = ortho(target.width, target.height);
        self.draw_with(target, geometry, instances, subpixel, mode, &projection)
    }

    pub fn draw_with(
        &self,
        target: &mut Target<'_>,
        geometry: &Geometry<'_>,
        instances: &[GlyphInstance],
        subpixel: &SubpixelParams,
        mode: Mode,
        projection: &[f32; 16],
    ) -> Result<(), Error> {
        if target.width == 0
            || target.height == 0
            || target.device != self.device
            || geometry.device != self.device
        {
            return Err(Error::BadTarget);
        }

        let uniform = super::draw_uniform(
            projection,
            &ortho(target.width, target.height),
            target.height as f32,
        );
        let d = &self.dfns;

        let index = self.next_frame.get() % FRAMES_IN_FLIGHT;
        self.next_frame.set(self.next_frame.get().wrapping_add(1));
        let mut frames = self.frames.borrow_mut();
        let frame = &mut frames[index];
        if frame.used {
            check("vkWaitForFences", unsafe {
                (d.wait_for_fences)(self.device, 1, &frame.fence, vk::TRUE, vk::TIMEOUT_INFINITE)
            })?;
        }

        let want = core::mem::size_of_val(instances) as u64;
        if frame.instances.size < want {
            let bigger = alloc_buffer(
                d, self.device, &self.memory, want, vk::BUFFER_USAGE_STORAGE_BUFFER_BIT,
            )?;
            free_buffer(d, self.device, &frame.instances);
            frame.instances = bigger;
        }
        frame.instances.write(instances);
        frame.subpixel.write(core::slice::from_ref(subpixel));
        frame.projection.write(&uniform);

        let infos = [
            buffer_info(&geometry.curves),
            buffer_info(&geometry.band_curves),
            buffer_info(&geometry.bands),
            buffer_info(&frame.instances),
            buffer_info(&frame.subpixel),
            buffer_info(&geometry.hulls),
            buffer_info(&frame.projection),
        ];
        let mut writes = [vk::WriteDescriptorSet {
            s_type: vk::STRUCTURE_TYPE_WRITE_DESCRIPTOR_SET,
            p_next: core::ptr::null(),
            dst_set: frame.set,
            dst_binding: 0,
            dst_array_element: 0,
            descriptor_count: 1,
            descriptor_type: vk::DESCRIPTOR_TYPE_STORAGE_BUFFER,
            p_image_info: core::ptr::null(),
            p_buffer_info: core::ptr::null(),
            p_texel_buffer_view: core::ptr::null(),
        }; 7];
        for (i, w) in writes.iter_mut().enumerate() {
            w.dst_binding = i as u32;
            w.p_buffer_info = &infos[i];
        }
        writes[6].descriptor_type = vk::DESCRIPTOR_TYPE_UNIFORM_BUFFER;
        unsafe {
            (d.update_descriptor_sets)(self.device, writes.len() as u32, writes.as_ptr(), 0, core::ptr::null());
        }

        let pipeline = match mode {
            Mode::Grayscale => self.formatted(target.format).grayscale,
            Mode::Subpixel => match self.formatted(target.format).subpixel {
                Some(p) => p,
                None => {
                    return Err(Error::Unsupported(
                        "this device does not support dualSrcBlend, so it has no subpixel pipeline",
                    ))
                }
            },
        };
        let area = vk::Rect2D {
            offset: vk::Offset2D { x: 0, y: 0 },
            extent: vk::Extent2D { width: target.width, height: target.height },
        };
        let clear = vk::ClearValue {
            color: vk::ClearColorValue {
                float32: match target.clear {
                    Some(c) => [c.r, c.g, c.b, c.a].map(|v| f32::from(v) / 255.0),
                    None => [0.0; 4],
                },
            },
        };
        let pass = vk::RenderPassBeginInfo {
            s_type: vk::STRUCTURE_TYPE_RENDER_PASS_BEGIN_INFO,
            p_next: core::ptr::null(),
            render_pass: self.formatted(target.format).pass_for(target.clear),
            framebuffer: target.framebuffer,
            render_area: area,
            clear_value_count: 1,
            p_clear_values: &clear,
        };
        let viewport = vk::Viewport {
            x: 0.0,
            y: 0.0,
            width: target.width as f32,
            height: target.height as f32,
            min_depth: 0.0,
            max_depth: 1.0,
        };
        let begin = vk::CommandBufferBeginInfo {
            s_type: vk::STRUCTURE_TYPE_COMMAND_BUFFER_BEGIN_INFO,
            p_next: core::ptr::null(),
            flags: vk::COMMAND_BUFFER_USAGE_ONE_TIME_SUBMIT_BIT,
            p_inheritance_info: core::ptr::null(),
        };
        let cmd = frame.command;
        unsafe {
            check("vkResetCommandBuffer", (d.reset_command_buffer)(cmd, 0))?;
            check("vkBeginCommandBuffer", (d.begin_command_buffer)(cmd, &begin))?;
            (d.cmd_begin_render_pass)(cmd, &pass, vk::SUBPASS_CONTENTS_INLINE);
            (d.cmd_bind_pipeline)(cmd, vk::PIPELINE_BIND_POINT_GRAPHICS, pipeline);
            (d.cmd_bind_descriptor_sets)(
                cmd, vk::PIPELINE_BIND_POINT_GRAPHICS, self.pipeline_layout, 0, 1, &frame.set, 0,
                core::ptr::null(),
            );
            (d.cmd_set_viewport)(cmd, 0, 1, &viewport);
            (d.cmd_set_scissor)(cmd, 0, 1, &area);
            (d.cmd_draw)(cmd, super::HULL_VERTICES as u32, instances.len() as u32, 0, 0);
            (d.cmd_end_render_pass)(cmd);
            check("vkEndCommandBuffer", (d.end_command_buffer)(cmd))?;
        }

        let submit = vk::SubmitInfo {
            s_type: vk::STRUCTURE_TYPE_SUBMIT_INFO,
            p_next: core::ptr::null(),
            wait_semaphore_count: 0,
            p_wait_semaphores: core::ptr::null(),
            p_wait_dst_stage_mask: core::ptr::null(),
            command_buffer_count: 1,
            p_command_buffers: &cmd,
            signal_semaphore_count: 0,
            p_signal_semaphores: core::ptr::null(),
        };
        unsafe {
            check("vkResetFences", (d.reset_fences)(self.device, 1, &frame.fence))?;
            if let Err(e) = check("vkQueueSubmit", (d.queue_submit)(self.queue, 1, &submit, frame.fence)) {
                frame.used = false;
                return Err(e);
            }
        }
        frame.used = true;
        target.pending = Some(index);
        Ok(())
    }

    pub fn wait(&self, target: &mut Target<'_>) -> Result<(), Error> {
        let Some(index) = target.pending.take() else { return Ok(()) };
        let frames = self.frames.borrow();
        let frame = &frames[index];
        if !frame.used {
            return Ok(());
        }
        check("vkWaitForFences", unsafe {
            (self.dfns.wait_for_fences)(
                self.device, 1, &frame.fence, vk::TRUE, vk::TIMEOUT_INFINITE,
            )
        })
    }

    pub fn read_pixels<'t>(&self, target: &'t mut Target<'_>) -> Result<&'t [u8], Error> {
        if target.device != self.device || target.width == 0 || target.height == 0 {
            return Err(Error::BadTarget);
        }
        // A borrowed image has no staging buffer to copy into, and a caller rendering into its own
        // swapchain does not want the copy anyway.
        if target.borrowed {
            return Err(Error::BadTarget);
        }
        self.wait(target)?;
        let d = &self.dfns;
        let copy = vk::BufferImageCopy {
            buffer_offset: 0,
            buffer_row_length: 0,
            buffer_image_height: 0,
            image_subresource: vk::ImageSubresourceLayers {
                aspect_mask: vk::IMAGE_ASPECT_COLOR_BIT,
                mip_level: 0,
                base_array_layer: 0,
                layer_count: 1,
            },
            image_offset: vk::Offset3D { x: 0, y: 0, z: 0 },
            image_extent: vk::Extent3D { width: target.width, height: target.height, depth: 1 },
        };
        self.one_shot(|cmd| {
            self.barrier(
                cmd,
                target.image,
                vk::IMAGE_LAYOUT_COLOR_ATTACHMENT_OPTIMAL,
                vk::IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL,
                vk::ACCESS_COLOR_ATTACHMENT_WRITE_BIT,
                vk::ACCESS_TRANSFER_READ_BIT,
                vk::PIPELINE_STAGE_COLOR_ATTACHMENT_OUTPUT_BIT,
                vk::PIPELINE_STAGE_TRANSFER_BIT,
            );
            unsafe {
                (d.cmd_copy_image_to_buffer)(
                    cmd,
                    target.image,
                    vk::IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL,
                    target.staging,
                    1,
                    &copy,
                );
            }
            self.barrier(
                cmd,
                target.image,
                vk::IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL,
                vk::IMAGE_LAYOUT_COLOR_ATTACHMENT_OPTIMAL,
                vk::ACCESS_TRANSFER_READ_BIT,
                vk::ACCESS_COLOR_ATTACHMENT_WRITE_BIT,
                vk::PIPELINE_STAGE_TRANSFER_BIT,
                vk::PIPELINE_STAGE_COLOR_ATTACHMENT_OUTPUT_BIT,
            );
        })?;
        Ok(target.pixels())
    }

    #[allow(clippy::too_many_arguments, reason = "a barrier is these arguments; naming a struct for one call shape would hide them")]
    fn barrier(
        &self,
        cmd: vk::CommandBuffer,
        image: vk::Image,
        old: i32,
        new: i32,
        src_access: u32,
        dst_access: u32,
        src_stage: u32,
        dst_stage: u32,
    ) {
        let b = vk::ImageMemoryBarrier {
            s_type: vk::STRUCTURE_TYPE_IMAGE_MEMORY_BARRIER,
            p_next: core::ptr::null(),
            src_access_mask: src_access,
            dst_access_mask: dst_access,
            old_layout: old,
            new_layout: new,
            src_queue_family_index: vk::QUEUE_FAMILY_IGNORED,
            dst_queue_family_index: vk::QUEUE_FAMILY_IGNORED,
            image,
            subresource_range: COLOR_RANGE,
        };
        unsafe {
            (self.dfns.cmd_pipeline_barrier)(
                cmd, src_stage, dst_stage, 0, 0, core::ptr::null(), 0, core::ptr::null(), 1, &b,
            );
        }
    }

    fn buffer(&self, size: u64, usage: u32) -> Result<Buffer, Error> {
        alloc_buffer(&self.dfns, self.device, &self.memory, size, usage)
    }

    pub fn geometry(&self, batch: &GpuBatch) -> Result<Geometry<'_>, Error> {
        let u = vk::BUFFER_USAGE_STORAGE_BUFFER_BIT;
        let mut made: Vec<Buffer> = Vec::with_capacity(4);
        for bytes in [
            core::mem::size_of_val(batch.curves()),
            core::mem::size_of_val(batch.band_curves()),
            core::mem::size_of_val(batch.bands()),
            core::mem::size_of_val(batch.hulls()),
        ] {
            match self.buffer(bytes as u64, u) {
                Ok(b) => made.push(b),
                Err(e) => {
                    for b in &made {
                        free_buffer(&self.dfns, self.device, b);
                    }
                    return Err(e);
                }
            }
        }
        let mut made = made.into_iter();
        #[expect(clippy::expect_used, reason = "as above: `made` holds exactly the four buffers pushed by the loop directly overhead, which returns early on failure, and `next` is called exactly four times")]
        let mut next = || made.next().expect("four were pushed, so four come back");
        let g = Geometry {
            revision: batch.revision(),
            curves: next(),
            band_curves: next(),
            bands: next(),
            hulls: next(),
            device: self.device,
            dfns: self.dfns,
            renderer: core::marker::PhantomData,
        };
        g.curves.write(batch.curves());
        g.band_curves.write(batch.band_curves());
        g.bands.write(batch.bands());
        g.hulls.write(batch.hulls());
        Ok(g)
    }

    fn allocate(&self, req: &vk::MemoryRequirements, want: u32) -> Result<vk::DeviceMemory, Error> {
        let info = vk::MemoryAllocateInfo {
            s_type: vk::STRUCTURE_TYPE_MEMORY_ALLOCATE_INFO,
            p_next: core::ptr::null(),
            allocation_size: req.size,
            memory_type_index: memory_type_index(&self.memory, req.memory_type_bits, want)?,
        };
        let mut memory = vk::NULL_HANDLE;
        check("vkAllocateMemory", unsafe {
            (self.dfns.allocate_memory)(self.device, &info, core::ptr::null(), &mut memory)
        })?;
        Ok(memory)
    }

}

pub fn ortho(width: u32, height: u32) -> [f32; 16] {
    let w = width.max(1) as f32;
    let h = height.max(1) as f32;
    [
        2.0 / w, 0.0, 0.0, 0.0,
        0.0, -2.0 / h, 0.0, 0.0,
        0.0, 0.0, 1.0, 0.0,
        -1.0, 1.0, 0.0, 1.0,
    ]
}

#[allow(clippy::too_many_arguments, reason = "six handles is what a partly-built target is")]
fn undo_target(
    d: &DeviceFns,
    device: vk::Device,
    framebuffer: vk::Framebuffer,
    view: vk::ImageView,
    image: vk::Image,
    image_memory: vk::DeviceMemory,
    staging: vk::Buffer,
    staging_memory: vk::DeviceMemory,
) {
    unsafe {
        if staging_memory != vk::NULL_HANDLE {
            (d.free_memory)(device, staging_memory, core::ptr::null());
        }
        if staging != vk::NULL_HANDLE {
            (d.destroy_buffer)(device, staging, core::ptr::null());
        }
        if framebuffer != vk::NULL_HANDLE {
            (d.destroy_framebuffer)(device, framebuffer, core::ptr::null());
        }
        if view != vk::NULL_HANDLE {
            (d.destroy_image_view)(device, view, core::ptr::null());
        }
        if image_memory != vk::NULL_HANDLE {
            (d.free_memory)(device, image_memory, core::ptr::null());
        }
        if image != vk::NULL_HANDLE {
            (d.destroy_image)(device, image, core::ptr::null());
        }
    }
}

fn buffer_info(b: &Buffer) -> vk::DescriptorBufferInfo {
    vk::DescriptorBufferInfo { buffer: b.buffer, offset: 0, range: b.size }
}

fn memory_type_index(
    memory: &vk::PhysicalDeviceMemoryProperties,
    bits: u32,
    want: u32,
) -> Result<u32, Error> {
    (0..memory.memory_type_count)
        .find(|&i| bits & (1 << i) != 0 && memory.memory_types[i as usize].property_flags & want == want)
        .ok_or(Error::Unsupported("a memory type this backend needs"))
}

fn alloc_buffer(
    d: &DeviceFns,
    device: vk::Device,
    memory: &vk::PhysicalDeviceMemoryProperties,
    size: u64,
    usage: u32,
) -> Result<Buffer, Error> {
    let size = size.max(4);
    let info = vk::BufferCreateInfo {
        s_type: vk::STRUCTURE_TYPE_BUFFER_CREATE_INFO,
        p_next: core::ptr::null(),
        flags: 0,
        size,
        usage,
        sharing_mode: vk::SHARING_MODE_EXCLUSIVE,
        queue_family_index_count: 0,
        p_queue_family_indices: core::ptr::null(),
    };
    let mut buffer = vk::NULL_HANDLE;
    check("vkCreateBuffer", unsafe {
        (d.create_buffer)(device, &info, core::ptr::null(), &mut buffer)
    })?;

    let mut req = vk::MemoryRequirements::default();
    unsafe { (d.get_buffer_memory_requirements)(device, buffer, &mut req) };

    let index = match memory_type_index(
        memory,
        req.memory_type_bits,
        vk::MEMORY_PROPERTY_HOST_VISIBLE_BIT | vk::MEMORY_PROPERTY_HOST_COHERENT_BIT,
    ) {
        Ok(i) => i,
        Err(e) => {
            unsafe { (d.destroy_buffer)(device, buffer, core::ptr::null()) };
            return Err(e);
        }
    };
    let alloc = vk::MemoryAllocateInfo {
        s_type: vk::STRUCTURE_TYPE_MEMORY_ALLOCATE_INFO,
        p_next: core::ptr::null(),
        allocation_size: req.size,
        memory_type_index: index,
    };
    let mut mem = vk::NULL_HANDLE;
    if let Err(e) = check("vkAllocateMemory", unsafe {
        (d.allocate_memory)(device, &alloc, core::ptr::null(), &mut mem)
    }) {
        unsafe { (d.destroy_buffer)(device, buffer, core::ptr::null()) };
        return Err(e);
    }
    if let Err(e) = check("vkBindBufferMemory", unsafe {
        (d.bind_buffer_memory)(device, buffer, mem, 0)
    }) {
        unsafe {
            (d.free_memory)(device, mem, core::ptr::null());
            (d.destroy_buffer)(device, buffer, core::ptr::null());
        }
        return Err(e);
    }
    let mut mapped: *mut core::ffi::c_void = core::ptr::null_mut();
    if let Err(e) = check("vkMapMemory", unsafe {
        (d.map_memory)(device, mem, 0, req.size, 0, &mut mapped)
    }) {
        unsafe {
            (d.free_memory)(device, mem, core::ptr::null());
            (d.destroy_buffer)(device, buffer, core::ptr::null());
        }
        return Err(e);
    }
    Ok(Buffer { buffer, memory: mem, mapped: mapped.cast::<u8>(), size })
}

fn free_buffer(d: &DeviceFns, device: vk::Device, b: &Buffer) {
    unsafe {
        (d.unmap_memory)(device, b.memory);
        (d.destroy_buffer)(device, b.buffer, core::ptr::null());
        (d.free_memory)(device, b.memory, core::ptr::null());
    }
}

struct Buffer {
    buffer: vk::Buffer,
    memory: vk::DeviceMemory,
    mapped: *mut u8,
    size: u64,
}

impl Buffer {
    fn write<T: Copy>(&self, data: &[T]) {
        let bytes = core::mem::size_of_val(data);
        if bytes == 0 || self.mapped.is_null() {
            return;
        }
        debug_assert!(bytes as u64 <= self.size, "a buffer write ran past its allocation");
        unsafe { core::ptr::copy_nonoverlapping(data.as_ptr().cast::<u8>(), self.mapped, bytes) };
    }
}

pub struct Geometry<'r> {
    revision: u64,
    curves: Buffer,
    band_curves: Buffer,
    bands: Buffer,
    hulls: Buffer,
    device: vk::Device,
    dfns: DeviceFns,
    renderer: core::marker::PhantomData<&'r Renderer>,
}

impl Geometry<'_> {
    pub fn revision(&self) -> u64 {
        self.revision
    }
}

impl Drop for Geometry<'_> {
    fn drop(&mut self) {
        unsafe {
            (self.dfns.device_wait_idle)(self.device);
        }
        for b in [&self.curves, &self.band_curves, &self.bands, &self.hulls] {
            free_buffer(&self.dfns, self.device, b);
        }
    }
}

const COLOR_RANGE: vk::ImageSubresourceRange = vk::ImageSubresourceRange {
    aspect_mask: vk::IMAGE_ASPECT_COLOR_BIT,
    base_mip_level: 0,
    level_count: 1,
    base_array_layer: 0,
    layer_count: 1,
};

impl Drop for Renderer {
    fn drop(&mut self) {
        let cmd = self.command.borrow();
        let frames = self.frames.borrow();
        unsafe {
            (self.dfns.device_wait_idle)(self.device);
            (self.dfns.destroy_descriptor_pool)(self.device, self.descriptor_pool, core::ptr::null());
            self.bgra.destroy(&self.dfns, self.device);
            self.rgba.destroy(&self.dfns, self.device);
            (self.dfns.destroy_pipeline_layout)(self.device, self.pipeline_layout, core::ptr::null());
            (self.dfns.destroy_descriptor_set_layout)(self.device, self.set_layout, core::ptr::null());
            (self.dfns.destroy_fence)(self.device, cmd.fence, core::ptr::null());
            for f in frames.iter() {
                (self.dfns.destroy_fence)(self.device, f.fence, core::ptr::null());
            }
            (self.dfns.destroy_command_pool)(self.device, cmd.pool, core::ptr::null());
        }
        for f in frames.iter() {
            for b in [&f.instances, &f.subpixel, &f.projection] {
                free_buffer(&self.dfns, self.device, b);
            }
        }
        if self.owns_device {
            unsafe {
                (self.dfns.destroy_device)(self.device, core::ptr::null());
                (self.ifns.destroy_instance)(self.instance, core::ptr::null());
            }
        }
    }
}

fn zeroed_vec<T>(n: usize) -> Vec<T> {
    let mut v: Vec<T> = Vec::with_capacity(n);
    unsafe {
        core::ptr::write_bytes(v.as_mut_ptr(), 0, n);
        v.set_len(n);
    }
    v
}

pub struct Target<'r> {
    // A borrowed image is the caller's: daegun makes the view and framebuffer over it and destroys
    // only those.
    borrowed: bool,
    format: SurfaceFormat,
    clear: Option<crate::daerizer::Rgba>,
    image: vk::Image,
    image_memory: vk::DeviceMemory,
    view: vk::ImageView,
    framebuffer: vk::Framebuffer,
    staging: vk::Buffer,
    staging_memory: vk::DeviceMemory,
    mapped: *mut u8,
    width: u32,
    height: u32,
    device: vk::Device,
    dfns: DeviceFns,
    pending: Option<usize>,
    renderer: core::marker::PhantomData<&'r Renderer>,
}

impl Drop for Target<'_> {
    fn drop(&mut self) {
        // The framebuffer is the sentinel rather than `mapped`, because a borrowed target has no
        // mapping and would otherwise skip its own view and framebuffer.
        if self.framebuffer == vk::NULL_HANDLE {
            return;
        }
        unsafe {
            (self.dfns.device_wait_idle)(self.device);
            (self.dfns.destroy_framebuffer)(self.device, self.framebuffer, core::ptr::null());
            (self.dfns.destroy_image_view)(self.device, self.view, core::ptr::null());
            if !self.borrowed {
                (self.dfns.destroy_image)(self.device, self.image, core::ptr::null());
                (self.dfns.free_memory)(self.device, self.image_memory, core::ptr::null());
                (self.dfns.unmap_memory)(self.device, self.staging_memory);
                (self.dfns.destroy_buffer)(self.device, self.staging, core::ptr::null());
                (self.dfns.free_memory)(self.device, self.staging_memory, core::ptr::null());
            }
        }
        self.framebuffer = vk::NULL_HANDLE;
        self.mapped = core::ptr::null_mut();
    }
}

impl Target<'_> {
    pub fn format(&self) -> SurfaceFormat {
        self.format
    }

    pub fn clear(&self) -> Option<crate::daerizer::Rgba> {
        self.clear
    }

    // `None` keeps what the target already holds instead of clearing.
    pub fn set_clear(&mut self, clear: Option<crate::daerizer::Rgba>) {
        self.clear = clear;
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    fn len(&self) -> usize {
        self.width as usize * self.height as usize * 4
    }

    pub fn pixels(&self) -> &[u8] {
        if self.mapped.is_null() {
            return &[];
        }
        unsafe { core::slice::from_raw_parts(self.mapped, self.len()) }
    }

    pub fn pixel(&self, x: u32, y: u32) -> Option<[u8; 4]> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let i = (y as usize * self.width as usize + x as usize) * 4;
        let p = self.pixels();
        Some([*p.get(i)?, *p.get(i + 1)?, *p.get(i + 2)?, *p.get(i + 3)?])
    }

    pub unsafe fn image(&self) -> u64 {
        self.image
    }
}

fn enumerate<T, F>(mut f: F) -> Option<Vec<T>>
where
    F: FnMut(*mut u32, *mut T) -> vk::Result_,
{
    loop {
        let mut count: u32 = 0;
        if f(&mut count, core::ptr::null_mut()) != vk::SUCCESS {
            return None;
        }
        if count == 0 {
            return Some(Vec::new());
        }
        let mut out = zeroed_vec::<T>(count as usize);
        match f(&mut count, out.as_mut_ptr()) {
            vk::SUCCESS => {
                out.truncate(count as usize);
                return Some(out);
            }
            vk::INCOMPLETE => continue,
            _ => return None,
        }
    }
}

fn enumerate_void<T, F>(mut f: F) -> Vec<T>
where
    F: FnMut(*mut u32, *mut T),
{
    let mut count: u32 = 0;
    f(&mut count, core::ptr::null_mut());
    if count == 0 {
        return Vec::new();
    }
    let mut out = zeroed_vec::<T>(count as usize);
    f(&mut count, out.as_mut_ptr());
    out
}

impl super::backend::Backend for Renderer {
    type Error = Error;
    type Target<'r> = Target<'r>;
    type Geometry<'r> = Geometry<'r>;
    const NAME: &'static str = "vulkan";

    fn new() -> Result<Self, Error> {
        Renderer::new()
    }

    fn refusal(e: &Error) -> super::backend::Refusal {
        use super::backend::Refusal;
        match e {
            Error::NoDevice | Error::MissingEntryPoint(_) => Refusal::NoDevice,
            Error::BadTarget => Refusal::BadTarget,
            Error::Unsupported(_) => Refusal::Unsupported,
            Error::Call { .. } => Refusal::Failed,
        }
    }

    fn target(&self, w: u32, h: u32) -> Result<Target<'_>, Error> {
        Renderer::target(self, w, h)
    }

    fn geometry(&self, batch: &GpuBatch) -> Result<Geometry<'_>, Error> {
        Renderer::geometry(self, batch)
    }

    fn draw(
        &self, t: &mut Target<'_>, g: &Geometry<'_>, i: &[GlyphInstance], s: &SubpixelParams, m: Mode,
    ) -> Result<(), Error> {
        Renderer::draw(self, t, g, i, s, m)
    }

    fn draw_with(
        &self, t: &mut Target<'_>, g: &Geometry<'_>, i: &[GlyphInstance], s: &SubpixelParams,
        m: Mode, p: &[f32; 16],
    ) -> Result<(), Error> {
        Renderer::draw_with(self, t, g, i, s, m, p)
    }

    fn wait(&self, t: &mut Target<'_>) -> Result<(), Error> {
        Renderer::wait(self, t)
    }

    fn read_pixels<'t>(&self, t: &'t mut Target<'_>) -> Result<&'t [u8], Error> {
        Renderer::read_pixels(self, t)
    }

    fn profile(&self) -> crate::daerizer::draw::DeviceProfile {
        Renderer::profile(self)
    }

    fn device_name(&self) -> String {
        Renderer::device_name(self)
    }

    fn supports_subpixel(&self) -> bool {
        Renderer::supports_subpixel(self)
    }

    fn ortho(w: u32, h: u32) -> [f32; 16] {
        ortho(w, h)
    }
}

impl super::backend::Surface for Target<'_> {
    fn width(&self) -> u32 { Target::width(self) }
    fn height(&self) -> u32 { Target::height(self) }
    fn pixels(&self) -> &[u8] { Target::pixels(self) }
    fn pixel(&self, x: u32, y: u32) -> Option<[u8; 4]> { Target::pixel(self, x, y) }
}

impl super::backend::Uploaded for Geometry<'_> {
    fn revision(&self) -> u64 { Geometry::revision(self) }
}
