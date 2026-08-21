use alloc::string::String;
use super::{Band, CurvePoint, GlyphInstance, GpuBatch, HullVertex, ShaderLanguage, ShaderStage, SubpixelParams, binding};
use super::backend::SurfaceFormat;
use super::metal;
use super::objc::{Id, Owned, Pool, sel, send0, send1, send4};

pub const PROJECTION_INDEX: u32 = 6;

// What a target clears to unless the caller says otherwise. Transparent, because an offscreen target
// is composited by whoever asked for it.
const TRANSPARENT: crate::daerizer::Rgba = crate::daerizer::Rgba { r: 0, g: 0, b: 0, a: 0 };

pub use super::Mode;
pub use super::backend::SurfaceFormat as Format;

#[derive(Debug)]
pub enum Error {
    NoDevice,
    ShaderCompile {
        stage: &'static str,
        message: String,
    },
    PipelineCreate {
        stage: &'static str,
        message: String,
    },
    Allocation(&'static str),
    Draw(String),
    BadTarget,
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Error::NoDevice => write!(f, "no Metal device"),
            Error::ShaderCompile { stage, message } => {
                write!(f, "{stage} shader did not compile: {message}")
            }
            Error::PipelineCreate { stage, message } => {
                write!(f, "{stage} pipeline could not be created: {message}")
            }
            Error::Allocation(what) => write!(f, "could not allocate {what}"),
            Error::Draw(message) => write!(f, "draw failed: {message}"),
            Error::BadTarget => write!(f, "render target is empty or from another device"),
        }
    }
}

impl core::error::Error for Error {}

pub struct Target {
    texture: Owned,
    // Absent on a surface daegun did not create: there is nowhere to read back to, and a caller
    // rendering into its own swapchain does not want the copy anyway.
    readback: Option<Owned>,
    // Present only when the surface came from a drawable, which daegun then presents on the same
    // command buffer as the draw.
    drawable: Option<Owned>,
    format: SurfaceFormat,
    clear: Option<crate::daerizer::Rgba>,
    width: u32,
    height: u32,
    device: u64,
    pending: Option<Owned>,
}

impl Target {
    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    fn len(&self) -> usize {
        self.width as usize * self.height as usize * 4
    }

    // A view of the readback buffer, not a copy, and all zero before the first `read_pixels` –
    // the buffer is cleared when the target is made, so this is defined rather than usually-zero.
    pub fn pixels(&self) -> &[u8] {
        let Some(readback) = self.readback.as_ref() else { return &[] };
        unsafe {
            let p = metal::buffer_contents(readback.id());
            if p.is_null() {
                return &[];
            }
            core::slice::from_raw_parts(p.cast::<u8>(), self.len())
        }
    }

    pub fn format(&self) -> SurfaceFormat {
        self.format
    }

    pub fn clear(&self) -> Option<crate::daerizer::Rgba> {
        self.clear
    }

    // `None` keeps what the target already holds instead of clearing, so a second geometry can be
    // drawn over the first rather than erasing it.
    pub fn set_clear(&mut self, clear: Option<crate::daerizer::Rgba>) {
        self.clear = clear;
    }

    pub fn pixel(&self, x: u32, y: u32) -> Option<[u8; 4]> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let i = ((y as usize) * (self.width as usize) + x as usize) * 4;
        self.pixels().get(i..i + 4).map(|p| [p[0], p[1], p[2], p[3]])
    }

    pub unsafe fn texture(&self) -> *mut core::ffi::c_void {
        self.texture.id()
    }
}

const FRAMES_IN_FLIGHT: usize = 3;

#[derive(Default)]
struct Slot {
    instances: Option<Owned>,
    capacity: usize,
    inflight: Option<Owned>,
}

pub struct Geometry {
    revision: u64,
    curves: Owned,
    band_curves: Owned,
    bands: Owned,
    hulls: Owned,
}

impl Geometry {
    pub fn sync(&mut self, renderer: &Renderer, batch: &GpuBatch) -> Result<(), Error> {
        if self.revision == batch.revision() {
            return Ok(());
        }
        *self = renderer.geometry(batch)?;
        Ok(())
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }
}

struct Pipelines {
    grayscale: Owned,
    subpixel: Owned,
}

pub struct Renderer {
    device: Owned,
    // A Mac with two GPUs can hand a texture from one to a renderer on the other, and Metal's
    // answer to that is undefined behavior rather than an error. This is what lets a draw refuse.
    device_id: u64,
    queue: Owned,
    // A pipeline is bound to its attachment's pixel format, and a surface daegun did not create
    // chooses its own, so both orders are built up front and drawing stays infallible.
    rgba: Pipelines,
    bgra: Pipelines,
    slots: core::cell::RefCell<[Slot; FRAMES_IN_FLIGHT]>,
    frame: core::cell::Cell<usize>,
}

impl Renderer {
    pub fn new() -> Result<Renderer, Error> {
        let _pool = Pool::new();
        Renderer::build(metal::system_default_device().ok_or(Error::NoDevice)?)
    }

    // `device` must be a live `MTLDevice`. It is retained for the renderer's lifetime and never
    // released beyond that, so the caller keeps its own reference.
    pub unsafe fn from_device(device: *mut core::ffi::c_void) -> Result<Renderer, Error> {
        if device.is_null() {
            return Err(Error::NoDevice);
        }
        let _pool = Pool::new();
        let device = unsafe { super::objc::retain(device) }.ok_or(Error::NoDevice)?;
        Renderer::build(device)
    }

    fn build(device: Owned) -> Result<Renderer, Error> {
        let _pool = Pool::new();

        let queue =
            unsafe { metal::new_command_queue(device.id()) }.ok_or(Error::Allocation("a command queue"))?;

        let compile = |stage: ShaderStage, name: &'static str| -> Result<Owned, Error> {
            let source = super::shader(ShaderLanguage::Metal, stage);
            unsafe { metal::new_library(device.id(), source) }
                .map_err(|message| Error::ShaderCompile { stage: name, message })
        };

        let vertex_lib = compile(ShaderStage::Vertex, "vertex")?;
        let gray_lib = compile(ShaderStage::Fragment, "fragment")?;
        let subpixel_lib = compile(ShaderStage::SubpixelFragment, "subpixel fragment")?;

        let entry = |lib: &Owned, name: &'static str| -> Result<Owned, Error> {
            unsafe { metal::new_function(lib.id(), name) }.ok_or(Error::Allocation(name))
        };

        let vertex_fn = entry(&vertex_lib, "daegunGlyphVertex")?;
        let gray_fn = entry(&gray_lib, "daegunGlyphFragment")?;
        let subpixel_fn = entry(&subpixel_lib, "daegunGlyphSubpixelFragment")?;

        let build = |fragment: &Owned, dual: bool, stage: &'static str, format: u64| {
            unsafe { metal::new_pipeline(device.id(), vertex_fn.id(), fragment.id(), dual, format) }
                .map_err(|message| Error::PipelineCreate { stage, message })
        };
        let pair = |format: u64| -> Result<Pipelines, Error> {
            Ok(Pipelines {
                grayscale: build(&gray_fn, false, "grayscale", format)?,
                subpixel: build(&subpixel_fn, true, "subpixel", format)?,
            })
        };

        let rgba = pair(metal::PIXEL_FORMAT_RGBA8_UNORM)?;
        let bgra = pair(metal::PIXEL_FORMAT_BGRA8_UNORM)?;

        let device_id = unsafe { metal::registry_id(device.id()) };

        Ok(Renderer {
            device,
            device_id,
            queue,
            rgba,
            bgra,
            slots: core::cell::RefCell::new(Default::default()),
            frame: core::cell::Cell::new(0),
        })
    }

    pub fn geometry(&self, batch: &GpuBatch) -> Result<Geometry, Error> {
        let _pool = Pool::new();
        let curves: &[CurvePoint] = batch.curves();
        let band_curves: &[u32] = batch.band_curves();
        let bands: &[Band] = batch.bands();
        let hulls: &[HullVertex] = batch.hulls();

        Ok(Geometry {
            revision: batch.revision(),
            curves: unsafe { metal::new_buffer(self.device.id(), curves) }
                .ok_or(Error::Allocation("the curve buffer"))?,
            band_curves: unsafe { metal::new_buffer(self.device.id(), band_curves) }
                .ok_or(Error::Allocation("the band-curve buffer"))?,
            bands: unsafe { metal::new_buffer(self.device.id(), bands) }
                .ok_or(Error::Allocation("the band buffer"))?,
            hulls: unsafe { metal::new_buffer(self.device.id(), hulls) }
                .ok_or(Error::Allocation("the hull buffer"))?,
        })
    }

    pub fn profile(&self) -> crate::daerizer::draw::DeviceProfile {
        crate::daerizer::draw::DeviceProfile::from_metal(self.is_uma(), self.device_name())
    }

    fn is_uma(&self) -> Option<bool> {
        unsafe { metal::has_unified_memory(self.device.id()) }
    }

    pub fn device_name(&self) -> String {
        unsafe { metal::device_name(self.device.id()) }
    }

    pub fn target(&self, width: u32, height: u32) -> Result<Target, Error> {
        self.target_with_format(width, height, SurfaceFormat::Rgba8Unorm)
    }

    // An offscreen target in the caller's byte order, for compositing into a surface that is not
    // daegun's without a swizzle on the way out.
    pub fn target_with_format(
        &self,
        width: u32,
        height: u32,
        format: SurfaceFormat,
    ) -> Result<Target, Error> {
        if width == 0 || height == 0 {
            return Err(Error::BadTarget);
        }
        let len = (width as usize)
            .checked_mul(height as usize)
            .and_then(|p| p.checked_mul(4))
            .ok_or(Error::BadTarget)?;
        let _pool = Pool::new();
        let pixel_format = match format {
            SurfaceFormat::Rgba8Unorm => metal::PIXEL_FORMAT_RGBA8_UNORM,
            SurfaceFormat::Bgra8Unorm => metal::PIXEL_FORMAT_BGRA8_UNORM,
        };
        let texture =
            unsafe { metal::new_render_target(self.device.id(), width, height, pixel_format) }
                .ok_or(Error::Allocation("a render target"))?;
        let readback = unsafe { metal::new_buffer_uninit(self.device.id(), len) }
            .ok_or(Error::Allocation("the readback buffer"))?;
        unsafe {
            let p = metal::buffer_contents(readback.id());
            if !p.is_null() {
                core::ptr::write_bytes(p.cast::<u8>(), 0, len);
            }
        }
        Ok(Target {
            texture,
            readback: Some(readback),
            drawable: None,
            format,
            clear: Some(TRANSPARENT),
            width,
            height,
            device: self.device_id,
            pending: None,
        })
    }

    // `texture` must be a live `MTLTexture` from this renderer's device, at least `width` by
    // `height`. It is retained for the target's lifetime, so the caller keeps its own reference.
    pub unsafe fn target_from_texture(
        &self,
        texture: *mut core::ffi::c_void,
        width: u32,
        height: u32,
    ) -> Result<Target, Error> {
        unsafe { self.borrowed(texture, core::ptr::null_mut(), width, height) }
    }

    // `drawable` must be a live `CAMetalDrawable` whose texture belongs to this renderer's device.
    // daegun presents it on the command buffer carrying the draw, so the caller must not present it
    // as well.
    pub unsafe fn target_from_drawable(
        &self,
        drawable: *mut core::ffi::c_void,
        width: u32,
        height: u32,
    ) -> Result<Target, Error> {
        if drawable.is_null() {
            return Err(Error::BadTarget);
        }
        let _pool = Pool::new();
        let texture = unsafe { metal::drawable_texture(drawable) };
        unsafe { self.borrowed(texture, drawable, width, height) }
    }

    // The format comes off the texture rather than from the caller, because a surface that says one
    // thing and is another fails inside Metal rather than here.
    unsafe fn borrowed(
        &self,
        texture: *mut core::ffi::c_void,
        drawable: *mut core::ffi::c_void,
        width: u32,
        height: u32,
    ) -> Result<Target, Error> {
        if texture.is_null() || width == 0 || height == 0 {
            return Err(Error::BadTarget);
        }
        let _pool = Pool::new();
        let format = match unsafe { metal::texture_pixel_format(texture) } {
            metal::PIXEL_FORMAT_RGBA8_UNORM => SurfaceFormat::Rgba8Unorm,
            metal::PIXEL_FORMAT_BGRA8_UNORM => SurfaceFormat::Bgra8Unorm,
            _ => return Err(Error::BadTarget),
        };
        let texture = unsafe { super::objc::retain(texture) }.ok_or(Error::BadTarget)?;
        let drawable = if drawable.is_null() {
            None
        } else {
            Some(unsafe { super::objc::retain(drawable) }.ok_or(Error::BadTarget)?)
        };
        Ok(Target {
            texture,
            readback: None,
            drawable,
            format,
            clear: Some(TRANSPARENT),
            width,
            height,
            device: self.device_id,
            pending: None,
        })
    }

    pub fn read_pixels<'t>(&self, target: &'t mut Target) -> Result<&'t [u8], Error> {
        if target.device != self.device_id || target.width == 0 || target.height == 0 {
            return Err(Error::BadTarget);
        }
        let Some(readback) = target.readback.as_ref().map(Owned::id) else {
            return Err(Error::BadTarget);
        };
        let _pool = Pool::new();
        self.wait(target)?;

        unsafe {
            let commands: Id = send0(self.queue.id(), sel(c"commandBuffer"));
            if commands.is_null() {
                return Err(Error::Allocation("a command buffer"));
            }
            let encoder = metal::blit_encoder(commands);
            if encoder.is_null() {
                return Err(Error::Allocation("a blit encoder"));
            }
            metal::blit_texture_to_buffer(
                encoder,
                target.texture.id(),
                readback,
                target.width,
                target.height,
            );
            metal::send_void(encoder, sel(c"endEncoding"));
            metal::send_void(commands, sel(c"commit"));
            metal::send_void(commands, sel(c"waitUntilCompleted"));

            let error: Id = send0(commands, sel(c"error"));
            if !error.is_null() {
                return Err(Error::Draw(super::objc::error_message(error)));
            }
        }
        Ok(target.pixels())
    }

    pub fn wait(&self, target: &mut Target) -> Result<(), Error> {
        let _pool = Pool::new();
        let Some(commands) = target.pending.take() else { return Ok(()) };
        unsafe {
            metal::send_void(commands.id(), sel(c"waitUntilCompleted"));
            let error: Id = send0(commands.id(), sel(c"error"));
            if !error.is_null() {
                return Err(Error::Draw(super::objc::error_message(error)));
            }
        }
        Ok(())
    }

    pub fn draw(
        &self,
        target: &mut Target,
        geometry: &Geometry,
        instances: &[GlyphInstance],
        subpixel: &SubpixelParams,
        mode: Mode,
    ) -> Result<(), Error> {
        let projection = ortho(target.width, target.height);
        self.draw_with(target, geometry, instances, subpixel, mode, &projection)
    }

    pub fn draw_with(
        &self,
        target: &mut Target,
        geometry: &Geometry,
        instances: &[GlyphInstance],
        subpixel: &SubpixelParams,
        mode: Mode,
        projection: &[f32; 16],
    ) -> Result<(), Error> {
        if target.width == 0 || target.height == 0 || target.device != self.device_id {
            return Err(Error::BadTarget);
        }

        let uniform = super::draw_uniform(
            projection,
            &ortho(target.width, target.height),
            target.height as f32,
        );
        let _pool = Pool::new();

        let index = self.frame.get() % FRAMES_IN_FLIGHT;
        self.frame.set(self.frame.get().wrapping_add(1));
        let mut slots = self.slots.borrow_mut();
        let slot = &mut slots[index];
        if let Some(previous) = slot.inflight.take() {
            unsafe { metal::send_void(previous.id(), sel(c"waitUntilCompleted")) };
        }

        let want = core::mem::size_of_val(instances).max(4);
        if slot.capacity < want || slot.instances.is_none() {
            slot.instances = Some(
                unsafe { metal::new_buffer_uninit(self.device.id(), want) }
                    .ok_or(Error::Allocation("the instance buffer"))?,
            );
            slot.capacity = want;
        }
        let instance_buf = slot.instances.as_ref().ok_or(Error::Allocation("the instance buffer"))?;
        unsafe {
            metal::write_buffer(instance_buf.id(), instances);
        }

        let pipelines = match target.format {
            SurfaceFormat::Rgba8Unorm => &self.rgba,
            SurfaceFormat::Bgra8Unorm => &self.bgra,
        };
        let pipeline = match mode {
            Mode::Grayscale => &pipelines.grayscale,
            Mode::Subpixel => &pipelines.subpixel,
        };

        unsafe {
            let clear = target.clear.map(|c| metal::ClearColor {
                red: f64::from(c.r) / 255.0,
                green: f64::from(c.g) / 255.0,
                blue: f64::from(c.b) / 255.0,
                alpha: f64::from(c.a) / 255.0,
            });
            let pass = metal::render_pass(target.texture.id(), clear);
            let commands: Id = send0(self.queue.id(), sel(c"commandBuffer"));
            let encoder: Id =
                send1(commands, sel(c"renderCommandEncoderWithDescriptor:"), pass);
            if commands.is_null() || encoder.is_null() {
                return Err(Error::Allocation("a command encoder"));
            }

            send1::<Id, ()>(encoder, sel(c"setRenderPipelineState:"), pipeline.id());

            metal::set_fragment_buffer(encoder, geometry.curves.id(), binding::CURVES);
            metal::set_fragment_buffer(encoder, geometry.band_curves.id(), binding::BAND_CURVES);
            metal::set_fragment_buffer(encoder, geometry.bands.id(), binding::BANDS);
            metal::set_vertex_buffer(encoder, instance_buf.id(), binding::INSTANCES);
            metal::set_vertex_buffer(encoder, geometry.hulls.id(), binding::HULL);
            metal::set_vertex_bytes(encoder, subpixel, binding::SUBPIXEL);
            if mode == Mode::Subpixel {
                metal::set_fragment_bytes(encoder, subpixel, binding::SUBPIXEL);
            }
            metal::set_vertex_bytes(encoder, &uniform, PROJECTION_INDEX);

            // An instance count of zero is a validation error, not a no-op, so the pass is still
            // encoded – the caller gets the clear – and only the draw is skipped.
            if !instances.is_empty() {
                send4::<u64, u64, u64, u64, ()>(
                    encoder,
                    sel(c"drawPrimitives:vertexStart:vertexCount:instanceCount:"),
                    metal::PRIMITIVE_TRIANGLE_STRIP,
                    0,
                    super::HULL_VERTICES as u64,
                    instances.len() as u64,
                );
            }

            metal::send_void(encoder, sel(c"endEncoding"));
            // Presentation rides the command buffer carrying the draw, so the queue orders the two
            // and nothing has to wait on anything.
            if let Some(drawable) = target.drawable.as_ref() {
                metal::present_drawable(commands, drawable.id());
            }
            metal::send_void(commands, sel(c"commit"));

            slot.inflight = super::objc::retain(commands);
            target.pending = super::objc::retain(commands);
        }

        Ok(())
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

impl super::backend::Backend for Renderer {
    type Error = Error;
    type Target<'r> = Target;
    type Geometry<'r> = Geometry;
    const NAME: &'static str = "metal";

    fn new() -> Result<Self, Error> {
        Renderer::new()
    }

    fn refusal(e: &Error) -> super::backend::Refusal {
        use super::backend::Refusal;
        match e {
            Error::NoDevice => Refusal::NoDevice,
            Error::BadTarget => Refusal::BadTarget,
            _ => Refusal::Failed,
        }
    }

    fn target(&self, w: u32, h: u32) -> Result<Target, Error> {
        Renderer::target(self, w, h)
    }

    fn geometry(&self, batch: &GpuBatch) -> Result<Geometry, Error> {
        Renderer::geometry(self, batch)
    }

    fn draw(
        &self, t: &mut Target, g: &Geometry, i: &[GlyphInstance], s: &SubpixelParams, m: Mode,
    ) -> Result<(), Error> {
        Renderer::draw(self, t, g, i, s, m)
    }

    fn draw_with(
        &self, t: &mut Target, g: &Geometry, i: &[GlyphInstance], s: &SubpixelParams, m: Mode,
        p: &[f32; 16],
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
