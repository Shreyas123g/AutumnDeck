use windows::core::Interface;
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Direct2D::Common::{D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_PIXEL_FORMAT};
use windows::Win32::Graphics::Direct2D::{
    D2D1CreateFactory, ID2D1Bitmap1, ID2D1Device, ID2D1DeviceContext, ID2D1Factory, ID2D1Factory1,
    ID2D1RenderTarget, D2D1_BITMAP_OPTIONS_CANNOT_DRAW, D2D1_BITMAP_OPTIONS_TARGET,
    D2D1_BITMAP_PROPERTIES1, D2D1_DEVICE_CONTEXT_OPTIONS_NONE, D2D1_FACTORY_TYPE_SINGLE_THREADED,
};
use windows::Win32::Graphics::Direct3D::{
    D3D_DRIVER_TYPE_HARDWARE, D3D_FEATURE_LEVEL_10_0, D3D_FEATURE_LEVEL_10_1,
    D3D_FEATURE_LEVEL_11_0, D3D_FEATURE_LEVEL_11_1,
};
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, D3D11_CREATE_DEVICE_BGRA_SUPPORT,
    D3D11_SDK_VERSION,
};
use windows::Win32::Graphics::DirectComposition::{
    DCompositionCreateDevice, IDCompositionDevice, IDCompositionTarget, IDCompositionVisual,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_ALPHA_MODE_PREMULTIPLIED, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC,
};
use windows::Win32::Graphics::Dxgi::{
    IDXGIDevice, IDXGIDevice3, IDXGIFactory2, IDXGISurface, IDXGISwapChain1, DXGI_PRESENT,
    DXGI_SCALING_STRETCH, DXGI_SWAP_CHAIN_DESC1, DXGI_SWAP_CHAIN_FLAG,
    DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL, DXGI_USAGE_RENDER_TARGET_OUTPUT,
};

/// Hardware-accelerated Direct3D11 + Direct2D 1.1 + DXGI Flip-Model SwapChain + DirectComposition
/// presentation surface for per-pixel premultiplied-alpha overlay windows (`WS_EX_NOREDIRECTIONBITMAP`).
///
/// Keeps all Direct2D rasterization and compositing 100% on the GPU without system-memory GDI DIB
/// allocations or `UpdateLayeredWindow` CPU copies.
pub struct DCompSwapSurface {
    _d3d_device: ID3D11Device,
    d3d_ctx: ID3D11DeviceContext,
    dxgi_device: IDXGIDevice,
    dxgi_factory: IDXGIFactory2,
    d2d_factory: ID2D1Factory1,
    _d2d_device: ID2D1Device,
    d2d_ctx: ID2D1DeviceContext,
    dcomp_device: IDCompositionDevice,
    _dcomp_target: IDCompositionTarget,
    dcomp_visual: IDCompositionVisual,
    swapchain: Option<IDXGISwapChain1>,
    target_bmp: Option<ID2D1Bitmap1>,
    width: u32,
    height: u32,
    visual_bound: bool,
    needs_commit: bool,
}

impl DCompSwapSurface {
    /// Initializes a hardware DirectComposition swapchain stack bound to `hwnd`.
    /// Returns `Err((stage, hresult))` if hardware D3D11 or DirectComposition is unavailable so
    /// the caller can log the exact stage and cleanly fall back to `ID2D1DCRenderTarget`.
    pub fn new(hwnd: HWND) -> Result<Self, (&'static str, i32)> {
        if hwnd.0.is_null() {
            return Err(("null_hwnd", -1));
        }
        unsafe {
            let feature_levels = [
                D3D_FEATURE_LEVEL_11_1,
                D3D_FEATURE_LEVEL_11_0,
                D3D_FEATURE_LEVEL_10_1,
                D3D_FEATURE_LEVEL_10_0,
            ];
            let mut d3d_dev_opt: Option<ID3D11Device> = None;
            let mut d3d_ctx_opt: Option<ID3D11DeviceContext> = None;
            if let Err(e) = D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_HARDWARE,
                windows::Win32::Foundation::HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                Some(&feature_levels),
                D3D11_SDK_VERSION,
                Some(&mut d3d_dev_opt),
                None,
                Some(&mut d3d_ctx_opt),
            ) {
                return Err(("d3d11_create_device", e.code().0));
            }
            let Some(d3d_device) = d3d_dev_opt else {
                return Err(("d3d11_null_device", -1));
            };
            let Some(d3d_ctx) = d3d_ctx_opt else {
                return Err(("d3d11_null_ctx", -1));
            };
            let dxgi_device: IDXGIDevice = d3d_device
                .cast()
                .map_err(|e| ("cast_dxgi_device", e.code().0))?;
            let adapter = dxgi_device
                .GetAdapter()
                .map_err(|e| ("get_adapter", e.code().0))?;
            let dxgi_factory: IDXGIFactory2 = adapter
                .GetParent()
                .map_err(|e| ("get_dxgi_factory2", e.code().0))?;

            let d2d_factory: ID2D1Factory1 =
                D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)
                    .map_err(|e| ("d2d1_factory1", e.code().0))?;
            let d2d_device = d2d_factory
                .CreateDevice(&dxgi_device)
                .map_err(|e| ("d2d1_create_device", e.code().0))?;
            let d2d_ctx = d2d_device
                .CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE)
                .map_err(|e| ("d2d1_create_ctx", e.code().0))?;
            d2d_ctx.SetDpi(96.0, 96.0);

            let dcomp_device: IDCompositionDevice = DCompositionCreateDevice(&dxgi_device)
                .map_err(|e| ("dcomp_create_device", e.code().0))?;
            let dcomp_target = dcomp_device
                .CreateTargetForHwnd(hwnd, true)
                .map_err(|e| ("dcomp_create_target", e.code().0))?;
            let dcomp_visual = dcomp_device
                .CreateVisual()
                .map_err(|e| ("dcomp_create_visual", e.code().0))?;
            dcomp_target
                .SetRoot(&dcomp_visual)
                .map_err(|e| ("dcomp_set_root", e.code().0))?;
            dcomp_device
                .Commit()
                .map_err(|e| ("dcomp_commit", e.code().0))?;

            Ok(Self {
                _d3d_device: d3d_device,
                d3d_ctx,
                dxgi_device,
                dxgi_factory,
                d2d_factory,
                _d2d_device: d2d_device,
                d2d_ctx,
                dcomp_device,
                _dcomp_target: dcomp_target,
                dcomp_visual,
                swapchain: None,
                target_bmp: None,
                width: 0,
                height: 0,
                visual_bound: false,
                needs_commit: true,
            })
        }
    }

    /// Returns the underlying hardware `ID2D1RenderTarget` (`ID2D1DeviceContext` derefs directly).
    #[inline]
    pub fn render_target(&self) -> &ID2D1RenderTarget {
        &self.d2d_ctx
    }

    /// Returns the `ID2D1Factory` that owns `render_target()`.
    /// All geometries, stroke styles, and layers drawn onto `render_target()` MUST come from this
    /// factory to avoid `D2DERR_WRONG_FACTORY` (`0x88990012`).
    #[inline]
    pub fn d2d_factory(&self) -> ID2D1Factory {
        let base: &ID2D1Factory = &self.d2d_factory;
        base.clone()
    }

    /// Ensures the DXGI composition swapchain & Direct2D target bitmap match `(w, h)`.
    /// Resizing preserves `d2d_ctx` and all device-level cached bitmaps/brushes/PBR textures.
    pub fn ensure_size(&mut self, w: i32, h: i32) -> bool {
        let bw = w.max(2) as u32;
        let bh = h.max(2) as u32;
        if self.swapchain.is_some()
            && self.target_bmp.is_some()
            && self.width == bw
            && self.height == bh
        {
            return true;
        }

        unsafe {
            self.d2d_ctx.SetTarget(None);
            self.target_bmp = None;
            self.d3d_ctx.Flush();

            if let Some(ref sc) = self.swapchain {
                if sc
                    .ResizeBuffers(2, bw, bh, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SWAP_CHAIN_FLAG(0))
                    .is_err()
                {
                    let _ = self.dcomp_visual.SetContent(None);
                    let _ = self.dcomp_device.Commit();
                    self.swapchain = None;
                    self.visual_bound = false;
                } else {
                    self.needs_commit = true;
                }
            }

            if self.swapchain.is_none() {
                let desc = DXGI_SWAP_CHAIN_DESC1 {
                    Width: bw,
                    Height: bh,
                    Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    Stereo: false.into(),
                    SampleDesc: DXGI_SAMPLE_DESC {
                        Count: 1,
                        Quality: 0,
                    },
                    BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
                    BufferCount: 2,
                    Scaling: DXGI_SCALING_STRETCH,
                    SwapEffect: DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
                    AlphaMode: DXGI_ALPHA_MODE_PREMULTIPLIED,
                    Flags: 0,
                };
                let Ok(sc) =
                    self.dxgi_factory
                        .CreateSwapChainForComposition(&self.dxgi_device, &desc, None)
                else {
                    return false;
                };
                if self.dcomp_visual.SetContent(&sc).is_err() {
                    return false;
                }
                if self.dcomp_device.Commit().is_err() {
                    return false;
                }
                self.swapchain = Some(sc);
                self.visual_bound = true;
                self.needs_commit = true;
            }

            let Some(ref sc) = self.swapchain else {
                return false;
            };
            let Ok(surface) = sc.GetBuffer::<IDXGISurface>(0) else {
                return false;
            };
            let bmp_props = D2D1_BITMAP_PROPERTIES1 {
                pixelFormat: D2D1_PIXEL_FORMAT {
                    format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
                },
                dpiX: 96.0,
                dpiY: 96.0,
                bitmapOptions: D2D1_BITMAP_OPTIONS_TARGET | D2D1_BITMAP_OPTIONS_CANNOT_DRAW,
                colorContext: std::mem::ManuallyDrop::new(None),
            };
            let Ok(target) = self
                .d2d_ctx
                .CreateBitmapFromDxgiSurface(&surface, Some(&bmp_props))
            else {
                return false;
            };
            self.d2d_ctx.SetTarget(&target);
            self.target_bmp = Some(target);
            self.width = bw;
            self.height = bh;
            true
        }
    }

    /// Presents the newly drawn frame with `SyncInterval = 0` (DWM composites tear-free on VSync
    /// without blocking the UI thread). Returns `Err(hresult)` on device lost / recreate target.
    pub fn present(&mut self) -> Result<(), i32> {
        let Some(ref sc) = self.swapchain else {
            return Err(-1);
        };
        unsafe {
            let hr = sc.Present(0, DXGI_PRESENT(0));
            if hr.is_err() {
                return Err(hr.0);
            }
            if !self.visual_bound {
                let _ = self.dcomp_visual.SetContent(sc);
                self.visual_bound = true;
                self.needs_commit = true;
            }
            if self.needs_commit {
                let _ = self.dcomp_device.Commit();
                self.needs_commit = false;
            }
        }
        Ok(())
    }

    /// Releases swapchain backbuffers and trims DXGI driver memory when widgets are hidden/covered.
    pub fn release_buffers(&mut self) {
        unsafe {
            self.d2d_ctx.SetTarget(None);
            self.target_bmp = None;
            self.d3d_ctx.Flush();
            if self.visual_bound {
                let _ = self.dcomp_visual.SetContent(None);
                let _ = self.dcomp_device.Commit();
                self.visual_bound = false;
            }
            self.swapchain = None;
            self.width = 0;
            self.height = 0;
            self.needs_commit = true;
            if let Ok(dxgi3) = self.dxgi_device.cast::<IDXGIDevice3>() {
                dxgi3.Trim();
            }
        }
    }
}

impl Drop for DCompSwapSurface {
    fn drop(&mut self) {
        self.release_buffers();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::core::{w, PCWSTR};
    use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;
    use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DestroyWindow, WS_EX_NOACTIVATE, WS_EX_NOREDIRECTIONBITMAP,
        WS_EX_TOOLWINDOW, WS_POPUP,
    };

    #[test]
    fn dcomp_swap_surface_initializes_resizes_presents_and_trims() {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            let hinst = GetModuleHandleW(None).unwrap_or_default();
            let hwnd = CreateWindowExW(
                WS_EX_NOREDIRECTIONBITMAP | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                w!("STATIC"),
                PCWSTR::null(),
                WS_POPUP,
                0,
                0,
                64,
                64,
                None,
                None,
                Some(hinst.into()),
                None,
            )
            .expect("test host HWND");

            match DCompSwapSurface::new(hwnd) {
                Ok(mut surf) => {
                    assert!(surf.ensure_size(320, 180));
                    {
                        let rt = surf.render_target();
                        let fac = surf.d2d_factory();
                        let rr = windows::Win32::Graphics::Direct2D::D2D1_ROUNDED_RECT {
                            rect: windows::Win32::Graphics::Direct2D::Common::D2D_RECT_F {
                                left: 8.0,
                                top: 8.0,
                                right: 200.0,
                                bottom: 100.0,
                            },
                            radiusX: 14.0,
                            radiusY: 14.0,
                        };
                        let geo = fac.CreateRoundedRectangleGeometry(&rr).expect("rr geo");
                        let layer = rt.CreateLayer(None).expect("layer");
                        let brush = rt
                            .CreateSolidColorBrush(
                                &D2D1_COLOR_F { r: 0.9, g: 0.6, b: 0.2, a: 0.9 },
                                None,
                            )
                            .expect("brush");

                        rt.BeginDraw();
                        rt.Clear(Some(&D2D1_COLOR_F {
                            r: 0.1,
                            g: 0.08,
                            b: 0.06,
                            a: 0.5,
                        }));
                        let params = windows::Win32::Graphics::Direct2D::D2D1_LAYER_PARAMETERS {
                            contentBounds: rr.rect,
                            geometricMask: std::mem::ManuallyDrop::new(Some(geo.cast().expect("geo cast"))),
                            maskAntialiasMode: windows::Win32::Graphics::Direct2D::D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
                            maskTransform: windows_numerics::Matrix3x2::identity(),
                            opacity: 1.0,
                            opacityBrush: std::mem::ManuallyDrop::new(None),
                            layerOptions: windows::Win32::Graphics::Direct2D::D2D1_LAYER_OPTIONS_NONE,
                        };
                        rt.PushLayer(&params, &layer);
                        std::mem::ManuallyDrop::into_inner(params.geometricMask);
                        std::mem::ManuallyDrop::into_inner(params.opacityBrush);
                        rt.FillRoundedRectangle(&rr, &brush);
                        rt.PopLayer();
                        assert!(
                            rt.EndDraw(None, None).is_ok(),
                            "EndDraw must succeed without D2DERR_WRONG_FACTORY when using dcomp.d2d_factory()"
                        );
                    }
                    assert!(surf.present().is_ok());

                    // Resize must succeed without dropping device context
                    assert!(surf.ensure_size(640, 240));
                    {
                        let rt = surf.render_target();
                        rt.BeginDraw();
                        rt.Clear(Some(&D2D1_COLOR_F {
                            r: 0.0,
                            g: 0.0,
                            b: 0.0,
                            a: 0.0,
                        }));
                        assert!(rt.EndDraw(None, None).is_ok());
                    }
                    assert!(surf.present().is_ok());

                    // Trim & recreate cleanly
                    surf.release_buffers();
                    assert!(surf.ensure_size(200, 120));
                }
                Err((stage, hr)) => {
                    panic!("DCompSwapSurface::new failed at stage={} hr=0x{:08X}", stage, hr as u32);
                }
            }
            let _ = DestroyWindow(hwnd);
            CoUninitialize();
        }
    }
}
