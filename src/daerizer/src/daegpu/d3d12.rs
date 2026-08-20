// SAFETY, once for the file. As d3d11: checked calls, released once, locals outlive their call,
// vtable casts match the creating call. D3D12 adds one the compiler cannot see: a command list
// must be recording, and every resource must already be in the state its command requires –
// which `barrier` is what maintains, tracked per target in `state`.

use super::direct3d as d3d;

pub use super::direct3d::{RESOURCE_STATE_COPY_SOURCE, RESOURCE_STATE_RENDER_TARGET};
use super::{GpuBatch, binding};
use alloc::string::String;
use alloc::vec::Vec;
use core::cell::{Cell, RefCell};
use core::ffi::{CStr, c_void};

pub use super::Mode;

#[derive(Debug)]
pub enum Error {
    NoDevice,
    Call {
        what: &'static str,
        hr: i32,
    },
    MissingEntryPoint(&'static str),
    Shader(String),
    BadTarget,
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Error::NoDevice => write!(f, "no Direct3D 12 device"),
            Error::Call { what, hr } => write!(f, "{what} failed: HRESULT 0x{hr:08x}"),
            Error::MissingEntryPoint(name) => write!(f, "entry point {name} is missing"),
            Error::Shader(text) => write!(f, "shader compilation failed: {text}"),
            Error::BadTarget => write!(f, "target does not belong to this device, or has no area"),
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for Error {}

fn check(what: &'static str, hr: d3d::Hresult) -> Result<(), Error> {
    if d3d::succeeded(hr) { Ok(()) } else { Err(Error::Call { what, hr }) }
}

const SRV_COUNT: u32 = 6;

const _: () = {
    assert!(binding::CURVES == 0);
    assert!(binding::BAND_CURVES == 1);
    assert!(binding::BANDS == 2);
    assert!(binding::INSTANCES == 3);
    assert!(binding::SUBPIXEL == 4);
    assert!(binding::HULL == 5);
};

struct Buffer {
    resource: *mut d3d::Unknown,
    mapped: *mut u8,
    stride: u32,
    count: u32,
}

impl Buffer {
    const EMPTY: Buffer =
        Buffer { resource: core::ptr::null_mut(), mapped: core::ptr::null_mut(), stride: 0, count: 0 };

    fn write<T: Copy>(&self, data: &[T]) {
        let bytes = size_of_val(data);
        if bytes == 0 || self.mapped.is_null() {
            return;
        }
        unsafe { core::ptr::copy_nonoverlapping(data.as_ptr().cast::<u8>(), self.mapped, bytes) };
    }

    unsafe fn destroy(&self) {
        unsafe {
            if !self.resource.is_null() {
                let v = &*(*self.resource).vtable.cast::<d3d::D12ResourceVtbl>();
                (v.unmap)(self.resource, 0, core::ptr::null());
            }
            d3d::release(self.resource);
        }
    }
}

struct PerDraw {
    instances: Buffer,
    subpixel: Buffer,
    projection: Buffer,
}

pub struct Renderer {
    _d3d12: d3d::Library,
    _compiler: d3d::Library,
    compile: d3d::PfnCompile,
    device: *mut d3d::Unknown,
    adapter: String,
    software: bool,
    queue: *mut d3d::Unknown,
    allocator: *mut d3d::Unknown,
    list: *mut d3d::Unknown,
    fence: *mut d3d::Unknown,
    fence_value: Cell<u64>,
    event: Option<d3d::Event>,
    root: *mut d3d::Unknown,
    grayscale: *mut d3d::Unknown,
    subpixel: *mut d3d::Unknown,
    srv_heap: *mut d3d::Unknown,
    srv_stride: u32,
    per_draw: RefCell<PerDraw>,
    feature_level: i32,
}

impl Renderer {
    pub fn new() -> Result<Renderer, Error> {
        let Some(d3d12) = d3d::Library::open(c"d3d12.dll") else {
            return Err(Error::NoDevice);
        };
        let Some(compiler) = d3d::Library::open(c"d3dcompiler_47.dll") else {
            return Err(Error::MissingEntryPoint("d3dcompiler_47.dll"));
        };
        let create: d3d::PfnCreateDevice12 = unsafe { d3d12.symbol(c"D3D12CreateDevice") }
            .ok_or(Error::MissingEntryPoint("D3D12CreateDevice"))?;
        let compile: d3d::PfnCompile = unsafe { compiler.symbol(c"D3DCompile") }
            .ok_or(Error::MissingEntryPoint("D3DCompile"))?;

        let mut device = core::ptr::null_mut();
        let mut feature_level = 0;
        for level in [
            d3d::FEATURE_LEVEL_12_1,
            d3d::FEATURE_LEVEL_12_0,
            d3d::FEATURE_LEVEL_11_1,
            d3d::FEATURE_LEVEL_11_0,
        ] {
            let hr = unsafe {
                create(core::ptr::null_mut(), level, &d3d::IID_D3D12_DEVICE, &mut device)
            };
            if d3d::succeeded(hr) && !device.is_null() {
                feature_level = level;
                break;
            }
            device = core::ptr::null_mut();
        }
        if device.is_null() {
            return Err(Error::NoDevice);
        }

        let (adapter, software) = unsafe { Self::adapter_of(device) };

        let mut r = Renderer {
            _d3d12: d3d12,
            _compiler: compiler,
            compile,
            device,
            adapter,
            software,
            queue: core::ptr::null_mut(),
            allocator: core::ptr::null_mut(),
            list: core::ptr::null_mut(),
            fence: core::ptr::null_mut(),
            fence_value: Cell::new(0),
            event: d3d::Event::new(),
            root: core::ptr::null_mut(),
            grayscale: core::ptr::null_mut(),
            subpixel: core::ptr::null_mut(),
            srv_heap: core::ptr::null_mut(),
            srv_stride: 0,
            per_draw: RefCell::new(PerDraw {
                instances: Buffer::EMPTY,
                subpixel: Buffer::EMPTY,
                projection: Buffer::EMPTY,
            }),
            feature_level,
        };
        r.build()?;
        Ok(r)
    }

    pub fn profile(&self) -> crate::daerizer::draw::DeviceProfile {
        crate::daerizer::draw::DeviceProfile::from_d3d(
            self.software,
            unsafe { Self::is_uma(self.device) },
            self.device_name(),
        )
    }

    unsafe fn adapter_of(device: *mut d3d::Unknown) -> (String, bool) {
        let unknown = || (String::from("Direct3D 12"), false);
        let Some(dxgi) = d3d::Library::open(c"dxgi.dll") else { return unknown() };
        let Some(create): Option<d3d::PfnCreateDxgiFactory> =
            (unsafe { dxgi.symbol(c"CreateDXGIFactory") })
        else {
            return unknown();
        };

        unsafe {
            let mut luid = [0u32; 2];
            let v = &*(*device).vtable.cast::<d3d::D12DeviceVtbl>();
            (v.get_adapter_luid)(device, &mut luid);

            let mut factory = core::ptr::null_mut();
            let hr = create(&d3d::IID_DXGI_FACTORY, &mut factory);
            if !d3d::succeeded(hr) || factory.is_null() {
                return unknown();
            }
            let fv = &*(*factory).vtable.cast::<d3d::DxgiFactoryVtbl>();

            let mut found = unknown();
            for index in 0..64u32 {
                let mut adapter = core::ptr::null_mut();
                if !d3d::succeeded((fv.enum_adapters)(factory, index, &mut adapter))
                    || adapter.is_null()
                {
                    break;
                }
                let mut desc: d3d::AdapterDesc = core::mem::zeroed();
                let av = &*(*adapter).vtable.cast::<d3d::DxgiAdapterVtbl>();
                let ok = d3d::succeeded((av.get_desc)(adapter, &mut desc));
                d3d::release(adapter);
                if ok && desc.adapter_luid == luid {
                    let end = desc
                        .description
                        .iter()
                        .position(|&c| c == 0)
                        .unwrap_or(desc.description.len());
                    found = (
                        String::from_utf16_lossy(&desc.description[..end]),
                        desc.vendor_id == d3d::VENDOR_MICROSOFT,
                    );
                    break;
                }
            }
            d3d::release(factory);
            found
        }
    }

    unsafe fn is_uma(device: *mut d3d::Unknown) -> Option<bool> {
        let mut data = d3d::FeatureDataArchitecture::default();
        let hr = unsafe {
            let v = &*(*device).vtable.cast::<d3d::D12DeviceVtbl>();
            (v.check_feature_support)(
                device,
                d3d::FEATURE_ARCHITECTURE12,
                core::ptr::from_mut(&mut data).cast::<c_void>(),
                size_of::<d3d::FeatureDataArchitecture>() as u32,
            )
        };
        d3d::succeeded(hr).then_some(data.uma != 0)
    }

    pub fn is_software(&self) -> bool {
        self.software
    }

    pub fn device_name(&self) -> String {
        self.adapter.clone()
    }

    pub fn feature_level(&self) -> &'static str {
        match self.feature_level {
            d3d::FEATURE_LEVEL_12_1 => "12_1",
            d3d::FEATURE_LEVEL_12_0 => "12_0",
            d3d::FEATURE_LEVEL_11_1 => "11_1",
            d3d::FEATURE_LEVEL_11_0 => "11_0",
            _ => "unknown",
        }
    }

    fn dev(&self) -> &d3d::D12DeviceVtbl {
        unsafe { &*(*self.device).vtable.cast::<d3d::D12DeviceVtbl>() }
    }

    fn cmd(&self) -> &d3d::D12GfxListVtbl {
        unsafe { &*(*self.list).vtable.cast::<d3d::D12GfxListVtbl>() }
    }

    fn build(&mut self) -> Result<(), Error> {
        let queue_desc = d3d::CommandQueueDesc {
            kind: d3d::COMMAND_LIST_TYPE_DIRECT,
            ..Default::default()
        };
        let heap_desc = d3d::DescriptorHeapDesc {
            kind: d3d::DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV,
            num_descriptors: SRV_COUNT,
            flags: d3d::DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE,
            node_mask: 0,
        };
        let mut queue = core::ptr::null_mut();
        {
            let d = self.dev();
            let hr = unsafe {
                (d.create_command_queue)(self.device, &queue_desc, &d3d::IID_D12_QUEUE, &mut queue)
            };
            check("CreateCommandQueue", hr)?;
        }
        self.queue = queue;

        let mut allocator = core::ptr::null_mut();
        {
            let d = self.dev();
            let hr = unsafe {
                (d.create_command_allocator)(
                    self.device, d3d::COMMAND_LIST_TYPE_DIRECT, &d3d::IID_D12_ALLOCATOR,
                    &mut allocator,
                )
            };
            check("CreateCommandAllocator", hr)?;
        }
        self.allocator = allocator;

        let mut list = core::ptr::null_mut();
        {
            let d = self.dev();
            let hr = unsafe {
                (d.create_command_list)(
                    self.device, 0, d3d::COMMAND_LIST_TYPE_DIRECT, self.allocator,
                    core::ptr::null_mut(), &d3d::IID_D12_GFX_LIST, &mut list,
                )
            };
            check("CreateCommandList", hr)?;
        }
        self.list = list;
        check("Close", unsafe { (self.cmd().close)(self.list) })?;

        let mut fence = core::ptr::null_mut();
        {
            let d = self.dev();
            let hr =
                unsafe { (d.create_fence)(self.device, 0, 0, &d3d::IID_D12_FENCE, &mut fence) };
            check("CreateFence", hr)?;
        }
        self.fence = fence;

        let mut srv_heap = core::ptr::null_mut();
        {
            let d = self.dev();
            let hr = unsafe {
                (d.create_descriptor_heap)(
                    self.device, &heap_desc, &d3d::IID_D12_DESCRIPTOR_HEAP, &mut srv_heap,
                )
            };
            check("CreateDescriptorHeap", hr)?;
        }
        self.srv_heap = srv_heap;
        self.srv_stride = unsafe {
            (self.dev().get_descriptor_handle_increment_size)(
                self.device, d3d::DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV,
            )
        };

        self.build_root_signature()?;
        self.build_pipelines()?;

        let subpixel_bytes = size_of::<super::SubpixelParams>() as u64;
        let instance_bytes = size_of::<super::GlyphInstance>() as u64;
        let (subpixel, instances, projection) = (
            self.upload(subpixel_bytes, 1)?,
            self.upload(instance_bytes, 1)?,
            self.upload(256, 1)?,
        );
        let mut per = self.per_draw.borrow_mut();
        per.subpixel = subpixel;
        per.instances = instances;
        per.projection = projection;
        Ok(())
    }

    fn build_root_signature(&mut self) -> Result<(), Error> {
        let serialize: d3d::PfnSerializeRootSignature =
            unsafe { self._d3d12.symbol(c"D3D12SerializeRootSignature") }
                .ok_or(Error::MissingEntryPoint("D3D12SerializeRootSignature"))?;

        let range = d3d::DescriptorRange {
            range_type: d3d::DESCRIPTOR_RANGE_TYPE_SRV,
            num_descriptors: SRV_COUNT,
            base_shader_register: 0,
            register_space: 0,
            offset_in_descriptors_from_table_start: 0,
        };
        let params = [
            d3d::RootParameter {
                parameter_type: d3d::ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE,
                value: d3d::RootParameterValue {
                    table: d3d::RootDescriptorTable {
                        num_descriptor_ranges: 1,
                        descriptor_ranges: &range,
                    },
                },
                shader_visibility: d3d::SHADER_VISIBILITY_ALL,
            },
            d3d::RootParameter {
                parameter_type: d3d::ROOT_PARAMETER_TYPE_CBV,
                value: d3d::RootParameterValue {
                    descriptor: d3d::RootDescriptor { shader_register: 0, register_space: 0 },
                },
                shader_visibility: d3d::SHADER_VISIBILITY_ALL,
            },
        ];
        let desc = d3d::RootSignatureDesc {
            num_parameters: params.len() as u32,
            parameters: params.as_ptr(),
            num_static_samplers: 0,
            static_samplers: core::ptr::null(),
            flags: d3d::ROOT_SIGNATURE_FLAG_NONE,
        };

        let mut blob = core::ptr::null_mut();
        let mut errors = core::ptr::null_mut();
        let hr = unsafe {
            serialize(&desc, d3d::ROOT_SIGNATURE_VERSION_1, &mut blob, &mut errors)
        };
        let (blob, errors) =
            unsafe { (d3d::Blob::from_raw(blob), d3d::Blob::from_raw(errors)) };
        let message = errors.as_ref().map(d3d::Blob::text).unwrap_or_default();

        let Some(blob) = blob.filter(|_| d3d::succeeded(hr)) else {
            return Err(Error::Shader(if message.is_empty() {
                alloc::format!("D3D12SerializeRootSignature: HRESULT 0x{hr:08x}")
            } else {
                message
            }));
        };

        let bytes = blob.bytes();
        let mut root = core::ptr::null_mut();
        let hr = unsafe {
            (self.dev().create_root_signature)(
                self.device, 0, bytes.as_ptr().cast::<c_void>(), bytes.len(),
                &d3d::IID_D12_ROOT_SIGNATURE, &mut root,
            )
        };
        check("CreateRootSignature", hr)?;
        self.root = root;
        Ok(())
    }

    fn build_pipelines(&mut self) -> Result<(), Error> {
        use super::ShaderStage::{Fragment, SubpixelFragment, Vertex};
        let vs = self.compile_stage(Vertex, c"vs_5_1")?;
        let ps_gray = self.compile_stage(Fragment, c"ps_5_1")?;
        let ps_sub = self.compile_stage(SubpixelFragment, c"ps_5_1")?;

        let mut none = d3d::BlendDesc12::default();
        none.render_target[0].logic_op = d3d::LOGIC_OP_NOOP;
        none.render_target[0].render_target_write_mask = 15;
        let mut dual = d3d::BlendDesc12::default();
        dual.render_target[0] = d3d::RenderTargetBlendDesc12 {
            blend_enable: 1,
            logic_op_enable: 0,
            src_blend: d3d::BLEND_SRC1_COLOR,
            dest_blend: d3d::BLEND_INV_SRC1_COLOR,
            blend_op: d3d::BLEND_OP_ADD,
            src_blend_alpha: d3d::BLEND_SRC1_ALPHA,
            dest_blend_alpha: d3d::BLEND_INV_SRC1_ALPHA,
            blend_op_alpha: d3d::BLEND_OP_ADD,
            logic_op: d3d::LOGIC_OP_NOOP,
            render_target_write_mask: 15,
        };
        let rasterizer = d3d::RasterizerDesc12 {
            fill_mode: d3d::FILL_MODE_SOLID12,
            cull_mode: d3d::CULL_MODE_NONE12,
            depth_clip_enable: 1,
            conservative_raster: 0,
            ..Default::default()
        };
        let depth = d3d::DepthStencilDesc {
            depth_enable: 0,
            depth_write_mask: d3d::DEPTH_WRITE_MASK_ZERO,
            depth_func: d3d::COMPARISON_FUNC_ALWAYS,
            stencil_enable: 0,
            stencil_read_mask: 0,
            stencil_write_mask: 0,
            front_face: d3d::DepthStencilOpDesc {
                stencil_fail_op: d3d::STENCIL_OP_KEEP,
                stencil_depth_fail_op: d3d::STENCIL_OP_KEEP,
                stencil_pass_op: d3d::STENCIL_OP_KEEP,
                stencil_func: d3d::COMPARISON_FUNC_ALWAYS,
            },
            back_face: d3d::DepthStencilOpDesc {
                stencil_fail_op: d3d::STENCIL_OP_KEEP,
                stencil_depth_fail_op: d3d::STENCIL_OP_KEEP,
                stencil_pass_op: d3d::STENCIL_OP_KEEP,
                stencil_func: d3d::COMPARISON_FUNC_ALWAYS,
            },
        };

        let mut rtv_formats = [0i32; 8];
        rtv_formats[0] = d3d::FORMAT_R8G8B8A8_UNORM;
        let base = d3d::GraphicsPipelineStateDesc {
            root_signature: self.root,
            vs: d3d::ShaderBytecode { bytecode: vs.as_ptr().cast::<c_void>(), length: vs.len() },
            ps: d3d::ShaderBytecode::default(),
            ds: d3d::ShaderBytecode::default(),
            hs: d3d::ShaderBytecode::default(),
            gs: d3d::ShaderBytecode::default(),
            stream_output: d3d::StreamOutputDesc::default(),
            blend_state: none,
            sample_mask: u32::MAX,
            rasterizer_state: rasterizer,
            depth_stencil_state: depth,
            input_layout: d3d::InputLayoutDesc::default(),
            ib_strip_cut_value: d3d::INDEX_BUFFER_STRIP_CUT_VALUE_DISABLED,
            primitive_topology_type: d3d::PRIMITIVE_TOPOLOGY_TYPE_TRIANGLE,
            num_render_targets: 1,
            rtv_formats,
            dsv_format: d3d::FORMAT_UNKNOWN,
            sample_desc: d3d::SampleDesc { count: 1, quality: 0 },
            node_mask: 0,
            cached_pso: d3d::CachedPipelineState::default(),
            flags: d3d::PIPELINE_STATE_FLAG_NONE,
        };

        for (ps, blend, out) in [
            (&ps_gray, none, 0usize),
            (&ps_sub, dual, 1usize),
        ] {
            let desc = d3d::GraphicsPipelineStateDesc {
                ps: d3d::ShaderBytecode { bytecode: ps.as_ptr().cast::<c_void>(), length: ps.len() },
                blend_state: blend,
                ..base
            };
            let mut pso = core::ptr::null_mut();
            let hr = unsafe {
                (self.dev().create_graphics_pipeline_state)(
                    self.device, &desc, &d3d::IID_D12_PIPELINE_STATE, &mut pso,
                )
            };
            check("CreateGraphicsPipelineState", hr)?;
            if out == 0 { self.grayscale = pso } else { self.subpixel = pso }
        }
        Ok(())
    }

    fn compile_stage(&self, stage: super::ShaderStage, target: &CStr) -> Result<Vec<u8>, Error> {
        let source = super::shader(super::ShaderLanguage::Hlsl, stage);
        let mut code = core::ptr::null_mut();
        let mut errors = core::ptr::null_mut();
        let hr = unsafe {
            (self.compile)(
                source.as_ptr().cast::<c_void>(),
                source.len(),
                c"daegun.hlsl".as_ptr(),
                core::ptr::null(),
                core::ptr::null_mut(),
                c"main".as_ptr(),
                target.as_ptr(),
                0,
                0,
                &mut code,
                &mut errors,
            )
        };
        let (code, errors) = unsafe { (d3d::Blob::from_raw(code), d3d::Blob::from_raw(errors)) };
        let message = errors.as_ref().map(d3d::Blob::text).unwrap_or_default();

        let Some(code) = code.filter(|_| d3d::succeeded(hr)) else {
            return Err(Error::Shader(if message.is_empty() {
                alloc::format!("HRESULT 0x{hr:08x}")
            } else {
                message
            }));
        };
        Ok(code.bytes().to_vec())
    }

    fn upload(&self, stride: u64, count: u32) -> Result<Buffer, Error> {
        let props = d3d::HeapProperties { kind: d3d::HEAP_TYPE_UPLOAD, ..Default::default() };
        let desc = d3d::ResourceDesc {
            dimension: d3d::RESOURCE_DIMENSION_BUFFER,
            alignment: 0,
            width: (stride * u64::from(count.max(1))).max(4),
            height: 1,
            depth_or_array_size: 1,
            mip_levels: 1,
            format: d3d::FORMAT_UNKNOWN,
            sample_desc: d3d::SampleDesc { count: 1, quality: 0 },
            layout: d3d::TEXTURE_LAYOUT_ROW_MAJOR,
            flags: d3d::RESOURCE_FLAG_NONE,
        };
        let mut resource = core::ptr::null_mut();
        check("CreateCommittedResource (upload)", unsafe {
            (self.dev().create_committed_resource)(
                self.device, &props, 0, &desc, d3d::RESOURCE_STATE_GENERIC_READ,
                core::ptr::null(), &d3d::IID_D12_RESOURCE, &mut resource,
            )
        })?;

        let mut mapped: *mut c_void = core::ptr::null_mut();
        let nothing_read = d3d::Range { begin: 0, end: 0 };
        let hr = unsafe {
            let v = &*(*resource).vtable.cast::<d3d::D12ResourceVtbl>();
            (v.map)(resource, 0, &nothing_read, &mut mapped)
        };
        if let Err(e) = check("Map (upload)", hr) {
            unsafe { d3d::release(resource) };
            return Err(e);
        }
        Ok(Buffer { resource, mapped: mapped.cast::<u8>(), stride: stride as u32, count: count.max(1) })
    }

    unsafe fn write_srv(&self, index: u32, resource: *mut d3d::Unknown, stride: u32, count: u32) {
        let desc = d3d::ShaderResourceViewDesc12 {
            format: d3d::FORMAT_UNKNOWN,
            view_dimension: d3d::SRV_DIMENSION_BUFFER12,
            shader4_component_mapping: d3d::DEFAULT_SHADER_4_COMPONENT_MAPPING,
            first_element: 0,
            num_elements: count.max(1),
            structure_byte_stride: stride,
            flags: 0,
        };
        unsafe {
            let v = &*(*self.srv_heap).vtable.cast::<d3d::D12HeapVtbl>();
            let mut handle = d3d::CpuDescriptorHandle::default();
            (v.get_cpu_descriptor_handle_for_heap_start)(self.srv_heap, &mut handle);
            handle.ptr += (index * self.srv_stride) as usize;
            (self.dev().create_shader_resource_view)(self.device, resource, &desc, handle);
        }
    }

    fn submit(&self) -> Result<(), Error> {
        unsafe {
            (self.queue_vtbl().execute_command_lists)(self.queue, 1, &self.list);
        }
        self.signal()?;
        self.wait_for_gpu();
        Ok(())
    }

    fn queue_vtbl(&self) -> &d3d::D12QueueVtbl {
        unsafe { &*(*self.queue).vtable.cast::<d3d::D12QueueVtbl>() }
    }

    fn signal(&self) -> Result<(), Error> {
        let next = self.fence_value.get() + 1;
        self.fence_value.set(next);
        check("Signal", unsafe { (self.queue_vtbl().signal)(self.queue, self.fence, next) })
    }

    fn wait_for_gpu(&self) {
        unsafe { wait_fence(self.fence, self.fence_value.get(), self.event.as_ref()) };
    }

    fn begin(&self, pipeline: *mut d3d::Unknown) -> Result<(), Error> {
        self.wait_for_gpu();
        unsafe {
            let a = &*(*self.allocator).vtable.cast::<d3d::D12AllocatorVtbl>();
            check("Reset (allocator)", (a.reset)(self.allocator))?;
            check("Reset (list)", (self.cmd().reset)(self.list, self.allocator, pipeline))?;
        }
        Ok(())
    }

    // The single point where a resource's state changes, so `from` is whatever the caller last set
    // and `state` stays the truth. Transitioning from a state the resource is not in is undefined,
    // not an error the runtime reports.
    unsafe fn barrier(&self, resource: *mut d3d::Unknown, from: i32, to: i32) {
        if from == to {
            return;
        }
        let b = d3d::ResourceBarrier {
            barrier_type: d3d::RESOURCE_BARRIER_TYPE_TRANSITION,
            flags: 0,
            resource,
            subresource: 0,
            state_before: from,
            state_after: to,
        };
        unsafe { (self.cmd().resource_barrier)(self.list, 1, &b) };
    }
}

unsafe fn wait_fence(fence: *mut d3d::Unknown, value: u64, event: Option<&d3d::Event>) {
    unsafe {
        let v = &*(*fence).vtable.cast::<d3d::D12FenceVtbl>();
        if (v.get_completed_value)(fence) >= value {
            return;
        }
        if let Some(event) = event
            && d3d::succeeded((v.set_event_on_completion)(fence, value, event.handle()))
        {
            event.wait();
            return;
        }
        while (v.get_completed_value)(fence) < value {
            core::hint::spin_loop();
        }
    }
}

fn align_up(value: u32, to: u32) -> u32 {
    (value + to - 1) & !(to - 1)
}

impl Drop for Renderer {
    fn drop(&mut self) {
        if !self.fence.is_null() && !self.queue.is_null() {
            self.wait_for_gpu();
        }
        let per = self.per_draw.borrow();
        unsafe {
            per.instances.destroy();
            per.subpixel.destroy();
            per.projection.destroy();
            d3d::release(self.srv_heap);
            d3d::release(self.subpixel);
            d3d::release(self.grayscale);
            d3d::release(self.root);
            d3d::release(self.fence);
            d3d::release(self.list);
            d3d::release(self.allocator);
            d3d::release(self.queue);
            d3d::release(self.device);
        }
    }
}

pub struct Target {
    texture: *mut d3d::Unknown,
    rtv_heap: *mut d3d::Unknown,
    rtv: d3d::CpuDescriptorHandle,
    readback: *mut d3d::Unknown,
    width: u32,
    height: u32,
    row_pitch: u32,
    state: Cell<i32>,
    pixels: Vec<u8>,
    device: *mut d3d::Unknown,
    pending: Option<(*mut d3d::Unknown, u64)>,
}

impl Target {
    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    pub fn pixel(&self, x: u32, y: u32) -> Option<[u8; 4]> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let i = (y as usize * self.width as usize + x as usize) * 4;
        let p = self.pixels();
        Some([*p.get(i)?, *p.get(i + 1)?, *p.get(i + 2)?, *p.get(i + 3)?])
    }

    pub unsafe fn texture(&self) -> (*mut core::ffi::c_void, i32) {
        (self.texture.cast(), self.state.get())
    }
}

impl Drop for Target {
    fn drop(&mut self) {
        unsafe {
            if let Some((fence, _)) = self.pending.take() {
                d3d::release(fence);
            }
            d3d::release(self.readback);
            d3d::release(self.rtv_heap);
            d3d::release(self.texture);
            d3d::release(self.device);
        }
    }
}

pub struct Geometry {
    revision: u64,
    curves: Buffer,
    band_curves: Buffer,
    bands: Buffer,
    hulls: Buffer,
    device: *mut d3d::Unknown,
}

impl Geometry {
    pub fn revision(&self) -> u64 {
        self.revision
    }
}

impl Drop for Geometry {
    fn drop(&mut self) {
        unsafe {
            for b in [&self.curves, &self.band_curves, &self.bands, &self.hulls] {
                b.destroy();
            }
            d3d::release(self.device);
        }
    }
}

pub fn ortho(width: u32, height: u32) -> [f32; 16] {
    let w = width.max(1) as f32;
    let h = height.max(1) as f32;
    [
        2.0 / w, 0.0, 0.0, 0.0,
        0.0, 2.0 / h, 0.0, 0.0,
        0.0, 0.0, 1.0, 0.0,
        -1.0, -1.0, 0.0, 1.0,
    ]
}

impl Renderer {
    pub fn target(&self, width: u32, height: u32) -> Result<Target, Error> {
        if width == 0 || height == 0 {
            return Err(Error::BadTarget);
        }
        let row_pitch = align_up(width * 4, d3d::TEXTURE_DATA_PITCH_ALIGNMENT);

        let tex_props = d3d::HeapProperties { kind: d3d::HEAP_TYPE_DEFAULT, ..Default::default() };
        let tex_desc = d3d::ResourceDesc {
            dimension: d3d::RESOURCE_DIMENSION_TEXTURE2D,
            alignment: 0,
            width: u64::from(width),
            height,
            depth_or_array_size: 1,
            mip_levels: 1,
            format: d3d::FORMAT_R8G8B8A8_UNORM,
            sample_desc: d3d::SampleDesc { count: 1, quality: 0 },
            layout: d3d::TEXTURE_LAYOUT_UNKNOWN,
            flags: d3d::RESOURCE_FLAG_ALLOW_RENDER_TARGET,
        };
        let back_props = d3d::HeapProperties { kind: d3d::HEAP_TYPE_READBACK, ..Default::default() };
        let back_desc = d3d::ResourceDesc {
            dimension: d3d::RESOURCE_DIMENSION_BUFFER,
            alignment: 0,
            width: u64::from(row_pitch) * u64::from(height),
            height: 1,
            depth_or_array_size: 1,
            mip_levels: 1,
            format: d3d::FORMAT_UNKNOWN,
            sample_desc: d3d::SampleDesc { count: 1, quality: 0 },
            layout: d3d::TEXTURE_LAYOUT_ROW_MAJOR,
            flags: d3d::RESOURCE_FLAG_NONE,
        };
        let heap_desc = d3d::DescriptorHeapDesc {
            kind: d3d::DESCRIPTOR_HEAP_TYPE_RTV,
            num_descriptors: 1,
            flags: d3d::DESCRIPTOR_HEAP_FLAG_NONE,
            node_mask: 0,
        };

        let d = self.dev();
        let (mut texture, mut readback, mut rtv_heap) =
            (core::ptr::null_mut(), core::ptr::null_mut(), core::ptr::null_mut());
        let made = unsafe {
            check("CreateCommittedResource (texture)", (d.create_committed_resource)(
                self.device, &tex_props, 0, &tex_desc, d3d::RESOURCE_STATE_RENDER_TARGET,
                core::ptr::null(), &d3d::IID_D12_RESOURCE, &mut texture,
            ))
            .and_then(|()| check("CreateCommittedResource (readback)", (d.create_committed_resource)(
                self.device, &back_props, 0, &back_desc, d3d::RESOURCE_STATE_COPY_DEST,
                core::ptr::null(), &d3d::IID_D12_RESOURCE, &mut readback,
            )))
            .and_then(|()| check("CreateDescriptorHeap (rtv)", (d.create_descriptor_heap)(
                self.device, &heap_desc, &d3d::IID_D12_DESCRIPTOR_HEAP, &mut rtv_heap,
            )))
        };
        if let Err(e) = made {
            unsafe {
                d3d::release(rtv_heap);
                d3d::release(readback);
                d3d::release(texture);
            }
            return Err(e);
        }

        let rtv = unsafe {
            let v = &*(*rtv_heap).vtable.cast::<d3d::D12HeapVtbl>();
            let mut handle = d3d::CpuDescriptorHandle::default();
            (v.get_cpu_descriptor_handle_for_heap_start)(rtv_heap, &mut handle);
            (d.create_render_target_view)(self.device, texture, core::ptr::null(), handle);
            handle
        };

        let target = Target {
            texture,
            rtv_heap,
            rtv,
            readback,
            width,
            height,
            row_pitch,
            state: Cell::new(d3d::RESOURCE_STATE_RENDER_TARGET),
            pixels: Vec::new(),
            device: unsafe { d3d::add_ref(self.device) },
            pending: None,
        };

        self.begin(core::ptr::null_mut())?;
        unsafe {
            (self.cmd().clear_render_target_view)(
                self.list, target.rtv, [0.0f32; 4].as_ptr(), 0, core::ptr::null(),
            );
            self.barrier(
                target.texture,
                d3d::RESOURCE_STATE_RENDER_TARGET,
                d3d::RESOURCE_STATE_COPY_SOURCE,
            );
            check("Close", (self.cmd().close)(self.list))?;
        }
        target.state.set(d3d::RESOURCE_STATE_COPY_SOURCE);
        self.submit()?;
        Ok(target)
    }

    pub fn geometry(&self, batch: &GpuBatch) -> Result<Geometry, Error> {
        let mut made: Vec<Buffer> = Vec::with_capacity(4);
        let mut failure = None;
        for r in [
            self.structured(batch.curves()),
            self.structured(batch.band_curves()),
            self.structured(batch.bands()),
            self.structured(batch.hulls()),
        ] {
            match r {
                Ok(b) => made.push(b),
                Err(e) => failure = failure.or(Some(e)),
            }
        }
        if let Some(e) = failure {
            unsafe {
                for b in &made {
                    b.destroy();
                }
            }
            return Err(e);
        }
        let mut it = made.into_iter();
        #[expect(clippy::expect_used, reason = "the merge brought `daegun`'s no-panic lint over this code, and it is the right lint: a font is untrusted input and a panic is a denial of service. This is not that. The loop directly above made exactly four buffers and returned early on failure, so the `None` arm is unreachable rather than merely unlikely")]
        let mut next = || it.next().expect("all four were made, or the failure above returned");
        Ok(Geometry {
            revision: batch.revision(),
            curves: next(),
            band_curves: next(),
            bands: next(),
            hulls: next(),
            device: unsafe { d3d::add_ref(self.device) },
        })
    }

    fn structured<T: Copy>(&self, data: &[T]) -> Result<Buffer, Error> {
        let buffer = self.upload(size_of::<T>() as u64, data.len().max(1) as u32)?;
        buffer.write(data);
        Ok(buffer)
    }

    pub fn draw(
        &self,
        target: &mut Target,
        geometry: &Geometry,
        instances: &[super::GlyphInstance],
        subpixel: &super::SubpixelParams,
        mode: Mode,
    ) -> Result<(), Error> {
        let projection = ortho(target.width, target.height);
        self.draw_with(target, geometry, instances, subpixel, mode, &projection)
    }

    pub fn draw_with(
        &self,
        target: &mut Target,
        geometry: &Geometry,
        instances: &[super::GlyphInstance],
        subpixel: &super::SubpixelParams,
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
        let pipeline = match mode {
            Mode::Grayscale => self.grayscale,
            Mode::Subpixel => self.subpixel,
        };
        self.begin(pipeline)?;

        let mut per = self.per_draw.borrow_mut();
        let wanted = instances.len().max(1) as u32;
        if per.instances.count < wanted {
            let bigger = self.upload(size_of::<super::GlyphInstance>() as u64, wanted)?;
            unsafe { per.instances.destroy() };
            per.instances = bigger;
        }
        per.instances.write(instances);
        per.subpixel.write(core::slice::from_ref(subpixel));
        per.projection.write(&uniform);

        unsafe {
            for (slot, b) in [
                (binding::CURVES, &geometry.curves),
                (binding::BAND_CURVES, &geometry.band_curves),
                (binding::BANDS, &geometry.bands),
                (binding::INSTANCES, &per.instances),
                (binding::SUBPIXEL, &per.subpixel),
                (binding::HULL, &geometry.hulls),
            ] {
                self.write_srv(slot, b.resource, b.stride, b.count);
            }
        }

        let viewport = d3d::Viewport {
            top_left_x: 0.0,
            top_left_y: 0.0,
            width: target.width as f32,
            height: target.height as f32,
            min_depth: 0.0,
            max_depth: 1.0,
        };
        let scissor = d3d::Rect {
            left: 0,
            top: 0,
            right: target.width as i32,
            bottom: target.height as i32,
        };
        let c = self.cmd();

        unsafe {
            self.barrier(target.texture, target.state.get(), d3d::RESOURCE_STATE_RENDER_TARGET);
            (c.om_set_render_targets)(self.list, 1, &target.rtv, 0, core::ptr::null());
            (c.clear_render_target_view)(self.list, target.rtv, [0.0f32; 4].as_ptr(), 0, core::ptr::null());
            (c.set_graphics_root_signature)(self.list, self.root);
            (c.set_descriptor_heaps)(self.list, 1, &self.srv_heap);

            let h = &*(*self.srv_heap).vtable.cast::<d3d::D12HeapVtbl>();
            let mut table = d3d::GpuDescriptorHandle::default();
            (h.get_gpu_descriptor_handle_for_heap_start)(self.srv_heap, &mut table);
            (c.set_graphics_root_descriptor_table)(self.list, 0, table);
            let pv = &*(*per.projection.resource).vtable.cast::<d3d::D12ResourceVtbl>();
            (c.set_graphics_root_constant_buffer_view)(
                self.list, 1, (pv.get_gpu_virtual_address)(per.projection.resource),
            );

            (c.rs_set_viewports)(self.list, 1, &viewport);
            (c.rs_set_scissor_rects)(self.list, 1, &scissor);
            (c.ia_set_primitive_topology)(self.list, d3d::PRIMITIVE_TOPOLOGY_TRIANGLESTRIP);
            (c.draw_instanced)(self.list, super::HULL_VERTICES as u32, instances.len() as u32, 0, 0);

            self.barrier(
                target.texture,
                d3d::RESOURCE_STATE_RENDER_TARGET,
                d3d::RESOURCE_STATE_COPY_SOURCE,
            );
            check("Close", (c.close)(self.list))?;
            (self.queue_vtbl().execute_command_lists)(self.queue, 1, &self.list);
        }
        target.state.set(d3d::RESOURCE_STATE_COPY_SOURCE);
        self.signal()?;
        unsafe {
            if let Some((old, _)) = target.pending.take() {
                d3d::release(old);
            }
            target.pending = Some((d3d::add_ref(self.fence), self.fence_value.get()));
        }
        Ok(())
    }

    pub fn wait(&self, target: &mut Target) -> Result<(), Error> {
        if let Some((fence, value)) = target.pending.take() {
            unsafe {
                wait_fence(fence, value, self.event.as_ref());
                d3d::release(fence);
            }
        }
        Ok(())
    }

    pub fn read_pixels<'t>(&self, target: &'t mut Target) -> Result<&'t [u8], Error> {
        if target.device != self.device || target.width == 0 || target.height == 0 {
            return Err(Error::BadTarget);
        }
        self.wait(target)?;
        let (w, h) = (target.width as usize, target.height as usize);
        target.pixels.resize(w * h * 4, 0);

        let dst = d3d::TextureCopyLocation {
            resource: target.readback,
            kind: d3d::TEXTURE_COPY_TYPE_PLACED_FOOTPRINT,
            placed_footprint: d3d::PlacedSubresourceFootprint {
                offset: 0,
                footprint: d3d::SubresourceFootprint {
                    format: d3d::FORMAT_R8G8B8A8_UNORM,
                    width: target.width,
                    height: target.height,
                    depth: 1,
                    row_pitch: target.row_pitch,
                },
            },
        };
        let src = d3d::TextureCopyLocation {
            resource: target.texture,
            kind: d3d::TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
            placed_footprint: d3d::PlacedSubresourceFootprint::default(),
        };

        self.begin(core::ptr::null_mut())?;
        unsafe {
            self.barrier(target.texture, target.state.get(), d3d::RESOURCE_STATE_COPY_SOURCE);
            (self.cmd().copy_texture_region)(self.list, &dst, 0, 0, 0, &src, core::ptr::null());
            check("Close", (self.cmd().close)(self.list))?;
        }
        target.state.set(d3d::RESOURCE_STATE_COPY_SOURCE);
        self.submit()?;

        let pitch = target.row_pitch as usize;
        let read_all = d3d::Range { begin: 0, end: pitch * h };
        let nothing_written = d3d::Range { begin: 0, end: 0 };
        let mut mapped: *mut c_void = core::ptr::null_mut();
        unsafe {
            // The read range covers exactly what the copy wrote, and nothing else holds a mapping:
            // an over-wide range here is not a bounds error but a stall, and a stale one is garbage.
            let v = &*(*target.readback).vtable.cast::<d3d::D12ResourceVtbl>();
            check("Map (readback)", (v.map)(target.readback, 0, &read_all, &mut mapped))?;
            let base = mapped.cast::<u8>();
            for y in 0..h {
                core::ptr::copy_nonoverlapping(
                    base.add(y * pitch),
                    target.pixels.as_mut_ptr().add(y * w * 4),
                    w * 4,
                );
            }
            (v.unmap)(target.readback, 0, &nothing_written);
        }
        Ok(&target.pixels)
    }
}

impl super::backend::Backend for Renderer {
    type Error = Error;
    type Target<'r> = Target;
    type Geometry<'r> = Geometry;
    const NAME: &'static str = "d3d12";

    fn new() -> Result<Self, Error> {
        Renderer::new()
    }

    fn refusal(e: &Error) -> super::backend::Refusal {
        use super::backend::Refusal;
        match e {
            Error::NoDevice | Error::MissingEntryPoint(_) => Refusal::NoDevice,
            Error::BadTarget => Refusal::BadTarget,
            Error::Call { .. } | Error::Shader(_) => Refusal::Failed,
        }
    }

    fn target(&self, w: u32, h: u32) -> Result<Target, Error> {
        Renderer::target(self, w, h)
    }

    fn geometry(&self, batch: &GpuBatch) -> Result<Geometry, Error> {
        Renderer::geometry(self, batch)
    }

    fn draw(
        &self, t: &mut Target, g: &Geometry, i: &[super::GlyphInstance],
        s: &super::SubpixelParams, m: Mode,
    ) -> Result<(), Error> {
        Renderer::draw(self, t, g, i, s, m)
    }

    fn draw_with(
        &self, t: &mut Target, g: &Geometry, i: &[super::GlyphInstance],
        s: &super::SubpixelParams, m: Mode, p: &[f32; 16],
    ) -> Result<(), Error> {
        Renderer::draw_with(self, t, g, i, s, m, p)
    }

    fn wait(&self, t: &mut Target) -> Result<(), Error> {
        Renderer::wait(self, t)
    }

    fn read_pixels<'t>(&self, t: &'t mut Target) -> Result<&'t [u8], Error> {
        Renderer::read_pixels(self, t)
    }

    fn profile(&self) -> crate::daerizer::draw::DeviceProfile {
        Renderer::profile(self)
    }

    fn device_name(&self) -> String {
        Renderer::device_name(self)
    }

    fn supports_subpixel(&self) -> bool {
        true
    }

    fn ortho(w: u32, h: u32) -> [f32; 16] {
        ortho(w, h)
    }
}

impl super::backend::Surface for Target {
    fn width(&self) -> u32 { Target::width(self) }
    fn height(&self) -> u32 { Target::height(self) }
    fn pixels(&self) -> &[u8] { Target::pixels(self) }
    fn pixel(&self, x: u32, y: u32) -> Option<[u8; 4]> { Target::pixel(self, x, y) }
}

impl super::backend::Uploaded for Geometry {
    fn revision(&self) -> u64 { Geometry::revision(self) }
}
