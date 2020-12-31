use crate::ChannelMask;
use crate::ChannelDescriptor;

use std::collections::HashMap;
use std::io::{Read, Write, Seek, Cursor};
use std::io::SeekFrom;

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

struct Wave<T: Seek> {
    inner : T
}

impl<T: Seek> Wave<T> {

    pub fn into_inner(self) -> T {
        self.inner
    }
}

impl<T> Wave<T> where T:Read + Seek {
    
    /// seek to the beginning of chunk `ident` and return the length of the chunk
    fn seek_chunk(&mut self, ident: FourCC, at_index: u32) -> Result<u64, Error> {

        fn read_ds64<F: Seek+Read>(mut r: F) -> Result<(u64, HashMap<FourCC, u64>), Error> {
            if r.read_fourcc()? != DS64_SIG {
                return Err( Error::MissingRequiredDS64 );
            }
            let ds64_len = r.read_u32::<LittleEndian>()? as u64;
            let mut ds64_bin = Vec::with_capacity(ds64_len as usize);
            r.read(&mut ds64_bin)?;

            let mut ds64_cursor = Cursor::new(ds64_bin);
            let form_length = ds64_cursor.read_u64::<LittleEndian>()?;

            let mut overrides = HashMap::new();
            overrides.insert(DATA_SIG, ds64_cursor.read_u64::<LittleEndian>()? );

            let _fact = ds64_cursor.read_u64::<LittleEndian>()?; // this is not used

            let field_count = ds64_cursor.read_u32::<LittleEndian>()?;

            for _ in 0..field_count {
                let this_fourcc = ds64_cursor.read_fourcc()?;
                let this_field_size = ds64_cursor.read_u64::<LittleEndian>()?;
                overrides.insert(this_fourcc, this_field_size);
            }

            if ds64_len % 2 == 1 { 
                r.read_u8()?; 
                // 4 for `WAVE`, 8 for `ds64` and size, 1 for framing
                Ok( (form_length - (4 + 8 + ds64_len + 1), overrides) )
            } else {
                Ok( (form_length - (4 + 8 + ds64_len), overrides) )
            }
        }

        fn seek_chunk_riff<F: Seek+Read>(mut r: F, ident: FourCC, at_index: u32, form_length: u64,
            override_chunk_lengths: HashMap<FourCC, u64>) -> Result<u64, Error> {

            let mut count = 0;
            let mut form_remain = form_length;

            // note we return from the middle of this

            // in principle we could have done this with a tail call but since rust
            // doesn't optimize tail calls we're just doing iteration

            loop {
                let this_fourcc = r.read_fourcc()?;
                let this_length : u64;
                if override_chunk_lengths.contains_key(&this_fourcc) {
                    this_length = override_chunk_lengths[&this_fourcc];
                } else {
                    this_length = r.read_u32::<LittleEndian>()? as u64;
                }

                if this_fourcc == ident {
                    if count == at_index {
                        return Ok( this_length )
                    }
                } else {
                    count = count + 1;
                }
                    
                let this_displacement = (this_length + this_length % 2) as i64;
                r.seek(SeekFrom::Current(this_displacement))?;
                form_remain = form_remain - this_displacement as u64;
                if form_remain < 8 { break }
            }

            Err( Error:: ChunkMissing { signature: ident } )
        }

        self.inner.seek(SeekFrom::Start(0))?;
        let magic = self.inner.read_fourcc()?;
        let magic_length = self.inner.read_u32::<LittleEndian>()?;
        let form = self.inner.read_fourcc()?;

        match (magic, magic_length, form) {
            (RIFF_SIG, length, WAVE_SIG) => {
                // subtract 4 for the `WAVE` header
                seek_chunk_riff(&mut self.inner, ident, at_index, length as u64 - 4, HashMap::new())
            },
            (RF64_SIG, 0xFFFFFFFF, WAVE_SIG) | (BW64_SIG, 0xFFFFFFFF, WAVE_SIG) => {
                let (length, overrides) = read_ds64(&mut self.inner)?;
                seek_chunk_riff(&mut self.inner, ident, at_index, length as u64, overrides)
            },
            (_,_,_) => {
                Err( Error::HeaderNotRecognized )
            }
        }
    }

    /// Seek to the beginning of a list if present
    fn seek_list(&mut self, form_ident: FourCC, at_index: u32) -> Result<u64, Error> { 
        let mut at = 0;
        loop {
            let result = self.seek_chunk(LIST_SIG, at);
            if let Err(Error::ChunkMissing { signature: _ }) = result {
                return result;

            } else if let Ok( length ) = result {
                let this_form = self.inner.read_fourcc()?;
                if this_form == form_ident {
                    if at == at_index {
                        return Ok( length - 4 )
                    } else {
                        at = at + 1;
                    }
                }
            }
        }
    }

    pub fn read_chunk(&mut self, ident : FourCC, at_index: u32, buffer: &mut [u8]) -> Result<usize, Error> {
        self.seek_chunk(ident, at_index)?;
        self.inner.read(buffer).map_err(|e| e.into())
    }

    pub fn format(&mut self) -> Result<WaveFmt, Error> {
        let _len = self.seek_chunk(FMT__SIG, 0)?;
        self.inner.read_wave_fmt()
    }

    pub fn broadcast_extension(&mut self) -> Result<Bext, Error> {
        let _len = self.seek_chunk(BEXT_SIG, 0)?;
        self.inner.read_bext()
    }

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

    pub fn read_ixml(&mut self, buffer: &mut Vec<u8>) -> Result<usize, Error> {
        self.read_chunk(IXML_SIG, 0, buffer) 
    }

    pub fn read_axml(&mut self, buffer: &mut Vec<u8>) -> Result<usize, Error> {
        self.read_chunk(AXML_SIG, 0, buffer) 
    }

}