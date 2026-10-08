use std::{
    ffi::c_void,
    mem::ManuallyDrop,
};

use windows::{
    core::{Interface, Result},
    Win32::{
        Media::MediaFoundation::{
            IMFMediaBuffer,
            IMFMediaEventGenerator,
            IMFSample,
            IMFTransform,
            MFCreateMediaType,
            MFCreateMemoryBuffer,
            MFCreateSample,
            MF_E_NO_EVENTS_AVAILABLE,
            MF_E_TRANSFORM_NEED_MORE_INPUT,
            MF_EVENT_FLAG_NO_WAIT,
            MF_MT_FRAME_RATE,
            MF_MT_FRAME_SIZE,
            MF_MT_INTERLACE_MODE,
            MF_MT_MAJOR_TYPE,
            MF_MT_SUBTYPE,
            MF_TRANSFORM_ASYNC,
            MF_TRANSFORM_ASYNC_UNLOCK,
            MFVideoInterlace_MixedInterlaceOrProgressive,
            MFVideoFormat_H264,
            MFVideoFormat_NV12,
            MFMediaType_Video,
            MFShutdown,
            MFStartup,
            MF_VERSION,
            MFSTARTUP_FULL,
            METransformHaveOutput,
            METransformNeedInput,
            MFT_CATEGORY_VIDEO_DECODER,
            MFT_ENUM_FLAG,
            MFT_ENUM_FLAG_HARDWARE,
            MFT_ENUM_FLAG_SORTANDFILTER,
            MFT_MESSAGE_NOTIFY_BEGIN_STREAMING,
            MFT_MESSAGE_NOTIFY_START_OF_STREAM,
            MFT_OUTPUT_DATA_BUFFER,
            MFT_OUTPUT_STREAM_PROVIDES_SAMPLES,
            MFT_REGISTER_TYPE_INFO,
            MFT_ENUM_FLAG_SYNCMFT,
            MFT_ENUM_FLAG_ASYNCMFT,
            MFTEnumEx,
        },
        System::Com::CoTaskMemFree,
    },
};
use windows::core::Error;
use windows::Win32::Media::MediaFoundation::{MFT_OUTPUT_STREAM_CAN_PROVIDE_SAMPLES, MF_E_INVALIDMEDIATYPE, MF_E_NO_MORE_TYPES, MF_E_TRANSFORM_STREAM_CHANGE, MF_MT_DEFAULT_STRIDE};

pub struct DecodedFrame {
    pub data: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub timestamp: i64,
    pub duration: i64,
}

pub struct Decoder {
    transform: IMFTransform,
    events: Option<IMFMediaEventGenerator>,
    width: u32,
    height: u32,
    surface_width: u32,
    surface_height: u32,
    stride: usize,
    fps: u32,
    started: bool,
    async_mode: bool,
    pending_inputs: u32,
    pending_outputs: u32,
}

impl Decoder {
    pub fn new(
        width: u32,
        height: u32,
        fps: u32,
    ) -> Result<Self> {
        unsafe {
            MFStartup(
                MF_VERSION,
                MFSTARTUP_FULL,
            )?;

            let transform =
                Self::find_decoder(
                    true,
                )?
                    .or_else(|| {
                        Self::find_decoder(
                            false,
                        )
                            .ok()
                            .flatten()
                    })
                    .unwrap();

            let attributes =
                transform.GetAttributes()?;

            let async_mode =
                attributes
                    .GetUINT32(
                        &MF_TRANSFORM_ASYNC,
                    )
                    .unwrap_or(0)
                    != 0;

            if async_mode {
                attributes.SetUINT32(
                    &MF_TRANSFORM_ASYNC_UNLOCK,
                    1,
                )?;
            }

            Self::configure(
                &transform,
                width,
                height,
                fps,
            )?;

            let events =
                if async_mode {
                    Some(
                        transform
                            .cast::<IMFMediaEventGenerator>()?,
                    )
                } else {
                    None
                };

            let mut decoder =
                Self {
                    transform,
                    events,
                    width,
                    height,
                    surface_width: width,
                    surface_height: height,
                    stride: width as usize,
                    fps,
                    started: true,
                    async_mode,
                    pending_inputs: 0,
                    pending_outputs: 0,
                };

            if decoder.async_mode {
                decoder.refresh_events()?;
            }

            Ok(decoder)
        }
    }

    unsafe fn find_decoder(
        hardware: bool,
    ) -> Result<Option<IMFTransform>> {
        let input_type =
            MFT_REGISTER_TYPE_INFO {
                guidMajorType:
                MFMediaType_Video,
                guidSubtype:
                MFVideoFormat_H264,
            };

        let output_type =
            MFT_REGISTER_TYPE_INFO {
                guidMajorType:
                MFMediaType_Video,
                guidSubtype:
                MFVideoFormat_NV12,
            };

        let flags: MFT_ENUM_FLAG =
            if hardware {
                MFT_ENUM_FLAG_HARDWARE
                    | MFT_ENUM_FLAG_SORTANDFILTER
            } else {
                MFT_ENUM_FLAG_SORTANDFILTER
            };

        let mut activates =
            std::ptr::null_mut();

        let mut count =
            0u32;

        MFTEnumEx(
            MFT_CATEGORY_VIDEO_DECODER,
            flags,
            Some(&input_type),
            Some(&output_type),
            &mut activates,
            &mut count,
        )?;

        if count == 0
            || activates.is_null()
        {
            if !activates.is_null() {
                CoTaskMemFree(
                    Some(
                        activates
                            as *const c_void
                    ),
                );
            }

            return Ok(None);
        }

        let mut transform =
            None;

        for index in 0..count {
            let activate =
                (*activates.add(index as usize))
                    .clone();

            if let Some(activate) =
                activate
            {
                if let Ok(value) =
                    activate
                        .ActivateObject::<IMFTransform>()
                {
                    transform =
                        Some(value);

                    break;
                }
            }
        }

        CoTaskMemFree(
            Some(
                activates
                    as *const c_void
            ),
        );

        Ok(transform)
    }

    unsafe fn configure(
        transform: &IMFTransform,
        width: u32,
        height: u32,
        fps: u32,
    ) -> Result<()> {
        let frame_size =
            ((width as u64) << 32)
                | height as u64;

        let frame_rate =
            ((fps as u64) << 32)
                | 1;

        let input_type =
            MFCreateMediaType()?;

        input_type.SetGUID(
            &MF_MT_MAJOR_TYPE,
            &MFMediaType_Video,
        )?;

        input_type.SetGUID(
            &MF_MT_SUBTYPE,
            &MFVideoFormat_H264,
        )?;

        input_type.SetUINT64(
            &MF_MT_FRAME_SIZE,
            frame_size,
        )?;

        input_type.SetUINT64(
            &MF_MT_FRAME_RATE,
            frame_rate,
        )?;

        input_type.SetUINT32(
            &MF_MT_INTERLACE_MODE,
            MFVideoInterlace_MixedInterlaceOrProgressive.0 as u32,
        )?;

        transform.SetInputType(
            0,
            &input_type,
            0,
        )?;

        let output_type =
            MFCreateMediaType()?;

        output_type.SetGUID(
            &MF_MT_MAJOR_TYPE,
            &MFMediaType_Video,
        )?;

        output_type.SetGUID(
            &MF_MT_SUBTYPE,
            &MFVideoFormat_NV12,
        )?;

        output_type.SetUINT64(
            &MF_MT_FRAME_SIZE,
            frame_size,
        )?;

        output_type.SetUINT64(
            &MF_MT_FRAME_RATE,
            frame_rate,
        )?;

        output_type.SetUINT32(
            &MF_MT_INTERLACE_MODE,
            MFVideoInterlace_MixedInterlaceOrProgressive.0 as u32,
        )?;

        transform.SetOutputType(
            0,
            &output_type,
            0,
        )?;

        transform.ProcessMessage(
            MFT_MESSAGE_NOTIFY_BEGIN_STREAMING,
            0,
        )?;

        transform.ProcessMessage(
            MFT_MESSAGE_NOTIFY_START_OF_STREAM,
            0,
        )?;

        Ok(())
    }

    unsafe fn refresh_events(
        &mut self,
    ) -> Result<()> {
        let events =
            match &self.events {
                Some(events) => events,
                None => return Ok(()),
            };

        loop {
            let event =
                match events.GetEvent(
                    MF_EVENT_FLAG_NO_WAIT,
                ) {
                    Ok(event) => event,
                    Err(error) => {
                        if error.code()
                            == MF_E_NO_EVENTS_AVAILABLE
                        {
                            break;
                        }

                        return Err(error);
                    }
                };

            match event.GetType()? {
                value if value == METransformNeedInput.0 as u32 => {
                    self.pending_inputs += 1;
                }
                value if value == METransformHaveOutput.0 as u32 => {
                    self.pending_outputs += 1;
                }
                _ => {}
            }
        }

        Ok(())
    }unsafe fn renegotiate_output_type(&mut self) -> Result<()> {
        let mut index = 0;

        loop {
            let media_type =
                match self.transform.GetOutputAvailableType(0, index) {
                    Ok(media_type) => media_type,
                    Err(error) => {
                        if error.code() == MF_E_NO_MORE_TYPES {
                            break;
                        }

                        return Err(error);
                    }
                };

            let subtype =
                media_type.GetGUID(&MF_MT_SUBTYPE)?;

            if subtype == MFVideoFormat_NV12 {
                self.transform.SetOutputType(
                    0,
                    &media_type,
                    0,
                )?;

                let frame_size =
                    media_type.GetUINT64(
                        &MF_MT_FRAME_SIZE,
                    )?;

                self.surface_width =
                    (frame_size >> 32) as u32;

                self.surface_height =
                    frame_size as u32;

                self.stride =
                    match media_type.GetUINT32(
                        &MF_MT_DEFAULT_STRIDE,
                    ) {
                        Ok(value) => {
                            let value = value as i32;
                            if value == 0 {
                                self.surface_width as usize
                            } else {
                                value.unsigned_abs() as usize
                            }
                        }
                        Err(_) => {
                            self.surface_width as usize
                        }
                    };

                return Ok(());
            }

            index += 1;
        }

        Err(Error::new(
            MF_E_INVALIDMEDIATYPE,
            "decoder did not expose an NV12 output type",
        ))
    }
    pub fn create_sample(
        &self,
        data: &[u8],
        timestamp: i64,
        duration: i64,
    ) -> Result<IMFSample> {
        unsafe {
            let buffer =
                MFCreateMemoryBuffer(
                    data.len() as u32,
                )?;

            let mut data_ptr =
                std::ptr::null_mut();

            let mut max_length =
                0u32;

            let mut current_length =
                0u32;

            buffer.Lock(
                &mut data_ptr,
                Some(&mut max_length),
                Some(&mut current_length),
            )?;

            std::ptr::copy_nonoverlapping(
                data.as_ptr(),
                data_ptr,
                data.len(),
            );

            buffer.Unlock()?;

            buffer.SetCurrentLength(
                data.len() as u32,
            )?;

            let sample =
                MFCreateSample()?;

            sample.AddBuffer(
                &buffer,
            )?;

            sample.SetSampleTime(
                timestamp,
            )?;

            sample.SetSampleDuration(
                duration,
            )?;

            Ok(sample)
        }
    }

    pub fn submit(
        &mut self,
        sample: &IMFSample,
    ) -> Result<()> {
        unsafe {
            if self.async_mode {
                while self.pending_inputs == 0 {
                    self.refresh_events()?;

                    if self.pending_inputs == 0 {
                        std::thread::sleep(
                            std::time::Duration::from_millis(1),
                        );
                    }
                }

                self.pending_inputs -= 1;
            }

            self.transform.ProcessInput(
                0,
                sample,
                0,
            )
        }
    }

    pub fn process_output(
        &mut self,
    ) -> Result<Option<DecodedFrame>> {
        unsafe {
            if self.async_mode {
                self.refresh_events()?;

                if self.pending_outputs == 0 {
                    return Ok(None);
                }

                self.pending_outputs -= 1;
            }

            loop {
                let stream_info =
                    self.transform
                        .GetOutputStreamInfo(0)?;

                let mut output =
                    MFT_OUTPUT_DATA_BUFFER::default();

                output.dwStreamID = 0;

                if stream_info.dwFlags
                    & (MFT_OUTPUT_STREAM_PROVIDES_SAMPLES.0 as u32
                    | MFT_OUTPUT_STREAM_CAN_PROVIDE_SAMPLES.0 as u32)
                    == 0
                {
                    let surface_size =
                    (self.stride
                        * self.surface_height as usize
                        * 3
                        / 2) as u32;

                    let buffer =
                        MFCreateMemoryBuffer(
                            stream_info.cbSize.max(
                                surface_size,
                            ),
                        )?;

                    let sample =
                        MFCreateSample()?;

                    sample.AddBuffer(
                        &buffer,
                    )?;

                    output.pSample =
                        ManuallyDrop::new(
                            Some(sample),
                        );
                }

                let mut status = 0u32;

                match self.transform.ProcessOutput(
                    0,
                    std::slice::from_mut(
                        &mut output,
                    ),
                    &mut status,
                ) {
                    Ok(()) => {}
                    Err(error) => {
                        let _events =
                            ManuallyDrop::take(
                                &mut output.pEvents,
                            );

                        let _sample =
                            ManuallyDrop::take(
                                &mut output.pSample,
                            );

                        if error.code()
                            == MF_E_TRANSFORM_STREAM_CHANGE
                        {
                            self.renegotiate_output_type()?;
                            continue;
                        }

                        if error.code()
                            == MF_E_TRANSFORM_NEED_MORE_INPUT
                        {
                            return Ok(None);
                        }

                        return Err(error);
                    }
                }

                let sample =
                    ManuallyDrop::take(
                        &mut output.pSample,
                    );

                let _events =
                    ManuallyDrop::take(
                        &mut output.pEvents,
                    );

                let sample =
                    match sample {
                        Some(sample) => sample,
                        None => return Ok(None),
                    };

                let timestamp =
                    sample
                        .GetSampleTime()
                        .unwrap_or(0);

                let duration =
                    sample
                        .GetSampleDuration()
                        .unwrap_or(
                            10_000_000i64
                                / self.fps
                                as i64,
                        );

                let buffer =
                    sample
                        .ConvertToContiguousBuffer()?;

                let length =
                    buffer.GetCurrentLength()?
                        as usize;

                if length == 0 {
                    return Ok(None);
                }

                let mut data_ptr =
                    std::ptr::null_mut();

                let mut max_length = 0u32;
                let mut current_length = 0u32;

                buffer.Lock(
                    &mut data_ptr,
                    Some(&mut max_length),
                    Some(&mut current_length),
                )?;

                let data =
                    std::slice::from_raw_parts(
                        data_ptr,
                        length,
                    )
                        .to_vec();

                buffer.Unlock()?;

                return Ok(Some(
                    DecodedFrame {
                        data,
                        width: self.width,
                        height: self.height,
                        timestamp,
                        duration,
                    },
                ));
            }
        }
    }

    pub fn decode(
        &mut self,
        data: &[u8],
        timestamp: i64,
        duration: i64,
    ) -> Result<Vec<DecodedFrame>> {
        let sample =
            self.create_sample(
                data,
                timestamp,
                duration,
            )?;

        self.submit(
            &sample,
        )?;

        self.drain_outputs()
    }
    pub fn nv12_to_bgra(
        &self,
        nv12: &[u8],
    ) -> Vec<u8> {
        let width =
            self.width as usize;

        let height =
            self.height as usize;

        let surface_height =
            self.surface_height as usize;

        let stride =
            self.stride;

        let y_size =
            stride * surface_height;

        let required =
            y_size
                + stride * surface_height / 2;

        assert!(
            nv12.len() >= required
        );

        let mut bgra =
            vec![
                0u8;
                width
                    * height
                    * 4
            ];

        let y_plane =
            &nv12[..y_size];

        let uv_plane =
            &nv12[y_size..required];

        for y in 0..height {
            for x in 0..width {
                let y_value =
                    y_plane[
                        y * stride + x
                        ] as i32;

                let uv_index =
                    (y / 2) * stride
                        + (x / 2) * 2;

                let u =
                    uv_plane[
                        uv_index
                        ] as i32;

                let v =
                    uv_plane[
                        uv_index + 1
                        ] as i32;

                let c =
                    (y_value - 16)
                        .max(0);

                let d =
                    u - 128;

                let e =
                    v - 128;

                let r =
                    (298 * c
                        + 409 * e
                        + 128)
                        >> 8;

                let g =
                    (298 * c
                        - 100 * d
                        - 208 * e
                        + 128)
                        >> 8;

                let b =
                    (298 * c
                        + 516 * d
                        + 128)
                        >> 8;

                let dst =
                    (y * width + x)
                        * 4;

                bgra[dst] =
                    b.clamp(0, 255)
                        as u8;

                bgra[dst + 1] =
                    g.clamp(0, 255)
                        as u8;

                bgra[dst + 2] =
                    r.clamp(0, 255)
                        as u8;

                bgra[dst + 3] =
                    255;
            }
        }

        bgra
    }
    pub fn drain_outputs(
        &mut self,
    ) -> Result<Vec<DecodedFrame>> {
        let mut frames = Vec::new();

        loop {
            match self.process_output()? {
                Some(frame) => {
                    frames.push(frame);
                }
                None => {
                    break;
                }
            }
        }

        Ok(frames)
    }
}

impl Drop for Decoder {
    fn drop(&mut self) {
        if self.started {
            unsafe {
                let _ =
                    MFShutdown();
            }
        }
    }
}