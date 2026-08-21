// SAFETY, once for the file. Every raw pointer here is a COM interface returned by a call whose
// HRESULT was checked, is released exactly once in `Drop`, and is null-checked before any use.
// Descriptors and arrays passed by pointer are locals that outlive their call, and each `symbol`
// signature is the one its header declares. Only what does not follow from that is noted below.

use core::ffi::{c_char, c_void};

pub type Hresult = i32;

pub fn succeeded(hr: Hresult) -> bool {
    hr >= 0
}

pub type Slot = unsafe extern "system" fn();

#[repr(C)]
pub struct Unknown {
    pub vtable: *const UnknownVtbl,
}

#[repr(C)]
pub struct UnknownVtbl {
    pub query_interface:
        unsafe extern "system" fn(*mut Unknown, *const [u8; 16], *mut *mut Unknown) -> Hresult,
    pub add_ref: unsafe extern "system" fn(*mut Unknown) -> u32,
    pub release: unsafe extern "system" fn(*mut Unknown) -> u32,
}

#[repr(C)]
pub struct DeviceChildVtbl {
    pub base: UnknownVtbl,
    pub _get_device: Slot,
    pub _get_private_data: Slot,
    pub _set_private_data: Slot,
    pub _set_private_data_interface: Slot,
}

pub unsafe fn release(obj: *mut Unknown) {
    if !obj.is_null() {
        unsafe { ((*(*obj).vtable).release)(obj) };
    }
}

pub unsafe fn add_ref(obj: *mut Unknown) -> *mut Unknown {
    unsafe { ((*(*obj).vtable).add_ref)(obj) };
    obj
}

pub struct Library(pub *mut c_void);

#[link(name = "kernel32")]
unsafe extern "system" {
    fn LoadLibraryA(name: *const c_char) -> *mut c_void;
    fn GetProcAddress(module: *mut c_void, name: *const c_char) -> *mut c_void;
    fn FreeLibrary(module: *mut c_void) -> i32;
}

impl Library {
    pub fn open(name: &core::ffi::CStr) -> Option<Library> {
        let handle = unsafe { LoadLibraryA(name.as_ptr()) };
        (!handle.is_null()).then_some(Library(handle))
    }

    pub unsafe fn symbol<T: Copy>(&self, name: &core::ffi::CStr) -> Option<T> {
        const { assert!(size_of::<T>() == size_of::<*mut c_void>()) };
        let p = unsafe { GetProcAddress(self.0, name.as_ptr()) };
        if p.is_null() {
            return None;
        }
        // Reading the pointer *as* `T` rather than transmuting: `T` is only asserted pointer-sized,
        // and a transmute of a generic that size check does not cover is a compile error.
        Some(unsafe { *core::ptr::from_ref(&p).cast::<T>() })
    }
}

impl Drop for Library {
    fn drop(&mut self) {
        unsafe { FreeLibrary(self.0) };
    }
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn CreateEventW(
        attributes: *mut c_void,
        manual_reset: i32,
        initial_state: i32,
        name: *const u16,
    ) -> *mut c_void;
    fn WaitForSingleObject(handle: *mut c_void, milliseconds: u32) -> u32;
    fn CloseHandle(handle: *mut c_void) -> i32;
}

pub const INFINITE: u32 = u32::MAX;

pub struct Event(*mut c_void);

impl Event {
    pub fn new() -> Option<Event> {
        let h = unsafe { CreateEventW(core::ptr::null_mut(), 0, 0, core::ptr::null()) };
        (!h.is_null()).then_some(Event(h))
    }

    pub fn handle(&self) -> *mut c_void {
        self.0
    }

    pub fn wait(&self) {
        unsafe { WaitForSingleObject(self.0, INFINITE) };
    }
}

impl Drop for Event {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}

pub const fn guid(d1: u32, d2: u16, d3: u16, d4: [u8; 8]) -> [u8; 16] {
    let (a, b, c) = (d1.to_le_bytes(), d2.to_le_bytes(), d3.to_le_bytes());
    [
        a[0], a[1], a[2], a[3], b[0], b[1], c[0], c[1],
        d4[0], d4[1], d4[2], d4[3], d4[4], d4[5], d4[6], d4[7],
    ]
}

pub const IID_DXGI_DEVICE: [u8; 16] =
    guid(0x54ec_77fa, 0x1377, 0x44e6, [0x8c, 0x32, 0x88, 0xfd, 0x5f, 0x44, 0xc8, 0x4c]);

pub const IID_DXGI_FACTORY: [u8; 16] =
    guid(0x7b71_66ec, 0x21c7, 0x44ae, [0xb2, 0x1a, 0xc9, 0xae, 0x32, 0x1a, 0xe3, 0x69]);

pub type PfnCreateDxgiFactory = unsafe extern "system" fn(
    iid: *const [u8; 16],
    factory: *mut *mut Unknown,
) -> Hresult;

#[repr(C)]
pub struct DxgiObjectVtbl {
    pub base: UnknownVtbl,
    pub _set_private_data: Slot,
    pub _set_private_data_interface: Slot,
    pub _get_private_data: Slot,
    pub _get_parent: Slot,
}

#[repr(C)]
pub struct DxgiDeviceVtbl {
    pub base: DxgiObjectVtbl,
    pub get_adapter: unsafe extern "system" fn(*mut Unknown, *mut *mut Unknown) -> Hresult,
    pub _create_surface: Slot,
    pub _query_resource_residency: Slot,
    pub _set_gpu_thread_priority: Slot,
    pub _get_gpu_thread_priority: Slot,
}

#[repr(C)]
pub struct DxgiFactoryVtbl {
    pub base: DxgiObjectVtbl,
    pub enum_adapters:
        unsafe extern "system" fn(*mut Unknown, u32, *mut *mut Unknown) -> Hresult,
    pub _make_window_association: Slot,
    pub _get_window_association: Slot,
    pub _create_swap_chain: Slot,
    pub _create_software_adapter: Slot,
}

#[repr(C)]
pub struct DxgiAdapterVtbl {
    pub base: DxgiObjectVtbl,
    pub _enum_outputs: Slot,
    pub get_desc: unsafe extern "system" fn(*mut Unknown, *mut AdapterDesc) -> Hresult,
    pub _check_interface_support: Slot,
}

const _: () = assert!(size_of::<DxgiObjectVtbl>() == (3 + 4) * PTR);
const _: () = assert!(size_of::<DxgiDeviceVtbl>() == (3 + 4 + 5) * PTR);
const _: () = assert!(size_of::<DxgiFactoryVtbl>() == (3 + 4 + 5) * PTR);
const _: () = assert!(size_of::<DxgiAdapterVtbl>() == (3 + 4 + 3) * PTR);

#[repr(C)]
pub struct AdapterDesc {
    pub description: [u16; 128],
    pub vendor_id: u32,
    pub device_id: u32,
    pub sub_sys_id: u32,
    pub revision: u32,
    pub dedicated_video_memory: usize,
    pub dedicated_system_memory: usize,
    pub shared_system_memory: usize,
    pub adapter_luid: [u32; 2],
}

pub const FEATURE_D3D11_OPTIONS2: i32 = 14;

pub const FEATURE_ARCHITECTURE12: i32 = 1;

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct FeatureDataOptions2 {
    pub ps_specified_stencil_ref_supported: i32,
    pub typed_uav_load_additional_formats: i32,
    pub rovs_supported: i32,
    pub conservative_rasterization_tier: i32,
    pub tiled_resources_tier: i32,
    pub map_on_default_textures: i32,
    pub standard_swizzle: i32,
    pub unified_memory_architecture: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct FeatureDataArchitecture {
    pub node_index: u32,
    pub tile_based_renderer: i32,
    pub uma: i32,
    pub cache_coherent_uma: i32,
}

const _: () = assert!(size_of::<FeatureDataOptions2>() == 32);
const _: () = assert!(size_of::<FeatureDataArchitecture>() == 16);

pub const VENDOR_MICROSOFT: u32 = 0x1414;

pub type PfnCreateDevice = unsafe extern "system" fn(
    adapter: *mut Unknown,
    driver_type: i32,
    software: *mut c_void,
    flags: u32,
    feature_levels: *const i32,
    feature_level_count: u32,
    sdk_version: u32,
    device: *mut *mut Unknown,
    feature_level: *mut i32,
    context: *mut *mut Unknown,
) -> Hresult;

pub const IID_D3D12_DEVICE: [u8; 16] =
    guid(0x1898_19f1, 0x1db6, 0x4b57, [0xbe, 0x54, 0x18, 0x21, 0x33, 0x9b, 0x85, 0xf7]);

pub type PfnCreateDevice12 = unsafe extern "system" fn(
    adapter: *mut Unknown,
    minimum_feature_level: i32,
    iid: *const [u8; 16],
    device: *mut *mut Unknown,
) -> Hresult;

pub const FEATURE_LEVEL_12_0: i32 = 0xc000;
pub const FEATURE_LEVEL_12_1: i32 = 0xc100;

pub type PfnCompile = unsafe extern "system" fn(
    src_data: *const c_void,
    src_size: usize,
    source_name: *const c_char,
    defines: *const ShaderMacro,
    include: *mut c_void,
    entry_point: *const c_char,
    target: *const c_char,
    flags1: u32,
    flags2: u32,
    code: *mut *mut Unknown,
    errors: *mut *mut Unknown,
) -> Hresult;

#[repr(C)]
pub struct ShaderMacro {
    pub name: *const c_char,
    pub definition: *const c_char,
}

#[repr(C)]
pub struct BlobVtbl {
    pub base: UnknownVtbl,
    pub get_buffer_pointer: unsafe extern "system" fn(*mut Unknown) -> *mut c_void,
    pub get_buffer_size: unsafe extern "system" fn(*mut Unknown) -> usize,
}

pub struct Blob(*mut Unknown);

impl Blob {
    pub unsafe fn from_raw(ptr: *mut Unknown) -> Option<Blob> {
        (!ptr.is_null()).then_some(Blob(ptr))
    }

    pub fn bytes(&self) -> &[u8] {
        unsafe {
            let t = (*self.0).vtable.cast::<BlobVtbl>();
            let p = ((*t).get_buffer_pointer)(self.0).cast::<u8>();
            let n = ((*t).get_buffer_size)(self.0);
            if p.is_null() || n == 0 { &[] } else { core::slice::from_raw_parts(p, n) }
        }
    }

    pub fn text(&self) -> alloc::string::String {
        alloc::string::String::from_utf8_lossy(self.bytes()).trim_end_matches('\0').into()
    }
}

impl Drop for Blob {
    fn drop(&mut self) {
        unsafe { release(self.0) };
    }
}

#[repr(C)]
pub struct DeviceVtbl {
    pub base: UnknownVtbl,
    pub create_buffer: unsafe extern "system" fn(*mut Unknown, *const BufferDesc, *const SubresourceData, *mut *mut Unknown) -> Hresult,
    pub _create_texture1_d: Slot,
    pub create_texture2_d: unsafe extern "system" fn(*mut Unknown, *const Texture2dDesc, *const SubresourceData, *mut *mut Unknown) -> Hresult,
    pub _create_texture3_d: Slot,
    pub create_shader_resource_view: unsafe extern "system" fn(*mut Unknown, *mut Unknown, *const ShaderResourceViewDesc, *mut *mut Unknown) -> Hresult,
    pub _create_unordered_access_view: Slot,
    pub create_render_target_view: unsafe extern "system" fn(*mut Unknown, *mut Unknown, *const core::ffi::c_void, *mut *mut Unknown) -> Hresult,
    pub _create_depth_stencil_view: Slot,
    pub _create_input_layout: Slot,
    pub create_vertex_shader: unsafe extern "system" fn(*mut Unknown, *const core::ffi::c_void, usize, *mut Unknown, *mut *mut Unknown) -> Hresult,
    pub _create_geometry_shader: Slot,
    pub _create_geometry_shader_with_stream_output: Slot,
    pub create_pixel_shader: unsafe extern "system" fn(*mut Unknown, *const core::ffi::c_void, usize, *mut Unknown, *mut *mut Unknown) -> Hresult,
    pub _create_hull_shader: Slot,
    pub _create_domain_shader: Slot,
    pub _create_compute_shader: Slot,
    pub _create_class_linkage: Slot,
    pub create_blend_state: unsafe extern "system" fn(*mut Unknown, *const BlendDesc, *mut *mut Unknown) -> Hresult,
    pub _create_depth_stencil_state: Slot,
    pub create_rasterizer_state: unsafe extern "system" fn(*mut Unknown, *const RasterizerDesc, *mut *mut Unknown) -> Hresult,
    pub _create_sampler_state: Slot,
    pub create_query: unsafe extern "system" fn(*mut Unknown, *const QueryDesc, *mut *mut Unknown) -> Hresult,
    pub _create_predicate: Slot,
    pub _create_counter: Slot,
    pub _create_deferred_context: Slot,
    pub _open_shared_resource: Slot,
    pub _check_format_support: Slot,
    pub _check_multisample_quality_levels: Slot,
    pub _check_counter_info: Slot,
    pub _check_counter: Slot,
    pub check_feature_support:
        unsafe extern "system" fn(*mut Unknown, i32, *mut c_void, u32) -> Hresult,
    pub _get_private_data: Slot,
    pub _set_private_data: Slot,
    pub _set_private_data_interface: Slot,
    pub get_feature_level: unsafe extern "system" fn(*mut Unknown) -> i32,
    pub _get_creation_flags: Slot,
    pub _get_device_removed_reason: Slot,
    pub get_immediate_context: unsafe extern "system" fn(*mut Unknown, *mut *mut Unknown),
    pub _set_exception_mode: Slot,
    pub _get_exception_mode: Slot,
}

#[repr(C)]
pub struct ContextVtbl {
    pub base: DeviceChildVtbl,
    pub vs_set_constant_buffers: unsafe extern "system" fn(*mut Unknown, u32, u32, *const *mut Unknown),
    pub ps_set_shader_resources: unsafe extern "system" fn(*mut Unknown, u32, u32, *const *mut Unknown),
    pub ps_set_shader: unsafe extern "system" fn(*mut Unknown, *mut Unknown, *const *mut Unknown, u32),
    pub _ps_set_samplers: Slot,
    pub vs_set_shader: unsafe extern "system" fn(*mut Unknown, *mut Unknown, *const *mut Unknown, u32),
    pub _draw_indexed: Slot,
    pub _draw: Slot,
    pub map: unsafe extern "system" fn(*mut Unknown, *mut Unknown, u32, i32, u32, *mut MappedSubresource) -> Hresult,
    pub unmap: unsafe extern "system" fn(*mut Unknown, *mut Unknown, u32),
    pub _ps_set_constant_buffers: Slot,
    pub _ia_set_input_layout: Slot,
    pub _ia_set_vertex_buffers: Slot,
    pub _ia_set_index_buffer: Slot,
    pub _draw_indexed_instanced: Slot,
    pub draw_instanced: unsafe extern "system" fn(*mut Unknown, u32, u32, u32, u32),
    pub _gs_set_constant_buffers: Slot,
    pub _gs_set_shader: Slot,
    pub ia_set_primitive_topology: unsafe extern "system" fn(*mut Unknown, i32),
    pub vs_set_shader_resources: unsafe extern "system" fn(*mut Unknown, u32, u32, *const *mut Unknown),
    pub _vs_set_samplers: Slot,
    pub _begin: Slot,
    pub end: unsafe extern "system" fn(*mut Unknown, *mut Unknown),
    pub get_data: unsafe extern "system" fn(*mut Unknown, *mut Unknown, *mut core::ffi::c_void, u32, u32) -> Hresult,
    pub _set_predication: Slot,
    pub _gs_set_shader_resources: Slot,
    pub _gs_set_samplers: Slot,
    pub om_set_render_targets: unsafe extern "system" fn(*mut Unknown, u32, *const *mut Unknown, *mut Unknown),
    pub _om_set_render_targets_and_unordered_access_views: Slot,
    pub om_set_blend_state: unsafe extern "system" fn(*mut Unknown, *mut Unknown, *const f32, u32),
    pub _om_set_depth_stencil_state: Slot,
    pub _so_set_targets: Slot,
    pub _draw_auto: Slot,
    pub _draw_indexed_instanced_indirect: Slot,
    pub _draw_instanced_indirect: Slot,
    pub _dispatch: Slot,
    pub _dispatch_indirect: Slot,
    pub rs_set_state: unsafe extern "system" fn(*mut Unknown, *mut Unknown),
    pub rs_set_viewports: unsafe extern "system" fn(*mut Unknown, u32, *const Viewport),
    pub _rs_set_scissor_rects: Slot,
    pub _copy_subresource_region: Slot,
    pub copy_resource: unsafe extern "system" fn(*mut Unknown, *mut Unknown, *mut Unknown),
    pub _update_subresource: Slot,
    pub _copy_structure_count: Slot,
    pub clear_render_target_view: unsafe extern "system" fn(*mut Unknown, *mut Unknown, *const f32),
    pub _clear_unordered_access_view_uint: Slot,
    pub _clear_unordered_access_view_float: Slot,
    pub _clear_depth_stencil_view: Slot,
    pub _generate_mips: Slot,
    pub _set_resource_min_lod: Slot,
    pub _get_resource_min_lod: Slot,
    pub _resolve_subresource: Slot,
    pub _execute_command_list: Slot,
    pub _hs_set_shader_resources: Slot,
    pub _hs_set_shader: Slot,
    pub _hs_set_samplers: Slot,
    pub _hs_set_constant_buffers: Slot,
    pub _ds_set_shader_resources: Slot,
    pub _ds_set_shader: Slot,
    pub _ds_set_samplers: Slot,
    pub _ds_set_constant_buffers: Slot,
    pub _cs_set_shader_resources: Slot,
    pub _cs_set_unordered_access_views: Slot,
    pub _cs_set_shader: Slot,
    pub _cs_set_samplers: Slot,
    pub _cs_set_constant_buffers: Slot,
    pub _vs_get_constant_buffers: Slot,
    pub _ps_get_shader_resources: Slot,
    pub _ps_get_shader: Slot,
    pub _ps_get_samplers: Slot,
    pub _vs_get_shader: Slot,
    pub _ps_get_constant_buffers: Slot,
    pub _ia_get_input_layout: Slot,
    pub _ia_get_vertex_buffers: Slot,
    pub _ia_get_index_buffer: Slot,
    pub _gs_get_constant_buffers: Slot,
    pub _gs_get_shader: Slot,
    pub _ia_get_primitive_topology: Slot,
    pub _vs_get_shader_resources: Slot,
    pub _vs_get_samplers: Slot,
    pub _get_predication: Slot,
    pub _gs_get_shader_resources: Slot,
    pub _gs_get_samplers: Slot,
    pub _om_get_render_targets: Slot,
    pub _om_get_render_targets_and_unordered_access_views: Slot,
    pub _om_get_blend_state: Slot,
    pub _om_get_depth_stencil_state: Slot,
    pub _so_get_targets: Slot,
    pub _rs_get_state: Slot,
    pub _rs_get_viewports: Slot,
    pub _rs_get_scissor_rects: Slot,
    pub _hs_get_shader_resources: Slot,
    pub _hs_get_shader: Slot,
    pub _hs_get_samplers: Slot,
    pub _hs_get_constant_buffers: Slot,
    pub _ds_get_shader_resources: Slot,
    pub _ds_get_shader: Slot,
    pub _ds_get_samplers: Slot,
    pub _ds_get_constant_buffers: Slot,
    pub _cs_get_shader_resources: Slot,
    pub _cs_get_unordered_access_views: Slot,
    pub _cs_get_shader: Slot,
    pub _cs_get_samplers: Slot,
    pub _cs_get_constant_buffers: Slot,
    pub _clear_state: Slot,
    pub flush: unsafe extern "system" fn(*mut Unknown),
    pub _get_type: Slot,
    pub _get_context_flags: Slot,
    pub _finish_command_list: Slot,
}

const PTR: usize = size_of::<*const c_void>();
const _: () = assert!(size_of::<UnknownVtbl>() == 3 * PTR);
const _: () = assert!(size_of::<DeviceChildVtbl>() == 7 * PTR);
const _: () = assert!(size_of::<DeviceVtbl>() == (3 + 40) * PTR);
const _: () = assert!(size_of::<ContextVtbl>() == (7 + 108) * PTR);

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct BufferDesc {
    pub byte_width: u32,
    pub usage: i32,
    pub bind_flags: u32,
    pub cpu_access_flags: u32,
    pub misc_flags: u32,
    pub structure_byte_stride: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct SampleDesc {
    pub count: u32,
    pub quality: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Texture2dDesc {
    pub width: u32,
    pub height: u32,
    pub mip_levels: u32,
    pub array_size: u32,
    pub format: i32,
    pub sample_desc: SampleDesc,
    pub usage: i32,
    pub bind_flags: u32,
    pub cpu_access_flags: u32,
    pub misc_flags: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct SubresourceData {
    pub sys_mem: *const c_void,
    pub sys_mem_pitch: u32,
    pub sys_mem_slice_pitch: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct ShaderResourceViewDesc {
    pub format: i32,
    pub view_dimension: i32,
    pub first_element: u32,
    pub num_elements: u32,
    pub flags: u32,
    pub _union_tail: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct RenderTargetBlendDesc {
    pub blend_enable: i32,
    pub src_blend: i32,
    pub dest_blend: i32,
    pub blend_op: i32,
    pub src_blend_alpha: i32,
    pub dest_blend_alpha: i32,
    pub blend_op_alpha: i32,
    pub render_target_write_mask: u8,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct BlendDesc {
    pub alpha_to_coverage_enable: i32,
    pub independent_blend_enable: i32,
    pub render_target: [RenderTargetBlendDesc; 8],
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct RasterizerDesc {
    pub fill_mode: i32,
    pub cull_mode: i32,
    pub front_counter_clockwise: i32,
    pub depth_bias: i32,
    pub depth_bias_clamp: f32,
    pub slope_scaled_depth_bias: f32,
    pub depth_clip_enable: i32,
    pub scissor_enable: i32,
    pub multisample_enable: i32,
    pub antialiased_line_enable: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Viewport {
    pub top_left_x: f32,
    pub top_left_y: f32,
    pub width: f32,
    pub height: f32,
    pub min_depth: f32,
    pub max_depth: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct MappedSubresource {
    pub data: *mut c_void,
    pub row_pitch: u32,
    pub depth_pitch: u32,
}

pub const SDK_VERSION: u32 = 7;

pub const DRIVER_TYPE_HARDWARE: i32 = 1;
pub const DRIVER_TYPE_WARP: i32 = 5;

pub const FEATURE_LEVEL_11_0: i32 = 0xb000;
pub const FEATURE_LEVEL_11_1: i32 = 0xb100;

pub const USAGE_DEFAULT: i32 = 0;
pub const USAGE_DYNAMIC: i32 = 2;
pub const USAGE_STAGING: i32 = 3;

pub const BIND_CONSTANT_BUFFER: u32 = 0x0004;
pub const BIND_SHADER_RESOURCE: u32 = 0x0008;
pub const BIND_RENDER_TARGET: u32 = 0x0020;

pub const CPU_ACCESS_WRITE: u32 = 0x0001_0000;
pub const CPU_ACCESS_READ: u32 = 0x0002_0000;

pub const RESOURCE_MISC_BUFFER_STRUCTURED: u32 = 0x0000_0040;

pub const FORMAT_UNKNOWN: i32 = 0;
pub const FORMAT_R8G8B8A8_UNORM: i32 = 28;
pub const FORMAT_B8G8R8A8_UNORM: i32 = 87;

pub const SRV_DIMENSION_BUFFEREX: i32 = 11;

pub const BLEND_SRC1_COLOR: i32 = 16;
pub const BLEND_INV_SRC1_COLOR: i32 = 17;
pub const BLEND_SRC1_ALPHA: i32 = 18;
pub const BLEND_INV_SRC1_ALPHA: i32 = 19;
pub const BLEND_OP_ADD: i32 = 1;
pub const COLOR_WRITE_ENABLE_ALL: u8 = 15;

pub const FILL_SOLID: i32 = 3;
pub const CULL_NONE: i32 = 1;

pub const MAP_READ: i32 = 1;
pub const MAP_WRITE_DISCARD: i32 = 4;

pub const PRIMITIVE_TOPOLOGY_TRIANGLESTRIP: i32 = 5;

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct QueryDesc {
    pub query: i32,
    pub misc_flags: u32,
}

pub const QUERY_EVENT: i32 = 0;

pub const S_FALSE: Hresult = 1;

pub const IID_D12_QUEUE: [u8; 16] =
    guid(0x0ec8_70a6, 0x5d7e, 0x4c22, [0x8c, 0xfc, 0x5b, 0xaa, 0xe0, 0x76, 0x16, 0xed]);
pub const IID_D12_ALLOCATOR: [u8; 16] =
    guid(0x6102_dee4, 0xaf59, 0x4b09, [0xb9, 0x99, 0xb4, 0x4d, 0x73, 0xf0, 0x9b, 0x24]);
pub const IID_D12_GFX_LIST: [u8; 16] =
    guid(0x5b16_0d0f, 0xac1b, 0x4185, [0x8b, 0xa8, 0xb3, 0xae, 0x42, 0xa5, 0xa4, 0x55]);
pub const IID_D12_DESCRIPTOR_HEAP: [u8; 16] =
    guid(0x8efb_471d, 0x616c, 0x4f49, [0x90, 0xf7, 0x12, 0x7b, 0xb7, 0x63, 0xfa, 0x51]);
pub const IID_D12_FENCE: [u8; 16] =
    guid(0x0a75_3dcf, 0xc4d8, 0x4b91, [0xad, 0xf6, 0xbe, 0x5a, 0x60, 0xd9, 0x5a, 0x76]);
pub const IID_D12_RESOURCE: [u8; 16] =
    guid(0x6964_42be, 0xa72e, 0x4059, [0xbc, 0x79, 0x5b, 0x5c, 0x98, 0x04, 0x0f, 0xad]);
pub const IID_D12_ROOT_SIGNATURE: [u8; 16] =
    guid(0xc54a_6b66, 0x72df, 0x4ee8, [0x8b, 0xe5, 0xa9, 0x46, 0xa1, 0x42, 0x92, 0x14]);
pub const IID_D12_PIPELINE_STATE: [u8; 16] =
    guid(0x765a_30f3, 0xf624, 0x4c6f, [0xa8, 0x28, 0xac, 0xe9, 0x48, 0x62, 0x24, 0x45]);

pub type PfnSerializeRootSignature = unsafe extern "system" fn(
    desc: *const RootSignatureDesc,
    version: i32,
    blob: *mut *mut Unknown,
    error_blob: *mut *mut Unknown,
) -> Hresult;

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct CpuDescriptorHandle {
    pub ptr: usize,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct GpuDescriptorHandle {
    pub ptr: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Range {
    pub begin: usize,
    pub end: usize,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct CommandQueueDesc {
    pub kind: i32,
    pub priority: i32,
    pub flags: u32,
    pub node_mask: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct DescriptorHeapDesc {
    pub kind: i32,
    pub num_descriptors: u32,
    pub flags: u32,
    pub node_mask: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct HeapProperties {
    pub kind: i32,
    pub cpu_page_property: i32,
    pub memory_pool_preference: i32,
    pub creation_node_mask: u32,
    pub visible_node_mask: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct ResourceDesc {
    pub dimension: i32,
    pub alignment: u64,
    pub width: u64,
    pub height: u32,
    pub depth_or_array_size: u16,
    pub mip_levels: u16,
    pub format: i32,
    pub sample_desc: SampleDesc,
    pub layout: i32,
    pub flags: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct ShaderBytecode {
    pub bytecode: *const c_void,
    pub length: usize,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct StreamOutputDesc {
    pub declaration: *const c_void,
    pub num_entries: u32,
    pub buffer_strides: *const u32,
    pub num_strides: u32,
    pub rasterized_stream: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct RenderTargetBlendDesc12 {
    pub blend_enable: i32,
    pub logic_op_enable: i32,
    pub src_blend: i32,
    pub dest_blend: i32,
    pub blend_op: i32,
    pub src_blend_alpha: i32,
    pub dest_blend_alpha: i32,
    pub blend_op_alpha: i32,
    pub logic_op: i32,
    pub render_target_write_mask: u8,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct BlendDesc12 {
    pub alpha_to_coverage_enable: i32,
    pub independent_blend_enable: i32,
    pub render_target: [RenderTargetBlendDesc12; 8],
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct RasterizerDesc12 {
    pub fill_mode: i32,
    pub cull_mode: i32,
    pub front_counter_clockwise: i32,
    pub depth_bias: i32,
    pub depth_bias_clamp: f32,
    pub slope_scaled_depth_bias: f32,
    pub depth_clip_enable: i32,
    pub multisample_enable: i32,
    pub antialiased_line_enable: i32,
    pub forced_sample_count: u32,
    pub conservative_raster: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct DepthStencilOpDesc {
    pub stencil_fail_op: i32,
    pub stencil_depth_fail_op: i32,
    pub stencil_pass_op: i32,
    pub stencil_func: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct DepthStencilDesc {
    pub depth_enable: i32,
    pub depth_write_mask: i32,
    pub depth_func: i32,
    pub stencil_enable: i32,
    pub stencil_read_mask: u8,
    pub stencil_write_mask: u8,
    pub front_face: DepthStencilOpDesc,
    pub back_face: DepthStencilOpDesc,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct InputLayoutDesc {
    pub input_element_descs: *const c_void,
    pub num_elements: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct CachedPipelineState {
    pub cached_blob: *const c_void,
    pub size: usize,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct GraphicsPipelineStateDesc {
    pub root_signature: *mut Unknown,
    pub vs: ShaderBytecode,
    pub ps: ShaderBytecode,
    pub ds: ShaderBytecode,
    pub hs: ShaderBytecode,
    pub gs: ShaderBytecode,
    pub stream_output: StreamOutputDesc,
    pub blend_state: BlendDesc12,
    pub sample_mask: u32,
    pub rasterizer_state: RasterizerDesc12,
    pub depth_stencil_state: DepthStencilDesc,
    pub input_layout: InputLayoutDesc,
    pub ib_strip_cut_value: i32,
    pub primitive_topology_type: i32,
    pub num_render_targets: u32,
    pub rtv_formats: [i32; 8],
    pub dsv_format: i32,
    pub sample_desc: SampleDesc,
    pub node_mask: u32,
    pub cached_pso: CachedPipelineState,
    pub flags: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct DescriptorRange {
    pub range_type: i32,
    pub num_descriptors: u32,
    pub base_shader_register: u32,
    pub register_space: u32,
    pub offset_in_descriptors_from_table_start: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct RootParameter {
    pub parameter_type: i32,
    pub value: RootParameterValue,
    pub shader_visibility: i32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub union RootParameterValue {
    pub table: RootDescriptorTable,
    pub descriptor: RootDescriptor,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct RootDescriptorTable {
    pub num_descriptor_ranges: u32,
    pub descriptor_ranges: *const DescriptorRange,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct RootDescriptor {
    pub shader_register: u32,
    pub register_space: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct RootSignatureDesc {
    pub num_parameters: u32,
    pub parameters: *const RootParameter,
    pub num_static_samplers: u32,
    pub static_samplers: *const c_void,
    pub flags: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct ShaderResourceViewDesc12 {
    pub format: i32,
    pub view_dimension: i32,
    pub shader4_component_mapping: u32,
    pub first_element: u64,
    pub num_elements: u32,
    pub structure_byte_stride: u32,
    pub flags: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct ResourceBarrier {
    pub barrier_type: i32,
    pub flags: u32,
    pub resource: *mut Unknown,
    pub subresource: u32,
    pub state_before: i32,
    pub state_after: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct SubresourceFootprint {
    pub format: i32,
    pub width: u32,
    pub height: u32,
    pub depth: u32,
    pub row_pitch: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct PlacedSubresourceFootprint {
    pub offset: u64,
    pub footprint: SubresourceFootprint,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct TextureCopyLocation {
    pub resource: *mut Unknown,
    pub kind: i32,
    pub placed_footprint: PlacedSubresourceFootprint,
}

pub const COMMAND_LIST_TYPE_DIRECT: i32 = 0;
pub const DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV: i32 = 0;
pub const DESCRIPTOR_HEAP_TYPE_RTV: i32 = 2;
pub const DESCRIPTOR_HEAP_FLAG_NONE: u32 = 0;
pub const DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE: u32 = 1;

pub const HEAP_TYPE_DEFAULT: i32 = 1;
pub const HEAP_TYPE_UPLOAD: i32 = 2;
pub const HEAP_TYPE_READBACK: i32 = 3;

pub const RESOURCE_DIMENSION_BUFFER: i32 = 1;
pub const RESOURCE_DIMENSION_TEXTURE2D: i32 = 3;
pub const TEXTURE_LAYOUT_UNKNOWN: i32 = 0;
pub const TEXTURE_LAYOUT_ROW_MAJOR: i32 = 1;
pub const RESOURCE_FLAG_NONE: u32 = 0;
pub const RESOURCE_FLAG_ALLOW_RENDER_TARGET: u32 = 1;

pub const RESOURCE_STATE_RENDER_TARGET: i32 = 4;
pub const RESOURCE_STATE_COPY_DEST: i32 = 1024;
pub const RESOURCE_STATE_COPY_SOURCE: i32 = 2048;
pub const RESOURCE_STATE_GENERIC_READ: i32 = 0x1 | 0x2 | 0x40 | 0x80 | 0x200 | 0x800;

pub const ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE: i32 = 0;
pub const ROOT_PARAMETER_TYPE_CBV: i32 = 2;
pub const DESCRIPTOR_RANGE_TYPE_SRV: i32 = 0;
pub const SHADER_VISIBILITY_ALL: i32 = 0;
pub const ROOT_SIGNATURE_FLAG_NONE: u32 = 0;
pub const ROOT_SIGNATURE_VERSION_1: i32 = 1;

pub const PRIMITIVE_TOPOLOGY_TYPE_TRIANGLE: i32 = 3;
pub const RESOURCE_BARRIER_TYPE_TRANSITION: i32 = 0;
pub const TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX: i32 = 0;
pub const TEXTURE_COPY_TYPE_PLACED_FOOTPRINT: i32 = 1;
pub const SRV_DIMENSION_BUFFER12: i32 = 1;

pub const FILL_MODE_SOLID12: i32 = 3;
pub const CULL_MODE_NONE12: i32 = 1;
pub const DEPTH_WRITE_MASK_ZERO: i32 = 0;
pub const COMPARISON_FUNC_ALWAYS: i32 = 8;
pub const STENCIL_OP_KEEP: i32 = 1;
pub const LOGIC_OP_NOOP: i32 = 4;
pub const INDEX_BUFFER_STRIP_CUT_VALUE_DISABLED: i32 = 0;
pub const PIPELINE_STATE_FLAG_NONE: u32 = 0;

pub const TEXTURE_DATA_PITCH_ALIGNMENT: u32 = 256;

pub const fn encode_component_mapping(x: u32, y: u32, z: u32, w: u32) -> u32 {
    const MASK: u32 = 0x7;
    const SHIFT: u32 = 3;
    (x & MASK)
        | ((y & MASK) << SHIFT)
        | ((z & MASK) << (SHIFT * 2))
        | ((w & MASK) << (SHIFT * 3))
        | (1 << (SHIFT * 4))
}

pub const DEFAULT_SHADER_4_COMPONENT_MAPPING: u32 = encode_component_mapping(0, 1, 2, 3);

#[repr(C)]
pub struct D12PageableVtbl {
    pub base: D12DeviceChildVtbl,
}

#[repr(C)]
pub struct D12ObjectVtbl {
    pub base: UnknownVtbl,
    pub _get_private_data: Slot,
    pub _set_private_data: Slot,
    pub _set_private_data_interface: Slot,
    pub _set_name: Slot,
}

#[repr(C)]
pub struct D12DeviceChildVtbl {
    pub base: D12ObjectVtbl,
    pub _get_device: Slot,
}

#[repr(C)]
pub struct D12DeviceVtbl {
    pub base: D12ObjectVtbl,
    pub _get_node_count: Slot,
    pub create_command_queue: unsafe extern "system" fn(*mut Unknown, *const CommandQueueDesc, *const [u8; 16], *mut *mut Unknown) -> Hresult,
    pub create_command_allocator: unsafe extern "system" fn(*mut Unknown, i32, *const [u8; 16], *mut *mut Unknown) -> Hresult,
    pub create_graphics_pipeline_state: unsafe extern "system" fn(*mut Unknown, *const GraphicsPipelineStateDesc, *const [u8; 16], *mut *mut Unknown) -> Hresult,
    pub _create_compute_pipeline_state: Slot,
    pub create_command_list: unsafe extern "system" fn(*mut Unknown, u32, i32, *mut Unknown, *mut Unknown, *const [u8; 16], *mut *mut Unknown) -> Hresult,
    pub check_feature_support:
        unsafe extern "system" fn(*mut Unknown, i32, *mut c_void, u32) -> Hresult,
    pub create_descriptor_heap: unsafe extern "system" fn(*mut Unknown, *const DescriptorHeapDesc, *const [u8; 16], *mut *mut Unknown) -> Hresult,
    pub get_descriptor_handle_increment_size: unsafe extern "system" fn(*mut Unknown, i32) -> u32,
    pub create_root_signature: unsafe extern "system" fn(*mut Unknown, u32, *const core::ffi::c_void, usize, *const [u8; 16], *mut *mut Unknown) -> Hresult,
    pub _create_constant_buffer_view: Slot,
    pub create_shader_resource_view: unsafe extern "system" fn(*mut Unknown, *mut Unknown, *const ShaderResourceViewDesc12, CpuDescriptorHandle),
    pub _create_unordered_access_view: Slot,
    pub create_render_target_view: unsafe extern "system" fn(*mut Unknown, *mut Unknown, *const core::ffi::c_void, CpuDescriptorHandle),
    pub _create_depth_stencil_view: Slot,
    pub _create_sampler: Slot,
    pub _copy_descriptors: Slot,
    pub _copy_descriptors_simple: Slot,
    pub _get_resource_allocation_info: Slot,
    pub _get_custom_heap_properties: Slot,
    pub create_committed_resource: unsafe extern "system" fn(*mut Unknown, *const HeapProperties, u32, *const ResourceDesc, i32, *const core::ffi::c_void, *const [u8; 16], *mut *mut Unknown) -> Hresult,
    pub _create_heap: Slot,
    pub _create_placed_resource: Slot,
    pub _create_reserved_resource: Slot,
    pub _create_shared_handle: Slot,
    pub _open_shared_handle: Slot,
    pub _open_shared_handle_by_name: Slot,
    pub _make_resident: Slot,
    pub _evict: Slot,
    pub create_fence: unsafe extern "system" fn(*mut Unknown, u64, u32, *const [u8; 16], *mut *mut Unknown) -> Hresult,
    pub _get_device_removed_reason: Slot,
    pub _get_copyable_footprints: Slot,
    pub _create_query_heap: Slot,
    pub _set_stable_power_state: Slot,
    pub _create_command_signature: Slot,
    pub _get_resource_tiling: Slot,
    pub get_adapter_luid:
        unsafe extern "system" fn(*mut Unknown, *mut [u32; 2]) -> *mut [u32; 2],
}

#[repr(C)]
pub struct D12QueueVtbl {
    pub base: D12PageableVtbl,
    pub _update_tile_mappings: Slot,
    pub _copy_tile_mappings: Slot,
    pub execute_command_lists: unsafe extern "system" fn(*mut Unknown, u32, *const *mut Unknown),
    pub _set_marker: Slot,
    pub _begin_event: Slot,
    pub _end_event: Slot,
    pub signal: unsafe extern "system" fn(*mut Unknown, *mut Unknown, u64) -> Hresult,
    pub _wait: Slot,
    pub _get_timestamp_frequency: Slot,
    pub _get_clock_calibration: Slot,
    pub _get_desc: Slot,
}

#[repr(C)]
pub struct D12AllocatorVtbl {
    pub base: D12PageableVtbl,
    pub reset: unsafe extern "system" fn(*mut Unknown) -> Hresult,
}

#[repr(C)]
pub struct D12HeapVtbl {
    pub base: D12PageableVtbl,
    pub _get_desc: Slot,
    pub get_cpu_descriptor_handle_for_heap_start:
        unsafe extern "system" fn(*mut Unknown, *mut CpuDescriptorHandle),
    pub get_gpu_descriptor_handle_for_heap_start:
        unsafe extern "system" fn(*mut Unknown, *mut GpuDescriptorHandle),
}

#[repr(C)]
pub struct D12FenceVtbl {
    pub base: D12PageableVtbl,
    pub get_completed_value: unsafe extern "system" fn(*mut Unknown) -> u64,
    pub set_event_on_completion:
        unsafe extern "system" fn(*mut Unknown, u64, *mut c_void) -> Hresult,
    pub signal: unsafe extern "system" fn(*mut Unknown, *mut Unknown, u64) -> Hresult,
}

#[repr(C)]
pub struct D12ResourceVtbl {
    pub base: D12PageableVtbl,
    pub map: unsafe extern "system" fn(*mut Unknown, u32, *const Range, *mut *mut core::ffi::c_void) -> Hresult,
    pub unmap: unsafe extern "system" fn(*mut Unknown, u32, *const Range),
    pub _get_desc: Slot,
    pub get_gpu_virtual_address: unsafe extern "system" fn(*mut Unknown) -> u64,
    pub _write_to_subresource: Slot,
    pub _read_from_subresource: Slot,
    pub _get_heap_properties: Slot,
}

#[repr(C)]
pub struct D12CommandListVtbl {
    pub base: D12DeviceChildVtbl,
    pub _get_type: Slot,
}

#[repr(C)]
pub struct D12GfxListVtbl {
    pub base: D12CommandListVtbl,
    pub close: unsafe extern "system" fn(*mut Unknown) -> Hresult,
    pub reset: unsafe extern "system" fn(*mut Unknown, *mut Unknown, *mut Unknown) -> Hresult,
    pub _clear_state: Slot,
    pub draw_instanced: unsafe extern "system" fn(*mut Unknown, u32, u32, u32, u32),
    pub _draw_indexed_instanced: Slot,
    pub _dispatch: Slot,
    pub _copy_buffer_region: Slot,
    pub copy_texture_region: unsafe extern "system" fn(*mut Unknown, *const TextureCopyLocation, u32, u32, u32, *const TextureCopyLocation, *const core::ffi::c_void),
    pub _copy_resource: Slot,
    pub _copy_tiles: Slot,
    pub _resolve_subresource: Slot,
    pub ia_set_primitive_topology: unsafe extern "system" fn(*mut Unknown, i32),
    pub rs_set_viewports: unsafe extern "system" fn(*mut Unknown, u32, *const Viewport),
    pub rs_set_scissor_rects: unsafe extern "system" fn(*mut Unknown, u32, *const Rect),
    pub _om_set_blend_factor: Slot,
    pub _om_set_stencil_ref: Slot,
    pub set_pipeline_state: unsafe extern "system" fn(*mut Unknown, *mut Unknown),
    pub resource_barrier: unsafe extern "system" fn(*mut Unknown, u32, *const ResourceBarrier),
    pub _execute_bundle: Slot,
    pub set_descriptor_heaps: unsafe extern "system" fn(*mut Unknown, u32, *const *mut Unknown),
    pub _set_compute_root_signature: Slot,
    pub set_graphics_root_signature: unsafe extern "system" fn(*mut Unknown, *mut Unknown),
    pub _set_compute_root_descriptor_table: Slot,
    pub set_graphics_root_descriptor_table: unsafe extern "system" fn(*mut Unknown, u32, GpuDescriptorHandle),
    pub _set_compute_root32_bit_constant: Slot,
    pub _set_graphics_root32_bit_constant: Slot,
    pub _set_compute_root32_bit_constants: Slot,
    pub _set_graphics_root32_bit_constants: Slot,
    pub _set_compute_root_constant_buffer_view: Slot,
    pub set_graphics_root_constant_buffer_view: unsafe extern "system" fn(*mut Unknown, u32, u64),
    pub _set_compute_root_shader_resource_view: Slot,
    pub _set_graphics_root_shader_resource_view: Slot,
    pub _set_compute_root_unordered_access_view: Slot,
    pub _set_graphics_root_unordered_access_view: Slot,
    pub _ia_set_index_buffer: Slot,
    pub _ia_set_vertex_buffers: Slot,
    pub _so_set_targets: Slot,
    pub om_set_render_targets: unsafe extern "system" fn(*mut Unknown, u32, *const CpuDescriptorHandle, i32, *const CpuDescriptorHandle),
    pub _clear_depth_stencil_view: Slot,
    pub clear_render_target_view: unsafe extern "system" fn(*mut Unknown, CpuDescriptorHandle, *const f32, u32, *const Rect),
    pub _clear_unordered_access_view_uint: Slot,
    pub _clear_unordered_access_view_float: Slot,
    pub _discard_resource: Slot,
    pub _begin_query: Slot,
    pub _end_query: Slot,
    pub _resolve_query_data: Slot,
    pub _set_predication: Slot,
    pub _set_marker: Slot,
    pub _begin_event: Slot,
    pub _end_event: Slot,
    pub _execute_indirect: Slot,
}

const _: () = assert!(size_of::<D12ObjectVtbl>() == (3 + 4) * PTR);
const _: () = assert!(size_of::<D12DeviceChildVtbl>() == (3 + 4 + 1) * PTR);
const _: () = assert!(size_of::<D12DeviceVtbl>() == (3 + 4 + 37) * PTR);
const _: () = assert!(size_of::<D12QueueVtbl>() == (8 + 11) * PTR);
const _: () = assert!(size_of::<D12AllocatorVtbl>() == (8 + 1) * PTR);
const _: () = assert!(size_of::<D12HeapVtbl>() == (8 + 3) * PTR);
const _: () = assert!(size_of::<D12FenceVtbl>() == (8 + 3) * PTR);
const _: () = assert!(size_of::<D12ResourceVtbl>() == (8 + 7) * PTR);
const _: () = assert!(size_of::<D12CommandListVtbl>() == (8 + 1) * PTR);
const _: () = assert!(size_of::<D12GfxListVtbl>() == (9 + 51) * PTR);
const _: () = assert!(DEFAULT_SHADER_4_COMPONENT_MAPPING == 5768);
const _: () = assert!(size_of::<RootParameter>() == 4 * PTR);
const _: () = assert!(size_of::<ResourceBarrier>() == 4 * PTR);

const _: () = assert!(size_of::<AdapterDesc>() == 304);
const _: () = assert!(size_of::<SampleDesc>() == 8);
const _: () = assert!(size_of::<ShaderMacro>() == 16);
const _: () = assert!(size_of::<BufferDesc>() == 24);
const _: () = assert!(size_of::<Texture2dDesc>() == 44);
const _: () = assert!(size_of::<SubresourceData>() == 16);
const _: () = assert!(size_of::<ShaderResourceViewDesc>() == 24);
const _: () = assert!(size_of::<RenderTargetBlendDesc>() == 32);
const _: () = assert!(size_of::<BlendDesc>() == 264);
const _: () = assert!(size_of::<RasterizerDesc>() == 40);
const _: () = assert!(size_of::<Viewport>() == 24);
const _: () = assert!(size_of::<MappedSubresource>() == 16);
const _: () = assert!(size_of::<QueryDesc>() == 8);
const _: () = assert!(size_of::<CpuDescriptorHandle>() == 8);
const _: () = assert!(size_of::<GpuDescriptorHandle>() == 8);
const _: () = assert!(size_of::<Range>() == 16);
const _: () = assert!(size_of::<CommandQueueDesc>() == 16);
const _: () = assert!(size_of::<DescriptorHeapDesc>() == 16);
const _: () = assert!(size_of::<HeapProperties>() == 20);
const _: () = assert!(size_of::<ResourceDesc>() == 56);
const _: () = assert!(size_of::<ShaderBytecode>() == 16);
const _: () = assert!(size_of::<StreamOutputDesc>() == 32);
const _: () = assert!(size_of::<RenderTargetBlendDesc12>() == 40);
const _: () = assert!(size_of::<BlendDesc12>() == 328);
const _: () = assert!(size_of::<RasterizerDesc12>() == 44);
const _: () = assert!(size_of::<DepthStencilOpDesc>() == 16);
const _: () = assert!(size_of::<DepthStencilDesc>() == 52);
const _: () = assert!(size_of::<InputLayoutDesc>() == 16);
const _: () = assert!(size_of::<CachedPipelineState>() == 16);
const _: () = assert!(size_of::<GraphicsPipelineStateDesc>() == 656);
const _: () = assert!(size_of::<DescriptorRange>() == 20);
const _: () = assert!(size_of::<RootParameter>() == 32);
const _: () = assert!(size_of::<RootDescriptorTable>() == 16);
const _: () = assert!(size_of::<RootDescriptor>() == 8);
const _: () = assert!(size_of::<RootSignatureDesc>() == 40);
const _: () = assert!(size_of::<ShaderResourceViewDesc12>() == 40);
const _: () = assert!(size_of::<ResourceBarrier>() == 32);
const _: () = assert!(size_of::<SubresourceFootprint>() == 20);
const _: () = assert!(size_of::<PlacedSubresourceFootprint>() == 32);
const _: () = assert!(size_of::<TextureCopyLocation>() == 48);
