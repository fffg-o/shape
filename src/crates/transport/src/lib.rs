use protocol::FrameAssembler;
use protocol::VideoFragment;
use protocol::VideoPacket;
use bytes::Bytes;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use quinn::{
    ClientConfig,
    Connection,
    Endpoint,
    ServerConfig,
    TransportConfig,
};
use rcgen::generate_simple_self_signed;
use rustls::{
    pki_types::{
        CertificateDer,
        PrivateKeyDer,
        PrivatePkcs8KeyDer,
    },
    RootCertStore,
};
use std::{
    error::Error,
    net::SocketAddr,
    sync::Arc,
};


const KEYFRAME_REQUEST_MAGIC: [u8; 8] = *b"SHAPEKF1";

pub type TransportResult<T> =
Result<T, Box<dyn Error + Send + Sync>>;

pub struct TransportServer {
    endpoint: Endpoint,
    certificate: Vec<u8>,
}

impl TransportServer {
    pub fn bind(
        addr: SocketAddr,
    ) -> TransportResult<Self> {
        let certified =
            generate_simple_self_signed(
                vec!["localhost".to_string()],
            )?;

        let certificate =
            certified.cert.der().to_vec();

        let key =
            PrivateKeyDer::Pkcs8(
                PrivatePkcs8KeyDer::from(
                    certified
                        .signing_key
                        .serialize_der(),
                ),
            );

        let mut server_config =
            ServerConfig::with_single_cert(
                vec![
                    certified.cert.der().clone()
                ],
                key,
            )?;

        let mut transport_config =
            TransportConfig::default();

        transport_config
            .datagram_receive_buffer_size(
                Some(4 * 1024 * 1024),
            );

        transport_config
            .datagram_send_buffer_size(
                4 * 1024 * 1024,
            );

        server_config.transport_config(
            Arc::new(transport_config),
        );

        let endpoint =
            Endpoint::server(
                server_config,
                addr,
            )?;

        Ok(Self {
            endpoint,
            certificate,
        })
    }

    pub fn local_addr(
        &self,
    ) -> TransportResult<SocketAddr> {
        Ok(self.endpoint.local_addr()?)
    }

    pub fn certificate(
        &self,
    ) -> &[u8] {
        &self.certificate
    }

    pub async fn accept(
        &self,
    ) -> TransportResult<Connection> {
        let incoming =
            self.endpoint
                .accept()
                .await
                .ok_or("endpoint closed")?;

        let connection =
            incoming.await?;

        Ok(connection)
    }
    pub async fn recv(
        connection: &Connection,
    ) -> TransportResult<Vec<u8>> {
        Ok(
            connection
                .read_datagram()
                .await?
                .to_vec()
        )
    }
}
pub struct TransportClient {
    endpoint: Endpoint,
    connection: Connection,
}

impl TransportClient {
    pub async fn connect(
        addr: SocketAddr,
        certificate: &[u8],
    ) -> TransportResult<Self> {
        let mut roots =
            RootCertStore::empty();

        roots.add(
            CertificateDer::from(
                certificate.to_vec(),
            ),
        )?;

        let client_config =
            ClientConfig::with_root_certificates(
                Arc::new(roots),
            )?;

        let mut endpoint =
            Endpoint::client(
                "0.0.0.0:0".parse()?,
            )?;

        endpoint
            .set_default_client_config(
                client_config,
            );

        let connection =
            endpoint
                .connect(
                    addr,
                    "localhost",
                )?
                .await?;

        Ok(Self {
            endpoint,
            connection,
        })
    }
    pub async fn request_keyframe(&self) -> TransportResult<()> {
        let mut stream = self.connection.open_uni().await?;

        stream.write_all(&KEYFRAME_REQUEST_MAGIC).await?;
        stream.finish()?;

        Ok(())
    }

    pub async fn recv_video_packet(
        &self,
        assembler: &mut FrameAssembler,
    ) -> TransportResult<VideoPacket> {
        loop {
            let data =
                self.connection
                    .read_datagram()
                    .await?;

            let fragment =
                VideoFragment::decode(
                    &data
                )?;

            if let Some(packet) =
                assembler.push(fragment)?
            {
                return Ok(packet);
            }
        }
    }

    pub fn connection(
        &self,
    ) -> &Connection {
        &self.connection
    }

    pub fn max_datagram_size(
        &self,
    ) -> Option<usize> {
        self.connection
            .max_datagram_size()
    }

    pub fn send(
        &self,
        data: Vec<u8>,
    ) -> TransportResult<()> {
        self.connection.send_datagram(
            Bytes::from(data),
        )?;

        Ok(())
    }

    pub async fn recv(
        &self,
    ) -> TransportResult<Vec<u8>> {
        Ok(
            self.connection
                .read_datagram()
                .await?
                .to_vec()
        )
    }
}

pub async fn connect(
    addr: SocketAddr,
    certificate: &[u8],
) -> TransportResult<Connection> {
    let mut roots =
        RootCertStore::empty();

    roots.add(
        CertificateDer::from(
            certificate.to_vec(),
        ),
    )?;

    let client_config =
        ClientConfig::with_root_certificates(
            Arc::new(roots),
        )?;

    let mut  endpoint =
        Endpoint::client(
            "0.0.0.0:0".parse()?,
        )?;

    endpoint
        .set_default_client_config(
            client_config,
        );

    let connection =
        endpoint
            .connect(
                addr,
                "localhost",
            )?
            .await?;

    std::mem::forget(endpoint);

    Ok(connection)
}

pub fn send_datagram(
    connection: &Connection,
    data: Vec<u8>,
) -> TransportResult<()> {
    connection.send_datagram(
        Bytes::from(data),
    )?;

    Ok(())
}

pub async fn recv_datagram(
    connection: &Connection,
) -> TransportResult<Vec<u8>> {
    let data =
        connection
            .read_datagram()
            .await?;

    Ok(data.to_vec())
}
pub fn send_video_packet(
    connection: &Connection,
    packet: &VideoPacket,
) -> TransportResult<usize> {
    let max_size =
        connection
            .max_datagram_size()
            .ok_or(
                "QUIC datagrams are not available"
            )?;

    let fragments =
        packet.fragment(
            max_size
        )?;

    let count =
        fragments.len();

    for fragment in fragments {
        let data =
            fragment.encode()?;

        connection.send_datagram(
            Bytes::from(data),
        )?;
    }

    Ok(count)
}
pub async fn listen_for_keyframe_requests(
    connection: Connection,
    tx: tokio::sync::mpsc::Sender<()>,
) -> TransportResult<()> {
    loop {
        let mut stream = connection.accept_uni().await?;
        let mut request = [0u8; 8];

        let result = tokio::time::timeout(
            Duration::from_millis(500),
            stream.read_exact(&mut request),
        )
            .await;

        if matches!(result, Ok(Ok(_))) && request == KEYFRAME_REQUEST_MAGIC {
            if tx.send(()).await.is_err() {
                return Ok(());
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use protocol::VideoPacket;

    #[tokio::test]
    async fn quic_datagram_roundtrip() {
        let server =
            TransportServer::bind(
                "127.0.0.1:0"
                    .parse()
                    .unwrap(),
            )
                .unwrap();

        let server_addr =
            server.local_addr().unwrap();

        let certificate =
            server.certificate().to_vec();

        let server_task =
            tokio::spawn(async move {
                server.accept().await.unwrap()
            });

        let client =
            TransportClient::connect(
                server_addr,
                &certificate,
            )
                .await
                .unwrap();

        let connection =
            server_task.await.unwrap();

        let packet =
            VideoPacket::new(
                42,
                100000,
                166666,
                true,
                vec![1, 2, 3, 4, 5],
            )
                .unwrap();

        let encoded =
            packet.encode().unwrap();

        client
            .send(encoded)
            .unwrap();

        let received =
            connection
                .read_datagram()
                .await
                .unwrap();

        let decoded =
            VideoPacket::decode(
                &received,
            )
                .unwrap();

        assert_eq!(
            decoded.sequence,
            42
        );

        assert_eq!(
            decoded.timestamp,
            100000
        );

        assert!(
            decoded.keyframe
        );

        assert_eq!(
            decoded.payload,
            vec![1, 2, 3, 4, 5]
        );

        println!(
            "QUIC datagram test passed: {} bytes",
            received.len()
        );
    }
}
#[tokio::test]
async fn quic_video_fragment_roundtrip() {
    let server =
        TransportServer::bind(
            "127.0.0.1:0"
                .parse()
                .unwrap(),
        )
            .unwrap();

    let server_addr =
        server.local_addr().unwrap();

    let certificate =
        server.certificate().to_vec();

    let server_task =
        tokio::spawn(async move {
            server.accept().await.unwrap()
        });

    let client =
        TransportClient::connect(
            server_addr,
            &certificate,
        )
            .await
            .unwrap();

    let connection =
        server_task.await.unwrap();

    let max_size =
        client
            .max_datagram_size()
            .unwrap();

    println!(
        "QUIC max datagram size: {}",
        max_size
    );

    let payload: Vec<u8> =
        (0..50000)
            .map(|value| {
                (value % 251) as u8
            })
            .collect();

    let packet =
        VideoPacket::new(
            100,
            500000,
            166666,
            true,
            payload.clone(),
        )
            .unwrap();

    let fragment_count =
        send_video_packet(
            client.connection(),
            &packet,
        )
            .unwrap();

    println!(
        "Sent {} fragments",
        fragment_count
    );

    let mut assembler =
        FrameAssembler::new();

    let rebuilt = loop {
        let data =
            connection
                .read_datagram()
                .await
                .unwrap();

        let fragment =
            VideoFragment::decode(
                &data
            )
                .unwrap();

        if let Some(packet) =
            assembler
                .push(fragment)
                .unwrap()
        {
            break packet;
        }
    };

    assert_eq!(
        rebuilt.sequence,
        packet.sequence
    );

    assert_eq!(
        rebuilt.timestamp,
        packet.timestamp
    );

    assert_eq!(
        rebuilt.keyframe,
        packet.keyframe
    );

    assert_eq!(
        rebuilt.payload,
        payload
    );
}