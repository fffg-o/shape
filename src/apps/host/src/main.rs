
use capture::Capture;
use encoder::Encoder;
use protocol::VideoPacket;
use std::time::{Duration, Instant};
use transport::{
    listen_for_keyframe_requests,
    send_video_packet,
    TransportServer,
};
const SERVER_ADDR: &str = "0.0.0.0:5000";
const CERTIFICATE_PATH: &str = "host.cert";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let server = TransportServer::bind(SERVER_ADDR.parse()?)?;

    std::fs::write(CERTIFICATE_PATH, server.certificate())?;

    println!("Host listening: {}", server.local_addr()?);
    println!("Certificate written: {}", CERTIFICATE_PATH);

    let mut capture = Capture::new()?;
    let size = capture.size()?;

    println!("Capture: {}x{}", size.0, size.1);

    let encoder = Encoder::new(
        size.0 as u32,
        size.1 as u32,
        60,
        8_000_000,
    )?;

    println!("Waiting for client");

    let connection = server.accept().await?;

    println!(
        "Client connected: {}",
        connection.remote_address()
    );
    let (keyframe_tx, mut keyframe_rx) =
        tokio::sync::mpsc::channel::<()>(1);

    let control_connection = connection.clone();

    tokio::spawn(async move {
        if let Err(error) = listen_for_keyframe_requests(
            control_connection,
            keyframe_tx,
        )
            .await
        {
            eprintln!("Control channel stopped: {error}");
        }
    });

    let mut force_keyframe = false;


    let mut pending_input = 0u32;
    let mut pending_output = 0u32;
    let mut sequence = 0u64;

    let mut stats_started = Instant::now();
    let mut sent_frames = 0u64;
    let mut sent_bytes = 0u64;
    let mut discarded_frames = 0u64;

    loop {
        while keyframe_rx.try_recv().is_ok() {
            force_keyframe = true;
        }
        let (need_input, have_output) = encoder.poll_events()?;

        pending_input = pending_input.saturating_add(need_input);
        pending_output = pending_output.saturating_add(have_output);

        while pending_output > 0 {
            pending_output -= 1;

            if let Some(packet) = encoder.process_output()? {
                let payload_size = packet.data.len() as u64;

                let video_packet = VideoPacket::new(
                    sequence,
                    packet.timestamp,
                    packet.duration,
                    packet.keyframe,
                    packet.data,
                )?;

                send_video_packet(&connection, &video_packet)?;

                sent_frames += 1;
                sent_bytes += payload_size;
                sequence += 1;
            }
        }

        if pending_input > 0 {
            if let Some((frame, discarded)) = capture.try_latest_frame()? {
                discarded_frames += discarded as u64;

                if frame.width != encoder.width()
                    || frame.height != encoder.height()
                {
                    return Err(
                        format!(
                            "Capture resolution changed from {}x{} to {}x{}. Encoder reconfiguration is not implemented.",
                            encoder.width(),
                            encoder.height(),
                            frame.width,
                            frame.height
                        )
                            .into(),
                    );
                }

                let data = capture.read_texture(&frame)?;

                let sample = encoder.create_input_sample(
                    &data,
                    frame.timestamp,
                )?;

                if force_keyframe {
                    match encoder.force_next_keyframe() {
                        Ok(()) => {
                            println!("Keyframe request accepted by encoder");
                        }
                        Err(error) => {
                            eprintln!("Encoder keyframe request failed: {error}");
                        }
                    }

                    force_keyframe = false;
                }


                encoder.submit(0, &sample)?;

                pending_input -= 1;
            }
        }

        let elapsed = stats_started.elapsed().as_secs_f64();

        if elapsed >= 1.0 {
            println!(
                "Stream: {:.1} FPS, {:.2} MiB/s, {} frames sent, {} capture frames discarded",
                sent_frames as f64 / elapsed,
                sent_bytes as f64 / elapsed / (1024.0 * 1024.0),
                sent_frames,
                discarded_frames,
            );

            sent_frames = 0;
            sent_bytes = 0;
            discarded_frames = 0;
            stats_started = Instant::now();
        }

        tokio::time::sleep(Duration::from_millis(1)).await;
    }
}