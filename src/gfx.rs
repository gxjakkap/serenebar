//! The glass is a Desktop Duplication copy of whatever sits behind the bar. A displacement
//! map bends it at the rim, then the built-in D2D blur and saturation effects finish it.
use crate::glass::GlassParams;
use std::cell::RefCell;
use std::collections::HashMap;
use windows::core::*;
use windows::Foundation::Numerics::Matrix3x2;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::Direct3D::*;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::DirectComposition::*;
use windows::Win32::Graphics::DirectWrite::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Dxgi::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::Graphics::Imaging::*;
use windows::Win32::System::Com::*;

pub type C = D2D1_COLOR_F;

pub fn rgba(hex: u32, a: f32) -> C {
    C {
        r: ((hex >> 16) & 255) as f32 / 255.0,
        g: ((hex >> 8) & 255) as f32 / 255.0,
        b: (hex & 255) as f32 / 255.0,
        a,
    }
}

#[derive(Clone, Copy, PartialEq)]
pub enum Align {
    Left,
    Center,
    Right,
}

struct Glass {
    output: IDXGIOutput1,
    origin: (i32, i32), // top-left of this output in virtual-desktop pixels
    dup: Option<IDXGIOutputDuplication>,
    desk: Option<ID3D11Texture2D>, // last full desktop frame, which never includes our window
    bg: Option<(ID3D11Texture2D, ID2D1Bitmap1, (u32, u32))>,
    map: Option<(ID2D1Bitmap1, [i32; 5])>,
    watch: RECT, // region we sample, in output coords. Changes outside it don't trigger a redraw.
    fx_disp: ID2D1Effect,
    fx_blur: ID2D1Effect,
    fx_sat: ID2D1Effect,
    shape_off: (f32, f32), // bar offset inside the sampled region, in DIPs
}

pub struct Gfx {
    d3d: ID3D11Device,
    ctx: ID3D11DeviceContext,
    swap: IDXGISwapChain1,
    _dcomp: (
        IDCompositionDevice,
        IDCompositionTarget,
        IDCompositionVisual,
    ),
    factory: ID2D1Factory1,
    dc: ID2D1DeviceContext,
    dwrite: IDWriteFactory,
    wic: IWICImagingFactory,
    brush: ID2D1SolidColorBrush,
    shadow: ID2D1Effect, // under dock icons
    round: Option<ID2D1StrokeStyle>,
    font: HSTRING,
    formats: RefCell<HashMap<(u32, i32, i32), IDWriteTextFormat>>,
    images: RefCell<HashMap<String, (u64, Option<ID2D1Bitmap1>)>>, // app and tray icons, by key and version
    glass: Option<Glass>,
    pub w: u32,
    pub h: u32,
    pub k: f32, // pixels per DIP
}

fn bitmap_props(
    alpha: D2D1_ALPHA_MODE,
    dpi: f32,
    opts: D2D1_BITMAP_OPTIONS,
) -> D2D1_BITMAP_PROPERTIES1 {
    D2D1_BITMAP_PROPERTIES1 {
        pixelFormat: D2D1_PIXEL_FORMAT {
            format: DXGI_FORMAT_B8G8R8A8_UNORM,
            alphaMode: alpha,
        },
        dpiX: dpi,
        dpiY: dpi,
        bitmapOptions: opts,
        ..Default::default()
    }
}

fn rr(x: f32, y: f32, w: f32, h: f32, r: f32) -> D2D1_ROUNDED_RECT {
    D2D1_ROUNDED_RECT {
        rect: D2D_RECT_F {
            left: x,
            top: y,
            right: x + w,
            bottom: y + h,
        },
        radiusX: r,
        radiusY: r,
    }
}

fn set_f32(fx: &ID2D1Effect, prop: u32, v: f32) {
    unsafe {
        let _ = fx.SetValue(prop, D2D1_PROPERTY_TYPE_FLOAT, &v.to_le_bytes());
    }
}
fn set_u32(fx: &ID2D1Effect, prop: u32, v: u32) {
    unsafe {
        let _ = fx.SetValue(prop, D2D1_PROPERTY_TYPE_ENUM, &v.to_le_bytes());
    }
}

impl Gfx {
    /// `glass` opens the desktop capture of that monitor. Panels don't need it.
    pub fn new(hwnd: HWND, glass: Option<HMONITOR>) -> Result<Self> {
        unsafe {
            let mut d3d = None;
            let mut ctx = None;
            D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_HARDWARE,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                None,
                D3D11_SDK_VERSION,
                Some(&mut d3d),
                None,
                Some(&mut ctx),
            )?;
            let (d3d, ctx): (ID3D11Device, ID3D11DeviceContext) = (d3d.unwrap(), ctx.unwrap());
            let dxgi: IDXGIDevice = d3d.cast()?;
            let adapter = dxgi.GetAdapter()?;
            let dxgi_factory: IDXGIFactory2 = adapter.GetParent()?;
            let desc = DXGI_SWAP_CHAIN_DESC1 {
                Width: 1,
                Height: 1,
                Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                SampleDesc: DXGI_SAMPLE_DESC {
                    Count: 1,
                    Quality: 0,
                },
                BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
                BufferCount: 2,
                SwapEffect: DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
                AlphaMode: DXGI_ALPHA_MODE_PREMULTIPLIED,
                ..Default::default()
            };
            let swap = dxgi_factory.CreateSwapChainForComposition(&d3d, &desc, None)?;

            let dcomp: IDCompositionDevice = DCompositionCreateDevice(&dxgi)?;
            let target = dcomp.CreateTargetForHwnd(hwnd, true)?;
            let visual = dcomp.CreateVisual()?;
            visual.SetContent(&swap)?;
            target.SetRoot(&visual)?;
            dcomp.Commit()?;

            let factory: ID2D1Factory1 =
                D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
            let dc = factory
                .CreateDevice(&dxgi)?
                .CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE)?;
            dc.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE); // ClearType needs an opaque target
            let brush = dc.CreateSolidColorBrush(&rgba(0, 1.0), None)?;
            let shadow = dc.CreateEffect(&CLSID_D2D1Shadow)?;
            let base: &ID2D1Factory = &factory;
            let round = base
                .CreateStrokeStyle(
                    &D2D1_STROKE_STYLE_PROPERTIES {
                        startCap: D2D1_CAP_STYLE_ROUND,
                        endCap: D2D1_CAP_STYLE_ROUND,
                        lineJoin: D2D1_LINE_JOIN_ROUND,
                        ..Default::default()
                    },
                    None,
                )
                .ok();
            let dwrite: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
            let wic: IWICImagingFactory =
                CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?;

            // SF Pro Display when the user has it installed, since we can't ship it. Otherwise Segoe UI Variable Display.
            let mut fonts = None;
            dwrite.GetSystemFontCollection(&mut fonts, false)?;
            let fonts = fonts.unwrap();
            let font = ["SF Pro Display", "Segoe UI Variable Display", "Segoe UI"]
                .into_iter()
                .map(HSTRING::from)
                .find(|f| {
                    let (mut i, mut ok) = (0u32, BOOL(0));
                    fonts.FindFamilyName(f, &mut i, &mut ok).is_ok() && ok.as_bool()
                })
                .unwrap_or_else(|| HSTRING::from("Segoe UI"));

            let glass = glass.and_then(|mon| Self::open_glass(&adapter, mon, &dc).ok());
            let mut g = Gfx {
                d3d,
                ctx,
                swap,
                _dcomp: (dcomp, target, visual),
                factory,
                dc,
                dwrite,
                wic,
                brush,
                shadow,
                round,
                font,
                formats: RefCell::default(),
                images: RefCell::default(),
                glass,
                w: 1,
                h: 1,
                k: 1.0,
            };
            g.bind_target()?;
            Ok(g)
        }
    }

    unsafe fn open_glass(
        adapter: &IDXGIAdapter,
        mon: HMONITOR,
        dc: &ID2D1DeviceContext,
    ) -> Result<Glass> {
        let mut i = 0;
        let output = loop {
            let o = adapter.EnumOutputs(i)?; // returns an error after the last output
            if o.GetDesc()?.Monitor == mon {
                break o;
            }
            i += 1;
        };
        let r = output.GetDesc()?.DesktopCoordinates;
        Ok(Glass {
            output: output.cast()?,
            origin: (r.left, r.top),
            dup: None,
            desk: None,
            bg: None,
            map: None,
            watch: RECT::default(),
            fx_disp: dc.CreateEffect(&CLSID_D2D1DisplacementMap)?,
            fx_blur: dc.CreateEffect(&CLSID_D2D1GaussianBlur)?,
            fx_sat: dc.CreateEffect(&CLSID_D2D1Saturation)?,
            shape_off: (0.0, 0.0),
        })
    }

    unsafe fn bind_target(&mut self) -> Result<()> {
        let surf: IDXGISurface = self.swap.GetBuffer(0)?;
        let props = bitmap_props(
            D2D1_ALPHA_MODE_PREMULTIPLIED,
            96.0 * self.k,
            D2D1_BITMAP_OPTIONS_TARGET | D2D1_BITMAP_OPTIONS_CANNOT_DRAW,
        );
        let bmp = self.dc.CreateBitmapFromDxgiSurface(&surf, Some(&props))?;
        self.dc.SetTarget(&bmp);
        self.dc.SetDpi(96.0 * self.k, 96.0 * self.k);
        Ok(())
    }

    pub fn resize(&mut self, w: u32, h: u32, k: f32) -> Result<()> {
        if (w, h, k) == (self.w, self.h, self.k) {
            return Ok(());
        }
        unsafe {
            self.dc.SetTarget(None);
            self.swap.ResizeBuffers(
                0,
                w.max(1),
                h.max(1),
                DXGI_FORMAT_UNKNOWN,
                DXGI_SWAP_CHAIN_FLAG(0),
            )?;
            (self.w, self.h, self.k) = (w, h, k);
            if let Some(g) = &mut self.glass {
                g.bg = None;
                g.map = None;
            } // both carry the old DPI, so rebuild them
            self.bind_target()
        }
    }

    /// Pulls a new desktop frame if one is waiting. Returns true when something changed behind the bar.
    pub fn poll_glass(&mut self) -> bool {
        let Some(g) = &mut self.glass else {
            return false;
        };
        unsafe {
            if g.dup.is_none() {
                g.dup = g.output.DuplicateOutput(&self.d3d).ok();
            }
            let Some(dup) = &g.dup else { return false };
            let mut info = DXGI_OUTDUPL_FRAME_INFO::default();
            let mut res = None;
            match dup.AcquireNextFrame(0, &mut info, &mut res) {
                Ok(()) => {}
                Err(e) if e.code() == DXGI_ERROR_WAIT_TIMEOUT => return false,
                Err(_) => {
                    g.dup = None;
                    return false;
                } // access lost after UAC or a mode change. Reopen on the next tick.
            }
            let mut changed = false;
            if info.LastPresentTime != 0 {
                if let Some(tex) = res.and_then(|r| r.cast::<ID3D11Texture2D>().ok()) {
                    if g.desk.is_none() {
                        let mut d = D3D11_TEXTURE2D_DESC::default();
                        tex.GetDesc(&mut d);
                        d.Usage = D3D11_USAGE_DEFAULT;
                        d.BindFlags = 0;
                        d.CPUAccessFlags = 0;
                        d.MiscFlags = 0;
                        let mut t = None;
                        if self.d3d.CreateTexture2D(&d, None, Some(&mut t)).is_ok() {
                            g.desk = t;
                        }
                        changed = true;
                    }
                    if let Some(desk) = &g.desk {
                        self.ctx.CopyResource(desk, &tex);
                    }
                    changed |= frame_touches(dup, &info, &g.watch);
                }
            }
            let _ = dup.ReleaseFrame();
            changed
        }
    }

    /// Copies the strip behind `win`, given in screen pixels, into a texture for this frame.
    /// Call it before `begin`. `radius` is in DIPs. Returns false when there is no capture,
    /// and the caller then draws a plain tint.
    pub fn prepare_glass(&mut self, win: RECT, radius: f32, p: GlassParams) -> bool {
        let k = self.k;
        let Some(g) = &mut self.glass else {
            return false;
        };
        let Some(desk) = &g.desk else { return false };
        unsafe {
            let mut d = D3D11_TEXTURE2D_DESC::default();
            desk.GetDesc(&mut d);
            let m = p.margin(k);
            let (ox, oy) = g.origin;
            let want = RECT {
                left: win.left - ox - m,
                top: win.top - oy - m,
                right: win.right - ox + m,
                bottom: win.bottom - oy + m,
            };
            let src = RECT {
                left: want.left.max(0),
                top: want.top.max(0),
                right: want.right.min(d.Width as i32),
                bottom: want.bottom.min(d.Height as i32),
            };
            if src.right <= src.left || src.bottom <= src.top {
                return false;
            }
            g.watch = src;
            let (bw, bh) = ((src.right - src.left) as u32, (src.bottom - src.top) as u32);

            if g.bg.as_ref().map(|b| b.2) != Some((bw, bh)) {
                let td = D3D11_TEXTURE2D_DESC {
                    Width: bw,
                    Height: bh,
                    MipLevels: 1,
                    ArraySize: 1,
                    Format: d.Format,
                    SampleDesc: DXGI_SAMPLE_DESC {
                        Count: 1,
                        Quality: 0,
                    },
                    Usage: D3D11_USAGE_DEFAULT,
                    BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
                    ..Default::default()
                };
                let mut t = None;
                if self.d3d.CreateTexture2D(&td, None, Some(&mut t)).is_err() {
                    return false;
                }
                let t = t.unwrap();
                let Ok(surf) = t.cast::<IDXGISurface>() else {
                    return false;
                };
                let props =
                    bitmap_props(D2D1_ALPHA_MODE_IGNORE, 96.0 * k, D2D1_BITMAP_OPTIONS_NONE);
                let Ok(bmp) = self.dc.CreateBitmapFromDxgiSurface(&surf, Some(&props)) else {
                    return false;
                };
                g.bg = Some((t, bmp, (bw, bh)));
            }
            let (bg_tex, bg_bmp, _) = g.bg.as_ref().unwrap();
            let bx = D3D11_BOX {
                left: src.left as u32,
                top: src.top as u32,
                front: 0,
                right: src.right as u32,
                bottom: src.bottom as u32,
                back: 1,
            };
            self.ctx
                .CopySubresourceRegion(bg_tex, 0, 0, 0, 0, desk, 0, Some(&bx));

            // the map puts the bar shape at its position inside the sampled region
            let sx = win.left - ox - src.left;
            let sy = win.top - oy - src.top;
            let key = [
                bw as i32,
                bh as i32,
                sx,
                sy,
                (radius * k) as i32 * 1000 + (p.bend * k) as i32,
            ];
            if g.map.as_ref().map(|m| m.1) != Some(key) {
                let bytes = crate::glass::displacement_map(
                    bw,
                    bh,
                    sx,
                    sy,
                    win.right - win.left,
                    win.bottom - win.top,
                    radius * k,
                    p.band(k),
                );
                let props = bitmap_props(
                    D2D1_ALPHA_MODE_PREMULTIPLIED,
                    96.0 * k,
                    D2D1_BITMAP_OPTIONS_NONE,
                );
                let Ok(map) = self.dc.CreateBitmap(
                    D2D_SIZE_U {
                        width: bw,
                        height: bh,
                    },
                    Some(bytes.as_ptr() as _),
                    bw * 4,
                    &props,
                ) else {
                    return false;
                };
                g.map = Some((map, key));
            }
            g.shape_off = (sx as f32 / k, sy as f32 / k);

            g.fx_disp.SetInput(0, bg_bmp, true);
            g.fx_disp.SetInput(1, &g.map.as_ref().unwrap().0, true);
            set_f32(
                &g.fx_disp,
                D2D1_DISPLACEMENTMAP_PROP_SCALE.0 as u32,
                p.scale(),
            );
            set_u32(
                &g.fx_disp,
                D2D1_DISPLACEMENTMAP_PROP_X_CHANNEL_SELECT.0 as u32,
                D2D1_CHANNEL_SELECTOR_R.0 as u32,
            );
            set_u32(
                &g.fx_disp,
                D2D1_DISPLACEMENTMAP_PROP_Y_CHANNEL_SELECT.0 as u32,
                D2D1_CHANNEL_SELECTOR_G.0 as u32,
            );
            let Ok(o) = g.fx_disp.GetOutput() else {
                return false;
            };
            g.fx_blur.SetInput(0, &o, true);
            set_f32(
                &g.fx_blur,
                D2D1_GAUSSIANBLUR_PROP_STANDARD_DEVIATION.0 as u32,
                p.blur,
            );
            set_u32(
                &g.fx_blur,
                D2D1_GAUSSIANBLUR_PROP_BORDER_MODE.0 as u32,
                D2D1_BORDER_MODE_HARD.0 as u32,
            );
            let Ok(o) = g.fx_blur.GetOutput() else {
                return false;
            };
            g.fx_sat.SetInput(0, &o, true);
            set_f32(
                &g.fx_sat,
                D2D1_SATURATION_PROP_SATURATION.0 as u32,
                p.saturation,
            );
        }
        true
    }

    /// Fills the bar shape with the refracted desktop that `prepare_glass` set up.
    pub fn fill_glass(&self, x: f32, y: f32, w: f32, h: f32, r: f32) {
        let Some(g) = &self.glass else { return };
        unsafe {
            let Ok(img) = g.fx_sat.GetOutput() else {
                return;
            };
            let Some((_, _, (bw, bh))) = &g.bg else {
                return;
            };
            let props = D2D1_IMAGE_BRUSH_PROPERTIES {
                sourceRectangle: D2D_RECT_F {
                    left: 0.0,
                    top: 0.0,
                    right: *bw as f32 / self.k,
                    bottom: *bh as f32 / self.k,
                },
                extendModeX: D2D1_EXTEND_MODE_CLAMP,
                extendModeY: D2D1_EXTEND_MODE_CLAMP,
                interpolationMode: D2D1_INTERPOLATION_MODE_LINEAR,
            };
            let bp = D2D1_BRUSH_PROPERTIES {
                opacity: 1.0,
                transform: Matrix3x2::translation(-g.shape_off.0, -g.shape_off.1),
            };
            if let Ok(b) = self.dc.CreateImageBrush(&img, &props, Some(&bp)) {
                self.dc.FillRoundedRectangle(&rr(x, y, w, h, r), &b);
            }
        }
    }

    pub fn begin(&self) {
        unsafe {
            self.dc.BeginDraw();
            self.dc.SetTransform(&Matrix3x2::identity());
            self.dc.Clear(Some(&rgba(0, 0.0)));
        }
    }

    pub fn end(&self) {
        unsafe {
            // ponytail: nothing recovers a lost device after a driver update. Restart the app if the bar goes blank.
            let _ = self.dc.EndDraw(None, None);
            let _ = self.swap.Present(1, DXGI_PRESENT(0));
        }
    }

    fn solid(&self, c: C) -> &ID2D1SolidColorBrush {
        unsafe { self.brush.SetColor(&c) };
        &self.brush
    }

    pub fn fill_rr(&self, x: f32, y: f32, w: f32, h: f32, r: f32, c: C) {
        unsafe {
            self.dc
                .FillRoundedRectangle(&rr(x, y, w, h, r), self.solid(c))
        }
    }

    pub fn stroke_rr(&self, x: f32, y: f32, w: f32, h: f32, r: f32, c: C, width: f32) {
        let i = width / 2.0;
        unsafe {
            self.dc.DrawRoundedRectangle(
                &rr(x + i, y + i, w - width, h - width, r - i),
                self.solid(c),
                width,
                None,
            )
        }
    }

    /// Rounded-rect stroke with a vertical gradient.
    pub fn stroke_rr_grad(
        &self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        r: f32,
        stops: &[(f32, C)],
        width: f32,
    ) {
        if let Some(b) = self.vgrad(y, y + h, stops) {
            let i = width / 2.0;
            unsafe {
                self.dc.DrawRoundedRectangle(
                    &rr(x + i, y + i, w - width, h - width, r - i),
                    &b,
                    width,
                    None,
                )
            }
        }
    }

    pub fn fill_rr_grad(
        &self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        r: f32,
        stops: &[(f32, C)],
        diagonal: bool,
    ) {
        let s: Vec<D2D1_GRADIENT_STOP> = stops
            .iter()
            .map(|&(p, c)| D2D1_GRADIENT_STOP {
                position: p,
                color: c,
            })
            .collect();
        unsafe {
            let rt: &ID2D1RenderTarget = &self.dc;
            let Ok(coll) =
                rt.CreateGradientStopCollection(&s, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP)
            else {
                return;
            };
            let end = if diagonal {
                D2D_POINT_2F { x: x + w, y: y + h }
            } else {
                D2D_POINT_2F { x, y: y + h }
            };
            let props = D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES {
                startPoint: D2D_POINT_2F { x, y },
                endPoint: end,
            };
            if let Ok(b) = self.dc.CreateLinearGradientBrush(&props, None, &coll) {
                self.dc.FillRoundedRectangle(&rr(x, y, w, h, r), &b);
            }
        }
    }

    fn vgrad(&self, y0: f32, y1: f32, stops: &[(f32, C)]) -> Option<ID2D1LinearGradientBrush> {
        let s: Vec<D2D1_GRADIENT_STOP> = stops
            .iter()
            .map(|&(p, c)| D2D1_GRADIENT_STOP {
                position: p,
                color: c,
            })
            .collect();
        unsafe {
            let rt: &ID2D1RenderTarget = &self.dc;
            let coll = rt
                .CreateGradientStopCollection(&s, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP)
                .ok()?;
            let props = D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES {
                startPoint: D2D_POINT_2F { x: 0.0, y: y0 },
                endPoint: D2D_POINT_2F { x: 0.0, y: y1 },
            };
            self.dc.CreateLinearGradientBrush(&props, None, &coll).ok()
        }
    }

    pub fn fill_circle(&self, cx: f32, cy: f32, r: f32, c: C) {
        let e = D2D1_ELLIPSE {
            point: D2D_POINT_2F { x: cx, y: cy },
            radiusX: r,
            radiusY: r,
        };
        unsafe { self.dc.FillEllipse(&e, self.solid(c)) }
    }

    /// Draws `frac` of a ring, clockwise from 12 o'clock.
    pub fn arc(&self, cx: f32, cy: f32, r: f32, frac: f32, c: C, width: f32) {
        let frac = frac.clamp(0.0, 1.0);
        if frac >= 0.999 {
            let e = D2D1_ELLIPSE {
                point: D2D_POINT_2F { x: cx, y: cy },
                radiusX: r,
                radiusY: r,
            };
            unsafe { self.dc.DrawEllipse(&e, self.solid(c), width, None) };
        } else if frac > 0.001 {
            self.arc_between(cx, cy, r, 0.0, frac * std::f32::consts::TAU, c, width);
        }
    }

    /// Ring segment between two angles in radians, measured clockwise from 12 o'clock. Round caps.
    pub fn arc_between(&self, cx: f32, cy: f32, r: f32, a0: f32, a1: f32, c: C, width: f32) {
        let at = |a: f32| D2D_POINT_2F {
            x: cx + r * a.sin(),
            y: cy - r * a.cos(),
        };
        unsafe {
            let Ok(path) = self.factory.CreatePathGeometry() else {
                return;
            };
            let Ok(sink) = path.Open() else { return };
            sink.BeginFigure(at(a0), D2D1_FIGURE_BEGIN_HOLLOW);
            sink.AddArc(&D2D1_ARC_SEGMENT {
                point: at(a1),
                size: D2D_SIZE_F {
                    width: r,
                    height: r,
                },
                rotationAngle: 0.0,
                sweepDirection: D2D1_SWEEP_DIRECTION_CLOCKWISE,
                arcSize: if a1 - a0 > std::f32::consts::PI {
                    D2D1_ARC_SIZE_LARGE
                } else {
                    D2D1_ARC_SIZE_SMALL
                },
            });
            sink.EndFigure(D2D1_FIGURE_END_OPEN);
            let _ = sink.Close();
            self.dc
                .DrawGeometry(&path, self.solid(c), width, self.round.as_ref());
        }
    }

    pub fn line(&self, x0: f32, y0: f32, x1: f32, y1: f32, c: C, width: f32) {
        unsafe {
            self.dc.DrawLine(
                D2D_POINT_2F { x: x0, y: y0 },
                D2D_POINT_2F { x: x1, y: y1 },
                self.solid(c),
                width,
                self.round.as_ref(),
            )
        }
    }

    /// Draws what follows at `opacity`, scaled by `scale` about (cx, cy) and shifted down by `dy`.
    /// Pair with `pop_fade`.
    pub fn push_fade(&self, opacity: f32, scale: f32, cx: f32, cy: f32, dy: f32) {
        let params = D2D1_LAYER_PARAMETERS1 {
            contentBounds: D2D_RECT_F {
                left: -1e6,
                top: -1e6,
                right: 1e6,
                bottom: 1e6,
            },
            maskTransform: Matrix3x2::identity(),
            opacity,
            ..Default::default()
        };
        unsafe {
            self.dc.SetTransform(&Matrix3x2 {
                M11: scale,
                M12: 0.0,
                M21: 0.0,
                M22: scale,
                M31: cx * (1.0 - scale),
                M32: cy * (1.0 - scale) + dy,
            });
            self.dc.PushLayer(&params, None);
        }
    }

    pub fn pop_fade(&self) {
        unsafe {
            self.dc.PopLayer();
            self.dc.SetTransform(&Matrix3x2::identity());
        }
    }

    pub fn set_transform(&self, m: Matrix3x2) {
        unsafe { self.dc.SetTransform(&m) }
    }

    pub fn transform(&self) -> Matrix3x2 {
        let mut m = Matrix3x2::default();
        unsafe { self.dc.GetTransform(&mut m) };
        m
    }

    pub fn poly(&self, pts: &[(f32, f32)], c: C) {
        unsafe {
            let Ok(path) = self.factory.CreatePathGeometry() else {
                return;
            };
            let Ok(sink) = path.Open() else { return };
            sink.BeginFigure(
                D2D_POINT_2F {
                    x: pts[0].0,
                    y: pts[0].1,
                },
                D2D1_FIGURE_BEGIN_FILLED,
            );
            let rest: Vec<D2D_POINT_2F> = pts[1..]
                .iter()
                .map(|&(x, y)| D2D_POINT_2F { x, y })
                .collect();
            sink.AddLines(&rest);
            sink.EndFigure(D2D1_FIGURE_END_CLOSED);
            let _ = sink.Close();
            self.dc.FillGeometry(&path, self.solid(c), None);
        }
    }

    pub fn clip(&self, x: f32, y: f32, w: f32, h: f32) {
        unsafe {
            self.dc.PushAxisAlignedClip(
                &D2D_RECT_F {
                    left: x,
                    top: y,
                    right: x + w,
                    bottom: y + h,
                },
                D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
            )
        }
    }
    pub fn unclip(&self) {
        unsafe { self.dc.PopAxisAlignedClip() }
    }

    fn format(&self, size: f32, weight: i32, align: Align) -> Option<IDWriteTextFormat> {
        let key = ((size * 10.0) as u32, weight, align as i32);
        if let Some(f) = self.formats.borrow().get(&key) {
            return Some(f.clone());
        }
        unsafe {
            let f = self
                .dwrite
                .CreateTextFormat(
                    &self.font,
                    None,
                    DWRITE_FONT_WEIGHT(weight),
                    DWRITE_FONT_STYLE_NORMAL,
                    DWRITE_FONT_STRETCH_NORMAL,
                    size,
                    w!("en-us"),
                )
                .ok()?;
            let _ = f.SetTextAlignment(match align {
                Align::Left => DWRITE_TEXT_ALIGNMENT_LEADING,
                Align::Center => DWRITE_TEXT_ALIGNMENT_CENTER,
                Align::Right => DWRITE_TEXT_ALIGNMENT_TRAILING,
            });
            let _ = f.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
            let _ = f.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP);
            let trim = DWRITE_TRIMMING {
                granularity: DWRITE_TRIMMING_GRANULARITY_CHARACTER,
                delimiter: 0,
                delimiterCount: 0,
            };
            let sign = self.dwrite.CreateEllipsisTrimmingSign(&f).ok();
            let _ = f.SetTrimming(&trim, sign.as_ref());
            self.formats.borrow_mut().insert(key, f.clone());
            Some(f)
        }
    }

    /// One line, centred vertically in the box. Text that doesn't fit ends in an ellipsis.
    pub fn text(
        &self,
        s: &str,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        size: f32,
        weight: i32,
        c: C,
        align: Align,
    ) {
        let Some(f) = self.format(size, weight, align) else {
            return;
        };
        let t: Vec<u16> = s.encode_utf16().collect();
        let r = D2D_RECT_F {
            left: x,
            top: y,
            right: x + w,
            bottom: y + h,
        };
        unsafe {
            self.dc.DrawText(
                &t,
                &f,
                &r,
                self.solid(c),
                D2D1_DRAW_TEXT_OPTIONS_CLIP | D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT,
                DWRITE_MEASURING_MODE_NATURAL,
            )
        }
    }

    /// Width of one line of text.
    pub fn measure(&self, s: &str, size: f32, weight: i32) -> f32 {
        let Some(f) = self.format(size, weight, Align::Left) else {
            return 0.0;
        };
        let t: Vec<u16> = s.encode_utf16().collect();
        unsafe {
            let Ok(layout) = self.dwrite.CreateTextLayout(&t, &f, 10_000.0, 100.0) else {
                return 0.0;
            };
            let mut m = DWRITE_TEXT_METRICS::default();
            if layout.GetMetrics(&mut m).is_err() {
                return 0.0;
            }
            m.widthIncludingTrailingWhitespace
        }
    }

    pub fn bitmap(&self, b: &ID2D1Bitmap1, x: f32, y: f32, w: f32, h: f32) {
        let dst = D2D_RECT_F {
            left: x,
            top: y,
            right: x + w,
            bottom: y + h,
        };
        unsafe {
            self.dc.DrawBitmap(
                b,
                Some(&dst),
                1.0,
                D2D1_INTERPOLATION_MODE_HIGH_QUALITY_CUBIC,
                None,
                None,
            )
        }
    }

    /// A soft shadow of `b`'s shape, under where `bitmap` would put it. `blur` (the standard
    /// deviation) and `dy` are in the same units as `x` and `y`.
    pub fn bitmap_shadow(
        &self,
        b: &ID2D1Bitmap1,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        blur: f32,
        dy: f32,
        alpha: f32,
    ) {
        unsafe {
            let size = b.GetSize();
            if size.width <= 0.0 || size.height <= 0.0 {
                return;
            }
            let (sx, sy) = (w / size.width, h / size.height);
            self.shadow.SetInput(0, b, true);
            // the blur runs in the bitmap's own size, before it's scaled to the icon
            set_f32(
                &self.shadow,
                D2D1_SHADOW_PROP_BLUR_STANDARD_DEVIATION.0 as u32,
                blur / sx,
            );
            let _ = self.shadow.SetValue(
                D2D1_SHADOW_PROP_COLOR.0 as u32,
                D2D1_PROPERTY_TYPE_VECTOR4,
                &[0f32, 0.0, 0.0, alpha].map(f32::to_le_bytes).concat(),
            );
            let Ok(img) = self.shadow.GetOutput() else {
                return;
            };
            let mut old = Matrix3x2::default();
            self.dc.GetTransform(&mut old);
            let place = Matrix3x2 {
                M11: sx,
                M12: 0.0,
                M21: 0.0,
                M22: sy,
                M31: x,
                M32: y + dy,
            };
            self.dc.SetTransform(&(place * old));
            self.dc.DrawImage(
                &img,
                None,
                None,
                D2D1_INTERPOLATION_MODE_LINEAR,
                D2D1_COMPOSITE_MODE_SOURCE_OVER,
            );
            self.dc.SetTransform(&old);
        }
    }

    fn bitmap_from_wic(&self, src: &IWICBitmapSource) -> Option<ID2D1Bitmap1> {
        unsafe {
            let conv = self.wic.CreateFormatConverter().ok()?;
            conv.Initialize(
                src,
                &GUID_WICPixelFormat32bppPBGRA,
                WICBitmapDitherTypeNone,
                None,
                0.0,
                WICBitmapPaletteTypeMedianCut,
            )
            .ok()?;
            self.dc.CreateBitmapFromWicBitmap(&conv, None).ok()
        }
    }

    /// Decodes a PNG, JPEG or the like, such as an album cover.
    pub fn bitmap_from_bytes(&self, bytes: &[u8]) -> Option<ID2D1Bitmap1> {
        unsafe {
            let stream = self.wic.CreateStream().ok()?;
            stream.InitializeFromMemory(bytes).ok()?;
            let dec = self
                .wic
                .CreateDecoderFromStream(&stream, std::ptr::null(), WICDecodeMetadataCacheOnDemand)
                .ok()?;
            self.bitmap_from_wic(&dec.GetFrame(0).ok()?.cast().ok()?)
        }
    }

    /// `b` stretched over a rounded rectangle, its corners cut to the radius.
    pub fn bitmap_rr(&self, b: &ID2D1Bitmap1, x: f32, y: f32, w: f32, h: f32, r: f32) {
        unsafe {
            let props = D2D1_BITMAP_BRUSH_PROPERTIES1 {
                extendModeX: D2D1_EXTEND_MODE_CLAMP,
                extendModeY: D2D1_EXTEND_MODE_CLAMP,
                interpolationMode: D2D1_INTERPOLATION_MODE_HIGH_QUALITY_CUBIC,
            };
            let Ok(brush) = self.dc.CreateBitmapBrush(b, Some(&props), None) else {
                return;
            };
            let size = b.GetSize();
            if size.width <= 0.0 || size.height <= 0.0 {
                return;
            }
            brush.SetTransform(&Matrix3x2 {
                M11: w / size.width,
                M12: 0.0,
                M21: 0.0,
                M22: h / size.height,
                M31: x,
                M32: y,
            });
            self.dc.FillRoundedRectangle(&rr(x, y, w, h, r), &brush);
        }
    }

    /// Average colour of an encoded image, for tinting the music tile after its cover.
    pub fn average_color(&self, bytes: &[u8]) -> Option<C> {
        unsafe {
            let stream = self.wic.CreateStream().ok()?;
            stream.InitializeFromMemory(bytes).ok()?;
            let dec = self
                .wic
                .CreateDecoderFromStream(&stream, std::ptr::null(), WICDecodeMetadataCacheOnDemand)
                .ok()?;
            let scaler = self.wic.CreateBitmapScaler().ok()?;
            scaler
                .Initialize(&dec.GetFrame(0).ok()?, 8, 8, WICBitmapInterpolationModeFant)
                .ok()?;
            let conv = self.wic.CreateFormatConverter().ok()?;
            conv.Initialize(
                &scaler,
                &GUID_WICPixelFormat32bppBGRA,
                WICBitmapDitherTypeNone,
                None,
                0.0,
                WICBitmapPaletteTypeMedianCut,
            )
            .ok()?;
            let mut px = [0u8; 8 * 8 * 4];
            conv.CopyPixels(std::ptr::null(), 8 * 4, &mut px).ok()?;
            let sum = px.chunks(4).fold([0u32; 3], |a, p| {
                [a[0] + p[2] as u32, a[1] + p[1] as u32, a[2] + p[0] as u32]
            });
            Some(C {
                r: sum[0] as f32 / 64.0 / 255.0,
                g: sum[1] as f32 / 64.0 / 255.0,
                b: sum[2] as f32 / 64.0 / 255.0,
                a: 1.0,
            })
        }
    }

    /// A cached image. `make` runs only when `key` is new or its `version` changed.
    pub fn image(&self, key: &str, version: u64, make: impl FnOnce(&Gfx) -> Option<ID2D1Bitmap1>) {
        if self
            .images
            .borrow()
            .get(key)
            .is_some_and(|(v, _)| *v == version)
        {
            return;
        }
        let b = make(self);
        self.images
            .borrow_mut()
            .insert(key.to_string(), (version, b));
    }

    /// A cached image, without making it.
    pub fn cached(&self, key: &str) -> Option<ID2D1Bitmap1> {
        self.images.borrow().get(key).and_then(|(_, b)| b.clone())
    }

    /// Takes ownership of `h` (32-bit, premultiplied alpha) and frees it.
    pub fn bitmap_from_hbitmap(&self, h: HBITMAP) -> Option<ID2D1Bitmap1> {
        unsafe {
            let wb = self
                .wic
                .CreateBitmapFromHBITMAP(h, None, WICBitmapUsePremultipliedAlpha)
                .ok();
            let _ = DeleteObject(h);
            self.bitmap_from_wic(&wb?.cast().ok()?)
        }
    }

    /// The icon stays the caller's.
    pub fn bitmap_from_hicon(&self, h: isize) -> Option<ID2D1Bitmap1> {
        unsafe {
            self.bitmap_from_wic(
                &self
                    .wic
                    .CreateBitmapFromHICON(windows::Win32::UI::WindowsAndMessaging::HICON(h as _))
                    .ok()?
                    .cast()
                    .ok()?,
            )
        }
    }
}

unsafe fn frame_touches(
    dup: &IDXGIOutputDuplication,
    info: &DXGI_OUTDUPL_FRAME_INFO,
    watch: &RECT,
) -> bool {
    let n = info.TotalMetadataBufferSize as usize;
    if n == 0 {
        return false;
    }
    let hit = |r: &RECT| {
        r.left < watch.right && r.right > watch.left && r.top < watch.bottom && r.bottom > watch.top
    };
    let mut rects = vec![RECT::default(); n / std::mem::size_of::<RECT>() + 1];
    let mut used = 0u32;
    if dup
        .GetFrameDirtyRects(
            (rects.len() * std::mem::size_of::<RECT>()) as u32,
            rects.as_mut_ptr(),
            &mut used,
        )
        .is_err()
    {
        return true;
    }
    if rects[..used as usize / std::mem::size_of::<RECT>()]
        .iter()
        .any(hit)
    {
        return true;
    }
    let mut moves = vec![
        DXGI_OUTDUPL_MOVE_RECT::default();
        n / std::mem::size_of::<DXGI_OUTDUPL_MOVE_RECT>() + 1
    ];
    if dup
        .GetFrameMoveRects(
            (moves.len() * std::mem::size_of::<DXGI_OUTDUPL_MOVE_RECT>()) as u32,
            moves.as_mut_ptr(),
            &mut used,
        )
        .is_err()
    {
        return true;
    }
    moves[..used as usize / std::mem::size_of::<DXGI_OUTDUPL_MOVE_RECT>()]
        .iter()
        .any(|m| hit(&m.DestinationRect))
}
