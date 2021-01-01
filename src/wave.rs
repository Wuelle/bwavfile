use crate::ChannelMask;
use crate::ChannelDescriptor;

use std::collections::HashMap;
use std::io::{Read, Write, Seek, Cursor};
use std::io::SeekFrom;
use std::fs::File;

use super::errors::Error;
use super::fmt::WaveFmt;
use super::bext::Bext;
use super::chunks::ReadBWaveChunks;
use super::fourcc::{FourCC, ReadFourCC, RIFF_SIG, RF64_SIG, BW64_SIG, WAVE_SIG, 
    LIST_SIG,
    DS64_SIG, FMT__SIG, DATA_SIG, BEXT_SIG, IXML_SIG, AXML_SIG};

use byteorder::LittleEndian;
use byteorder::WriteBytesExt;
use byteorder::ReadBytesExt;

pub struct Wave<T: Seek> {
    inner : T
}

impl<T: Seek> Wave<T> {

    pub fn new(inner : T) -> Self {
        Wave { inner }
    }

    pub fn into_inner(self) -> T {
        self.inner
    }
}

impl Wave<File> {
    pub fn open(path: &str) -> Result<Self,Error> {
        let f = File::open(path)?;
        Ok( Self::new(f) )
    }
}

impl<T> Wave<T> where T:Read + Seek {

    /// Get the u64 length of a chunk
    /// Use `RF64_SIG` for `ident` to get the file form size
    /// 
    /// Note: this function saves and restores the file position before returning
    fn get_ds64_length(&mut self, ident:FourCC) -> Result<u64, Error> {
        let old_pos = self.inner.seek(SeekFrom::Current(0))?;
        let mut retval : Option<u64> = None;

        self.inner.seek( SeekFrom::Start(12) )?;
        let ( ds64_header, _, _ ) = self.read_chunk_header_immediate()?;
        
        if ds64_header == DS64_SIG {
            // this loop is dopey but it lets us break out of the middle
            loop {
                let form_size = self.inner.read_u64::<LittleEndian>()?;
                if ident == RF64_SIG || ident == BW64_SIG { retval = Some(form_size); break; }

                let data_size = self.inner.read_u64::<LittleEndian>()?;
                if ident == DATA_SIG { retval = Some(data_size); break; }

                let _fact = self.inner.read_u64::<LittleEndian>()?;
                
                let field_count = self.inner.read_u32::<LittleEndian>()?;
                for _ in 0..field_count {
                    let this_fourcc = self.inner.read_fourcc()?;
                    let this_field_size = self.inner.read_u64::<LittleEndian>()?;
                    if this_fourcc == ident { retval = Some(this_field_size); break; }
                }

                break;
            }
        }

        self.inner.seek( SeekFrom::Start(old_pos) )?;

        retval.ok_or( Error::MissingDS64Length(ident) )
    }

    /// True if the file is a valid RF64 format
    ///
    /// Note: this function saves and restores the file position before returning
    pub fn is_rf64(&mut self) -> Result<bool, Error> {
        let old = self.inner.seek(SeekFrom::Current(0))?;
        self.inner.seek(SeekFrom::Start(0))?;
        let magic = self.inner.read_fourcc()?;
        let length_field = self.inner.read_u32::<LittleEndian>()?;
        let form_ident = self.inner.read_fourcc()?;

        match (magic, length_field, form_ident) {
            (RF64_SIG, 0xFFFFFFFF, WAVE_SIG) | (BW64_SIG, 0xFFFFFFFF, WAVE_SIG) => {
                let ds64 = self.inner.read_fourcc()?;
                self.inner.seek(SeekFrom::Start(old))?;
                if ds64 == DS64_SIG {
                    Ok(true)
                } else {
                    Err( Error::MissingRequiredDS64 )
                }
                
            } ,
            (RIFF_SIG, _, WAVE_SIG) => {
                self.inner.seek(SeekFrom::Start(old))?;
                Ok(false)
            },
            (_,_,_) => Err( Error::HeaderNotRecognized)
        }
    }

    /// read the chunk header at the cursor, return the ident, length, and 
    /// displacement. The cursor is positioned at the beginning of the chunk content
    fn read_chunk_header_immediate(&mut self) -> Result<(FourCC, u64, u64), Error> {
        let ident_field = self.inner.read_fourcc()?;
        let length_field_u32 = self.inner.read_u32::<LittleEndian>()?;
        let chunk_length :u64;
        if length_field_u32 == 0xFFFFFFFF && self.is_rf64()? {
            chunk_length = self.get_ds64_length(ident_field)?;
        } else {
            chunk_length = length_field_u32 as u64;
        }

        let chunk_displacement = chunk_length + chunk_length % 2;

        Ok( (ident_field, chunk_length, chunk_displacement) )
    }

    /// Seek to the beginning of a chunk
    fn seek_chunk(&mut self, ident: FourCC, index: u32) -> Result<u64, Error> {
        self.inner.seek( SeekFrom::Start(0) )?;
        let (_, length, _) = self.read_chunk_header_immediate()?;
        let _ = self.inner.read_fourcc()?;

        let mut remain = length - 4;
        let mut count = 0;

        while remain > 0 {
            let (fourcc, length, displacement) = self.read_chunk_header_immediate()?;
            if fourcc == ident {
                if count == index {
                    return Ok(length)
                } else {
                    count = count + 1;
                }
            }
            self.inner.seek(SeekFrom::Current(displacement as i64))?;
            remain = remain - (8 + displacement);
        }

        Err( Error::ChunkMissing { signature: ident } )
    }

    fn read_chunk(&mut self, ident : FourCC, at_index: u32, buffer: &mut [u8]) -> Result<usize, Error> {
        self.seek_chunk(ident, at_index)?;
        self.inner.read(buffer).map_err(|e| e.into())
    }

    /// Get the audio format of this wave file.
    /// 
    /// ```
    /// use bwavfile::Wave;
    /// use std::fs::File;
    /// 
    /// let f = File::open("tests/media/ff_bwav_stereo.wav").unwrap();
    /// let mut wave = Wave::new(f);
    /// let fmt = wave.format().unwrap();
    /// assert_eq!(fmt.sample_rate, 48000);
    /// ```
    pub fn format(&mut self) -> Result<WaveFmt, Error> {
        let _len = self.seek_chunk(FMT__SIG, 0)?;
        self.inner.read_wave_fmt()
    }

    /// Get the frame length of this wave file.
    /// 
    /// ```
    /// # use bwavfile::Wave;
    /// # use std::fs::File;
    /// let mut w = Wave::open("tests/media/ff_silence.wav").unwrap();
    /// assert_eq!(w.frame_length().unwrap(), 44100);
    /// ```
    pub fn frame_length(&mut self) -> Result<u64, Error> {
        let length = self.seek_chunk(DATA_SIG, 0)?;
        let block_align = self.format()?.block_alignment;
        Ok( length / (block_align as u64) )
    }

    /// Get the Broadast-WAV Metadata record
    ///
    pub fn broadcast_extension(&mut self) -> Result<Bext, Error> {
        let _len = self.seek_chunk(BEXT_SIG, 0)?;
        self.inner.read_bext()
    }

    /// Describe channels in the Wave file
    /// 
    /// ```rust
    /// use bwavfile::Wave;
    /// use bwavfile::ChannelMask;
    ///
    /// let mut f = Wave::open("tests/media/pt_24bit_51.wav").unwrap();
    /// 
    /// let chans = f.channels().unwrap();
    /// assert_eq!(chans[0].index, 0);
    /// assert_eq!(chans[0].speaker, ChannelMask::FrontLeft);
    /// assert_eq!(chans[3].index, 3);
    /// assert_eq!(chans[3].speaker, ChannelMask::LowFrequency);
    /// assert_eq!(chans[4].speaker, ChannelMask::BackLeft);
    /// ```
    pub fn channels(&mut self) -> Result<Vec<ChannelDescriptor>, Error> {
        
        let format = self.format()?;
        let channel_masks : Vec<ChannelMask> = match (format.channel_count, format.extended_format) {
            (1,_) => vec![ChannelMask::FrontCenter],
            (2,_) => vec![ChannelMask::FrontLeft, ChannelMask::FrontRight],
            (n,Some(x)) => ChannelMask::channels(x.channel_mask, n),
            (n,_) => vec![ChannelMask::DirectOut; n as usize]
        };

        Ok( (0..format.channel_count).zip(channel_masks)
            .map(|(i,m)| ChannelDescriptor { index: i, speaker:m, adm_track_audio_ids: vec![] } )
            .collect() )
    }

    /// Read iXML metadata
    pub fn read_ixml(&mut self, buffer: &mut Vec<u8>) -> Result<usize, Error> {
        self.read_chunk(IXML_SIG, 0, buffer) 
    }

    /// Read axml metadata
    pub fn read_axml(&mut self, buffer: &mut Vec<u8>) -> Result<usize, Error> {
        self.read_chunk(AXML_SIG, 0, buffer) 
    }
}