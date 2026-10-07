use windows::{
    core::{factory, Interface, Result},
    Graphics::{
        Capture::{
            Direct3D11CaptureFramePool,
            GraphicsCaptureItem,
            GraphicsCaptureSession,
        },
        DirectX::{
            Direct3D11::IDirect3DDevice,
            DirectXPixelFormat,
        },
    },
    Win32::{
        Foundation::{HMODULE, POINT},
        Graphics::{
            Direct3D::{
                D3D_DRIVER_TYPE_HARDWARE,
                D3D_FEATURE_LEVEL_11_0,
            },
            Direct3D11::{
                D3D11CreateDevice,
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                D3D11_SDK_VERSION,
                ID3D11Device,
                ID3D11DeviceContext,
            },
            Dxgi::IDXGIDevice,
            Gdi::{
                MonitorFromPoint,
                MONITOR_DEFAULTTOPRIMARY,
            },
        },
        System::WinRT::{
            Direct3D11::CreateDirect3D11DeviceFromDXGIDevice,
            Graphics::Capture::IGraphicsCaptureItemInterop,
        },
    },
};
use windows::Graphics::Capture::Direct3D11CaptureFrame;
use windows::Win32::Graphics::Direct3D11::ID3D11Texture2D;
use windows::Win32::System::WinRT::Direct3D11::IDirect3DDxgiInterfaceAccess;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_CPU_ACCESS_READ,
    D3D11_MAP_READ,
    D3D11_MAPPED_SUBRESOURCE,
    D3D11_USAGE_STAGING,
};
pub struct Capture {
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    direct3d_device: IDirect3DDevice,
    item: GraphicsCaptureItem,
    frame_pool: Direct3D11CaptureFramePool,
    session: GraphicsCaptureSession,
    staging: Option<ID3D11Texture2D>,
    staging_width: u32,
    staging_height: u32,
}
pub struct CapturedFrame {
    pub texture: ID3D11Texture2D,
    pub width: u32,
    pub height: u32,
    pub timestamp: i64,
}

impl Capture {
    pub fn new() -> Result<Self> {
        unsafe {
            let mut device = None;
            let mut context = None;

            D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_HARDWARE,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                Some(&[D3D_FEATURE_LEVEL_11_0]),
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut context),
            )?;

            let device = device.unwrap();
            let context = context.unwrap();

            let dxgi_device: IDXGIDevice = device.cast()?;

            let inspectable =
                CreateDirect3D11DeviceFromDXGIDevice(&dxgi_device)?;

            let direct3d_device: IDirect3DDevice =
                inspectable.cast()?;

            let monitor = MonitorFromPoint(
                POINT { x: 0, y: 0 },
                MONITOR_DEFAULTTOPRIMARY,
            );

            if monitor.is_invalid() {
                panic!("Failed to get primary monitor");
            }

            let interop =
                factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()?;

            let item: GraphicsCaptureItem =
                interop.CreateForMonitor(monitor)?;

            let size = item.Size()?;

            let frame_pool =
                Direct3D11CaptureFramePool::CreateFreeThreaded(
                    &direct3d_device,
                    DirectXPixelFormat::B8G8R8A8UIntNormalized,
                    2,
                    size,
                )?;

            let session =
                frame_pool.CreateCaptureSession(&item)?;

            session.StartCapture()?;

            Ok(Self {
                device,
                context,
                direct3d_device,
                item,
                frame_pool,
                session,
                staging: None,
                staging_width: 0,
                staging_height: 0,
            })
        }
    }

    pub fn is_ready(&self) -> bool {
        true
    }

    pub fn direct3d_device(&self) -> &IDirect3DDevice {
        &self.direct3d_device
    }

    pub fn item(&self) -> &GraphicsCaptureItem {
        &self.item
    }

    pub fn size(&self) -> Result<(i32, i32)> {
        let size = self.item.Size()?;
        Ok((size.Width, size.Height))
    }

    pub fn try_latest_frame(
        &self,
    ) -> Result<Option<(CapturedFrame, u32)>> {
        let mut latest = None;
        let mut discarded = 0;

        loop {
            match self.frame_pool.TryGetNextFrame() {
                Ok(frame) => {
                    if latest.is_some() {
                        discarded += 1;
                    }

                    latest = Some(frame);
                }
                Err(_) => {
                    break;
                }
            }
        }

        let frame = match latest {
            Some(frame) => frame,
            None => return Ok(None),
        };

        let size = frame.ContentSize()?;

        let surface = frame.Surface()?;

        let access =
            surface.cast::<IDirect3DDxgiInterfaceAccess>()?;

        let texture =
            unsafe {
                access.GetInterface::<ID3D11Texture2D>()?
            };

        let timestamp =
            frame.SystemRelativeTime()?.Duration;

        Ok(Some((
            CapturedFrame {
                texture,
                width: size.Width as u32,
                height: size.Height as u32,
                timestamp,
            },
            discarded,
        )))
    }

    pub fn frame_size(
        &self,
        frame: &Direct3D11CaptureFrame,
    ) -> Result<(i32, i32)> {
        let size = frame.ContentSize()?;
        Ok((size.Width, size.Height))
    }

    pub fn frame_timestamp(
        &self,
        frame: &Direct3D11CaptureFrame,
    ) -> Result<i64> {
        Ok(frame.SystemRelativeTime()?.Duration)
    }
    pub fn texture(
        &self,
        frame: &Direct3D11CaptureFrame,
    ) -> Result<ID3D11Texture2D> {
        unsafe {
            let surface = frame.Surface()?;

            let access: IDirect3DDxgiInterfaceAccess =
                surface.cast()?;

            let texture =
                access.GetInterface::<ID3D11Texture2D>()?;

            Ok(texture)
        }
    }
    pub fn read_texture(
        &mut self,
        frame: &CapturedFrame,
    ) -> Result<Vec<u8>> {
        unsafe {
            let mut desc = std::mem::zeroed();

            frame.texture.GetDesc(&mut desc);

            let width = desc.Width as usize;
            let height = desc.Height as usize;

            let needs_new_staging =
                self.staging.is_none()
                    || self.staging_width != desc.Width
                    || self.staging_height != desc.Height;

            if needs_new_staging {
                desc.Usage = D3D11_USAGE_STAGING;
                desc.BindFlags = 0;
                desc.CPUAccessFlags =
                    D3D11_CPU_ACCESS_READ.0 as u32;
                desc.MiscFlags = 0;

                let mut staging = None;

                self.device.CreateTexture2D(
                    &desc,
                    None,
                    Some(&mut staging),
                )?;

                self.staging = staging;
                self.staging_width = desc.Width;
                self.staging_height = desc.Height;
            }

            let staging =
                self.staging.as_ref().unwrap();

            self.context.CopyResource(
                staging,
                &frame.texture,
            );

            let mut mapped =
                D3D11_MAPPED_SUBRESOURCE::default();

            self.context.Map(
                staging,
                0,
                D3D11_MAP_READ,
                0,
                Some(&mut mapped),
            )?;

            let row_bytes = width * 4;

            let mut data =
                vec![0u8; row_bytes * height];

            let source =
                std::slice::from_raw_parts(
                    mapped.pData as *const u8,
                    mapped.RowPitch as usize * height,
                );

            for y in 0..height {
                let src_start =
                    y * mapped.RowPitch as usize;

                let dst_start =
                    y * row_bytes;

                data[dst_start..dst_start + row_bytes]
                    .copy_from_slice(
                        &source[
                            src_start
                                ..src_start + row_bytes
                            ],
                    );
            }

            self.context.Unmap(
                staging,
                0,
            );

            Ok(data)
        }
    
    }
}