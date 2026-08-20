// SAFETY, once for the file. Every raw pointer here is a live COM interface from a checked call,
// released once in `Drop`; descriptors, blobs and arrays passed by pointer are locals that
// outlive their call; a vtable cast is sound because the interface was made by the call naming
// it. Only the arguments that do not follow from that are written at their own site.

use super::direct3d as d3d;
use super::{GpuBatch, binding};
use alloc::string::String;
use alloc::vec::Vec;
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
            Error::NoDevice => write!(f, "no Direct3D 11 device"),
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

pub struct Renderer {
    _d3d11: d3d::Library,
    _compiler: d3d::Library,
    compile: d3d::PfnCompile,
    device: *mut d3d::Unknown,
    context: *mut d3d::Unknown,
    feature_level: i32,
    adapter: String,
    software: bool,
    vertex_shader: *mut d3d::Unknown,
    grayscale: *mut d3d::Unknown,
    subpixel: *mut d3d::Unknown,
    blend_none: *mut d3d::Unknown,
    blend_dual: *mut d3d::Unknown,
    raster: *mut d3d::Unknown,
    per_draw: core::cell::RefCell<PerDraw>,
}

struct PerDraw {
    instances: Buffer,
    capacity: u32,
    subpixel: Buffer,
    projection: *mut d3d::Unknown,
}

impl Renderer {
    pub fn new() -> Result<Renderer, Error> {
        let Some(d3d11) = d3d::Library::open(c"d3d11.dll") else {
            return Err(Error::NoDevice);
        };
        let Some(compiler) = d3d::Library::open(c"d3dcompiler_47.dll") else {
            return Err(Error::MissingEntryPoint("d3dcompiler_47.dll"));
        };
        let create: d3d::PfnCreateDevice = unsafe { d3d11.symbol(c"D3D11CreateDevice") }
            .ok_or(Error::MissingEntryPoint("D3D11CreateDevice"))?;
        let compile: d3d::PfnCompile = unsafe { compiler.symbol(c"D3DCompile") }
            .ok_or(Error::MissingEntryPoint("D3DCompile"))?;

        let levels = [d3d::FEATURE_LEVEL_11_1, d3d::FEATURE_LEVEL_11_0];
        let mut device = core::ptr::null_mut();
        let mut context = core::ptr::null_mut();
        let mut level = 0i32;

        for driver in [d3d::DRIVER_TYPE_HARDWARE, d3d::DRIVER_TYPE_WARP] {
            let hr = unsafe {
                create(
                    core::ptr::null_mut(),
                    driver,
                    core::ptr::null_mut(),
                    0,
                    levels.as_ptr(),
                    levels.len() as u32,
                    d3d::SDK_VERSION,
                    &mut device,
                    &mut level,
                    &mut context,
                )
            };
            if d3d::succeeded(hr) && !device.is_null() && !context.is_null() {
                break;
            }
            device = core::ptr::null_mut();
            context = core::ptr::null_mut();
        }
        if device.is_null() {
            return Err(Error::NoDevice);
        }

        let (adapter, vendor) = unsafe { adapter_of(device) };
        let mut r = Renderer {
            _d3d11: d3d11,
            _compiler: compiler,
            compile,
            device,
            context,
            feature_level: level,
            adapter,
            software: vendor == Some(d3d::VENDOR_MICROSOFT),
            vertex_shader: core::ptr::null_mut(),
            grayscale: core::ptr::null_mut(),
            subpixel: core::ptr::null_mut(),
            blend_none: core::ptr::null_mut(),
            blend_dual: core::ptr::null_mut(),
            raster: core::ptr::null_mut(),
            per_draw: core::cell::RefCell::new(PerDraw {
                instances: Buffer::EMPTY,
                capacity: 0,
                subpixel: Buffer::EMPTY,
                projection: core::ptr::null_mut(),
            }),
        };
        r.build()?;
        Ok(r)
    }

    fn build(&mut self) -> Result<(), Error> {
        use super::ShaderStage::{Fragment, SubpixelFragment, Vertex};
        let vs = self.compile(Vertex, c"vs_5_0")?;
        let ps_gray = self.compile(Fragment, c"ps_5_0")?;
        let ps_sub = self.compile(SubpixelFragment, c"ps_5_0")?;
        let mut vertex = core::ptr::null_mut();
        {
            let d = self.dev();
            let hr = unsafe {
                (d.create_vertex_shader)(
                    self.device, vs.as_ptr().cast::<c_void>(), vs.len(),
                    core::ptr::null_mut(), &mut vertex,
                )
            };
            check("CreateVertexShader", hr)?;
        }
        self.vertex_shader = vertex;

        let mut gray = core::ptr::null_mut();
        {
            let d = self.dev();
            let hr = unsafe {
                (d.create_pixel_shader)(
                    self.device, ps_gray.as_ptr().cast::<c_void>(), ps_gray.len(),
                    core::ptr::null_mut(), &mut gray,
                )
            };
            check("CreatePixelShader", hr)?;
        }
        self.grayscale = gray;

        let mut sub = core::ptr::null_mut();
        {
            let d = self.dev();
            let hr = unsafe {
                (d.create_pixel_shader)(
                    self.device, ps_sub.as_ptr().cast::<c_void>(), ps_sub.len(),
                    core::ptr::null_mut(), &mut sub,
                )
            };
            check("CreatePixelShader (subpixel)", hr)?;
        }
        self.subpixel = sub;

        let mut none = d3d::BlendDesc::default();
        none.render_target[0].render_target_write_mask = d3d::COLOR_WRITE_ENABLE_ALL;
        let mut dual = d3d::BlendDesc::default();
        dual.render_target[0] = d3d::RenderTargetBlendDesc {
            blend_enable: 1,
            src_blend: d3d::BLEND_SRC1_COLOR,
            dest_blend: d3d::BLEND_INV_SRC1_COLOR,
            blend_op: d3d::BLEND_OP_ADD,
            src_blend_alpha: d3d::BLEND_SRC1_ALPHA,
            dest_blend_alpha: d3d::BLEND_INV_SRC1_ALPHA,
            blend_op_alpha: d3d::BLEND_OP_ADD,
            render_target_write_mask: d3d::COLOR_WRITE_ENABLE_ALL,
        };
        let raster = d3d::RasterizerDesc {
            fill_mode: d3d::FILL_SOLID,
            cull_mode: d3d::CULL_NONE,
            depth_clip_enable: 1,
            ..Default::default()
        };
        let mut bn = core::ptr::null_mut();
        {
            let d = self.dev();
            let hr = unsafe { (d.create_blend_state)(self.device, &none, &mut bn) };
            check("CreateBlendState", hr)?;
        }
        self.blend_none = bn;

        let mut bd = core::ptr::null_mut();
        {
            let d = self.dev();
            let hr = unsafe { (d.create_blend_state)(self.device, &dual, &mut bd) };
            check("CreateBlendState (dual-source)", hr)?;
        }
        self.blend_dual = bd;

        let mut rs = core::ptr::null_mut();
        {
            let d = self.dev();
            let hr = unsafe { (d.create_rasterizer_state)(self.device, &raster, &mut rs) };
            check("CreateRasterizerState", hr)?;
        }
        self.raster = rs;

        let subpixel_bytes = size_of::<super::SubpixelParams>() as u32;
        let mut per = self.per_draw.borrow_mut();
        per.subpixel = self.dynamic_structured(subpixel_bytes, 1)?;
        per.instances = self.dynamic_structured(size_of::<super::GlyphInstance>() as u32, 1)?;
        per.capacity = 1;
        per.projection = self.dynamic_constant(80)?;
        Ok(())
    }

    pub fn device_name(&self) -> String {
        self.adapter.clone()
    }

    pub fn feature_level(&self) -> &'static str {
        match self.feature_level {
            d3d::FEATURE_LEVEL_11_1 => "11_1",
            d3d::FEATURE_LEVEL_11_0 => "11_0",
            _ => "unknown",
        }
    }

    fn is_uma(&self) -> Option<bool> {
        let mut data = d3d::FeatureDataOptions2::default();
        let hr = unsafe {
            (self.dev().check_feature_support)(
                self.device,
                d3d::FEATURE_D3D11_OPTIONS2,
                core::ptr::from_mut(&mut data).cast::<c_void>(),
                size_of::<d3d::FeatureDataOptions2>() as u32,
            )
        };
        d3d::succeeded(hr).then_some(data.unified_memory_architecture != 0)
    }

    pub fn is_software(&self) -> bool {
        self.software
    }

    pub fn profile(&self) -> crate::daerizer::draw::DeviceProfile {
        crate::daerizer::draw::DeviceProfile::from_d3d(self.software, self.is_uma(), self.device_name())
    }

    fn dev(&self) -> &d3d::DeviceVtbl {
        unsafe { &*(*self.device).vtable.cast::<d3d::DeviceVtbl>() }
    }

    fn ctx(&self) -> &d3d::ContextVtbl {
        unsafe { &*(*self.context).vtable.cast::<d3d::ContextVtbl>() }
    }

    pub fn target(&self, width: u32, height: u32) -> Result<Target, Error> {
        if width == 0 || height == 0 {
            return Err(Error::BadTarget);
        }
        let d = self.dev();

        let render_desc = d3d::Texture2dDesc {
            width,
            height,
            mip_levels: 1,
            array_size: 1,
            format: d3d::FORMAT_R8G8B8A8_UNORM,
            sample_desc: d3d::SampleDesc { count: 1, quality: 0 },
            usage: d3d::USAGE_DEFAULT,
            bind_flags: d3d::BIND_RENDER_TARGET,
            cpu_access_flags: 0,
            misc_flags: 0,
        };
        let staging_desc = d3d::Texture2dDesc {
            usage: d3d::USAGE_STAGING,
            bind_flags: 0,
            cpu_access_flags: d3d::CPU_ACCESS_READ,
            ..render_desc
        };

        let mut texture = core::ptr::null_mut();
        let mut staging = core::ptr::null_mut();
        let mut view = core::ptr::null_mut();
        unsafe {
            check("CreateTexture2D", (d.create_texture2_d)(
                self.device, &render_desc, core::ptr::null(), &mut texture,
            ))?;
            if let Err(e) = check("CreateTexture2D (staging)", (d.create_texture2_d)(
                self.device, &staging_desc, core::ptr::null(), &mut staging,
            )) {
                d3d::release(texture);
                return Err(e);
            }
            if let Err(e) = check("CreateRenderTargetView", (d.create_render_target_view)(
                self.device, texture, core::ptr::null(), &mut view,
            )) {
                d3d::release(staging);
                d3d::release(texture);
                return Err(e);
            }
        }

        let target = Target {
            texture,
            staging,
            view,
            width,
            height,
            // The reference is what lets a target outlive the renderer that made it without a
            // lifetime – the asymmetry with `vk::Target`, whose `VkDevice` is not refcounted.
            device: unsafe { d3d::add_ref(self.device) },
            context: unsafe { d3d::add_ref(self.context) },
            pixels: Vec::new(),
            pending: false,
        };

        unsafe {
            (self.ctx().clear_render_target_view)(self.context, view, [0.0f32; 4].as_ptr());
        }
        Ok(target)
    }

    pub fn read_pixels<'t>(&self, target: &'t mut Target) -> Result<&'t [u8], Error> {
        if target.device != self.device || target.width == 0 || target.height == 0 {
            return Err(Error::BadTarget);
        }
        self.wait(target)?;
        let c = self.ctx();
        let (w, h) = (target.width as usize, target.height as usize);
        target.pixels.resize(w * h * 4, 0);

        unsafe {
            (c.copy_resource)(self.context, target.staging, target.texture);
        }
        let mut mapped = d3d::MappedSubresource::default();
        check("Map", unsafe {
            (c.map)(self.context, target.staging, 0, d3d::MAP_READ, 0, &mut mapped)
        })?;

        let pitch = mapped.row_pitch as usize;
        unsafe {
            let src = mapped.data.cast::<u8>();
            for y in 0..h {
                core::ptr::copy_nonoverlapping(
                    src.add(y * pitch),
                    target.pixels.as_mut_ptr().add(y * w * 4),
                    w * 4,
                );
            }
            (c.unmap)(self.context, target.staging, 0);
        }
        Ok(&target.pixels)
    }
}

impl Drop for Renderer {
    fn drop(&mut self) {
        let per = self.per_draw.borrow();
        unsafe {
            (self.ctx().flush)(self.context);
            per.instances.destroy();
            per.subpixel.destroy();
            d3d::release(per.projection);
            d3d::release(self.raster);
            d3d::release(self.blend_dual);
            d3d::release(self.blend_none);
            d3d::release(self.subpixel);
            d3d::release(self.grayscale);
            d3d::release(self.vertex_shader);
            d3d::release(self.context);
            d3d::release(self.device);
        }
    }
}

pub struct Target {
    texture: *mut d3d::Unknown,
    staging: *mut d3d::Unknown,
    view: *mut d3d::Unknown,
    width: u32,
    height: u32,
    device: *mut d3d::Unknown,
    context: *mut d3d::Unknown,
    pixels: Vec<u8>,
    pending: bool,
}

impl Drop for Target {
    fn drop(&mut self) {
        unsafe {
            let c = &*(*self.context).vtable.cast::<d3d::ContextVtbl>();
            (c.flush)(self.context);
            d3d::release(self.view);
            d3d::release(self.staging);
            d3d::release(self.texture);
            d3d::release(self.context);
            d3d::release(self.device);
        }
    }
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

    pub unsafe fn texture(&self) -> *mut core::ffi::c_void {
        self.texture.cast()
    }
}

unsafe fn adapter_of(device: *mut d3d::Unknown) -> (String, Option<u32>) {
    let unknown = || (String::from("Direct3D 11"), None);
    unsafe {
        let mut dxgi = core::ptr::null_mut();
        let hr = ((*(*device).vtable).query_interface)(device, &d3d::IID_DXGI_DEVICE, &mut dxgi);
        if !d3d::succeeded(hr) || dxgi.is_null() {
            return unknown();
        }
        let mut adapter = core::ptr::null_mut();
        let v = &*(*dxgi).vtable.cast::<d3d::DxgiDeviceVtbl>();
        let hr = (v.get_adapter)(dxgi, &mut adapter);
        d3d::release(dxgi);
        if !d3d::succeeded(hr) || adapter.is_null() {
            return unknown();
        }

        let mut desc: d3d::AdapterDesc = core::mem::zeroed();
        let v = &*(*adapter).vtable.cast::<d3d::DxgiAdapterVtbl>();
        let hr = (v.get_desc)(adapter, &mut desc);
        d3d::release(adapter);
        if !d3d::succeeded(hr) {
            return unknown();
        }
        let end = desc.description.iter().position(|&c| c == 0).unwrap_or(desc.description.len());
        (String::from_utf16_lossy(&desc.description[..end]), Some(desc.vendor_id))
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

const _: () = {
    assert!(binding::CURVES == 0);
    assert!(binding::BAND_CURVES == 1);
    assert!(binding::BANDS == 2);
    assert!(binding::INSTANCES == 3);
    assert!(binding::SUBPIXEL == 4);
    assert!(binding::HULL == 5);
};

struct Buffer {
    buffer: *mut d3d::Unknown,
    srv: *mut d3d::Unknown,
}

impl Buffer {
    const EMPTY: Buffer =
        Buffer { buffer: core::ptr::null_mut(), srv: core::ptr::null_mut() };

    unsafe fn destroy(&self) {
        unsafe {
            d3d::release(self.srv);
            d3d::release(self.buffer);
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

impl Renderer {
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
        let stride = size_of::<T>() as u32;
        let count = data.len().max(1) as u32;
        let desc = d3d::BufferDesc {
            byte_width: stride * count,
            usage: d3d::USAGE_DEFAULT,
            bind_flags: d3d::BIND_SHADER_RESOURCE,
            cpu_access_flags: 0,
            misc_flags: d3d::RESOURCE_MISC_BUFFER_STRUCTURED,
            structure_byte_stride: stride,
        };
        let empty = [0u8; 64];
        let initial = d3d::SubresourceData {
            sys_mem: if data.is_empty() {
                empty.as_ptr().cast::<c_void>()
            } else {
                data.as_ptr().cast::<c_void>()
            },
            sys_mem_pitch: 0,
            sys_mem_slice_pitch: 0,
        };
        debug_assert!(size_of::<T>() <= empty.len(), "the empty stand-in is smaller than one element");

        let mut buffer = core::ptr::null_mut();
        check("CreateBuffer", unsafe {
            (self.dev().create_buffer)(self.device, &desc, &initial, &mut buffer)
        })?;

        let view = d3d::ShaderResourceViewDesc {
            format: d3d::FORMAT_UNKNOWN,
            view_dimension: d3d::SRV_DIMENSION_BUFFEREX,
            first_element: 0,
            num_elements: count,
            flags: 0,
            _union_tail: 0,
        };
        let mut srv = core::ptr::null_mut();
        let hr = unsafe {
            (self.dev().create_shader_resource_view)(self.device, buffer, &view, &mut srv)
        };
        if let Err(e) = check("CreateShaderResourceView", hr) {
            unsafe { d3d::release(buffer) };
            return Err(e);
        }
        Ok(Buffer { buffer, srv })
    }

    fn dynamic_structured(&self, stride: u32, count: u32) -> Result<Buffer, Error> {
        let count = count.max(1);
        let desc = d3d::BufferDesc {
            byte_width: stride * count,
            usage: d3d::USAGE_DYNAMIC,
            bind_flags: d3d::BIND_SHADER_RESOURCE,
            cpu_access_flags: d3d::CPU_ACCESS_WRITE,
            misc_flags: d3d::RESOURCE_MISC_BUFFER_STRUCTURED,
            structure_byte_stride: stride,
        };
        let mut buffer = core::ptr::null_mut();
        check("CreateBuffer (dynamic)", unsafe {
            (self.dev().create_buffer)(self.device, &desc, core::ptr::null(), &mut buffer)
        })?;
        let view = d3d::ShaderResourceViewDesc {
            format: d3d::FORMAT_UNKNOWN,
            view_dimension: d3d::SRV_DIMENSION_BUFFEREX,
            first_element: 0,
            num_elements: count,
            flags: 0,
            _union_tail: 0,
        };
        let mut srv = core::ptr::null_mut();
        let hr = unsafe {
            (self.dev().create_shader_resource_view)(self.device, buffer, &view, &mut srv)
        };
        if let Err(e) = check("CreateShaderResourceView (dynamic)", hr) {
            unsafe { d3d::release(buffer) };
            return Err(e);
        }
        Ok(Buffer { buffer, srv })
    }

    fn dynamic_constant(&self, bytes: u32) -> Result<*mut d3d::Unknown, Error> {
        let desc = d3d::BufferDesc {
            byte_width: bytes,
            usage: d3d::USAGE_DYNAMIC,
            bind_flags: d3d::BIND_CONSTANT_BUFFER,
            cpu_access_flags: d3d::CPU_ACCESS_WRITE,
            misc_flags: 0,
            structure_byte_stride: 0,
        };
        let mut buffer = core::ptr::null_mut();
        check("CreateBuffer (constant)", unsafe {
            (self.dev().create_buffer)(self.device, &desc, core::ptr::null(), &mut buffer)
        })?;
        Ok(buffer)
    }

    fn write<T: Copy>(&self, buffer: *mut d3d::Unknown, data: &[T]) -> Result<(), Error> {
        let bytes = size_of_val(data);
        if bytes == 0 {
            return Ok(());
        }
        let mut mapped = d3d::MappedSubresource::default();
        check("Map (dynamic)", unsafe {
            (self.ctx().map)(self.context, buffer, 0, d3d::MAP_WRITE_DISCARD, 0, &mut mapped)
        })?;
        unsafe {
            core::ptr::copy_nonoverlapping(
                data.as_ptr().cast::<u8>(),
                mapped.data.cast::<u8>(),
                bytes,
            );
            (self.ctx().unmap)(self.context, buffer, 0);
        }
        Ok(())
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
        let mut per = self.per_draw.borrow_mut();

        let wanted = instances.len().max(1) as u32;
        if per.capacity < wanted {
            let bigger =
                self.dynamic_structured(size_of::<super::GlyphInstance>() as u32, wanted)?;
            unsafe { per.instances.destroy() };
            per.instances = bigger;
            per.capacity = wanted;
        }
        self.write(per.instances.buffer, instances)?;
        self.write(per.subpixel.buffer, core::slice::from_ref(subpixel))?;
        self.write(per.projection, &uniform)?;

        let views: [*mut d3d::Unknown; 6] = [
            geometry.curves.srv,
            geometry.band_curves.srv,
            geometry.bands.srv,
            per.instances.srv,
            per.subpixel.srv,
            geometry.hulls.srv,
        ];
        let pixel_shader = match mode {
            Mode::Grayscale => self.grayscale,
            Mode::Subpixel => self.subpixel,
        };
        let blend = match mode {
            Mode::Grayscale => self.blend_none,
            Mode::Subpixel => self.blend_dual,
        };
        let viewport = d3d::Viewport {
            top_left_x: 0.0,
            top_left_y: 0.0,
            width: target.width as f32,
            height: target.height as f32,
            min_depth: 0.0,
            max_depth: 1.0,
        };
        let c = self.ctx();

        unsafe {
            (c.om_set_render_targets)(self.context, 1, &target.view, core::ptr::null_mut());
            (c.clear_render_target_view)(self.context, target.view, [0.0f32; 4].as_ptr());
            (c.rs_set_viewports)(self.context, 1, &viewport);
            (c.rs_set_state)(self.context, self.raster);
            (c.ia_set_primitive_topology)(self.context, d3d::PRIMITIVE_TOPOLOGY_TRIANGLESTRIP);
            (c.vs_set_shader)(self.context, self.vertex_shader, core::ptr::null(), 0);
            (c.ps_set_shader)(self.context, pixel_shader, core::ptr::null(), 0);
            (c.vs_set_shader_resources)(self.context, 0, views.len() as u32, views.as_ptr());
            (c.ps_set_shader_resources)(self.context, 0, views.len() as u32, views.as_ptr());
            (c.vs_set_constant_buffers)(self.context, 0, 1, &per.projection);
            (c.om_set_blend_state)(self.context, blend, core::ptr::null(), 0xFFFF_FFFF);
            (c.draw_instanced)(
                self.context,
                super::HULL_VERTICES as u32,
                instances.len() as u32,
                0,
                0,
            );
        }
        target.pending = true;
        Ok(())
    }

    pub fn wait(&self, target: &mut Target) -> Result<(), Error> {
        if !core::mem::take(&mut target.pending) {
            return Ok(());
        }
        let desc = d3d::QueryDesc { query: d3d::QUERY_EVENT, misc_flags: 0 };
        let mut query = core::ptr::null_mut();
        check("CreateQuery", unsafe {
            (self.dev().create_query)(self.device, &desc, &mut query)
        })?;
        let c = self.ctx();
        unsafe {
            (c.end)(self.context, query);
            (c.flush)(self.context);
            let mut done: i32 = 0;
            while (c.get_data)(
                self.context,
                query,
                core::ptr::from_mut(&mut done).cast::<c_void>(),
                size_of::<i32>() as u32,
                0,
            ) == d3d::S_FALSE
            {
                core::hint::spin_loop();
            }
            d3d::release(query);
        }
        Ok(())
    }

    fn compile(&self, stage: super::ShaderStage, target: &CStr) -> Result<Vec<u8>, Error> {
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

        let errors = unsafe { d3d::Blob::from_raw(errors) };
        let message = errors.as_ref().map(d3d::Blob::text).unwrap_or_default();
        let code = unsafe { d3d::Blob::from_raw(code) };

        let Some(code) = code.filter(|_| d3d::succeeded(hr)) else {
            return Err(Error::Shader(if message.is_empty() {
                alloc::format!("HRESULT 0x{hr:08x}")
            } else {
                message
            }));
        };
        Ok(code.bytes().to_vec())
    }
}

impl super::backend::Backend for Renderer {
    type Error = Error;
    type Target<'r> = Target;
    type Geometry<'r> = Geometry;
    const NAME: &'static str = "d3d11";

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
