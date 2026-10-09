use decoder::Decoder;
use protocol::{
    FrameAssembler,
    VideoPacket,
};
use renderer::Renderer;
use std::{
    sync::mpsc::{
        self,
        Receiver,
        SyncSender,
    },
    time::Instant,
};
use std::time::Duration;
use transport::TransportClient;
use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::{
        ActiveEventLoop,
        EventLoop,
    },
    window::{
        Window,
        WindowId,
    },
};

const SERVER_ADDR: &str =
    "127.0.0.1:5000";

const CERTIFICATE_PATH: &str =
    "host.cert";

struct App {
    window: Option<&'static Window>,
    renderer: Option<Renderer<'static>>,
    decoder: Option<Decoder>,
    video_rx: Option<Receiver<VideoPacket>>,
    network_started: bool,
    decoded_frame_count: u64,
    last_fps_time: Instant,
}

impl Default for App {
    fn default() -> Self {
        Self {
            window: None,
            renderer: None,
            decoder: None,
            video_rx: None,
            network_started: false,
            decoded_frame_count: 0,
            last_fps_time: Instant::now(),
        }
    }
}

fn start_network(tx: SyncSender<VideoPacket>) {
    std::thread::spawn(move || {
        let runtime = match tokio::runtime::Runtime::new() {
            Ok(runtime) => runtime,
            Err(error) => {
                println!("Tokio runtime error: {}", error);
                return;
            }
        };

        let result = runtime.block_on(async move {
            let certificate = std::fs::read(CERTIFICATE_PATH)?;

            let debug_drop_every = std::env::var(
                "SHAPE_DEBUG_DROP_EVERY_NTH_FRAME",
            )
                .ok()
                .and_then(|value| value.parse::<u64>().ok())
                .filter(|value| *value > 0);

            let client = TransportClient::connect(
                SERVER_ADDR.parse()?,
                &certificate,
            )
                .await?;

            println!("Connected to host: {}", SERVER_ADDR);

            let mut assembler = FrameAssembler::new();
            let mut last_sequence: Option<u64> = None;
            let mut awaiting_keyframe = true;
            let mut last_keyframe_request =
                Instant::now() - Duration::from_millis(500);

            loop {
                let packet = client
                    .recv_video_packet(&mut assembler)
                    .await?;

                if debug_drop_every.is_some_and(|n| {
                    packet.sequence > 0 && packet.sequence % n == 0
                }) {
                    println!(
                        "DEBUG: intentionally dropped frame {}",
                        packet.sequence
                    );
                    continue;
                }

                if let Some(last) = last_sequence {
                    if packet.sequence <= last {
                        continue;
                    }

                    if packet.sequence > last.saturating_add(1) {
                        println!(
                            "Video frame gap: expected {}, received {}",
                            last.saturating_add(1),
                            packet.sequence
                        );

                        awaiting_keyframe = true;
                    }
                }

                last_sequence = Some(packet.sequence);

                if awaiting_keyframe && !packet.keyframe {
                    if last_keyframe_request.elapsed()
                        >= Duration::from_millis(500)
                    {
                        last_keyframe_request = Instant::now();

                        match client.request_keyframe().await {
                            Ok(()) => {
                                println!("Keyframe request sent");
                            }
                            Err(error) => {
                                println!(
                                    "Keyframe request failed: {}",
                                    error
                                );
                            }
                        }
                    }

                    continue;
                }

                if packet.keyframe && awaiting_keyframe {
                    println!(
                        "Video recovery at keyframe {}",
                        packet.sequence
                    );

                    awaiting_keyframe = false;
                }

                if tx.send(packet).is_err() {
                    break;
                }
            }

            Ok::<
                (),
                Box<dyn std::error::Error + Send + Sync>,
            >(())
        });

        if let Err(error) = result {
            println!("Transport error: {}", error);
        }
    });
}

impl ApplicationHandler for App {
    fn resumed(
        &mut self,
        event_loop: &ActiveEventLoop,
    ) {
        if self.window.is_some() {
            return;
        }

        let window =
            event_loop.create_window(
                Window::default_attributes()
                    .with_title(
                        "Remote Desktop Client"
                    ),
            )
                .unwrap();

        let window =
            Box::leak(
                Box::new(window)
            );

        let renderer =
            pollster::block_on(
                Renderer::new(window)
            );

        let decoder =
            Decoder::new(
                1920,
                1080,
                60,
            )
                .unwrap();

        let (
            tx,
            rx,
        ) =
            mpsc::sync_channel(
                4
            );

        if !self.network_started {
            start_network(tx);
            self.network_started =
                true;
        }

        self.window =
            Some(window);

        self.renderer =
            Some(renderer);

        self.decoder =
            Some(decoder);

        self.video_rx =
            Some(rx);

        self.last_fps_time =
            Instant::now();

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
                if let (
                    Some(receiver),
                    Some(decoder),
                    Some(renderer),
                ) = (
                    &self.video_rx,
                    &mut self.decoder,
                    &mut self.renderer,
                ) {
                    let mut latest_frame: Option<(u32, u32, Vec<u8>)> = None;

                    loop {
                        let packet = match receiver.try_recv() {
                            Ok(packet) => packet,
                            Err(mpsc::TryRecvError::Empty) => break,
                            Err(mpsc::TryRecvError::Disconnected) => break,
                        };

                        let decoded_frames = match decoder.decode(
                            &packet.payload,
                            packet.timestamp,
                            packet.duration,
                        ) {
                            Ok(frames) => frames,
                            Err(error) => {
                                println!("Decoder error: {:?}", error);
                                continue;
                            }
                        };

                        for decoded in decoded_frames {
                            latest_frame = Some((
                                decoded.width,
                                decoded.height,
                                decoded.data,
                            ));

                            self.decoded_frame_count += 1;
                        }
                    }

                    if let Some((width, height, nv12)) = latest_frame {
                        let bgra = decoder.nv12_to_bgra(&nv12);
                        renderer.update_frame(width, height, &bgra);
                    }
                }

                if let Some(renderer) = &self.renderer {
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
        if let Some(window) =
            self.window
        {
            window.request_redraw();
        }

        let elapsed =
            self.last_fps_time
                .elapsed();

        if elapsed.as_secs_f64()
            >= 1.0
        {
            println!(
                "Decoded FPS: {:.1}",
                self.decoded_frame_count
                    as f64
                    / elapsed.as_secs_f64()
            );

            self.decoded_frame_count = 0;

            self.last_fps_time =
                Instant::now();
        }
    }
}

fn main() {
    let event_loop =
        EventLoop::new()
            .unwrap();

    let mut app =
        App::default();

    event_loop
        .run_app(&mut app)
        .unwrap();
}