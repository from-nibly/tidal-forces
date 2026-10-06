use anyhow::{Context, Result, ensure};
use fdk_aac::dec::{Decoder as FdkDecoder, Transport};
use std::io::Cursor;
use symphonia::core::{
    codecs::CODEC_TYPE_AAC,
    errors::Error,
    formats::{FormatOptions, FormatReader},
    io::{MediaSourceStream, MediaSourceStreamOptions},
    meta::MetadataOptions,
    probe::Hint,
};

// Keep the AAC decoder alive across DASH fragments: MDCT overlap, SBR and
// parametric-stereo state must not reset at each network segment boundary.
pub struct Decoder {
    format: Box<dyn FormatReader>,
    decoder: FdkDecoder,
    config: Vec<u8>,
    track_id: u32,
    pcm: Vec<f32>,
    position: usize,
    pub channels: u16,
    pub sample_rate: u32,
    pub failed: bool,
}

fn demux(data: Vec<u8>) -> Result<Box<dyn FormatReader>> {
    let input = MediaSourceStream::new(
        Box::new(Cursor::new(data)),
        MediaSourceStreamOptions::default(),
    );
    let mut hint = Hint::new();
    hint.with_extension("mp4");
    Ok(symphonia::default::get_probe()
        .format(
            &hint,
            input,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )?
        .format)
}

impl Decoder {
    pub fn new(data: Vec<u8>) -> Result<Self> {
        let format = demux(data)?;
        let track = format.default_track().context("AAC stream has no track")?;
        ensure!(
            track.codec_params.codec == CODEC_TYPE_AAC,
            "Expected AAC audio"
        );
        let config = track
            .codec_params
            .extra_data
            .as_ref()
            .context("AAC configuration missing")?
            .to_vec();
        let track_id = track.id;
        let mut decoder = FdkDecoder::new(Transport::Raw);
        decoder
            .config_raw(&config)
            .map_err(|e| anyhow::anyhow!("AAC configuration: {e}"))?;
        let mut source = Self {
            format,
            decoder,
            config,
            track_id,
            pcm: Vec::new(),
            position: 0,
            channels: 0,
            sample_rate: 0,
            failed: false,
        };
        ensure!(source.read_frame()?, "AAC fragment contains no samples");
        Ok(source)
    }

    pub fn advance(&mut self, data: Vec<u8>) -> Result<()> {
        let format = demux(data)?;
        let track = format
            .default_track()
            .context("AAC fragment has no track")?;
        ensure!(
            track.codec_params.codec == CODEC_TYPE_AAC
                && track.codec_params.extra_data.as_deref() == Some(self.config.as_slice()),
            "AAC format changed between fragments"
        );
        self.track_id = track.id;
        self.format = format;
        self.pcm.clear();
        self.position = 0;
        self.failed = false;
        ensure!(self.read_frame()?, "AAC fragment contains no samples");
        Ok(())
    }

    fn read_frame(&mut self) -> Result<bool> {
        loop {
            let packet = match self.format.next_packet() {
                Ok(packet) => packet,
                Err(Error::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                    return Ok(false);
                }
                Err(e) => return Err(e.into()),
            };
            if packet.track_id() != self.track_id {
                continue;
            }
            let consumed = self
                .decoder
                .fill(&packet.data)
                .map_err(|e| anyhow::anyhow!("AAC input: {e}"))?;
            ensure!(
                consumed == packet.data.len(),
                "AAC decoder did not consume the complete packet"
            );
            let mut pcm = [0i16; 2048 * 8];
            self.decoder
                .decode_frame(&mut pcm)
                .map_err(|e| anyhow::anyhow!("AAC decode: {e}"))?;
            let info = self.decoder.stream_info();
            ensure!(
                (1..=8).contains(&info.numChannels) && info.sampleRate > 0 && info.frameSize > 0,
                "Invalid decoded AAC format"
            );
            let count = info.numChannels as usize * info.frameSize as usize;
            ensure!(count <= pcm.len(), "AAC frame exceeds output buffer");
            let channels = info.numChannels as u16;
            let rate = info.sampleRate as u32;
            ensure!(
                self.channels == 0 || (self.channels == channels && self.sample_rate == rate),
                "Decoded AAC format changed"
            );
            self.channels = channels;
            self.sample_rate = rate;
            self.pcm.clear();
            self.pcm
                .extend(pcm[..count].iter().map(|&v| v as f32 / 32768.));
            self.position = 0;
            return Ok(true);
        }
    }
}

impl Iterator for Decoder {
    type Item = f32;
    fn next(&mut self) -> Option<Self::Item> {
        if self.failed {
            return None;
        }
        if self.position == self.pcm.len() {
            match self.read_frame() {
                Ok(true) => {}
                Ok(false) => return None,
                Err(_) => {
                    self.failed = true;
                    return None;
                }
            }
        }
        let sample = self.pcm[self.position];
        self.position += 1;
        Some(sample)
    }
}
