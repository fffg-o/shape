use std::ffi::c_void;

use windows::{
    core::{Interface, Result},
    Win32::{
        Media::MediaFoundation::{
            IMFTransform,
            MFTEnumEx,
            MFT_CATEGORY_VIDEO_ENCODER,
            MFT_ENUM_FLAG_HARDWARE,
            MFT_ENUM_FLAG_SORTANDFILTER,
            MFT_MESSAGE_NOTIFY_BEGIN_STREAMING,
            MFT_MESSAGE_NOTIFY_START_OF_STREAM,
            MFT_REGISTER_TYPE_INFO,
            MFCreateMediaType,
            MF_TRANSFORM_ASYNC_UNLOCK,
            MFMediaType_Video,
            MFVideoFormat_H264,
            MFVideoFormat_NV12,
            MF_MT_AVG_BITRATE,
            MF_MT_FRAME_RATE,
            MF_MT_FRAME_SIZE,
            MF_MT_INTERLACE_MODE,
            MFVideoInterlace_Progressive,
            MFStartup,
            MFShutdown,
            MF_VERSION,
            MFSTARTUP_FULL,
        },
        System::Com::CoTaskMemFree,
    },
};
use windows::Win32::Media::MediaFoundation::{IMFMediaEventGenerator, IMFSample, METransformHaveOutput, METransformNeedInput, MFCreateMemoryBuffer, MFCreateSample, MFSampleExtension_CleanPoint, MFT_OUTPUT_DATA_BUFFER, MFT_OUTPUT_STREAM_CAN_PROVIDE_SAMPLES, MFT_OUTPUT_STREAM_PROVIDES_SAMPLES, MF_EVENT_FLAG_NO_WAIT, MF_EVENT_MFT_INPUT_STREAM_ID, MF_E_NO_EVENTS_AVAILABLE, MF_MT_MAJOR_TYPE, MF_MT_SUBTYPE};

#[derive(Debug)]
pub struct EncodedPacket {
    pub data: Vec<u8>,
    pub timestamp: i64,
    pub duration: i64,
    pub keyframe: bool,
}
pub struct Encoder {
    transform: IMFTransform,
    events: IMFMediaEventGenerator,
    width: u32,
    height: u32,
    fps: u32,
    bitrate: u32,
    started: bool,
}

impl Encoder {
    pub fn new(
        width: u32,
        height: u32,
        fps: u32,
        bitrate: u32,
    ) -> Result<Self> {
        unsafe {
            MFStartup(
                MF_VERSION,
                MFSTARTUP_FULL,
            )?;

            let input_type = MFT_REGISTER_TYPE_INFO {
                guidMajorType: MFMediaType_Video,
                guidSubtype: MFVideoFormat_NV12,
            };

            let output_type = MFT_REGISTER_TYPE_INFO {
                guidMajorType: MFMediaType_Video,
                guidSubtype: MFVideoFormat_H264,
            };

            let mut activates =
                std::ptr::null_mut();

            let mut count = 0u32;

            MFTEnumEx(
                MFT_CATEGORY_VIDEO_ENCODER,
                MFT_ENUM_FLAG_HARDWARE
                    | MFT_ENUM_FLAG_SORTANDFILTER,
                Some(&input_type),
                Some(&output_type),
                &mut activates,
                &mut count,
            )?;

            if count == 0 || activates.is_null() {
                CoTaskMemFree(
                    Some(activates as *const c_void)
                );

                panic!("No hardware H264 encoder found");

            }

            let mut transform = None;

            for index in 0..count {
                let activate =
                    (*activates.add(index as usize))
                        .clone();

                if let Some(activate) =
                    activate
                {
                    match activate
                        .ActivateObject::<IMFTransform>()
                    {
                        Ok(value) => {
                            transform =
                                Some(value);
                            break;
                        }
                        Err(_) => {}
                    }
                }
            }

            CoTaskMemFree(
                Some(activates as *const c_void)
            );

            let transform =
                transform.unwrap();

            let attributes = transform.GetAttributes()?;

            attributes.SetUINT32(
                &MF_TRANSFORM_ASYNC_UNLOCK,
                1,
            )?;
            let events =
                transform.cast::<IMFMediaEventGenerator>()?;

            Self::configure_transform(
                &transform,
                width,
                height,
                fps,
                bitrate,
            )?;

            Ok(Self {
                transform,
                events,
                width,
                height,
                fps,
                bitrate,
                started: true,
            })
        }

    }

    unsafe fn configure_transform(
        transform: &IMFTransform,
        width: u32,
        height: u32,
        fps: u32,
        bitrate: u32,
    ) -> Result<()> {
        let frame_size =
            ((width as u64) << 32)
                | height as u64;

        let frame_rate =
            ((fps as u64) << 32) | 1;

        let output_type =
            MFCreateMediaType()?;

        output_type.SetGUID(
            &MF_MT_MAJOR_TYPE,
            &MFMediaType_Video,
        )?;

        output_type.SetGUID(
            &MF_MT_SUBTYPE,
            &MFVideoFormat_H264,
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
            &MF_MT_AVG_BITRATE,
            bitrate,
        )?;

        output_type.SetUINT32(
            &MF_MT_INTERLACE_MODE,
            MFVideoInterlace_Progressive.0 as u32,
        )?;

        transform.SetOutputType(
            0,
            &output_type,
            0,
        )?;

        let input_type =
            MFCreateMediaType()?;

        input_type.SetGUID(
            &MF_MT_MAJOR_TYPE,
            &MFMediaType_Video,
        )?;

        input_type.SetGUID(
            &MF_MT_SUBTYPE,
            &MFVideoFormat_NV12,
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
            MFVideoInterlace_Progressive.0 as u32,
        )?;

        transform.SetInputType(
            0,
            &input_type,
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

    pub fn is_ready(&self) -> bool {
        !self.transform.as_raw().is_null()
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn fps(&self) -> u32 {
        self.fps
    }

    pub fn bitrate(&self) -> u32 {
        self.bitrate
    }
    pub fn poll_events(
        &self,
    ) -> Result<(u32, u32)> {
        unsafe {
            let mut need_input = 0u32;
            let mut have_output = 0u32;

            loop {
                let event =
                    match self.events.GetEvent(
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

                let event_type =
                    event.GetType()?;

                if event_type
                    == METransformNeedInput.0 as u32
                {
                    need_input += 1;
                }

                if event_type
                    == METransformHaveOutput.0 as u32
                {
                    have_output += 1;
                }
            }

            Ok((
                need_input,
                have_output,
            ))
        }

    }
    pub fn bgra_to_nv12(
        &self,
        bgra: &[u8],
    ) -> Vec<u8> {
        let width = self.width as usize;
        let height = self.height as usize;

        assert!(width % 2 == 0);
        assert!(height % 2 == 0);

        assert_eq!(
            bgra.len(),
            width * height * 4
        );

        let y_size = width * height;

        let uv_size = width * height / 2;

        let mut nv12 =
            vec![0u8; y_size + uv_size];

        for y in 0..height {
            for x in 0..width {
                let src =
                    (y * width + x) * 4;

                let b = bgra[src] as i32;
                let g = bgra[src + 1] as i32;
                let r = bgra[src + 2] as i32;

                let value =
                    ((47 * r
                        + 157 * g
                        + 16 * b
                        + 128)
                        >> 8)
                        + 16;

                nv12[y * width + x] =
                    value.clamp(16, 235) as u8;
            }
        }

        let uv_start = y_size;

        for y in (0..height).step_by(2) {
            for x in (0..width).step_by(2) {
                let mut r_sum = 0i32;
                let mut g_sum = 0i32;
                let mut b_sum = 0i32;

                for dy in 0..2 {
                    for dx in 0..2 {
                        let px =
                            x + dx;
                        let py =
                            y + dy;

                        let src =
                            (py * width + px) * 4;

                        b_sum +=
                            bgra[src] as i32;

                        g_sum +=
                            bgra[src + 1]
                                as i32;

                        r_sum +=
                            bgra[src + 2]
                                as i32;
                    }
                }

                let r = r_sum / 4;
                let g = g_sum / 4;
                let b = b_sum / 4;

                let u =
                    ((-26 * r
                        - 87 * g
                        + 112 * b
                        + 512)
                        >> 10)
                        + 128;

                let v =
                    ((112 * r
                        - 102 * g
                        - 10 * b
                        + 512)
                        >> 10)
                        + 128;

                let uv_index =
                    uv_start
                        + (y / 2) * width
                        + x;

                nv12[uv_index] =
                    u.clamp(16, 240) as u8;

                nv12[uv_index + 1] =
                    v.clamp(16, 240) as u8;
            }
        }

        nv12
    }
    pub fn create_input_sample(
        &self,
        bgra: &[u8],
        timestamp: i64,
    ) -> Result<IMFSample> {
        let nv12 =
            self.bgra_to_nv12(bgra);

        let buffer_size =
            nv12.len() as u32;

        unsafe {
            let buffer =
                MFCreateMemoryBuffer(
                    buffer_size,
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
                nv12.as_ptr(),
                data_ptr,
                nv12.len(),
            );

            buffer.Unlock()?;

            buffer.SetCurrentLength(
                nv12.len() as u32,
            )?;

            let sample =
                MFCreateSample()?;

            sample.AddBuffer(&buffer)?;

            sample.SetSampleTime(
                timestamp,
            )?;

            sample.SetSampleDuration(
                10_000_000i64
                    / self.fps as i64,
            )?;

            Ok(sample)
        }
    }
    pub fn submit(
        &self,
        stream_id: u32,
        sample: &IMFSample,
    ) -> Result<()> {
        unsafe {
            self.transform.ProcessInput(
                stream_id,
                sample,
                0,
            )
        }
    }
    pub fn process_output(
        &self,
    ) -> Result<Option<EncodedPacket>> {
        unsafe {
            let stream_info =
                self.transform
                    .GetOutputStreamInfo(0)?;

            let mut output =
                MFT_OUTPUT_DATA_BUFFER::default();

            output.dwStreamID = 0;

            let sample_flags =
                MFT_OUTPUT_STREAM_PROVIDES_SAMPLES.0 as u32
                    | MFT_OUTPUT_STREAM_CAN_PROVIDE_SAMPLES.0 as u32;

            if (stream_info.dwFlags & sample_flags) == 0 {
                let size =
                    stream_info.cbSize.max(1);

                let buffer =
                    MFCreateMemoryBuffer(size)?;

                let sample =
                    MFCreateSample()?;

                sample.AddBuffer(&buffer)?;

                output.pSample =
                    std::mem::ManuallyDrop::new(
                        Some(sample),
                    );
            }

            let mut status = 0u32;

            self.transform.ProcessOutput(
                0,
                std::slice::from_mut(
                    &mut output,
                ),
                &mut status,
            )?;

            let sample =
                std::mem::ManuallyDrop::take(
                    &mut output.pSample,
                );

            let sample =
                match sample {
                    Some(sample) => sample,
                    None => return Ok(None),
                };

            let buffer =
                sample.ConvertToContiguousBuffer()?;

            let length =
                buffer.GetCurrentLength()? as usize;

            if length == 0 {
                return Ok(None);
            }

            let timestamp =
                sample.GetSampleTime()?;

            let duration =
                sample.GetSampleDuration()?;

            let keyframe =
                sample
                    .GetUINT32(
                        &MFSampleExtension_CleanPoint,
                    )
                    .unwrap_or(0)
                    != 0;

            let mut data_ptr =
                std::ptr::null_mut();

            buffer.Lock(
                &mut data_ptr,
                None,
                None,
            )?;

            let data =
                std::slice::from_raw_parts(
                    data_ptr,
                    length,
                )
                    .to_vec();

            buffer.Unlock()?;

            Ok(Some(
                EncodedPacket {
                    data,
                    timestamp,
                    duration,
                    keyframe,
                },
            ))
        }

    }
}

impl Drop for Encoder {
    fn drop(&mut self) {
        if self.started {
            unsafe {
                let _ = MFShutdown();
            }
        }
    }
}