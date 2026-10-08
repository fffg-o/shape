use capture::Capture;
use encoder::Encoder;
use protocol::VideoPacket;
use std::time::Duration;
use transport::{send_video_packet, TransportServer};

const SERVER_ADDR: &str = "0.0.0.0:5000";
const CERTIFICATE_PATH: &str = "host.cert";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let server =
        TransportServer::bind(SERVER_ADDR.parse()?)?;

    std::fs::write(
        CERTIFICATE_PATH,
        server.certificate(),
    )?;

    println!(
        "Host listening: {}",
        server.local_addr()?
    );

    println!(
        "Certificate written: {}",
        CERTIFICATE_PATH
    );

    let mut capture =
        Capture::new()?;

    let size =
        capture.size()?;

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
        )?;

    println!("Waiting for client");

    let connection =
        server.accept().await?;

    println!(
        "Client connected: {}",
        connection.remote_address()
    );

    let mut pending_input = 0u32;
    let mut sequence = 0u64;

    loop {
        let (need_input, have_output) =
            encoder.poll_events()?;

        pending_input +=
            need_input;

        if let Some((frame, discarded)) =
            capture.try_latest_frame()?
        {
            if discarded > 0 {
                println!(
                    "Capture discarded: {}",
                    discarded
                );
            }

            let data =
                capture.read_texture(
                    &frame
                )?;

            if pending_input > 0 {
                let sample =
                    encoder.create_input_sample(
                        &data,
                        frame.timestamp,
                    )?;

                encoder.submit(
                    0,
                    &sample,
                )?;

                pending_input -= 1;
            }
        }

        if have_output > 0 {
            if let Some(packet) =
                encoder.process_output()?
            {
                let video_packet =
                    VideoPacket::new(
                        sequence,
                        packet.timestamp,
                        packet.duration,
                        packet.keyframe,
                        packet.data,
                    )?;

                let fragment_count =
                    send_video_packet(
                        &connection,
                        &video_packet,
                    )?;

                println!(
                    "Sent seq={} payload={} fragments={} keyframe={}",
                    video_packet.sequence,
                    video_packet.payload.len(),
                    fragment_count,
                    video_packet.keyframe
                );

                sequence += 1;
            }
        }

        tokio::time::sleep(
            Duration::from_millis(1)
        )
            .await;
    }
}