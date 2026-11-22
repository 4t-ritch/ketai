//! Visualizador: captura de muestras y analisis de espectro.
//!
//! - `Tap` envuelve la fuente de audio y copia lo que suena a un `Ring`.
//!   Corre en el thread de audio, asi que nunca espera: si el `Ring` esta
//!   ocupado, ese bloque de muestras se descarta.
//! - `Analyzer` toma las ultimas muestras del `Ring` y calcula `bins` bandas
//!   de frecuencia normalizadas entre 0.0 y 1.0. Usa una FFT radix-2 propia,
//!   sin dependencias.

use std::{
  f32::consts::PI,
  num::NonZero,
  sync::{Arc, Mutex},
  time::Duration,
};

use rodio::{Source, source::SeekError};

/// Muestras (mono) que guarda el `Ring`: alcanza para la FFT.
const RING_LEN: usize = 4096;
/// Muestras que junta el `Tap` antes de copiarlas al `Ring`.
const TAP_CHUNK: usize = 512;

/// Ultimas muestras que sonaron, ya pasadas a mono.
pub struct Ring {
  samples: Vec<f32>,
  /// Proxima posicion a escribir (buffer circular).
  next: usize,
  sample_rate: u32,
}

impl Default for Ring {
  fn default() -> Self {
    Self { samples: vec![0.0; RING_LEN], next: 0, sample_rate: 44_100 }
  }
}

impl Ring {
  fn push(&mut self, mono: &[f32], sample_rate: u32) {
    self.sample_rate = sample_rate;
    for &s in mono {
      self.samples[self.next] = s;
      self.next = (self.next + 1) % RING_LEN;
    }
  }

  /// Copia las ultimas `out.len()` muestras, de la mas vieja a la mas nueva,
  /// y devuelve la frecuencia de muestreo.
  pub fn latest(&self, out: &mut [f32]) -> u32 {
    let n = out.len().min(RING_LEN);
    let start = (self.next + RING_LEN - n) % RING_LEN;
    for (i, o) in out.iter_mut().take(n).enumerate() {
      *o = self.samples[(start + i) % RING_LEN];
    }
    self.sample_rate
  }

  /// Borra lo guardado (al cambiar de pista o detenerse).
  pub fn clear(&mut self) {
    self.samples.fill(0.0);
  }
}

pub type SharedRing = Arc<Mutex<Ring>>;

/// Fuente que deja pasar el audio sin cambios y copia una version mono al
/// `Ring`.
pub struct Tap<S> {
  inner: S,
  ring: SharedRing,
  /// Muestras intercaladas pendientes de pasar a mono.
  frame: Vec<f32>,
  mono: Vec<f32>,
}

impl<S: Source<Item = f32>> Tap<S> {
  pub fn new(inner: S, ring: SharedRing) -> Self {
    Self { inner, ring, frame: Vec::with_capacity(8), mono: Vec::with_capacity(TAP_CHUNK) }
  }

  fn flush(&mut self) {
    // Nunca espera en el thread de audio: si el analizador tiene el lock,
    // este bloque se pierde (el visualizador tolera huecos).
    if let Ok(mut ring) = self.ring.try_lock() {
      ring.push(&self.mono, self.inner.sample_rate().get());
    }
    self.mono.clear();
  }
}

impl<S: Source<Item = f32>> Iterator for Tap<S> {
  type Item = f32;

  fn next(&mut self) -> Option<f32> {
    let sample = self.inner.next()?;
    self.frame.push(sample);
    if self.frame.len() >= self.inner.channels().get() as usize {
      let mono = self.frame.iter().sum::<f32>() / self.frame.len() as f32;
      self.frame.clear();
      self.mono.push(mono);
      if self.mono.len() >= TAP_CHUNK {
        self.flush();
      }
    }
    Some(sample)
  }
}

impl<S: Source<Item = f32>> Source for Tap<S> {
  fn current_span_len(&self) -> Option<usize> {
    self.inner.current_span_len()
  }

  fn channels(&self) -> NonZero<u16> {
    self.inner.channels()
  }

  fn sample_rate(&self) -> NonZero<u32> {
    self.inner.sample_rate()
  }

  fn total_duration(&self) -> Option<Duration> {
    self.inner.total_duration()
  }

  fn try_seek(&mut self, pos: Duration) -> Result<(), SeekError> {
    self.frame.clear();
    self.mono.clear();
    self.inner.try_seek(pos)
  }
}

/// Tamano de la FFT (potencia de 2).
pub const FFT_SIZE: usize = 2048;
/// Rango de frecuencias que se muestra.
const MIN_HZ: f32 = 30.0;
const MAX_HZ: f32 = 16_000.0;
/// Rango dinamico en dB: -60 dB o menos es 0.0; 0 dB es 1.0.
const FLOOR_DB: f32 = -60.0;

/// Calcula bandas de espectro a partir de muestras mono.
pub struct Analyzer {
  bins: usize,
  window: Vec<f32>,
  re: Vec<f32>,
  im: Vec<f32>,
  samples: Vec<f32>,
}

impl Analyzer {
  pub fn new(bins: usize) -> Self {
    // Ventana de Hann: reduce el "derrame" entre frecuencias vecinas.
    let window = (0..FFT_SIZE).map(|i| 0.5 - 0.5 * (2.0 * PI * i as f32 / FFT_SIZE as f32).cos()).collect();
    Self { bins, window, re: vec![0.0; FFT_SIZE], im: vec![0.0; FFT_SIZE], samples: vec![0.0; FFT_SIZE] }
  }

  /// Toma las ultimas muestras del `Ring` y llena `out` (`bins` valores).
  pub fn frame_from(&mut self, ring: &SharedRing, out: &mut [f32]) {
    let rate = {
      let ring = ring.lock().unwrap_or_else(|e| e.into_inner());
      ring.latest(&mut self.samples)
    };
    let samples = std::mem::take(&mut self.samples);
    self.frame(&samples, rate, out);
    self.samples = samples;
  }

  /// Espectro de `samples` (`FFT_SIZE` muestras mono) en `out`.
  pub fn frame(&mut self, samples: &[f32], sample_rate: u32, out: &mut [f32]) {
    for i in 0..FFT_SIZE {
      self.re[i] = samples.get(i).copied().unwrap_or(0.0) * self.window[i];
      self.im[i] = 0.0;
    }
    fft(&mut self.re, &mut self.im);

    let hz_per_bin = sample_rate as f32 / FFT_SIZE as f32;
    let nyquist = FFT_SIZE / 2;
    let max_hz = MAX_HZ.min(sample_rate as f32 / 2.0);
    // La suma de la ventana de Hann es N/2: un seno de amplitud 1 da una
    // magnitud de N/4 en su bin. Se normaliza para que eso sea 0 dB.
    let reference = FFT_SIZE as f32 / 4.0;
    for (b, o) in out.iter_mut().enumerate().take(self.bins) {
      // Bandas con escala logaritmica: cada una cubre el mismo intervalo
      // musical, como escucha el oido.
      let lo_hz = MIN_HZ * (max_hz / MIN_HZ).powf(b as f32 / self.bins as f32);
      let hi_hz = MIN_HZ * (max_hz / MIN_HZ).powf((b + 1) as f32 / self.bins as f32);
      let (lo_f, hi_f) = (lo_hz / hz_per_bin, hi_hz / hz_per_bin);
      // En graves una banda es mas angosta que un bin de la FFT: se usa el
      // bin que contiene su centro. Si no, todos los bins que cubre.
      let (lo, hi) = if hi_f - lo_f < 1.0 {
        let k = ((lo_f + hi_f) / 2.0).round() as usize;
        (k, k + 1)
      } else {
        (lo_f.ceil() as usize, hi_f.floor() as usize + 1)
      };
      let lo = lo.clamp(1, nyquist - 1);
      let hi = hi.clamp(lo + 1, nyquist);
      let peak = (lo..hi).map(|k| (self.re[k] * self.re[k] + self.im[k] * self.im[k]).sqrt()).fold(0.0f32, f32::max);
      let db = 20.0 * (peak / reference).max(1e-9).log10();
      *o = ((db - FLOOR_DB) / -FLOOR_DB).clamp(0.0, 1.0);
    }
  }
}

/// FFT radix-2 en el lugar (Cooley-Tukey). `re.len()` debe ser potencia de 2.
fn fft(re: &mut [f32], im: &mut [f32]) {
  let n = re.len();
  // Reordenar por inversion de bits.
  let mut j = 0;
  for i in 1..n {
    let mut bit = n >> 1;
    while j & bit != 0 {
      j ^= bit;
      bit >>= 1;
    }
    j |= bit;
    if i < j {
      re.swap(i, j);
      im.swap(i, j);
    }
  }
  let mut len = 2;
  while len <= n {
    let angle = -2.0 * PI / len as f32;
    let (w_re, w_im) = (angle.cos(), angle.sin());
    for start in (0..n).step_by(len) {
      let (mut cur_re, mut cur_im) = (1.0f32, 0.0f32);
      for k in 0..len / 2 {
        let a = start + k;
        let b = a + len / 2;
        let t_re = re[b] * cur_re - im[b] * cur_im;
        let t_im = re[b] * cur_im + im[b] * cur_re;
        re[b] = re[a] - t_re;
        im[b] = im[a] - t_im;
        re[a] += t_re;
        im[a] += t_im;
        let next_re = cur_re * w_re - cur_im * w_im;
        cur_im = cur_re * w_im + cur_im * w_re;
        cur_re = next_re;
      }
    }
    len <<= 1;
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  const RATE: u32 = 44_100;

  fn sine(hz: f32, amp: f32) -> Vec<f32> {
    (0..FFT_SIZE).map(|i| amp * (2.0 * PI * hz * i as f32 / RATE as f32).sin()).collect()
  }

  fn analyze(samples: &[f32]) -> Vec<f32> {
    let mut a = Analyzer::new(64);
    let mut out = vec![0.0; 64];
    a.frame(samples, RATE, &mut out);
    out
  }

  fn band_of(hz: f32) -> usize {
    ((hz / MIN_HZ).ln() / (MAX_HZ / MIN_HZ).ln() * 64.0) as usize
  }

  #[test]
  fn silence_is_all_zero() {
    assert!(analyze(&[0.0; FFT_SIZE]).iter().all(|&v| v == 0.0));
  }

  #[test]
  fn a_full_scale_sine_peaks_near_one_in_its_band() {
    for hz in [100.0, 1000.0, 5000.0] {
      let out = analyze(&sine(hz, 1.0));
      let (peak, value) = out.iter().enumerate().fold((0, 0.0f32), |m, (i, &v)| if v > m.1 { (i, v) } else { m });
      assert!(peak.abs_diff(band_of(hz)) <= 1, "{hz} Hz peaked in band {peak}, expected {}", band_of(hz));
      assert!(value > 0.9, "{hz} Hz peak {value}");
      assert!(out.iter().all(|v| (0.0..=1.0).contains(v)));
    }
  }

  #[test]
  fn quieter_signals_give_lower_values() {
    let loud = analyze(&sine(1000.0, 1.0))[band_of(1000.0)];
    let quiet = analyze(&sine(1000.0, 0.01))[band_of(1000.0)]; // -40 dB
    assert!((loud - quiet - 40.0 / 60.0).abs() < 0.05, "loud {loud} quiet {quiet}");
  }

  #[test]
  fn ring_keeps_the_latest_samples_in_order() {
    let mut r = Ring::default();
    let data: Vec<f32> = (0..RING_LEN + 10).map(|i| i as f32).collect();
    r.push(&data, 48_000);
    let mut out = vec![0.0; 4];
    assert_eq!(r.latest(&mut out), 48_000);
    let last = (RING_LEN + 9) as f32;
    assert_eq!(out, vec![last - 3.0, last - 2.0, last - 1.0, last]);
  }

  #[test]
  fn tap_passes_audio_through_and_fills_the_ring_in_mono() {
    let ring = SharedRing::default();
    // Estereo: izquierda 1.0, derecha 0.0 -> mono 0.5.
    let samples: Vec<f32> = (0..TAP_CHUNK * 4).map(|i| if i % 2 == 0 { 1.0 } else { 0.0 }).collect();
    let src = rodio::buffer::SamplesBuffer::new(NonZero::new(2).unwrap(), NonZero::new(RATE).unwrap(), samples.clone());
    let out: Vec<f32> = Tap::new(src, ring.clone()).collect();
    assert_eq!(out, samples, "tap must not change the audio");
    let mut last = vec![0.0; TAP_CHUNK * 2];
    assert_eq!(ring.lock().unwrap().latest(&mut last), RATE);
    assert!(last.iter().all(|&v| v == 0.5));
  }
}
