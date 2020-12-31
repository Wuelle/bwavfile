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

    // read the chunk header at the cursor, return the ident, length, and 
    // displacement. The cursor is positioned at the beginning of the chunk content
    fn read_chunk_header_immediate(&mut self, 
        ds64_lengths : HashMap<FourCC, u64>) -> Result<(FourCC, u64, u64), Error> {

        let ident_field = self.inner.read_fourcc()?;
        let length_field_u32 = self.inner.read_u32::<LittleEndian>()?;
        let chunk_length :u64;
        if ds64_lengths.contains_key(&ident_field) && length_field_u32 == 0xFFFFFFFF {
            chunk_length = ds64_lengths[&ident_field];
        } else {
            chunk_length = length_field_u32 as u64;
        }

        let chunk_displacement = chunk_length + chunk_length % 2;

        Ok((ident_field, chunk_length, chunk_displacement))
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