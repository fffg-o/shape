use std::{collections::HashMap, fmt,
          time::{Duration, Instant},};

pub const MAGIC: [u8; 4] = *b"RDP1";
pub const VERSION: u8 = 1;
pub const VIDEO_PACKET_TYPE: u8 = 1;
pub const VIDEO_FRAGMENT_PACKET_TYPE: u8 = 2;
pub const HEADER_SIZE: usize = 36;
pub const FRAGMENT_HEADER_SIZE: usize = 40;
pub const MAX_PAYLOAD_SIZE: usize = 64 * 1024 * 1024;
pub const MAX_FRAME_FRAGMENTS: usize = u16::MAX as usize;
pub const MAX_ASSEMBLY_FRAMES: usize = 8;
const MAX_ASSEMBLY_BYTES: usize = MAX_PAYLOAD_SIZE;
const ASSEMBLY_TIMEOUT: Duration = Duration::from_millis(250);

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
    created_at: Instant,
    payload_len: usize,
}

pub struct FrameAssembler {
    frames: HashMap<u64, PartialFrame>,
    buffered_bytes: usize,
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
            buffered_bytes: 0,
        }
    }

    fn remove_frame(&mut self, sequence: u64) -> Option<PartialFrame> {
        let frame = self.frames.remove(&sequence)?;
        self.buffered_bytes = self
            .buffered_bytes
            .saturating_sub(frame.payload_len);
        Some(frame)
    }

    fn remove_expired_frames(&mut self) {
        let now = Instant::now();

        let expired: Vec<u64> = self
            .frames
            .iter()
            .filter(|(_, frame)| {
                now.duration_since(frame.created_at) >= ASSEMBLY_TIMEOUT
            })
            .map(|(sequence, _)| *sequence)
            .collect();

        for sequence in expired {
            self.remove_frame(sequence);
        }
    }

    pub fn push(
        &mut self,
        fragment: VideoFragment,
    ) -> Result<Option<VideoPacket>, ProtocolError> {
        if fragment.fragment_count == 0 {
            return Err(ProtocolError::InvalidFragmentCount);
        }

        if fragment.fragment_index >= fragment.fragment_count {
            return Err(ProtocolError::InvalidFragmentIndex);
        }

        let sequence = fragment.sequence;
        let timestamp = fragment.timestamp;
        let duration = fragment.duration;
        let keyframe = fragment.keyframe;
        let fragment_count = fragment.fragment_count;
        let fragment_index = fragment.fragment_index as usize;

        self.remove_expired_frames();

        if fragment.payload.len() > MAX_PAYLOAD_SIZE {
            return Err(ProtocolError::PayloadTooLarge);
        }

        if !self.frames.contains_key(&sequence) {
            if self.frames.len() >= MAX_ASSEMBLY_FRAMES {
                let oldest = self
                    .frames
                    .iter()
                    .min_by_key(|(_, frame)| frame.created_at)
                    .map(|(sequence, _)| *sequence);

                if let Some(oldest) = oldest {
                    self.remove_frame(oldest);
                }
            }

            self.frames.insert(
                sequence,
                PartialFrame {
                    timestamp,
                    duration,
                    keyframe,
                    fragment_count,
                    fragments: vec![None; fragment_count as usize],
                    received: 0,
                    created_at: Instant::now(),
                    payload_len: 0,
                },
            );
        }

        let metadata_mismatch = {
            let frame = self.frames.get(&sequence).unwrap();

            frame.fragment_count != fragment_count
                || frame.timestamp != timestamp
                || frame.duration != duration
                || frame.keyframe != keyframe
        };

        if metadata_mismatch {
            self.remove_frame(sequence);
            return Ok(None);
        }

        let already_received = self.frames
            .get(&sequence)
            .unwrap()
            .fragments[fragment_index]
            .is_some();

        if !already_received {
            let added_len = fragment.payload.len();

            let current_len = self
                .frames
                .get(&sequence)
                .unwrap()
                .payload_len;

            if current_len.saturating_add(added_len) > MAX_PAYLOAD_SIZE {
                self.remove_frame(sequence);
                return Err(ProtocolError::PayloadTooLarge);
            }

            while self.buffered_bytes.saturating_add(added_len)
                > MAX_ASSEMBLY_BYTES
            {
                let oldest = self
                    .frames
                    .iter()
                    .filter(|(candidate, _)| **candidate != sequence)
                    .min_by_key(|(_, frame)| frame.created_at)
                    .map(|(sequence, _)| *sequence);

                match oldest {
                    Some(oldest) => {
                        self.remove_frame(oldest);
                    }
                    None => {
                        self.remove_frame(sequence);
                        return Err(ProtocolError::PayloadTooLarge);
                    }
                }
            }

            let frame = self.frames.get_mut(&sequence).unwrap();

            frame.fragments[fragment_index] = Some(fragment.payload);
            frame.received += 1;
            frame.payload_len += added_len;

            self.buffered_bytes += added_len;
        }

        let complete = {
            let frame = self.frames.get(&sequence).unwrap();
            frame.received == frame.fragment_count as usize
        };

        if !complete {
            return Ok(None);
        }

        let frame = self.remove_frame(sequence).unwrap();
        let mut payload = Vec::with_capacity(frame.payload_len);

        for fragment in frame.fragments {
            if let Some(fragment) = fragment {
                payload.extend_from_slice(&fragment);
            }
        }

        Ok(Some(VideoPacket {
            sequence,
            timestamp: frame.timestamp,
            duration: frame.duration,
            keyframe: frame.keyframe,
            payload,
        }))
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

#[test]
fn incomplete_frames_expire() {
    use std::time::{Duration, Instant};

    let packet = VideoPacket::new(
        7,
        0,
        1,
        false,
        vec![1; 100],
    )
        .unwrap();

    let fragments = packet.fragment(60).unwrap();
    let first = fragments[0].clone();
    let second = fragments[1].clone();

    let mut assembler = FrameAssembler::new();

    assert!(assembler.push(first).unwrap().is_none());
    assert_eq!(assembler.frames.get(&7).unwrap().received, 1);

    assembler.frames.get_mut(&7).unwrap().created_at =
        Instant::now() - ASSEMBLY_TIMEOUT - Duration::from_millis(1);

    assert!(assembler.push(second.clone()).unwrap().is_none());

    let frame = assembler.frames.get(&7).unwrap();

    assert_eq!(frame.received, 1);
    assert_eq!(frame.payload_len, second.payload.len());
    assert_eq!(assembler.buffered_bytes, second.payload.len());
}