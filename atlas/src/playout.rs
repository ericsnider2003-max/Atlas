//! Playing Atlas's voice inside Atlas, through the speaker it chose.
//!
//! Until 29 Sep 2026 every reply was handed to `ffplay`, which plays through
//! whatever Windows calls the default and can't be told otherwise. So the
//! speaker half of `audio::choose` -- your named speakers first, then
//! headphones when they're connected -- was worked out and never used, and
//! Windows' device listing (dshow) names no speakers at all, so a spoken
//! notice could never know it was going into your ear.
//!
//! Here: the speakers this machine has (WASAPI, through `cpal`, which Atlas
//! already uses to record calls), and a player for one WAV into one of them,
//! stopped the moment you cut in. Anything that goes wrong is an error the
//! caller answers by falling back to `ffplay`, so nothing that worked before
//! stops working.

/// A decoded WAV: interleaved samples, -1.0..1.0.
#[derive(Debug, Clone, PartialEq)]
pub struct Wav {
    pub rate: u32,
    pub channels: u16,
    pub samples: Vec<f32>,
}

/// Read a WAV file's bytes: 16-bit or 32-bit integer PCM, or 32-bit float.
/// Anything else is refused with what it was.
pub fn parse_wav(bytes: &[u8]) -> Result<Wav, String> {
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err("not a WAV file".into());
    }
    let u16_at = |i: usize| u16::from_le_bytes([bytes[i], bytes[i + 1]]);
    let u32_at = |i: usize| u32::from_le_bytes([bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]]);
    let (mut fmt, mut data): (Option<(u16, u16, u32, u16)>, Option<&[u8]>) = (None, None);
    let mut at = 12;
    while at + 8 <= bytes.len() {
        let id = &bytes[at..at + 4];
        let len = u32_at(at + 4) as usize;
        let body = at + 8;
        // A streamed WAV can say its data runs to the end (0 or too long).
        let end = if len == 0 || body + len > bytes.len() { bytes.len() } else { body + len };
        if id == b"fmt " && end >= body + 16 {
            fmt = Some((u16_at(body), u16_at(body + 2), u32_at(body + 4), u16_at(body + 14)));
        } else if id == b"data" {
            data = Some(&bytes[body..end]);
            break;
        }
        at = end + (end - body) % 2;
    }
    let (format, channels, rate, bits) = fmt.ok_or("the WAV has no format")?;
    let data = data.ok_or("the WAV has no sound in it")?;
    if channels == 0 || rate == 0 {
        return Err("the WAV says it has no channels or no rate".into());
    }
    // 0xFFFE is WAVE_FORMAT_EXTENSIBLE; its sub-format follows the same bits.
    let samples: Vec<f32> = match (format, bits) {
        (1 | 0xFFFE, 16) => data.as_chunks::<2>().0.iter().map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0).collect(),
        (1 | 0xFFFE, 32) => {
            data.as_chunks::<4>().0.iter().map(|b| i32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f32 / 2_147_483_648.0).collect()
        }
        (3, 32) => data.as_chunks::<4>().0.iter().map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect(),
        (f, b) => return Err(format!("a WAV I don't play (format {f}, {b}-bit)")),
    };
    Ok(Wav { rate, channels, samples })
}

/// `w` as a device wants it: `rate` and `channels`, interleaved. Linear
/// interpolation: speech, not music, and nothing here is worth a filter.
/// One channel is copied to every channel; more are mixed down to one, or
/// the first ones kept.
pub fn fit_to_speaker(w: &Wav, rate: u32, channels: u16) -> Vec<f32> {
    let from_ch = w.channels as usize;
    let to_ch = channels.max(1) as usize;
    let frames = w.samples.len() / from_ch.max(1);
    if frames == 0 {
        return Vec::new();
    }
    let frame = |i: usize| &w.samples[i * from_ch..i * from_ch + from_ch];
    let out_frames = ((frames as u64 * rate as u64) / w.rate.max(1) as u64).max(1) as usize;
    let step = w.rate as f64 / rate.max(1) as f64;
    let mut out = Vec::with_capacity(out_frames * to_ch);
    for o in 0..out_frames {
        let pos = o as f64 * step;
        let i = (pos.floor() as usize).min(frames - 1);
        let j = (i + 1).min(frames - 1);
        let t = (pos - i as f64) as f32;
        let (a, b) = (frame(i), frame(j));
        let at = |c: usize| a[c] + (b[c] - a[c]) * t;
        for c in 0..to_ch {
            let v = if from_ch == 1 {
                at(0)
            } else if to_ch == 1 {
                (0..from_ch).map(at).sum::<f32>() / from_ch as f32
            } else {
                at(c.min(from_ch - 1))
            };
            out.push(v);
        }
    }
    out
}

/// The speakers this machine has, by the names Windows gives them. Empty
/// where they can't be listed (and on other platforms, where `ffplay` plays).
fn outputs() -> Vec<String> {
    #[cfg(windows)]
    {
        use cpal::traits::{DeviceTrait, HostTrait};
        let host = cpal::default_host();
        match host.output_devices() {
            Ok(ds) => ds.filter_map(|d| d.name().ok()).collect(),
            Err(_) => Vec::new(),
        }
    }
    #[cfg(not(windows))]
    {
        Vec::new()
    }
}

/// The speakers, as devices `audio::choose` can pick from.
pub fn output_devices() -> Vec<crate::audio::Device> {
    outputs().iter().map(|n| crate::audio::Device::new(n, crate::audio::Kind::Output)).collect()
}

/// Which speaker to play through: `audio::choose`'s pick among `outputs`
/// (your named ones, then headphones when they're connected). `None`: the
/// system's default.
pub fn chosen_output(outputs: &[crate::audio::Device], cfg: &crate::audio::AudioConfig) -> Option<String> {
    if outputs.is_empty() {
        return None;
    }
    crate::audio::choose(outputs, cfg, true).output
}

/// The speaker picked, kept for a little while: listing devices for every
/// sentence would put that in front of every sentence.
static PICKED: std::sync::Mutex<Option<(std::time::Instant, Option<String>)>> = std::sync::Mutex::new(None);

/// How long a speaker pick is kept before the devices are listed again.
pub const PICK_KEPT_SECS: u64 = 20;

/// The speaker to play through now (see `chosen_output`), re-listed every
/// `PICK_KEPT_SECS` so headphones connecting are picked up within a reply or
/// two.
pub fn speaker_now(cfg: &crate::audio::AudioConfig) -> Option<String> {
    if let Ok(g) = PICKED.lock().or_else(crate::crash::unpoison) {
        if let Some((at, pick)) = g.as_ref() {
            if at.elapsed().as_secs() < PICK_KEPT_SECS {
                return pick.clone();
            }
        }
    }
    let pick = chosen_output(&output_devices(), cfg);
    if let Ok(mut g) = PICKED.lock().or_else(crate::crash::unpoison) {
        *g = Some((std::time::Instant::now(), pick.clone()));
    }
    pick
}

/// Play a decoded WAV (`parse_wav`) through `device` (by name; the default when `None` or
/// not found), until it ends or `stop()` says to. `Err` means nothing was
/// played and the caller should use `ffplay`.
#[cfg(windows)]
pub fn play(wav: &Wav, device: Option<&str>, stop: &dyn Fn() -> bool) -> Result<(), String> {
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::Arc;
    let host = cpal::default_host();
    let named = device.and_then(|want| {
        host.output_devices().ok()?.find(|d| d.name().is_ok_and(|n| n == want))
    });
    let dev = named.or_else(|| host.default_output_device()).ok_or("there's no speaker to play through")?;
    let config = dev.default_output_config().map_err(|e| format!("the speaker won't say how it plays: {e}"))?;
    let (rate, channels) = (config.sample_rate().0, config.channels());
    let samples: Arc<Vec<f32>> = Arc::new(fit_to_speaker(wav, rate, channels));
    let pos = Arc::new(AtomicUsize::new(0));
    let failed = Arc::new(AtomicBool::new(false));
    let err = {
        let failed = failed.clone();
        move |_e: cpal::StreamError| failed.store(true, Ordering::Relaxed)
    };
    // Fill `out` from where the last call stopped; silence once it's all out.
    fn fill<T: Copy>(out: &mut [T], samples: &[f32], pos: &AtomicUsize, from: impl Fn(f32) -> T) {
        let at = pos.load(std::sync::atomic::Ordering::Relaxed);
        for (k, o) in out.iter_mut().enumerate() {
            *o = from(samples.get(at + k).copied().unwrap_or(0.0));
        }
        pos.store(at + out.len(), std::sync::atomic::Ordering::Relaxed);
    }
    let cfg = config.config();
    let stream = match config.sample_format() {
        cpal::SampleFormat::F32 => {
            let (s, p) = (samples.clone(), pos.clone());
            dev.build_output_stream(&cfg, move |out: &mut [f32], _: &_| fill(out, &s, &p, |v| v), err, None)
        }
        cpal::SampleFormat::I16 => {
            let (s, p) = (samples.clone(), pos.clone());
            dev.build_output_stream(
                &cfg,
                move |out: &mut [i16], _: &_| fill(out, &s, &p, |v| (v.clamp(-1.0, 1.0) * i16::MAX as f32) as i16),
                err,
                None,
            )
        }
        cpal::SampleFormat::U16 => {
            let (s, p) = (samples.clone(), pos.clone());
            dev.build_output_stream(
                &cfg,
                move |out: &mut [u16], _: &_| fill(out, &s, &p, |v| ((v.clamp(-1.0, 1.0) + 1.0) * 32767.5) as u16),
                err,
                None,
            )
        }
        other => return Err(format!("the speaker plays in a form I don't write ({other:?})")),
    }
    .map_err(|e| format!("couldn't open the speaker: {e}"))?;
    stream.play().map_err(|e| format!("couldn't start the speaker: {e}"))?;
    let total = samples.len();
    // A little past the end, so the device's own buffer is heard out.
    let tail = std::time::Duration::from_millis(150);
    let mut ended_at: Option<std::time::Instant> = None;
    loop {
        if stop() {
            return Ok(());
        }
        if failed.load(Ordering::Relaxed) {
            // Before any sound: the caller plays it another way. After some:
            // not again from the start (it would be heard twice).
            return if pos.load(Ordering::Relaxed) == 0 {
                Err("the speaker wouldn't play (unplugged?)".into())
            } else {
                Ok(())
            };
        }
        if pos.load(Ordering::Relaxed) >= total {
            let at = *ended_at.get_or_insert_with(std::time::Instant::now);
            if at.elapsed() >= tail {
                return Ok(());
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(15));
    }
}

#[cfg(not(windows))]
pub fn play(_wav: &Wav, _device: Option<&str>, _stop: &dyn Fn() -> bool) -> Result<(), String> {
    Err("played by ffplay on this platform".into())
}

/// Is the configured player the shipped `ffplay`, which Atlas plays in place
/// of? A player you named yourself is always used as named.
pub fn plays_inside(command: &str) -> bool {
    let name = command.trim().rsplit(['/', '\\']).next().unwrap_or("").to_lowercase();
    name == "ffplay" || name == "ffplay.exe"
}

static NOTED: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

/// Why playing inside Atlas failed, said once per different reason (the
/// fallback then plays every sentence, and saying so each time is noise).
pub fn note_once(why: &str) -> Option<String> {
    let mut g = NOTED.lock().or_else(crate::crash::unpoison).ok()?;
    if g.as_deref() == Some(why) {
        return None;
    }
    *g = Some(why.to_string());
    Some(format!("Playing through ffplay instead: {why}."))
}
