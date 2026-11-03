//! Media conversions done in memory (ADR 0003: nothing is written to disk).
//!
//! WhatsApp voice notes are Ogg Opus. Not every system WebView plays
//! them (WKWebView on macOS, WebKitGTK without plugins), so the app can
//! ask for a WAV version decoded with libopus.

use std::io::Cursor;

/// Largest WAV we produce (about 45 minutes of 48 kHz stereo).
const MAX_WAV_BYTES: usize = 512 * 1024 * 1024;
const SAMPLE_RATE: u32 = 48_000;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("not an Ogg Opus stream: {0}")]
    Format(&'static str),
    #[error("Ogg container: {0}")]
    Ogg(#[from] ogg::OggReadError),
    #[error("Opus decoding failed: {0}")]
    Opus(#[from] opus::Error),
    #[error("audio is too long to convert")]
    TooLong,
}

/// Decodes an Ogg Opus file to 16-bit PCM WAV at 48 kHz.
pub fn opus_to_wav(data: &[u8]) -> Result<Vec<u8>, Error> {
    let mut reader = ogg::PacketReader::new(Cursor::new(data));

    let head = reader.read_packet()?.ok_or(Error::Format("empty stream"))?;
    let head = &head.data;
    if head.len() < 19 || &head[..8] != b"OpusHead" {
        return Err(Error::Format("missing OpusHead"));
    }
    let channels = match head[9] {
        1 => opus::Channels::Mono,
        2 => opus::Channels::Stereo,
        _ => return Err(Error::Format("only mono and stereo are supported")),
    };
    let channel_count = head[9] as usize;
    let pre_skip = u16::from_le_bytes([head[10], head[11]]) as usize;

    // The second packet holds tags (OpusTags); skip it.
    reader
        .read_packet()?
        .ok_or(Error::Format("missing OpusTags"))?;

    let mut decoder = opus::Decoder::new(SAMPLE_RATE, channels)?;
    // 120 ms is the largest Opus frame.
    let mut frame = vec![0i16; 5760 * channel_count];
    let mut pcm: Vec<i16> = Vec::new();
    let mut last_granule = None;
    while let Some(packet) = reader.read_packet()? {
        let samples = decoder.decode(&packet.data, &mut frame, false)?;
        pcm.extend_from_slice(&frame[..samples * channel_count]);
        if pcm.len() * 2 > MAX_WAV_BYTES {
            return Err(Error::TooLong);
        }
        if packet.last_in_page() {
            last_granule = Some(packet.absgp_page() as usize);
        }
    }

    // Drop the encoder delay, and padding beyond the final granule position.
    let start = (pre_skip * channel_count).min(pcm.len());
    let end = last_granule
        .map(|g| (g * channel_count).min(pcm.len()))
        .unwrap_or(pcm.len())
        .max(start);
    Ok(wav(&pcm[start..end], channel_count as u16))
}

fn wav(samples: &[i16], channels: u16) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let block_align = channels * 2;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    out.extend_from_slice(&(SAMPLE_RATE * u32::from(block_align)).to_le_bytes());
    out.extend_from_slice(&block_align.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Encodes `seconds` of a synthetic tone as Ogg Opus, like a voice
    /// note. Returns the file and the encoder delay (pre-skip).
    fn synthetic_voice_note(seconds: usize) -> (Vec<u8>, usize) {
        let mut encoder =
            opus::Encoder::new(SAMPLE_RATE, opus::Channels::Mono, opus::Application::Voip).unwrap();
        let pre_skip = encoder.get_lookahead().unwrap() as u16;
        let mut out = Vec::new();
        let mut writer = ogg::PacketWriter::new(&mut out);
        let serial = 7;

        let mut head = b"OpusHead".to_vec();
        head.push(1); // version
        head.push(1); // channels
        head.extend_from_slice(&pre_skip.to_le_bytes());
        head.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
        head.extend_from_slice(&0u16.to_le_bytes()); // gain
        head.push(0); // mapping family
        writer
            .write_packet(head, serial, ogg::PacketWriteEndInfo::EndPage, 0)
            .unwrap();
        let mut tags = b"OpusTags".to_vec();
        tags.extend_from_slice(&4u32.to_le_bytes());
        tags.extend_from_slice(b"test");
        tags.extend_from_slice(&0u32.to_le_bytes());
        writer
            .write_packet(tags, serial, ogg::PacketWriteEndInfo::EndPage, 0)
            .unwrap();

        let frame = 960; // 20 ms
        let total = SAMPLE_RATE as usize * seconds;
        let frames = total / frame;
        let mut buf = vec![0u8; 4000];
        for f in 0..frames {
            let pcm: Vec<i16> = (0..frame)
                .map(|i| {
                    let t = (f * frame + i) as f32 / SAMPLE_RATE as f32;
                    ((t * 440.0 * std::f32::consts::TAU).sin() * 8000.0) as i16
                })
                .collect();
            let n = encoder.encode(&pcm, &mut buf).unwrap();
            let granule = (pre_skip as usize + (f + 1) * frame) as u64;
            let end = if f + 1 == frames {
                ogg::PacketWriteEndInfo::EndStream
            } else {
                ogg::PacketWriteEndInfo::NormalPacket
            };
            writer
                .write_packet(buf[..n].to_vec(), serial, end, granule)
                .unwrap();
        }
        drop(writer);
        (out, pre_skip as usize)
    }

    #[test]
    fn decodes_ogg_opus_to_wav_with_exact_length() {
        let (note, pre_skip) = synthetic_voice_note(2);
        let wav = opus_to_wav(&note).unwrap();
        assert_eq!(&wav[..4], b"RIFF");
        assert_eq!(&wav[8..16], b"WAVEfmt ");
        let data_len = u32::from_le_bytes(wav[40..44].try_into().unwrap()) as usize;
        // Two seconds of 48 kHz mono, 16 bit, minus the encoder delay (this
        // test encoder does not pad the end the way real encoders do).
        assert_eq!(data_len, (2 * 48_000 - pre_skip) * 2);
        // The tone is audible, not silence.
        let peak = wav[44..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|b| i16::from_le_bytes([b[0], b[1]]).unsigned_abs())
            .max()
            .unwrap();
        assert!(peak > 2000, "peak {peak}");
    }

    #[test]
    fn rejects_other_data() {
        assert!(opus_to_wav(b"not audio").is_err());
        assert!(opus_to_wav(&[]).is_err());
    }
}
