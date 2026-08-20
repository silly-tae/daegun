// SAFETY, once for the file. Symbols are resolved by the exact name Vulkan defines and called at
// the signature declared beside them; names are `CStr` literals from this file; the library handle
// came from a successful open and is closed once.

use alloc::string::String;
use alloc::vec::Vec;
use core::ffi::{CStr, c_char, c_void};

pub type Handle = *mut c_void;
#[allow(dead_code, reason = "phase 3 creates the first of these; the alias exists now so the two-kinds rule above is a type rather than a comment")]
// `uint64_t` by definition on every target, pointer-sized or not – the two handle kinds are not
// interchangeable and a pointer here breaks wherever they differ.
pub type NonDispatchable = u64;

#[allow(dead_code, reason = "phase 3 uses it")]
pub const NULL_HANDLE: NonDispatchable = 0;

pub type Instance = Handle;
pub type PhysicalDevice = Handle;
pub type Device = Handle;
pub type Queue = Handle;

pub type Bool32 = u32;
pub const TRUE: Bool32 = 1;
pub const FALSE: Bool32 = 0;

pub type Result_ = i32;
pub const SUCCESS: Result_ = 0;
pub const INCOMPLETE: Result_ = 5;
pub const ERROR_OUT_OF_HOST_MEMORY: Result_ = -1;
pub const ERROR_OUT_OF_DEVICE_MEMORY: Result_ = -2;
pub const ERROR_INITIALIZATION_FAILED: Result_ = -3;
pub const ERROR_LAYER_NOT_PRESENT: Result_ = -6;
pub const ERROR_EXTENSION_NOT_PRESENT: Result_ = -7;
pub const ERROR_FEATURE_NOT_PRESENT: Result_ = -8;
pub const ERROR_INCOMPATIBLE_DRIVER: Result_ = -9;

pub fn result_name(r: Result_) -> &'static str {
    match r {
        SUCCESS => "success",
        INCOMPLETE => "incomplete",
        ERROR_OUT_OF_HOST_MEMORY => "out of host memory",
        ERROR_OUT_OF_DEVICE_MEMORY => "out of device memory",
        ERROR_INITIALIZATION_FAILED => "initialization failed",
        ERROR_LAYER_NOT_PRESENT => "layer not present",
        ERROR_EXTENSION_NOT_PRESENT => "extension not present",
        ERROR_FEATURE_NOT_PRESENT => "feature not present",
        ERROR_INCOMPATIBLE_DRIVER => "incompatible driver",
        _ => "unknown error",
    }
}

pub type StructureType = i32;
pub const STRUCTURE_TYPE_APPLICATION_INFO: StructureType = 0;
pub const STRUCTURE_TYPE_INSTANCE_CREATE_INFO: StructureType = 1;
pub const STRUCTURE_TYPE_DEVICE_QUEUE_CREATE_INFO: StructureType = 2;
pub const STRUCTURE_TYPE_DEVICE_CREATE_INFO: StructureType = 3;

pub const INSTANCE_CREATE_ENUMERATE_PORTABILITY_BIT_KHR: u32 = 0x0000_0001;

pub const QUEUE_GRAPHICS_BIT: u32 = 0x0000_0001;

pub const MAX_PHYSICAL_DEVICE_NAME_SIZE: usize = 256;
pub const MAX_EXTENSION_NAME_SIZE: usize = 256;
pub const UUID_SIZE: usize = 16;

pub fn api_version(major: u32, minor: u32, patch: u32) -> u32 {
    (major << 22) | (minor << 12) | patch
}

#[repr(C)]
pub struct ApplicationInfo {
    pub s_type: StructureType,
    pub p_next: *const c_void,
    pub p_application_name: *const c_char,
    pub application_version: u32,
    pub p_engine_name: *const c_char,
    pub engine_version: u32,
    pub api_version: u32,
}

#[repr(C)]
pub struct InstanceCreateInfo {
    pub s_type: StructureType,
    pub p_next: *const c_void,
    pub flags: u32,
    pub p_application_info: *const ApplicationInfo,
    pub enabled_layer_count: u32,
    pub pp_enabled_layer_names: *const *const c_char,
    pub enabled_extension_count: u32,
    pub pp_enabled_extension_names: *const *const c_char,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct ExtensionProperties {
    pub extension_name: [c_char; MAX_EXTENSION_NAME_SIZE],
    pub spec_version: u32,
}

#[repr(C, align(8))]
#[derive(Clone, Copy)]
pub struct PhysicalDeviceLimits {
    opaque: [u8; 504],
}

impl PhysicalDeviceLimits {
    fn u32_at(&self, offset: usize) -> u32 {
        let mut b = [0u8; 4];
        b.copy_from_slice(&self.opaque[offset..offset + 4]);
        u32::from_ne_bytes(b)
    }

    pub fn max_image_dimension_2d(&self) -> u32 {
        self.u32_at(MAX_IMAGE_DIMENSION_2D)
    }

    pub fn max_framebuffer_width(&self) -> u32 {
        self.u32_at(MAX_FRAMEBUFFER_WIDTH)
    }

    pub fn max_framebuffer_height(&self) -> u32 {
        self.u32_at(MAX_FRAMEBUFFER_HEIGHT)
    }
}

const _: () = {
    let size = core::mem::size_of::<PhysicalDeviceLimits>();
    assert!(MAX_IMAGE_DIMENSION_2D + 4 <= size, "maxImageDimension2D runs past the limits block");
    assert!(MAX_FRAMEBUFFER_WIDTH + 4 <= size, "maxFramebufferWidth runs past the limits block");
    assert!(MAX_FRAMEBUFFER_HEIGHT + 4 <= size, "maxFramebufferHeight runs past the limits block");
};

const MAX_IMAGE_DIMENSION_2D: usize = 4;
const MAX_FRAMEBUFFER_WIDTH: usize = 364;
const MAX_FRAMEBUFFER_HEIGHT: usize = 368;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct PhysicalDeviceSparseProperties {
    pub residency_standard_2d_block_shape: Bool32,
    pub residency_standard_2d_multisample_block_shape: Bool32,
    pub residency_standard_3d_block_shape: Bool32,
    pub residency_aligned_mip_size: Bool32,
    pub residency_non_resident_strict: Bool32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct PhysicalDeviceProperties {
    pub api_version: u32,
    pub driver_version: u32,
    pub vendor_id: u32,
    pub device_id: u32,
    pub device_type: i32,
    pub device_name: [c_char; MAX_PHYSICAL_DEVICE_NAME_SIZE],
    pub pipeline_cache_uuid: [u8; UUID_SIZE],
    pub limits: PhysicalDeviceLimits,
    pub sparse_properties: PhysicalDeviceSparseProperties,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Extent3D {
    pub width: u32,
    pub height: u32,
    pub depth: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct QueueFamilyProperties {
    pub queue_flags: u32,
    pub queue_count: u32,
    pub timestamp_valid_bits: u32,
    pub min_image_transfer_granularity: Extent3D,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
// Every field present and in order. A missing or reordered one is not a compile error but silent
// corruption at a byte offset, which is why all fifty-five booleans are spelled out rather than
// the space reserved – and why only what the backend calls is declared at all.
pub struct PhysicalDeviceFeatures {
    pub robust_buffer_access: Bool32,
    pub full_draw_index_uint32: Bool32,
    pub image_cube_array: Bool32,
    pub independent_blend: Bool32,
    pub geometry_shader: Bool32,
    pub tessellation_shader: Bool32,
    pub sample_rate_shading: Bool32,
    pub dual_src_blend: Bool32,
    pub logic_op: Bool32,
    pub multi_draw_indirect: Bool32,
    pub draw_indirect_first_instance: Bool32,
    pub depth_clamp: Bool32,
    pub depth_bias_clamp: Bool32,
    pub fill_mode_non_solid: Bool32,
    pub depth_bounds: Bool32,
    pub wide_lines: Bool32,
    pub large_points: Bool32,
    pub alpha_to_one: Bool32,
    pub multi_viewport: Bool32,
    pub sampler_anisotropy: Bool32,
    pub texture_compression_etc2: Bool32,
    pub texture_compression_astc_ldr: Bool32,
    pub texture_compression_bc: Bool32,
    pub occlusion_query_precise: Bool32,
    pub pipeline_statistics_query: Bool32,
    pub vertex_pipeline_stores_and_atomics: Bool32,
    pub fragment_stores_and_atomics: Bool32,
    pub shader_tessellation_and_geometry_point_size: Bool32,
    pub shader_image_gather_extended: Bool32,
    pub shader_storage_image_extended_formats: Bool32,
    pub shader_storage_image_multisample: Bool32,
    pub shader_storage_image_read_without_format: Bool32,
    pub shader_storage_image_write_without_format: Bool32,
    pub shader_uniform_buffer_array_dynamic_indexing: Bool32,
    pub shader_sampled_image_array_dynamic_indexing: Bool32,
    pub shader_storage_buffer_array_dynamic_indexing: Bool32,
    pub shader_storage_image_array_dynamic_indexing: Bool32,
    pub shader_clip_distance: Bool32,
    pub shader_cull_distance: Bool32,
    pub shader_float64: Bool32,
    pub shader_int64: Bool32,
    pub shader_int16: Bool32,
    pub shader_resource_residency: Bool32,
    pub shader_resource_min_lod: Bool32,
    pub sparse_binding: Bool32,
    pub sparse_residency_buffer: Bool32,
    pub sparse_residency_image_2d: Bool32,
    pub sparse_residency_image_3d: Bool32,
    pub sparse_residency_2_samples: Bool32,
    pub sparse_residency_4_samples: Bool32,
    pub sparse_residency_8_samples: Bool32,
    pub sparse_residency_16_samples: Bool32,
    pub sparse_residency_aliased: Bool32,
    pub variable_multisample_rate: Bool32,
    pub inherited_queries: Bool32,
}

#[repr(C)]
pub struct DeviceQueueCreateInfo {
    pub s_type: StructureType,
    pub p_next: *const c_void,
    pub flags: u32,
    pub queue_family_index: u32,
    pub queue_count: u32,
    pub p_queue_priorities: *const f32,
}

#[repr(C)]
pub struct DeviceCreateInfo {
    pub s_type: StructureType,
    pub p_next: *const c_void,
    pub flags: u32,
    pub queue_create_info_count: u32,
    pub p_queue_create_infos: *const DeviceQueueCreateInfo,
    pub enabled_layer_count: u32,
    pub pp_enabled_layer_names: *const *const c_char,
    pub enabled_extension_count: u32,
    pub pp_enabled_extension_names: *const *const c_char,
    pub p_enabled_features: *const PhysicalDeviceFeatures,
}

const _: () = {
    assert!(core::mem::size_of::<PhysicalDeviceLimits>() == 504);
    assert!(core::mem::align_of::<PhysicalDeviceLimits>() == 8);
    assert!(core::mem::size_of::<PhysicalDeviceFeatures>() == 55 * 4);
    assert!(core::mem::size_of::<PhysicalDeviceSparseProperties>() == 5 * 4);

    assert!(core::mem::size_of::<PhysicalDeviceProperties>() == 824);
    assert!(core::mem::offset_of!(PhysicalDeviceProperties, device_type) == 16);
    assert!(core::mem::offset_of!(PhysicalDeviceProperties, device_name) == 20);
    assert!(core::mem::offset_of!(PhysicalDeviceProperties, pipeline_cache_uuid) == 276);
    assert!(core::mem::offset_of!(PhysicalDeviceProperties, limits) == 296);
    assert!(core::mem::offset_of!(PhysicalDeviceProperties, sparse_properties) == 800);
};

pub type PfnGetInstanceProcAddr =
    unsafe extern "system" fn(Instance, *const c_char) -> Option<PfnVoidFunction>;
pub type PfnVoidFunction = unsafe extern "system" fn();

pub type PfnCreateInstance = unsafe extern "system" fn(
    *const InstanceCreateInfo,
    *const c_void,
    *mut Instance,
) -> Result_;
pub type PfnDestroyInstance = unsafe extern "system" fn(Instance, *const c_void);
pub type PfnEnumerateInstanceExtensionProperties = unsafe extern "system" fn(
    *const c_char,
    *mut u32,
    *mut ExtensionProperties,
) -> Result_;
pub type PfnEnumeratePhysicalDevices =
    unsafe extern "system" fn(Instance, *mut u32, *mut PhysicalDevice) -> Result_;
pub type PfnGetPhysicalDeviceProperties =
    unsafe extern "system" fn(PhysicalDevice, *mut PhysicalDeviceProperties);
pub type PfnGetPhysicalDeviceFeatures =
    unsafe extern "system" fn(PhysicalDevice, *mut PhysicalDeviceFeatures);
pub type PfnGetPhysicalDeviceQueueFamilyProperties =
    unsafe extern "system" fn(PhysicalDevice, *mut u32, *mut QueueFamilyProperties);
pub type PfnEnumerateDeviceExtensionProperties = unsafe extern "system" fn(
    PhysicalDevice,
    *const c_char,
    *mut u32,
    *mut ExtensionProperties,
) -> Result_;
pub type PfnCreateDevice = unsafe extern "system" fn(
    PhysicalDevice,
    *const DeviceCreateInfo,
    *const c_void,
    *mut Device,
) -> Result_;
pub type PfnDestroyDevice = unsafe extern "system" fn(Device, *const c_void);
pub type PfnGetDeviceQueue = unsafe extern "system" fn(Device, u32, u32, *mut Queue);
pub type PfnDeviceWaitIdle = unsafe extern "system" fn(Device) -> Result_;

pub struct Library {
    handle: *mut c_void,
}

unsafe impl Send for Library {}
unsafe impl Sync for Library {}

#[cfg(not(windows))]
unsafe extern "C" {
    fn dlopen(filename: *const c_char, flag: i32) -> *mut c_void;
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
    fn dlclose(handle: *mut c_void) -> i32;
}

#[cfg(windows)]
unsafe extern "system" {
    fn LoadLibraryA(name: *const c_char) -> *mut c_void;
    fn GetProcAddress(module: *mut c_void, name: *const c_char) -> *mut c_void;
    fn FreeLibrary(module: *mut c_void) -> i32;
}

impl Library {
    const NAMES: &'static [&'static CStr] = {
        #[cfg(target_vendor = "apple")]
        {
            &[
                c"libvulkan.1.dylib",
                c"libvulkan.dylib",
                c"/opt/homebrew/lib/libvulkan.1.dylib",
                c"/usr/local/lib/libvulkan.1.dylib",
                c"libMoltenVK.dylib",
                c"/opt/homebrew/lib/libMoltenVK.dylib",
            ]
        }
        #[cfg(windows)]
        {
            &[c"vulkan-1.dll"]
        }
        #[cfg(all(not(target_vendor = "apple"), not(windows)))]
        {
            &[c"libvulkan.so.1", c"libvulkan.so"]
        }
    };

    pub fn open() -> Option<Library> {
        for name in Self::NAMES {
            let handle = unsafe {
                #[cfg(not(windows))]
                {
                    dlopen(name.as_ptr(), 2)
                }
                #[cfg(windows)]
                {
                    LoadLibraryA(name.as_ptr())
                }
            };
            if !handle.is_null() {
                return Some(Library { handle });
            }
        }
        None
    }

    pub fn get_instance_proc_addr(&self) -> Option<PfnGetInstanceProcAddr> {
        let sym = unsafe {
            #[cfg(not(windows))]
            {
                dlsym(self.handle, c"vkGetInstanceProcAddr".as_ptr())
            }
            #[cfg(windows)]
            {
                GetProcAddress(self.handle, c"vkGetInstanceProcAddr".as_ptr())
            }
        };
        if sym.is_null() {
            return None;
        }
        Some(unsafe { core::mem::transmute::<*mut c_void, PfnGetInstanceProcAddr>(sym) })
    }
}

impl Drop for Library {
    fn drop(&mut self) {
        unsafe {
            #[cfg(not(windows))]
            {
                dlclose(self.handle);
            }
            #[cfg(windows)]
            {
                FreeLibrary(self.handle);
            }
        }
    }
}

pub unsafe fn load<T>(
    get: PfnGetInstanceProcAddr,
    instance: Instance,
    name: &CStr,
) -> Option<T> {
    let f = unsafe { get(instance, name.as_ptr()) }?;
    const {
        assert!(core::mem::size_of::<T>() == core::mem::size_of::<PfnVoidFunction>());
    }
    Some(unsafe { core::mem::transmute_copy::<PfnVoidFunction, T>(&f) })
}

pub fn c_array_to_string(buf: &[c_char]) -> String {
    let bytes: Vec<u8> = buf
        .iter()
        .take_while(|&&c| c != 0)
        .map(|&c| c as u8)
        .collect();
    String::from_utf8_lossy(&bytes).into_owned()
}

pub fn has_extension(list: &[ExtensionProperties], want: &CStr) -> bool {
    list.iter().any(|e| {
        let name: Vec<u8> = e
            .extension_name
            .iter()
            .take_while(|&&c| c != 0)
            .map(|&c| c as u8)
            .collect();
        name.as_slice() == want.to_bytes()
    })
}

pub type DeviceMemory = NonDispatchable;
pub type Image = NonDispatchable;
pub type ImageView = NonDispatchable;
pub type Buffer = NonDispatchable;
pub type RenderPass = NonDispatchable;
pub type Framebuffer = NonDispatchable;
pub type CommandPool = NonDispatchable;
pub type Fence = NonDispatchable;
pub type CommandBuffer = Handle;

pub const STRUCTURE_TYPE_SUBMIT_INFO: StructureType = 4;
pub const STRUCTURE_TYPE_MEMORY_ALLOCATE_INFO: StructureType = 5;
pub const STRUCTURE_TYPE_FENCE_CREATE_INFO: StructureType = 8;
pub const STRUCTURE_TYPE_BUFFER_CREATE_INFO: StructureType = 12;
pub const STRUCTURE_TYPE_IMAGE_CREATE_INFO: StructureType = 14;
pub const STRUCTURE_TYPE_IMAGE_VIEW_CREATE_INFO: StructureType = 15;
pub const STRUCTURE_TYPE_FRAMEBUFFER_CREATE_INFO: StructureType = 37;
pub const STRUCTURE_TYPE_RENDER_PASS_CREATE_INFO: StructureType = 38;
pub const STRUCTURE_TYPE_COMMAND_POOL_CREATE_INFO: StructureType = 39;
pub const STRUCTURE_TYPE_COMMAND_BUFFER_ALLOCATE_INFO: StructureType = 40;
pub const STRUCTURE_TYPE_COMMAND_BUFFER_BEGIN_INFO: StructureType = 42;
pub const STRUCTURE_TYPE_IMAGE_MEMORY_BARRIER: StructureType = 45;

pub const FORMAT_R8G8B8A8_UNORM: i32 = 37;

pub const IMAGE_TYPE_2D: i32 = 1;
pub const IMAGE_TILING_OPTIMAL: i32 = 0;
pub const IMAGE_VIEW_TYPE_2D: i32 = 1;
pub const SAMPLE_COUNT_1_BIT: u32 = 0x0000_0001;
pub const SHARING_MODE_EXCLUSIVE: i32 = 0;

pub const IMAGE_USAGE_TRANSFER_SRC_BIT: u32 = 0x0000_0001;
pub const IMAGE_USAGE_TRANSFER_DST_BIT: u32 = 0x0000_0002;
pub const IMAGE_USAGE_COLOR_ATTACHMENT_BIT: u32 = 0x0000_0010;
pub const BUFFER_USAGE_TRANSFER_DST_BIT: u32 = 0x0000_0002;

pub const IMAGE_LAYOUT_UNDEFINED: i32 = 0;
pub const IMAGE_LAYOUT_COLOR_ATTACHMENT_OPTIMAL: i32 = 2;
pub const IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL: i32 = 6;
pub const IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL: i32 = 7;

pub const IMAGE_ASPECT_COLOR_BIT: u32 = 0x0000_0001;
pub const COMPONENT_SWIZZLE_IDENTITY: i32 = 0;

pub const ATTACHMENT_LOAD_OP_CLEAR: i32 = 1;
pub const ATTACHMENT_STORE_OP_STORE: i32 = 0;
pub const ATTACHMENT_LOAD_OP_DONT_CARE: i32 = 2;
pub const ATTACHMENT_STORE_OP_DONT_CARE: i32 = 1;
pub const PIPELINE_BIND_POINT_GRAPHICS: i32 = 0;

pub const MEMORY_PROPERTY_DEVICE_LOCAL_BIT: u32 = 0x0000_0001;
pub const MEMORY_PROPERTY_HOST_VISIBLE_BIT: u32 = 0x0000_0002;
pub const MEMORY_PROPERTY_HOST_COHERENT_BIT: u32 = 0x0000_0004;

pub const COMMAND_BUFFER_LEVEL_PRIMARY: i32 = 0;
pub const COMMAND_POOL_CREATE_RESET_COMMAND_BUFFER_BIT: u32 = 0x0000_0002;
pub const COMMAND_BUFFER_USAGE_ONE_TIME_SUBMIT_BIT: u32 = 0x0000_0001;

pub const PIPELINE_STAGE_TOP_OF_PIPE_BIT: u32 = 0x0000_0001;
pub const PIPELINE_STAGE_TRANSFER_BIT: u32 = 0x0000_1000;
pub const PIPELINE_STAGE_COLOR_ATTACHMENT_OUTPUT_BIT: u32 = 0x0000_0400;
pub const ACCESS_COLOR_ATTACHMENT_WRITE_BIT: u32 = 0x0000_0100;
pub const ACCESS_TRANSFER_READ_BIT: u32 = 0x0000_0800;
pub const ACCESS_TRANSFER_WRITE_BIT: u32 = 0x0000_1000;

pub const QUEUE_FAMILY_IGNORED: u32 = u32::MAX;
pub const MAX_MEMORY_TYPES: usize = 32;
pub const MAX_MEMORY_HEAPS: usize = 16;

pub const TIMEOUT_INFINITE: u64 = u64::MAX;

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct MemoryType {
    pub property_flags: u32,
    pub heap_index: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct MemoryHeap {
    pub size: u64,
    pub flags: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct PhysicalDeviceMemoryProperties {
    pub memory_type_count: u32,
    pub memory_types: [MemoryType; MAX_MEMORY_TYPES],
    pub memory_heap_count: u32,
    pub memory_heaps: [MemoryHeap; MAX_MEMORY_HEAPS],
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct MemoryRequirements {
    pub size: u64,
    pub alignment: u64,
    pub memory_type_bits: u32,
}

#[repr(C)]
pub struct MemoryAllocateInfo {
    pub s_type: StructureType,
    pub p_next: *const c_void,
    pub allocation_size: u64,
    pub memory_type_index: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Offset3D {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

#[repr(C)]
pub struct ImageCreateInfo {
    pub s_type: StructureType,
    pub p_next: *const c_void,
    pub flags: u32,
    pub image_type: i32,
    pub format: i32,
    pub extent: Extent3D,
    pub mip_levels: u32,
    pub array_layers: u32,
    pub samples: u32,
    pub tiling: i32,
    pub usage: u32,
    pub sharing_mode: i32,
    pub queue_family_index_count: u32,
    pub p_queue_family_indices: *const u32,
    pub initial_layout: i32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct ComponentMapping {
    pub r: i32,
    pub g: i32,
    pub b: i32,
    pub a: i32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct ImageSubresourceRange {
    pub aspect_mask: u32,
    pub base_mip_level: u32,
    pub level_count: u32,
    pub base_array_layer: u32,
    pub layer_count: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct ImageSubresourceLayers {
    pub aspect_mask: u32,
    pub mip_level: u32,
    pub base_array_layer: u32,
    pub layer_count: u32,
}

#[repr(C)]
pub struct ImageViewCreateInfo {
    pub s_type: StructureType,
    pub p_next: *const c_void,
    pub flags: u32,
    pub image: Image,
    pub view_type: i32,
    pub format: i32,
    pub components: ComponentMapping,
    pub subresource_range: ImageSubresourceRange,
}

#[repr(C)]
pub struct BufferCreateInfo {
    pub s_type: StructureType,
    pub p_next: *const c_void,
    pub flags: u32,
    pub size: u64,
    pub usage: u32,
    pub sharing_mode: i32,
    pub queue_family_index_count: u32,
    pub p_queue_family_indices: *const u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct AttachmentDescription {
    pub flags: u32,
    pub format: i32,
    pub samples: u32,
    pub load_op: i32,
    pub store_op: i32,
    pub stencil_load_op: i32,
    pub stencil_store_op: i32,
    pub initial_layout: i32,
    pub final_layout: i32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct AttachmentReference {
    pub attachment: u32,
    pub layout: i32,
}

#[repr(C)]
pub struct SubpassDescription {
    pub flags: u32,
    pub pipeline_bind_point: i32,
    pub input_attachment_count: u32,
    pub p_input_attachments: *const AttachmentReference,
    pub color_attachment_count: u32,
    pub p_color_attachments: *const AttachmentReference,
    pub p_resolve_attachments: *const AttachmentReference,
    pub p_depth_stencil_attachment: *const AttachmentReference,
    pub preserve_attachment_count: u32,
    pub p_preserve_attachments: *const u32,
}

#[repr(C)]
pub struct RenderPassCreateInfo {
    pub s_type: StructureType,
    pub p_next: *const c_void,
    pub flags: u32,
    pub attachment_count: u32,
    pub p_attachments: *const AttachmentDescription,
    pub subpass_count: u32,
    pub p_subpasses: *const SubpassDescription,
    pub dependency_count: u32,
    pub p_dependencies: *const c_void,
}

#[repr(C)]
pub struct FramebufferCreateInfo {
    pub s_type: StructureType,
    pub p_next: *const c_void,
    pub flags: u32,
    pub render_pass: RenderPass,
    pub attachment_count: u32,
    pub p_attachments: *const ImageView,
    pub width: u32,
    pub height: u32,
    pub layers: u32,
}

#[repr(C)]
pub struct CommandPoolCreateInfo {
    pub s_type: StructureType,
    pub p_next: *const c_void,
    pub flags: u32,
    pub queue_family_index: u32,
}

#[repr(C)]
pub struct CommandBufferAllocateInfo {
    pub s_type: StructureType,
    pub p_next: *const c_void,
    pub command_pool: CommandPool,
    pub level: i32,
    pub command_buffer_count: u32,
}

#[repr(C)]
pub struct CommandBufferBeginInfo {
    pub s_type: StructureType,
    pub p_next: *const c_void,
    pub flags: u32,
    pub p_inheritance_info: *const c_void,
}

#[repr(C)]
pub struct SubmitInfo {
    pub s_type: StructureType,
    pub p_next: *const c_void,
    pub wait_semaphore_count: u32,
    pub p_wait_semaphores: *const NonDispatchable,
    pub p_wait_dst_stage_mask: *const u32,
    pub command_buffer_count: u32,
    pub p_command_buffers: *const CommandBuffer,
    pub signal_semaphore_count: u32,
    pub p_signal_semaphores: *const NonDispatchable,
}

#[repr(C)]
pub struct FenceCreateInfo {
    pub s_type: StructureType,
    pub p_next: *const c_void,
    pub flags: u32,
}

#[repr(C)]
pub struct ImageMemoryBarrier {
    pub s_type: StructureType,
    pub p_next: *const c_void,
    pub src_access_mask: u32,
    pub dst_access_mask: u32,
    pub old_layout: i32,
    pub new_layout: i32,
    pub src_queue_family_index: u32,
    pub dst_queue_family_index: u32,
    pub image: Image,
    pub subresource_range: ImageSubresourceRange,
}

#[repr(C)]
pub struct BufferImageCopy {
    pub buffer_offset: u64,
    pub buffer_row_length: u32,
    pub buffer_image_height: u32,
    pub image_subresource: ImageSubresourceLayers,
    pub image_offset: Offset3D,
    pub image_extent: Extent3D,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub union ClearColorValue {
    pub float32: [f32; 4],
    pub int32: [i32; 4],
    pub uint32: [u32; 4],
}

pub type PfnGetPhysicalDeviceMemoryProperties =
    unsafe extern "system" fn(PhysicalDevice, *mut PhysicalDeviceMemoryProperties);
pub type PfnCreateImage = unsafe extern "system" fn(Device, *const ImageCreateInfo, *const c_void, *mut Image) -> Result_;
pub type PfnDestroyImage = unsafe extern "system" fn(Device, Image, *const c_void);
pub type PfnGetImageMemoryRequirements = unsafe extern "system" fn(Device, Image, *mut MemoryRequirements);
pub type PfnAllocateMemory = unsafe extern "system" fn(Device, *const MemoryAllocateInfo, *const c_void, *mut DeviceMemory) -> Result_;
pub type PfnFreeMemory = unsafe extern "system" fn(Device, DeviceMemory, *const c_void);
pub type PfnBindImageMemory = unsafe extern "system" fn(Device, Image, DeviceMemory, u64) -> Result_;
pub type PfnCreateImageView = unsafe extern "system" fn(Device, *const ImageViewCreateInfo, *const c_void, *mut ImageView) -> Result_;
pub type PfnDestroyImageView = unsafe extern "system" fn(Device, ImageView, *const c_void);
pub type PfnCreateBuffer = unsafe extern "system" fn(Device, *const BufferCreateInfo, *const c_void, *mut Buffer) -> Result_;
pub type PfnDestroyBuffer = unsafe extern "system" fn(Device, Buffer, *const c_void);
pub type PfnGetBufferMemoryRequirements = unsafe extern "system" fn(Device, Buffer, *mut MemoryRequirements);
pub type PfnBindBufferMemory = unsafe extern "system" fn(Device, Buffer, DeviceMemory, u64) -> Result_;
pub type PfnMapMemory = unsafe extern "system" fn(Device, DeviceMemory, u64, u64, u32, *mut *mut c_void) -> Result_;
pub type PfnUnmapMemory = unsafe extern "system" fn(Device, DeviceMemory);
pub type PfnCreateRenderPass = unsafe extern "system" fn(Device, *const RenderPassCreateInfo, *const c_void, *mut RenderPass) -> Result_;
pub type PfnDestroyRenderPass = unsafe extern "system" fn(Device, RenderPass, *const c_void);
pub type PfnCreateFramebuffer = unsafe extern "system" fn(Device, *const FramebufferCreateInfo, *const c_void, *mut Framebuffer) -> Result_;
pub type PfnDestroyFramebuffer = unsafe extern "system" fn(Device, Framebuffer, *const c_void);
pub type PfnCreateCommandPool = unsafe extern "system" fn(Device, *const CommandPoolCreateInfo, *const c_void, *mut CommandPool) -> Result_;
pub type PfnDestroyCommandPool = unsafe extern "system" fn(Device, CommandPool, *const c_void);
pub type PfnAllocateCommandBuffers = unsafe extern "system" fn(Device, *const CommandBufferAllocateInfo, *mut CommandBuffer) -> Result_;
pub type PfnBeginCommandBuffer = unsafe extern "system" fn(CommandBuffer, *const CommandBufferBeginInfo) -> Result_;
pub type PfnEndCommandBuffer = unsafe extern "system" fn(CommandBuffer) -> Result_;
pub type PfnCmdPipelineBarrier = unsafe extern "system" fn(CommandBuffer, u32, u32, u32, u32, *const c_void, u32, *const c_void, u32, *const ImageMemoryBarrier);
pub type PfnCmdCopyImageToBuffer = unsafe extern "system" fn(CommandBuffer, Image, i32, Buffer, u32, *const BufferImageCopy);
pub type PfnCmdClearColorImage = unsafe extern "system" fn(CommandBuffer, Image, i32, *const ClearColorValue, u32, *const ImageSubresourceRange);
pub type PfnQueueSubmit = unsafe extern "system" fn(Queue, u32, *const SubmitInfo, Fence) -> Result_;
pub type PfnCreateFence = unsafe extern "system" fn(Device, *const FenceCreateInfo, *const c_void, *mut Fence) -> Result_;
pub type PfnDestroyFence = unsafe extern "system" fn(Device, Fence, *const c_void);
pub type PfnWaitForFences = unsafe extern "system" fn(Device, u32, *const Fence, Bool32, u64) -> Result_;
pub type PfnResetFences = unsafe extern "system" fn(Device, u32, *const Fence) -> Result_;

pub type ShaderModule = NonDispatchable;
pub type DescriptorSetLayout = NonDispatchable;
pub type PipelineLayout = NonDispatchable;
pub type Pipeline = NonDispatchable;
pub type PipelineCache = NonDispatchable;

pub const STRUCTURE_TYPE_SHADER_MODULE_CREATE_INFO: StructureType = 16;
pub const STRUCTURE_TYPE_PIPELINE_SHADER_STAGE_CREATE_INFO: StructureType = 18;
pub const STRUCTURE_TYPE_PIPELINE_VERTEX_INPUT_STATE_CREATE_INFO: StructureType = 19;
pub const STRUCTURE_TYPE_PIPELINE_INPUT_ASSEMBLY_STATE_CREATE_INFO: StructureType = 20;
pub const STRUCTURE_TYPE_PIPELINE_VIEWPORT_STATE_CREATE_INFO: StructureType = 22;
pub const STRUCTURE_TYPE_PIPELINE_RASTERIZATION_STATE_CREATE_INFO: StructureType = 23;
pub const STRUCTURE_TYPE_PIPELINE_MULTISAMPLE_STATE_CREATE_INFO: StructureType = 24;
pub const STRUCTURE_TYPE_PIPELINE_COLOR_BLEND_STATE_CREATE_INFO: StructureType = 26;
pub const STRUCTURE_TYPE_PIPELINE_DYNAMIC_STATE_CREATE_INFO: StructureType = 27;
pub const STRUCTURE_TYPE_GRAPHICS_PIPELINE_CREATE_INFO: StructureType = 28;
pub const STRUCTURE_TYPE_PIPELINE_LAYOUT_CREATE_INFO: StructureType = 30;
pub const STRUCTURE_TYPE_DESCRIPTOR_SET_LAYOUT_CREATE_INFO: StructureType = 32;

pub const SHADER_STAGE_VERTEX_BIT: u32 = 0x0000_0001;
pub const SHADER_STAGE_FRAGMENT_BIT: u32 = 0x0000_0010;

pub const DESCRIPTOR_TYPE_UNIFORM_BUFFER: i32 = 6;
pub const DESCRIPTOR_TYPE_STORAGE_BUFFER: i32 = 7;

pub const BUFFER_USAGE_STORAGE_BUFFER_BIT: u32 = 0x0000_0020;

pub const PRIMITIVE_TOPOLOGY_TRIANGLE_STRIP: i32 = 4;
pub const POLYGON_MODE_FILL: i32 = 0;
pub const CULL_MODE_NONE: u32 = 0;
pub const FRONT_FACE_COUNTER_CLOCKWISE: i32 = 0;

pub const DYNAMIC_STATE_VIEWPORT: i32 = 0;
pub const DYNAMIC_STATE_SCISSOR: i32 = 1;

pub const BLEND_OP_ADD: i32 = 0;
pub const BLEND_FACTOR_ONE: i32 = 1;
pub const BLEND_FACTOR_ZERO: i32 = 0;
pub const BLEND_FACTOR_SRC1_COLOR: i32 = 15;
pub const BLEND_FACTOR_ONE_MINUS_SRC1_COLOR: i32 = 16;
pub const BLEND_FACTOR_SRC1_ALPHA: i32 = 17;
pub const BLEND_FACTOR_ONE_MINUS_SRC1_ALPHA: i32 = 18;

pub const COLOR_COMPONENT_RGBA: u32 = 0x0000_000F;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Extent2D {
    pub width: u32,
    pub height: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Offset2D {
    pub x: i32,
    pub y: i32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Rect2D {
    pub offset: Offset2D,
    pub extent: Extent2D,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Viewport {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub min_depth: f32,
    pub max_depth: f32,
}

#[repr(C)]
pub struct ShaderModuleCreateInfo {
    pub s_type: StructureType,
    pub p_next: *const c_void,
    pub flags: u32,
    pub code_size: usize,
    pub p_code: *const u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct DescriptorSetLayoutBinding {
    pub binding: u32,
    pub descriptor_type: i32,
    pub descriptor_count: u32,
    pub stage_flags: u32,
    pub p_immutable_samplers: *const c_void,
}

#[repr(C)]
pub struct DescriptorSetLayoutCreateInfo {
    pub s_type: StructureType,
    pub p_next: *const c_void,
    pub flags: u32,
    pub binding_count: u32,
    pub p_bindings: *const DescriptorSetLayoutBinding,
}

#[repr(C)]
pub struct PipelineLayoutCreateInfo {
    pub s_type: StructureType,
    pub p_next: *const c_void,
    pub flags: u32,
    pub set_layout_count: u32,
    pub p_set_layouts: *const DescriptorSetLayout,
    pub push_constant_range_count: u32,
    pub p_push_constant_ranges: *const c_void,
}

#[repr(C)]
pub struct PipelineShaderStageCreateInfo {
    pub s_type: StructureType,
    pub p_next: *const c_void,
    pub flags: u32,
    pub stage: u32,
    pub module: ShaderModule,
    pub p_name: *const c_char,
    pub p_specialization_info: *const c_void,
}

#[repr(C)]
pub struct PipelineVertexInputStateCreateInfo {
    pub s_type: StructureType,
    pub p_next: *const c_void,
    pub flags: u32,
    pub vertex_binding_description_count: u32,
    pub p_vertex_binding_descriptions: *const c_void,
    pub vertex_attribute_description_count: u32,
    pub p_vertex_attribute_descriptions: *const c_void,
}

#[repr(C)]
pub struct PipelineInputAssemblyStateCreateInfo {
    pub s_type: StructureType,
    pub p_next: *const c_void,
    pub flags: u32,
    pub topology: i32,
    pub primitive_restart_enable: Bool32,
}

#[repr(C)]
pub struct PipelineViewportStateCreateInfo {
    pub s_type: StructureType,
    pub p_next: *const c_void,
    pub flags: u32,
    pub viewport_count: u32,
    pub p_viewports: *const Viewport,
    pub scissor_count: u32,
    pub p_scissors: *const Rect2D,
}

#[repr(C)]
pub struct PipelineRasterizationStateCreateInfo {
    pub s_type: StructureType,
    pub p_next: *const c_void,
    pub flags: u32,
    pub depth_clamp_enable: Bool32,
    pub rasterizer_discard_enable: Bool32,
    pub polygon_mode: i32,
    pub cull_mode: u32,
    pub front_face: i32,
    pub depth_bias_enable: Bool32,
    pub depth_bias_constant_factor: f32,
    pub depth_bias_clamp: f32,
    pub depth_bias_slope_factor: f32,
    pub line_width: f32,
}

#[repr(C)]
pub struct PipelineMultisampleStateCreateInfo {
    pub s_type: StructureType,
    pub p_next: *const c_void,
    pub flags: u32,
    pub rasterization_samples: u32,
    pub sample_shading_enable: Bool32,
    pub min_sample_shading: f32,
    pub p_sample_mask: *const u32,
    pub alpha_to_coverage_enable: Bool32,
    pub alpha_to_one_enable: Bool32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct PipelineColorBlendAttachmentState {
    pub blend_enable: Bool32,
    pub src_color_blend_factor: i32,
    pub dst_color_blend_factor: i32,
    pub color_blend_op: i32,
    pub src_alpha_blend_factor: i32,
    pub dst_alpha_blend_factor: i32,
    pub alpha_blend_op: i32,
    pub color_write_mask: u32,
}

#[repr(C)]
pub struct PipelineColorBlendStateCreateInfo {
    pub s_type: StructureType,
    pub p_next: *const c_void,
    pub flags: u32,
    pub logic_op_enable: Bool32,
    pub logic_op: i32,
    pub attachment_count: u32,
    pub p_attachments: *const PipelineColorBlendAttachmentState,
    pub blend_constants: [f32; 4],
}

#[repr(C)]
pub struct PipelineDynamicStateCreateInfo {
    pub s_type: StructureType,
    pub p_next: *const c_void,
    pub flags: u32,
    pub dynamic_state_count: u32,
    pub p_dynamic_states: *const i32,
}

#[repr(C)]
pub struct GraphicsPipelineCreateInfo {
    pub s_type: StructureType,
    pub p_next: *const c_void,
    pub flags: u32,
    pub stage_count: u32,
    pub p_stages: *const PipelineShaderStageCreateInfo,
    pub p_vertex_input_state: *const PipelineVertexInputStateCreateInfo,
    pub p_input_assembly_state: *const PipelineInputAssemblyStateCreateInfo,
    pub p_tessellation_state: *const c_void,
    pub p_viewport_state: *const PipelineViewportStateCreateInfo,
    pub p_rasterization_state: *const PipelineRasterizationStateCreateInfo,
    pub p_multisample_state: *const PipelineMultisampleStateCreateInfo,
    pub p_depth_stencil_state: *const c_void,
    pub p_color_blend_state: *const PipelineColorBlendStateCreateInfo,
    pub p_dynamic_state: *const PipelineDynamicStateCreateInfo,
    pub layout: PipelineLayout,
    pub render_pass: RenderPass,
    pub subpass: u32,
    pub base_pipeline_handle: Pipeline,
    pub base_pipeline_index: i32,
}

pub type PfnCreateShaderModule = unsafe extern "system" fn(Device, *const ShaderModuleCreateInfo, *const c_void, *mut ShaderModule) -> Result_;
pub type PfnDestroyShaderModule = unsafe extern "system" fn(Device, ShaderModule, *const c_void);
pub type PfnCreateDescriptorSetLayout = unsafe extern "system" fn(Device, *const DescriptorSetLayoutCreateInfo, *const c_void, *mut DescriptorSetLayout) -> Result_;
pub type PfnDestroyDescriptorSetLayout = unsafe extern "system" fn(Device, DescriptorSetLayout, *const c_void);
pub type PfnCreatePipelineLayout = unsafe extern "system" fn(Device, *const PipelineLayoutCreateInfo, *const c_void, *mut PipelineLayout) -> Result_;
pub type PfnDestroyPipelineLayout = unsafe extern "system" fn(Device, PipelineLayout, *const c_void);
pub type PfnCreateGraphicsPipelines = unsafe extern "system" fn(Device, PipelineCache, u32, *const GraphicsPipelineCreateInfo, *const c_void, *mut Pipeline) -> Result_;
pub type PfnDestroyPipeline = unsafe extern "system" fn(Device, Pipeline, *const c_void);

pub type DescriptorPool = NonDispatchable;
pub type DescriptorSet = NonDispatchable;

pub const STRUCTURE_TYPE_DESCRIPTOR_POOL_CREATE_INFO: StructureType = 33;
pub const STRUCTURE_TYPE_DESCRIPTOR_SET_ALLOCATE_INFO: StructureType = 34;
pub const STRUCTURE_TYPE_WRITE_DESCRIPTOR_SET: StructureType = 35;
pub const STRUCTURE_TYPE_RENDER_PASS_BEGIN_INFO: StructureType = 43;

pub const BUFFER_USAGE_UNIFORM_BUFFER_BIT: u32 = 0x0000_0010;
pub const SUBPASS_CONTENTS_INLINE: i32 = 0;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct DescriptorPoolSize {
    pub descriptor_type: i32,
    pub descriptor_count: u32,
}

#[repr(C)]
pub struct DescriptorPoolCreateInfo {
    pub s_type: StructureType,
    pub p_next: *const c_void,
    pub flags: u32,
    pub max_sets: u32,
    pub pool_size_count: u32,
    pub p_pool_sizes: *const DescriptorPoolSize,
}

#[repr(C)]
pub struct DescriptorSetAllocateInfo {
    pub s_type: StructureType,
    pub p_next: *const c_void,
    pub descriptor_pool: DescriptorPool,
    pub descriptor_set_count: u32,
    pub p_set_layouts: *const DescriptorSetLayout,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct DescriptorBufferInfo {
    pub buffer: Buffer,
    pub offset: u64,
    pub range: u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct WriteDescriptorSet {
    pub s_type: StructureType,
    pub p_next: *const c_void,
    pub dst_set: DescriptorSet,
    pub dst_binding: u32,
    pub dst_array_element: u32,
    pub descriptor_count: u32,
    pub descriptor_type: i32,
    pub p_image_info: *const c_void,
    pub p_buffer_info: *const DescriptorBufferInfo,
    pub p_texel_buffer_view: *const c_void,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub union ClearValue {
    pub color: ClearColorValue,
    pub depth_stencil: [u32; 2],
}

#[repr(C)]
pub struct RenderPassBeginInfo {
    pub s_type: StructureType,
    pub p_next: *const c_void,
    pub render_pass: RenderPass,
    pub framebuffer: Framebuffer,
    pub render_area: Rect2D,
    pub clear_value_count: u32,
    pub p_clear_values: *const ClearValue,
}

pub type PfnCreateDescriptorPool = unsafe extern "system" fn(Device, *const DescriptorPoolCreateInfo, *const c_void, *mut DescriptorPool) -> Result_;
pub type PfnDestroyDescriptorPool = unsafe extern "system" fn(Device, DescriptorPool, *const c_void);
pub type PfnAllocateDescriptorSets = unsafe extern "system" fn(Device, *const DescriptorSetAllocateInfo, *mut DescriptorSet) -> Result_;
pub type PfnUpdateDescriptorSets = unsafe extern "system" fn(Device, u32, *const WriteDescriptorSet, u32, *const c_void);
pub type PfnCmdBeginRenderPass = unsafe extern "system" fn(CommandBuffer, *const RenderPassBeginInfo, i32);
pub type PfnCmdEndRenderPass = unsafe extern "system" fn(CommandBuffer);
pub type PfnCmdBindPipeline = unsafe extern "system" fn(CommandBuffer, i32, Pipeline);
pub type PfnCmdBindDescriptorSets = unsafe extern "system" fn(CommandBuffer, i32, PipelineLayout, u32, u32, *const DescriptorSet, u32, *const u32);
pub type PfnCmdSetViewport = unsafe extern "system" fn(CommandBuffer, u32, u32, *const Viewport);
pub type PfnCmdSetScissor = unsafe extern "system" fn(CommandBuffer, u32, u32, *const Rect2D);
pub type PfnCmdDraw = unsafe extern "system" fn(CommandBuffer, u32, u32, u32, u32);
pub type PfnResetCommandBuffer = unsafe extern "system" fn(CommandBuffer, u32) -> Result_;

#[cfg(test)]
mod layout {
    use super::*;

    #[test]
    fn every_struct_matches_the_vulkan_header() {
        let mut bad = alloc::vec::Vec::new();
        let mut check = |name: &str, rs: usize, c: usize, ra: usize, ca: usize| {
            if rs != c || ra != ca {
                bad.push(alloc::format!("{name:<38} rust {rs:>4}/{ra}  header {c:>4}/{ca}"));
            }
        };
        check("ApplicationInfo", core::mem::size_of::<ApplicationInfo>(), 48, core::mem::align_of::<ApplicationInfo>(), 8);
        check("InstanceCreateInfo", core::mem::size_of::<InstanceCreateInfo>(), 64, core::mem::align_of::<InstanceCreateInfo>(), 8);
        check("ExtensionProperties", core::mem::size_of::<ExtensionProperties>(), 260, core::mem::align_of::<ExtensionProperties>(), 4);
        check("PhysicalDeviceLimits", core::mem::size_of::<PhysicalDeviceLimits>(), 504, core::mem::align_of::<PhysicalDeviceLimits>(), 8);
        check("PhysicalDeviceSparseProperties", core::mem::size_of::<PhysicalDeviceSparseProperties>(), 20, core::mem::align_of::<PhysicalDeviceSparseProperties>(), 4);
        check("PhysicalDeviceProperties", core::mem::size_of::<PhysicalDeviceProperties>(), 824, core::mem::align_of::<PhysicalDeviceProperties>(), 8);
        check("Extent3D", core::mem::size_of::<Extent3D>(), 12, core::mem::align_of::<Extent3D>(), 4);
        check("QueueFamilyProperties", core::mem::size_of::<QueueFamilyProperties>(), 24, core::mem::align_of::<QueueFamilyProperties>(), 4);
        check("PhysicalDeviceFeatures", core::mem::size_of::<PhysicalDeviceFeatures>(), 220, core::mem::align_of::<PhysicalDeviceFeatures>(), 4);
        check("DeviceQueueCreateInfo", core::mem::size_of::<DeviceQueueCreateInfo>(), 40, core::mem::align_of::<DeviceQueueCreateInfo>(), 8);
        check("DeviceCreateInfo", core::mem::size_of::<DeviceCreateInfo>(), 72, core::mem::align_of::<DeviceCreateInfo>(), 8);
        check("MemoryType", core::mem::size_of::<MemoryType>(), 8, core::mem::align_of::<MemoryType>(), 4);
        check("MemoryHeap", core::mem::size_of::<MemoryHeap>(), 16, core::mem::align_of::<MemoryHeap>(), 8);
        check("PhysicalDeviceMemoryProperties", core::mem::size_of::<PhysicalDeviceMemoryProperties>(), 520, core::mem::align_of::<PhysicalDeviceMemoryProperties>(), 8);
        check("MemoryRequirements", core::mem::size_of::<MemoryRequirements>(), 24, core::mem::align_of::<MemoryRequirements>(), 8);
        check("MemoryAllocateInfo", core::mem::size_of::<MemoryAllocateInfo>(), 32, core::mem::align_of::<MemoryAllocateInfo>(), 8);
        check("Offset3D", core::mem::size_of::<Offset3D>(), 12, core::mem::align_of::<Offset3D>(), 4);
        check("ImageCreateInfo", core::mem::size_of::<ImageCreateInfo>(), 88, core::mem::align_of::<ImageCreateInfo>(), 8);
        check("ComponentMapping", core::mem::size_of::<ComponentMapping>(), 16, core::mem::align_of::<ComponentMapping>(), 4);
        check("ImageSubresourceRange", core::mem::size_of::<ImageSubresourceRange>(), 20, core::mem::align_of::<ImageSubresourceRange>(), 4);
        check("ImageSubresourceLayers", core::mem::size_of::<ImageSubresourceLayers>(), 16, core::mem::align_of::<ImageSubresourceLayers>(), 4);
        check("ImageViewCreateInfo", core::mem::size_of::<ImageViewCreateInfo>(), 80, core::mem::align_of::<ImageViewCreateInfo>(), 8);
        check("BufferCreateInfo", core::mem::size_of::<BufferCreateInfo>(), 56, core::mem::align_of::<BufferCreateInfo>(), 8);
        check("AttachmentDescription", core::mem::size_of::<AttachmentDescription>(), 36, core::mem::align_of::<AttachmentDescription>(), 4);
        check("AttachmentReference", core::mem::size_of::<AttachmentReference>(), 8, core::mem::align_of::<AttachmentReference>(), 4);
        check("SubpassDescription", core::mem::size_of::<SubpassDescription>(), 72, core::mem::align_of::<SubpassDescription>(), 8);
        check("RenderPassCreateInfo", core::mem::size_of::<RenderPassCreateInfo>(), 64, core::mem::align_of::<RenderPassCreateInfo>(), 8);
        check("FramebufferCreateInfo", core::mem::size_of::<FramebufferCreateInfo>(), 64, core::mem::align_of::<FramebufferCreateInfo>(), 8);
        check("CommandPoolCreateInfo", core::mem::size_of::<CommandPoolCreateInfo>(), 24, core::mem::align_of::<CommandPoolCreateInfo>(), 8);
        check("CommandBufferAllocateInfo", core::mem::size_of::<CommandBufferAllocateInfo>(), 32, core::mem::align_of::<CommandBufferAllocateInfo>(), 8);
        check("CommandBufferBeginInfo", core::mem::size_of::<CommandBufferBeginInfo>(), 32, core::mem::align_of::<CommandBufferBeginInfo>(), 8);
        check("SubmitInfo", core::mem::size_of::<SubmitInfo>(), 72, core::mem::align_of::<SubmitInfo>(), 8);
        check("FenceCreateInfo", core::mem::size_of::<FenceCreateInfo>(), 24, core::mem::align_of::<FenceCreateInfo>(), 8);
        check("ImageMemoryBarrier", core::mem::size_of::<ImageMemoryBarrier>(), 72, core::mem::align_of::<ImageMemoryBarrier>(), 8);
        check("BufferImageCopy", core::mem::size_of::<BufferImageCopy>(), 56, core::mem::align_of::<BufferImageCopy>(), 8);
        check("Extent2D", core::mem::size_of::<Extent2D>(), 8, core::mem::align_of::<Extent2D>(), 4);
        check("Offset2D", core::mem::size_of::<Offset2D>(), 8, core::mem::align_of::<Offset2D>(), 4);
        check("Rect2D", core::mem::size_of::<Rect2D>(), 16, core::mem::align_of::<Rect2D>(), 4);
        check("Viewport", core::mem::size_of::<Viewport>(), 24, core::mem::align_of::<Viewport>(), 4);
        check("ShaderModuleCreateInfo", core::mem::size_of::<ShaderModuleCreateInfo>(), 40, core::mem::align_of::<ShaderModuleCreateInfo>(), 8);
        check("DescriptorSetLayoutBinding", core::mem::size_of::<DescriptorSetLayoutBinding>(), 24, core::mem::align_of::<DescriptorSetLayoutBinding>(), 8);
        check("DescriptorSetLayoutCreateInfo", core::mem::size_of::<DescriptorSetLayoutCreateInfo>(), 32, core::mem::align_of::<DescriptorSetLayoutCreateInfo>(), 8);
        check("PipelineLayoutCreateInfo", core::mem::size_of::<PipelineLayoutCreateInfo>(), 48, core::mem::align_of::<PipelineLayoutCreateInfo>(), 8);
        check("PipelineShaderStageCreateInfo", core::mem::size_of::<PipelineShaderStageCreateInfo>(), 48, core::mem::align_of::<PipelineShaderStageCreateInfo>(), 8);
        check("PipelineVertexInputStateCreateInfo", core::mem::size_of::<PipelineVertexInputStateCreateInfo>(), 48, core::mem::align_of::<PipelineVertexInputStateCreateInfo>(), 8);
        check("PipelineInputAssemblyStateCreateInfo", core::mem::size_of::<PipelineInputAssemblyStateCreateInfo>(), 32, core::mem::align_of::<PipelineInputAssemblyStateCreateInfo>(), 8);
        check("PipelineViewportStateCreateInfo", core::mem::size_of::<PipelineViewportStateCreateInfo>(), 48, core::mem::align_of::<PipelineViewportStateCreateInfo>(), 8);
        check("PipelineRasterizationStateCreateInfo", core::mem::size_of::<PipelineRasterizationStateCreateInfo>(), 64, core::mem::align_of::<PipelineRasterizationStateCreateInfo>(), 8);
        check("PipelineMultisampleStateCreateInfo", core::mem::size_of::<PipelineMultisampleStateCreateInfo>(), 48, core::mem::align_of::<PipelineMultisampleStateCreateInfo>(), 8);
        check("PipelineColorBlendAttachmentState", core::mem::size_of::<PipelineColorBlendAttachmentState>(), 32, core::mem::align_of::<PipelineColorBlendAttachmentState>(), 4);
        check("PipelineColorBlendStateCreateInfo", core::mem::size_of::<PipelineColorBlendStateCreateInfo>(), 56, core::mem::align_of::<PipelineColorBlendStateCreateInfo>(), 8);
        check("PipelineDynamicStateCreateInfo", core::mem::size_of::<PipelineDynamicStateCreateInfo>(), 32, core::mem::align_of::<PipelineDynamicStateCreateInfo>(), 8);
        check("GraphicsPipelineCreateInfo", core::mem::size_of::<GraphicsPipelineCreateInfo>(), 144, core::mem::align_of::<GraphicsPipelineCreateInfo>(), 8);
        check("DescriptorPoolSize", core::mem::size_of::<DescriptorPoolSize>(), 8, core::mem::align_of::<DescriptorPoolSize>(), 4);
        check("DescriptorPoolCreateInfo", core::mem::size_of::<DescriptorPoolCreateInfo>(), 40, core::mem::align_of::<DescriptorPoolCreateInfo>(), 8);
        check("DescriptorSetAllocateInfo", core::mem::size_of::<DescriptorSetAllocateInfo>(), 40, core::mem::align_of::<DescriptorSetAllocateInfo>(), 8);
        check("DescriptorBufferInfo", core::mem::size_of::<DescriptorBufferInfo>(), 24, core::mem::align_of::<DescriptorBufferInfo>(), 8);
        check("WriteDescriptorSet", core::mem::size_of::<WriteDescriptorSet>(), 64, core::mem::align_of::<WriteDescriptorSet>(), 8);
        check("RenderPassBeginInfo", core::mem::size_of::<RenderPassBeginInfo>(), 64, core::mem::align_of::<RenderPassBeginInfo>(), 8);
        check("ClearColorValue", core::mem::size_of::<ClearColorValue>(), 16, core::mem::align_of::<ClearColorValue>(), 4);
        check("ClearValue", core::mem::size_of::<ClearValue>(), 16, core::mem::align_of::<ClearValue>(), 4);
        assert!(
            bad.is_empty(),
            "{} struct(s) disagree with vulkan_core.h:\n{}",
            bad.len(),
            bad.join("\n"),
        );
    }
}
