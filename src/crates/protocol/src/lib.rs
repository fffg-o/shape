use std::{collections::HashMap, fmt};

pub const MAGIC: [u8; 4] = *b"RDP1";
pub const VERSION: u8 = 1;
pub const VIDEO_PACKET_TYPE: u8 = 1;
pub const VIDEO_FRAGMENT_PACKET_TYPE: u8 = 2;
pub const HEADER_SIZE: usize = 36;
pub const FRAGMENT_HEADER_SIZE: usize = 40;
pub const MAX_PAYLOAD_SIZE: usize = 64 * 1024 * 1024;
pub const MAX_FRAME_FRAGMENTS: usize = u16::MAX as usize;
pub const MAX_ASSEMBLY_FRAMES: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoPacket {
    pub sequence: u64,
    pub timestamp: i64,
    pub duration: i64,
    pub keyframe: bool,
    pub payload: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoFragment {
    pub sequence: u64,
    pub timestamp: i64,
    pub duration: i64,
    pub keyframe: bool,
    pub fragment_index: u16,
    pub fragment_count: u16,
    pub payload: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProtocolError {
    TooShort,
    InvalidMagic,
    InvalidVersion,
    InvalidPacketType,
    InvalidReserved,
    PayloadTooLarge,
    InvalidLength,
    InvalidFragmentCount,
    InvalidFragmentIndex,
    DatagramTooSmall,
    TooManyFragments,
}

impl fmt::Display for ProtocolError {
    fn fmt(
        &self,
        f: &mut fmt::Formatter<'_>,
    ) -> fmt::Result {
        match self {
            Self::TooShort => {
                write!(f, "packet too short")
            }
            Self::InvalidMagic => {
                write!(f, "invalid magic")
            }
            Self::InvalidVersion => {
                write!(f, "invalid version")
            }
            Self::InvalidPacketType => {
                write!(f, "invalid packet type")
            }
            Self::InvalidReserved => {
                write!(f, "invalid reserved field")
            }
            Self::PayloadTooLarge => {
                write!(f, "payload too large")
            }
            Self::InvalidLength => {
                write!(f, "invalid packet length")
            }
            Self::InvalidFragmentCount => {
                write!(f, "invalid fragment count")
            }
            Self::InvalidFragmentIndex => {
                write!(f, "invalid fragment index")
            }
            Self::DatagramTooSmall => {
                write!(f, "datagram too small")
            }
            Self::TooManyFragments => {
                write!(f, "too many fragments")
            }
        }
    }
}

impl std::error::Error for ProtocolError {}

impl VideoPacket {
    pub fn new(
        sequence: u64,
        timestamp: i64,
        duration: i64,
        keyframe: bool,
        payload: Vec<u8>,
    ) -> Result<Self, ProtocolError> {
        if payload.len() > MAX_PAYLOAD_SIZE {
            return Err(
                ProtocolError::PayloadTooLarge
            );
        }

        Ok(Self {
            sequence,
            timestamp,
            duration,
            keyframe,
            payload,
        })
    }

    pub fn encoded_len(&self) -> usize {
        HEADER_SIZE + self.payload.len()
    }

    pub fn encode(
        &self,
    ) -> Result<Vec<u8>, ProtocolError> {
        if self.payload.len() > MAX_PAYLOAD_SIZE {
            return Err(
                ProtocolError::PayloadTooLarge
            );
        }

        let payload_len =
            self.payload.len() as u32;

        let mut data =
            Vec::with_capacity(
                HEADER_SIZE
                    + self.payload.len(),
            );

        data.extend_from_slice(&MAGIC);
        data.push(VERSION);
        data.push(VIDEO_PACKET_TYPE);

        let flags =
            if self.keyframe {
                1u8
            } else {
                0u8
            };

        data.push(flags);
        data.push(0);

        data.extend_from_slice(
            &self.sequence.to_le_bytes(),
        );

        data.extend_from_slice(
            &self.timestamp.to_le_bytes(),
        );

        data.extend_from_slice(
            &self.duration.to_le_bytes(),
        );

        data.extend_from_slice(
            &payload_len.to_le_bytes(),
        );

        data.extend_from_slice(
            &self.payload,
        );

        Ok(data)
    }

    pub fn decode(
        data: &[u8],
    ) -> Result<Self, ProtocolError> {
        if data.len() < HEADER_SIZE {
            return Err(
                ProtocolError::TooShort
            );
        }

        if data[0..4] != MAGIC {
            return Err(
                ProtocolError::InvalidMagic
            );
        }

        if data[4] != VERSION {
            return Err(
                ProtocolError::InvalidVersion
            );
        }

        if data[5] != VIDEO_PACKET_TYPE {
            return Err(
                ProtocolError::InvalidPacketType
            );
        }

        if data[7] != 0 {
            return Err(
                ProtocolError::InvalidReserved
            );
        }

        let flags = data[6];

        let sequence =
            u64::from_le_bytes(
                data[8..16]
                    .try_into()
                    .unwrap(),
            );

        let timestamp =
            i64::from_le_bytes(
                data[16..24]
                    .try_into()
                    .unwrap(),
            );

        let duration =
            i64::from_le_bytes(
                data[24..32]
                    .try_into()
                    .unwrap(),
            );

        let payload_len =
            u32::from_le_bytes(
                data[32..36]
                    .try_into()
                    .unwrap(),
            ) as usize;

        if payload_len >
            MAX_PAYLOAD_SIZE
        {
            return Err(
                ProtocolError::PayloadTooLarge
            );
        }

        if data.len()
            != HEADER_SIZE + payload_len
        {
            return Err(
                ProtocolError::InvalidLength
            );
        }

        let payload =
            data[HEADER_SIZE..].to_vec();

        Ok(Self {
            sequence,
            timestamp,
            duration,
            keyframe: flags & 1 != 0,
            payload,
        })
    }

    pub fn fragment(
        &self,
        max_datagram_size: usize,
    ) -> Result<Vec<VideoFragment>, ProtocolError> {
        if max_datagram_size <= FRAGMENT_HEADER_SIZE {
            return Err(
                ProtocolError::DatagramTooSmall
            );
        }

        let max_payload =
            max_datagram_size
                - FRAGMENT_HEADER_SIZE;

        let fragment_count =
            self.payload
                .len()
                .div_ceil(max_payload);

        if fragment_count == 0 {
            return Ok(vec![
                VideoFragment {
                    sequence: self.sequence,
                    timestamp: self.timestamp,
                    duration: self.duration,
                    keyframe: self.keyframe,
                    fragment_index: 0,
                    fragment_count: 1,
                    payload: Vec::new(),
                }
            ]);
        }

        if fragment_count >
            MAX_FRAME_FRAGMENTS
        {
            return Err(
                ProtocolError::TooManyFragments
            );
        }

        let fragment_count =
            fragment_count as u16;

        let mut fragments =
            Vec::with_capacity(
                fragment_count as usize
            );

        for index in
            0..fragment_count as usize
        {
            let start =
                index * max_payload;

            let end =
                (start + max_payload)
                    .min(self.payload.len());

            fragments.push(
                VideoFragment {
                    sequence: self.sequence,
                    timestamp: self.timestamp,
                    duration: self.duration,
                    keyframe: self.keyframe,
                    fragment_index:
                    index as u16,
                    fragment_count,
                    payload:
                    self.payload[
                        start..end
                        ].to_vec(),
                },
            );
        }

        Ok(fragments)
    }
}

impl VideoFragment {
    pub fn encoded_len(&self) -> usize {
        FRAGMENT_HEADER_SIZE
            + self.payload.len()
    }

    pub fn encode(
        &self,
    ) -> Result<Vec<u8>, ProtocolError> {
        if self.fragment_count == 0 {
            return Err(
                ProtocolError::InvalidFragmentCount
            );
        }

        if self.fragment_index
            >= self.fragment_count
        {
            return Err(
                ProtocolError::InvalidFragmentIndex
            );
        }

        if self.payload.len() >
            MAX_PAYLOAD_SIZE
        {
            return Err(
                ProtocolError::PayloadTooLarge
            );
        }

        let payload_len =
            self.payload.len() as u32;

        let mut data =
            Vec::with_capacity(
                FRAGMENT_HEADER_SIZE
                    + self.payload.len(),
            );

        data.extend_from_slice(&MAGIC);
        data.push(VERSION);
        data.push(
            VIDEO_FRAGMENT_PACKET_TYPE
        );

        let flags =
            if self.keyframe {
                1u8
            } else {
                0u8
            };

        data.push(flags);
        data.push(0);

        data.extend_from_slice(
            &self.sequence.to_le_bytes(),
        );

        data.extend_from_slice(
            &self.timestamp.to_le_bytes(),
        );

        data.extend_from_slice(
            &self.duration.to_le_bytes(),
        );

        data.extend_from_slice(
            &self.fragment_index
                .to_le_bytes(),
        );

        data.extend_from_slice(
            &self.fragment_count
                .to_le_bytes(),
        );

        data.extend_from_slice(
            &payload_len.to_le_bytes(),
        );

        data.extend_from_slice(
            &self.payload,
        );

        Ok(data)
    }

    pub fn decode(
        data: &[u8],
    ) -> Result<Self, ProtocolError> {
        if data.len() < FRAGMENT_HEADER_SIZE {
            return Err(
                ProtocolError::TooShort
            );
        }

        if data[0..4] != MAGIC {
            return Err(
                ProtocolError::InvalidMagic
            );
        }

        if data[4] != VERSION {
            return Err(
                ProtocolError::InvalidVersion
            );
        }

        if data[5]
            != VIDEO_FRAGMENT_PACKET_TYPE
        {
            return Err(
                ProtocolError::InvalidPacketType
            );
        }

        if data[7] != 0 {
            return Err(
                ProtocolError::InvalidReserved
            );
        }

        let flags = data[6];

        let sequence =
            u64::from_le_bytes(
                data[8..16]
                    .try_into()
                    .unwrap(),
            );

        let timestamp =
            i64::from_le_bytes(
                data[16..24]
                    .try_into()
                    .unwrap(),
            );

        let duration =
            i64::from_le_bytes(
                data[24..32]
                    .try_into()
                    .unwrap(),
            );

        let fragment_index =
            u16::from_le_bytes(
                data[32..34]
                    .try_into()
                    .unwrap(),
            );

        let fragment_count =
            u16::from_le_bytes(
                data[34..36]
                    .try_into()
                    .unwrap(),
            );

        let payload_len =
            u32::from_le_bytes(
                data[36..40]
                    .try_into()
                    .unwrap(),
            ) as usize;

        if fragment_count == 0 {
            return Err(
                ProtocolError::InvalidFragmentCount
            );
        }

        if fragment_index
            >= fragment_count
        {
            return Err(
                ProtocolError::InvalidFragmentIndex
            );
        }

        if payload_len >
            MAX_PAYLOAD_SIZE
        {
            return Err(
                ProtocolError::PayloadTooLarge
            );
        }

        if data.len()
            != FRAGMENT_HEADER_SIZE
            + payload_len
        {
            return Err(
                ProtocolError::InvalidLength
            );
        }

        Ok(Self {
            sequence,
            timestamp,
            duration,
            keyframe: flags & 1 != 0,
            fragment_index,
            fragment_count,
            payload:
            data[
                FRAGMENT_HEADER_SIZE..
                ]
                .to_vec(),
        })
    }
}

struct PartialFrame {
    timestamp: i64,
    duration: i64,
    keyframe: bool,
    fragment_count: u16,
    fragments: Vec<Option<Vec<u8>>>,
    received: usize,
}

pub struct FrameAssembler {
    frames: HashMap<u64, PartialFrame>,
}

impl Default for FrameAssembler {
    fn default() -> Self {
        Self::new()
    }
}

impl FrameAssembler {
    pub fn new() -> Self {
        Self {
            frames: HashMap::new(),
        }
    }

    pub fn push(
        &mut self,
        fragment: VideoFragment,
    ) -> Result<Option<VideoPacket>, ProtocolError> {
        if fragment.fragment_count == 0 {
            return Err(
                ProtocolError::InvalidFragmentCount
            );
        }

        if fragment.fragment_index
            >= fragment.fragment_count
        {
            return Err(
                ProtocolError::InvalidFragmentIndex
            );
        }

        if !self.frames.contains_key(
            &fragment.sequence
        ) {
            if self.frames.len()
                >= MAX_ASSEMBLY_FRAMES
            {
                if let Some(oldest) =
                    self.frames.keys().min().copied()
                {
                    self.frames.remove(
                        &oldest
                    );
                }
            }

            self.frames.insert(
                fragment.sequence,
                PartialFrame {
                    timestamp:
                    fragment.timestamp,
                    duration:
                    fragment.duration,
                    keyframe:
                    fragment.keyframe,
                    fragment_count:
                    fragment.fragment_count,
                    fragments:
                    vec![
                        None;
                        fragment
                            .fragment_count
                            as usize
                    ],
                    received: 0,
                },
            );
        }

        let frame =
            self.frames.get_mut(
                &fragment.sequence
            ).unwrap();

        if frame.fragment_count
            != fragment.fragment_count
            || frame.timestamp
            != fragment.timestamp
            || frame.duration
            != fragment.duration
            || frame.keyframe
            != fragment.keyframe
        {
            self.frames.remove(
                &fragment.sequence
            );

            return Ok(None);
        }

        let slot =
            &mut frame.fragments[
                fragment.fragment_index
                    as usize
                ];

        if slot.is_none() {
            *slot =
                Some(fragment.payload);

            frame.received += 1;
        }

        if frame.received
            != frame.fragment_count as usize
        {
            return Ok(None);
        }

        let frame =
            self.frames
                .remove(
                    &fragment.sequence
                )
                .unwrap();

        let total_len =
            frame.fragments
                .iter()
                .filter_map(
                    |fragment| {
                        fragment
                            .as_ref()
                            .map(Vec::len)
                    }
                )
                .sum();

        let mut payload =
            Vec::with_capacity(
                total_len
            );

        for fragment in
            frame.fragments
        {
            if let Some(fragment) =
                fragment
            {
                payload.extend_from_slice(
                    &fragment
                );
            }
        }

        Ok(Some(
            VideoPacket {
                sequence:
                fragment.sequence,
                timestamp:
                frame.timestamp,
                duration:
                frame.duration,
                keyframe:
                frame.keyframe,
                payload,
            }
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fragment_roundtrip() {
        let payload: Vec<u8> =
            (0..10000)
                .map(|value| {
                    (value % 251) as u8
                })
                .collect();

        let packet =
            VideoPacket::new(
                42,
                100000,
                166666,
                true,
                payload.clone(),
            )
                .unwrap();

        let fragments =
            packet
                .fragment(1200)
                .unwrap();

        assert!(
            fragments.len() > 1
        );

        let mut assembler =
            FrameAssembler::new();

        let mut result = None;

        for fragment in fragments
            .into_iter()
            .rev()
        {
            let data =
                fragment
                    .encode()
                    .unwrap();

            assert!(
                data.len() <= 1200
            );

            let decoded =
                VideoFragment::decode(
                    &data
                )
                    .unwrap();

            result =
                assembler
                    .push(decoded)
                    .unwrap()
                    .or(result);
        }

        let rebuilt =
            result.unwrap();

        assert_eq!(
            rebuilt.sequence,
            packet.sequence
        );

        assert_eq!(
            rebuilt.timestamp,
            packet.timestamp
        );

        assert_eq!(
            rebuilt.duration,
            packet.duration
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
}