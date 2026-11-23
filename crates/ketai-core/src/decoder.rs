//! Decodificacion de audio basada en symphonia con codecs extra.
//!
//! rodio usa siempre el registro de codecs por defecto de symphonia, que no
//! soporta Opus ni HE-AAC. Por eso este modulo arma su propio registro y
//! expone una fuente compatible con rodio.

use std::{
  fs::File,
  num::NonZero,
  path::Path,
  sync::OnceLock,
  time::Duration,
};

use rodio::{Source, source::SeekError};
use symphonia::core::{
  audio::SampleBuffer,
  codecs::{CodecRegistry, Decoder, DecoderOptions},
  errors::Error as SymphoniaError,
  formats::{FormatOptions, FormatReader, SeekMode, SeekTo},
  io::MediaSourceStream,
  meta::MetadataOptions,
  probe::Hint,
  units::Time,
};
use symphonia_adapter_fdk_aac::AacDecoder;
use symphonia_adapter_libopus::OpusDecoder;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DecodeError {
  #[error("I/O error: {0}")]
  Io(#[from] std::io::Error),

  #[error("Could not decode audio: {0}")]
  Symphonia(#[from] SymphoniaError),

  #[error("No playable audio track found")]
  NoTrack,

  #[error("Unsupported stream parameters")]
  InvalidParams,
}

/// Registro de codecs de ketai.
///
/// Primero se registran los codecs por defecto de symphonia y despues los
/// adaptadores. Al registrar un codec ya existente, el ultimo gana: asi FDK
/// reemplaza al AAC nativo (que solo soporta AAC-LC) y libopus agrega Opus.
pub fn codecs() -> &'static CodecRegistry {
  static REGISTRY: OnceLock<CodecRegistry> = OnceLock::new();
  REGISTRY.get_or_init(|| {
    let mut registry = CodecRegistry::new();
    symphonia::default::register_enabled_codecs(&mut registry);
    registry.register_all::<AacDecoder>();
    registry.register_all::<OpusDecoder>();
    registry
  })
}

/// Fuente de audio decodificada, lista para entregar a un `rodio::Player`.
pub struct AudioSource {
  format: Box<dyn FormatReader>,
  decoder: Box<dyn Decoder>,
  track_id: u32,
  channels: NonZero<u16>,
  sample_rate: NonZero<u32>,
  total_duration: Option<Duration>,
  // Muestras intercaladas del paquete actual y la posicion de lectura.
  buffer: Vec<f32>,
  cursor: usize,
}

impl AudioSource {
  /// Abre un archivo y detecta el formato por su contenido.
  pub fn open(path: &Path) -> Result<Self, DecodeError> {
    let file = File::open(path)?;
    let stream = MediaSourceStream::new(Box::new(file), Default::default());

    // La extension es solo una pista; el probe decide por el contenido.
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
      hint.with_extension(ext);
    }

    let probed = symphonia::default::get_probe().format(
      &hint,
      stream,
      &FormatOptions { enable_gapless: true, ..Default::default() },
      &MetadataOptions::default(),
    )?;
    let format = probed.format;

    let track = format.default_track().ok_or(DecodeError::NoTrack)?;
    let params = &track.codec_params;
    let decoder = codecs().make(params, &DecoderOptions::default())?;

    let total_duration = params
      .time_base
      .zip(params.n_frames)
      .map(|(base, frames)| base.calc_time(frames).into());
    let track_id = track.id;

    let mut source = Self {
      track_id,
      format,
      decoder,
      // Valores provisorios: se reemplazan con los del primer paquete.
      channels: NonZero::<u16>::MIN,
      sample_rate: NonZero::<u32>::MIN,
      total_duration,
      buffer: Vec::new(),
      cursor: 0,
    };

    // Algunos contenedores (MP4 con AAC) no declaran canales ni frecuencia,
    // o declaran la del nucleo AAC y no la real con SBR. Por eso se toman
    // del primer paquete decodificado, que es la fuente confiable.
    if !source.refill() {
      return Err(DecodeError::NoTrack);
    }
    let spec = source.decoder.last_decoded().spec().to_owned();
    source.channels = u16::try_from(spec.channels.count())
      .ok()
      .and_then(NonZero::new)
      .ok_or(DecodeError::InvalidParams)?;
    source.sample_rate = NonZero::new(spec.rate).ok_or(DecodeError::InvalidParams)?;

    Ok(source)
  }

  /// Decodifica el siguiente paquete del track. Devuelve false al terminar.
  fn refill(&mut self) -> bool {
    loop {
      let packet = match self.format.next_packet() {
        Ok(packet) => packet,
        // Fin del archivo o error irrecuperable: se termina la fuente.
        Err(_) => return false,
      };
      if packet.track_id() != self.track_id {
        continue;
      }
      match self.decoder.decode(&packet) {
        Ok(decoded) => {
          let mut samples = SampleBuffer::<f32>::new(decoded.capacity() as u64, *decoded.spec());
          samples.copy_interleaved_ref(decoded);
          self.buffer.clear();
          self.buffer.extend_from_slice(samples.samples());
          self.cursor = 0;
          if !self.buffer.is_empty() {
            return true;
          }
        }
        // Un paquete corrupto se salta en lugar de cortar la reproduccion.
        Err(SymphoniaError::DecodeError(_)) => continue,
        Err(_) => return false,
      }
    }
  }
}

impl Iterator for AudioSource {
  type Item = f32;

  fn next(&mut self) -> Option<f32> {
    if self.cursor >= self.buffer.len() && !self.refill() {
      return None;
    }
    let sample = self.buffer[self.cursor];
    self.cursor += 1;
    Some(sample)
  }
}

impl Source for AudioSource {
  fn current_span_len(&self) -> Option<usize> {
    // Cada paquete es un tramo. Sin tramos, la cola de rodio nunca vuelve a
    // leer la frecuencia: una pista de 44,1 kHz despues de una de 48 kHz
    // sonaba un 8,8 % mas rapido.
    Some(self.buffer.len() - self.cursor)
  }

  fn channels(&self) -> NonZero<u16> {
    self.channels
  }

  fn sample_rate(&self) -> NonZero<u32> {
    self.sample_rate
  }

  fn total_duration(&self) -> Option<Duration> {
    self.total_duration
  }

  fn try_seek(&mut self, pos: Duration) -> Result<(), SeekError> {
    let time = Time::new(pos.as_secs(), pos.subsec_nanos() as f64 / 1e9);
    self
      .format
      .seek(SeekMode::Accurate, SeekTo::Time { time, track_id: Some(self.track_id) })
      .map_err(|e| SeekError::Other(std::sync::Arc::new(e)))?;
    // El estado interno del decoder queda invalido tras un seek.
    self.decoder.reset();
    self.buffer.clear();
    self.cursor = 0;
    Ok(())
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::path::PathBuf;

  fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name)
  }

  /// Decodifica todo el archivo y devuelve la duracion real en segundos.
  fn decoded_seconds(name: &str) -> f64 {
    let source = AudioSource::open(&fixture(name)).expect("open fixture");
    let channels = source.channels().get() as f64;
    let rate = source.sample_rate().get() as f64;
    let samples = source.count() as f64;
    samples / channels / rate
  }

  // Los fixtures son tonos de 440 Hz de aproximadamente 2 segundos.
  fn assert_about_two_seconds(name: &str) {
    let secs = decoded_seconds(name);
    assert!((1.8..2.3).contains(&secs), "{name}: decoded {secs}s");
  }

  #[test]
  fn decodes_opus_in_ogg() {
    assert_about_two_seconds("opus.ogg");
  }

  #[test]
  fn decodes_he_aac_in_m4a() {
    assert_about_two_seconds("he_aac.m4a");
  }

  #[test]
  fn still_decodes_aac_lc_in_m4a() {
    assert_about_two_seconds("aac_lc.m4a");
  }

  #[test]
  fn still_decodes_vorbis_sample() {
    assert_about_two_seconds("vorbis.ogg");
  }

  #[test]
  fn seek_moves_playback_position() {
    let mut source = AudioSource::open(&fixture("opus.ogg")).unwrap();
    source.try_seek(Duration::from_secs(1)).unwrap();
    let rate = source.sample_rate().get() as f64 * source.channels().get() as f64;
    let remaining = source.count() as f64 / rate;
    assert!((0.7..1.3).contains(&remaining), "remaining {remaining}s");
  }

  #[test]
  fn a_track_with_another_rate_after_the_first_keeps_its_speed() {
    // Igual que el player: una cola que entra una sola vez al mezclador, que
    // convierte a la frecuencia del dispositivo. Opus a 48 kHz y despues
    // vorbis a 44,1 kHz: la segunda tiene que durar lo mismo, no sonar mas
    // rapido.
    let (input, output) = rodio::queue::queue(false);
    input.append(AudioSource::open(&fixture("opus.ogg")).unwrap());
    input.append(AudioSource::open(&fixture("vorbis.ogg")).unwrap());
    let device = rodio::source::UniformSourceIterator::new(output, NonZero::new(2).unwrap(), NonZero::new(48_000).unwrap());
    let played = device.count() as f64 / 2.0 / 48_000.0;
    let expected = decoded_seconds("opus.ogg") + decoded_seconds("vorbis.ogg");
    assert!((played - expected).abs() < 0.05, "played {played:.3}s, expected {expected:.3}s");
  }
}
