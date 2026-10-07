use capture::Capture;
use renderer::Renderer;
use std::time::Instant;
use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, EventLoop},
    window::{Window, WindowId},
};
use decoder::Decoder;
use encoder::Encoder;
use protocol::VideoPacket;

struct App {
    window: Option<&'static Window>,
    renderer: Option<Renderer<'static>>,
    capture: Option<Capture>,
    last_fps_time: Instant,
    frame_count: u64,
    encoder: Option<Encoder>,
    encoded_frame_count: u64,
    pending_input: u32,
    h264_file: Option<std::fs::File>,
    video_sequence: u64,
    decoder: Option<Decoder>,
    decoded_frame_count: u64,

}

impl Default for App {
    fn default() -> Self {
        Self {
            window: None,
            renderer: None,
            capture: None,
            last_fps_time: Instant::now(),
            frame_count: 0,
            encoder: None,
            encoded_frame_count: 0,
            pending_input: 0,
            h264_file: None,
            video_sequence: 0,
            decoder: None,
            decoded_frame_count: 0,
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(
        &mut self,
        event_loop: &ActiveEventLoop,
    ) {
        if self.window.is_some() {
            return;
        }

        let window = event_loop
            .create_window(
                Window::default_attributes()
                    .with_title("Remote Desktop"),
            )
            .unwrap();

        let window =
            Box::leak(Box::new(window));

        let renderer =
            pollster::block_on(
                Renderer::new(window),
            );

        let capture =
            Capture::new().unwrap();

        let size =
            capture.size().unwrap();

        println!(
            "Capture: {}x{}",
            size.0,
            size.1
        );

        let encoder =
            Encoder::new(
                size.0 as u32,
                size.1 as u32,
                60,
                8_000_000,
            ).unwrap();

        let decoder =
            Decoder::new(
                size.0 as u32,
                size.1 as u32,
                60,
            ).unwrap();

        let h264_file =
            std::fs::File::create(
                "capture.h264"
            ).unwrap();

        let test_packet =
            VideoPacket::new(
                1,
                1000,
                166666,
                true,
                vec![1, 2, 3, 4],
            )
                .unwrap();

        let test_bytes =
            test_packet
                .encode()
                .unwrap();

        let decoded =
            VideoPacket::decode(
                &test_bytes,
            )
                .unwrap();

        assert_eq!(
            test_packet,
            decoded
        );

        println!(
            "Protocol test passed: {} bytes",
            test_bytes.len()
        );


        self.h264_file =
            Some(h264_file);
        self.encoder = Some(encoder);
        self.decoder = Some(decoder);
        self.window = Some(window);
        self.renderer = Some(renderer);
        self.capture = Some(capture);
        self.last_fps_time = Instant::now();
        window.request_redraw();
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => {
                event_loop.exit();
            }

            WindowEvent::Resized(size) => {
                if let Some(renderer) =
                    &mut self.renderer
                {
                    renderer.resize(
                        size.width,
                        size.height,
                    );
                }
            }

            WindowEvent::RedrawRequested => {
                let mut have_output = 0u32;

                if let Some(encoder) = &self.encoder {
                    match encoder.poll_events() {
                        Ok((input, output)) => {
                            have_output = output;
                            self.pending_input += input;
                        }
                        Err(error) => {
                            println!(
                                "Encoder event error: {:?}",
                                error
                            );
                        }
                    }
                }

                if let (
                    Some(capture),
                    Some(renderer),
                ) = (
                    &mut self.capture,
                    &mut self.renderer,
                ) {
                    if let Ok(Some((frame, _discarded))) =
                        capture.try_latest_frame()
                    {
                        let data =
                            capture
                                .read_texture(&frame)
                                .unwrap();

                        renderer.update_frame(
                            frame.width,
                            frame.height,
                            &data,
                        );

                        self.frame_count += 1;

                        if let Some(encoder) =
                            &self.encoder
                        {
                            if self.pending_input > 0 {
                                let sample =
                                    encoder
                                        .create_input_sample(
                                            &data,
                                            frame.timestamp,
                                        )
                                        .unwrap();

                                encoder
                                    .submit(
                                        0,
                                        &sample,
                                    )
                                    .unwrap();

                                self.pending_input -= 1;

                                println!(
                                    "Submitted input sample"
                                );
                            }
                        }
                    }
                }

                if have_output > 0 {
                    if let Some(encoder) =
                        &self.encoder
                    {if let Some(packet) =
                        encoder
                            .process_output()
                            .unwrap()
                    {
                        let video_packet =
                            VideoPacket::new(
                                self.video_sequence,
                                packet.timestamp,
                                packet.duration,
                                packet.keyframe,
                                packet.data,
                            )
                                .unwrap();
                        if let Some(decoder) =
                            &mut self.decoder
                        {
                            let decoded_frames =
                                decoder
                                    .decode(
                                        &video_packet.payload,
                                        video_packet.timestamp,
                                        video_packet.duration,
                                    )
                                    .unwrap();

                            for decoded in decoded_frames {
                                let bgra =
                                    decoder.nv12_to_bgra(
                                        &decoded.data,
                                    );

                                if let Some(renderer) =
                                    &mut self.renderer
                                {
                                    renderer.update_frame(
                                        decoded.width,
                                        decoded.height,
                                        &bgra,
                                    );
                                }

                                self.decoded_frame_count += 1;
                            }
                        }
                        self.video_sequence += 1;

                        let wire =
                            video_packet
                                .encode()
                                .unwrap();

                        self.encoded_frame_count += 1;

                        println!(
                            "VideoPacket seq={} payload={} wire={} keyframe={}",
                            video_packet.sequence,
                            video_packet.payload.len(),
                            wire.len(),
                            video_packet.keyframe
                        );
                    }
                    }
                }

                if let Some(renderer) =
                    &self.renderer
                {
                    renderer.render();
                }
            }

            _ => {}
        }
    }

    fn about_to_wait(
        &mut self,
        _event_loop: &ActiveEventLoop,
    ) {
        if let Some(window) = self.window {
            window.request_redraw();
        }

        let elapsed =
            self.last_fps_time.elapsed();

        if elapsed.as_secs_f64() >= 1.0 {
            println!(
                "Rendered FPS: {:.1}",
                self.frame_count as f64
                    / elapsed.as_secs_f64()
            );

            println!(
                "Encoded FPS: {:.1}",
                self.encoded_frame_count as f64
                    / elapsed.as_secs_f64()
            );

            self.frame_count = 0;
            self.encoded_frame_count = 0;
            self.last_fps_time =
                Instant::now();
        }
    }
}

fn main() {
    let event_loop =
        EventLoop::new().unwrap();

    let mut app = App::default();

    event_loop
        .run_app(&mut app)
        .unwrap();
}