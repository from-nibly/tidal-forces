//! Generate our own AAC test tones; ffmpeg can remux these into DASH without encoding.
use anyhow::{Context, Result, ensure};
use fdk_aac::enc::{AudioObjectType, BitRate, ChannelMode, Encoder, EncoderParams, Transport};
use std::{fs, io::Write};

fn main() -> Result<()> {
    let directory = std::env::args()
        .nth(1)
        .context("Supply an output directory")?;
    fs::create_dir_all(&directory)?;
    for (name, profile) in [
        ("aac-lc", AudioObjectType::Mpeg4LowComplexity),
        ("he-aac", AudioObjectType::Mpeg4HeAac),
        ("he-aac-v2", AudioObjectType::Mpeg4HeAacV2),
    ] {
        let encoder = Encoder::new(EncoderParams {
            bit_rate: BitRate::Cbr(64000),
            sample_rate: 44100,
            transport: Transport::Adts,
            channels: ChannelMode::Stereo,
            audio_object_type: profile,
        })
        .map_err(|e| anyhow::anyhow!("{e:?}"))?;
        let mut pcm = Vec::new();
        for frame in 0..44100 * 5 {
            let sample =
                ((frame as f64 * std::f64::consts::TAU * 440.0 / 44100.0).sin() * 10000.) as i16;
            pcm.extend_from_slice(&[sample, sample]);
        }
        let mut output = fs::File::create(format!("{directory}/{name}.aac"))?;
        let mut offset = 0;
        let mut buffer = vec![0; 8192];
        while offset < pcm.len() {
            let encoded = encoder
                .encode(&pcm[offset..], &mut buffer)
                .map_err(|e| anyhow::anyhow!("{e:?}"))?;
            ensure!(encoded.input_consumed > 0, "Encoder made no progress");
            offset += encoded.input_consumed;
            output.write_all(&buffer[..encoded.output_size])?;
        }
    }
    Ok(())
}
