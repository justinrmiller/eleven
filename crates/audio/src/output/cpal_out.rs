//! Sound-card output via cpal.
//!
//! The cpal stream is created and owned by a dedicated thread, so the (platform-dependent,
//! sometimes `!Send`) stream object never crosses threads. Dropping [`CpalOutput`] stops it.

use std::sync::Arc;
use std::sync::mpsc;
use std::thread::JoinHandle;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample, StreamConfig};

use super::{Opened, Output, Pipes, Renderer, Shared};
use crate::error::{AudioError, Result};

/// A running cpal output stream.
pub struct CpalOutput {
    sample_rate: u32,
    name: String,
    stop: Option<mpsc::Sender<()>>,
    thread: Option<JoinHandle<()>>,
}

type OpenResult = Result<(u32, String, rtrb::Producer<f32>, rtrb::Consumer<f32>)>;

impl CpalOutput {
    /// Opens `device_name` (or the default device) and starts streaming.
    ///
    /// # Errors
    /// Fails if no device is available or the stream cannot be built.
    pub fn open(device_name: Option<&str>, shared: Arc<Shared>) -> Result<Opened> {
        let (result_tx, result_rx) = mpsc::channel::<OpenResult>();
        let (stop_tx, stop_rx) = mpsc::channel::<()>();
        let wanted = device_name.map(str::to_owned);
        let thread = std::thread::Builder::new()
            .name("eleven-cpal-output".into())
            .spawn(move || {
                match start_stream(wanted.as_deref(), shared) {
                    Ok((stream, rate, name, producer, vis)) => {
                        let _ = result_tx.send(Ok((rate, name, producer, vis)));
                        // Keep the stream alive until asked to stop (or the owner is dropped).
                        let _ = stop_rx.recv();
                        drop(stream);
                    }
                    Err(e) => {
                        let _ = result_tx.send(Err(e));
                    }
                }
            })
            .map_err(|e| AudioError::Device(format!("cannot spawn output thread: {e}")))?;
        let (sample_rate, name, producer, vis) = result_rx
            .recv()
            .map_err(|_| AudioError::Device("output thread exited".into()))??;
        let output = Self {
            sample_rate,
            name,
            stop: Some(stop_tx),
            thread: Some(thread),
        };
        Ok((Box::new(output), producer, vis))
    }
}

impl Output for CpalOutput {
    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    fn name(&self) -> String {
        self.name.clone()
    }
}

impl Drop for CpalOutput {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Names of the available output devices on the default host.
#[must_use]
pub fn list_devices() -> Vec<String> {
    let host = cpal::default_host();
    host.output_devices()
        .map(|devices| {
            devices
                .filter_map(|d| d.description().ok().map(|desc| desc.name().to_owned()))
                .collect()
        })
        .unwrap_or_default()
}

type Started = (
    cpal::Stream,
    u32,
    String,
    rtrb::Producer<f32>,
    rtrb::Consumer<f32>,
);

fn start_stream(device_name: Option<&str>, shared: Arc<Shared>) -> Result<Started> {
    let host = cpal::default_host();
    let device = match device_name {
        Some(wanted) => host
            .output_devices()
            .map_err(|e| AudioError::Device(e.to_string()))?
            .find(|d| d.description().is_ok_and(|desc| desc.name() == wanted))
            .or_else(|| host.default_output_device()),
        None => host.default_output_device(),
    }
    .ok_or_else(|| AudioError::Device("no output device available".into()))?;
    let name = device
        .description()
        .map_or_else(|_| "Unknown device".to_owned(), |d| d.name().to_owned());
    let supported = device
        .default_output_config()
        .map_err(|e| AudioError::Device(e.to_string()))?;
    let format = supported.sample_format();
    let config: StreamConfig = supported.into();
    let rate = config.sample_rate;
    let Pipes {
        producer,
        renderer,
        vis,
    } = Pipes::new(shared, rate);
    let stream = match format {
        SampleFormat::F32 => build_f32(&device, config, renderer),
        SampleFormat::I16 => build::<i16>(&device, config, renderer),
        SampleFormat::U16 => build::<u16>(&device, config, renderer),
        SampleFormat::I32 => build::<i32>(&device, config, renderer),
        SampleFormat::U32 => build::<u32>(&device, config, renderer),
        SampleFormat::F64 => build::<f64>(&device, config, renderer),
        SampleFormat::I8 => build::<i8>(&device, config, renderer),
        SampleFormat::U8 => build::<u8>(&device, config, renderer),
        other => Err(AudioError::Device(format!(
            "unsupported sample format {other}"
        ))),
    }?;
    stream
        .play()
        .map_err(|e| AudioError::Device(e.to_string()))?;
    Ok((stream, rate, name, producer, vis))
}

#[allow(clippy::needless_pass_by_value)] // cpal's error callback takes the error by value.
fn on_error(err: cpal::Error) {
    log::warn!("audio stream error: {err}");
}

fn build_f32(
    device: &cpal::Device,
    config: StreamConfig,
    mut renderer: Renderer,
) -> Result<cpal::Stream> {
    let channels = usize::from(config.channels);
    device
        .build_output_stream(
            config,
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| renderer.render(data, channels),
            on_error,
            None,
        )
        .map_err(|e| AudioError::Device(e.to_string()))
}

fn build<T>(
    device: &cpal::Device,
    config: StreamConfig,
    mut renderer: Renderer,
) -> Result<cpal::Stream>
where
    T: SizedSample + FromSample<f32>,
{
    let channels = usize::from(config.channels);
    let mut scratch: Vec<f32> = Vec::new();
    device
        .build_output_stream(
            config,
            move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
                if scratch.len() != data.len() {
                    scratch.resize(data.len(), 0.0);
                }
                renderer.render(&mut scratch, channels);
                for (d, s) in data.iter_mut().zip(&scratch) {
                    *d = T::from_sample(*s);
                }
            },
            on_error,
            None,
        )
        .map_err(|e| AudioError::Device(e.to_string()))
}
