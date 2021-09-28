/*! 
# bwavfile
 
Rust Wave File Reader/Writer with Broadcast-WAV, MBWF and RF64 Support

## Interfaces

### `WaveReader`

`WaveReader` can open and parse a Wave, Broadcast-Wave, or RF64/BW64 64-bit 
wave file. Metadata can be accessed and parsed in arbitrary order and audio
samples can be accessed using the `AudioFrameReader` type, created by an 
accessor method of `WaveReader`.

### `WaveWriter`

`WaveWriter` can create a new Wave, Broadcast-Wave, or RF64/BW64 64-bit wave 
file. Metadata chunks and audio samples are added sequentially, write-only, to 
a Wave file which is automatically promoted from standard Wave to RF64 wave 
when the total WAVE form size exceeds 0xFFFFFFFF bytes.


## Objectives and Roadmap

This package aims to support read and writing any kind of WAV file you are likely 
to encounter in a professional audio, motion picture production, broadcast, or music 
production.

Apps we test against:
- Avid Pro Tools
- iZotope RX Audio Editor
- FFMpeg
- Audacity

[github]: https://github.com/iluvcapra/bwavfile
*/

// #![feature(external_doc)]

// #[doc(include="../README.md")]
// #[cfg(doctest)]
// pub struct ReadmeDoctests;

extern crate encoding;
extern crate byteorder;
extern crate uuid;

mod fourcc;
mod errors;
mod common_format;

mod parser;

mod audio_frame_reader;
mod list_form;

mod chunks;
mod cue;
mod bext;
mod fmt;

mod wavereader;
mod wavewriter;

mod wave;

pub use errors::Error;
pub use wavereader::WaveReader;
pub use wavewriter::{WaveWriter, AudioFrameWriter};
pub use bext::Bext;
pub use fmt::{WaveFmt, WaveFmtExtended, ChannelDescriptor, ChannelMask, ADMAudioID};
pub use common_format::CommonFormat;
pub use audio_frame_reader::AudioFrameReader;
pub use cue::Cue;

pub use wave::Wave;