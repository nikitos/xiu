use crate::fmp4::{Mp4Box, AUDIO_TRACK_ID, VIDEO_TRACK_ID};
use crate::io::{ByteCounter, WriteTo};
use crate::{ErrorKind, Result};
use std::io::Write;


/// 4.3.1 Segment Type Box (ISO/IEC 14496-12)
#[allow(missing_docs)]
#[derive(Debug, Clone)]
pub struct SegmentTypeBox {
    pub major_brand: [u8; 4],
    pub minor_version: u32,
    pub compatible_brands: Vec<[u8; 4]>,
}

impl SegmentTypeBox {
    /// Creates a new SegmentTypeBox with common fMP4 brands
    pub fn new() -> Self {
        SegmentTypeBox {
            major_brand: *b"iso5",  // ISO Base Media file with some features
            minor_version: 0,
            compatible_brands: vec![
                *b"iso5",  // ISO Base Media file
                *b"iso6",  // ISO Base Media file with some extensions
                *b"mp41",  // MP4 version 1
                *b"mp42",  // MP4 version 2
                *b"dash",  // DASH segmented
            ],
        }
    }
}

impl Default for SegmentTypeBox {
    fn default() -> Self {
        Self::new()
    }
}

impl Mp4Box for SegmentTypeBox {
    const BOX_TYPE: [u8; 4] = *b"styp";

    fn box_payload_size(&self) -> Result<u32> {
        Ok(8 + (self.compatible_brands.len() as u32 * 4))
    }

    fn write_box_payload<W: Write>(&self, mut writer: W) -> Result<()> {
        write_all!(writer, &self.major_brand);
        write_u32!(writer, self.minor_version);
        for brand in &self.compatible_brands {
            write_all!(writer, brand);
        }
        Ok(())
    }
}

/// 8.16.3 Segment Index Box (ISO/IEC 14496-12)
#[allow(missing_docs)]
#[derive(Debug, Clone)]
pub struct SegmentIndexBox {
    pub reference_id: u32,
    pub timescale: u32,
    pub earliest_presentation_time: u64,
    pub first_offset: u64,
    pub references: Vec<SegmentReference>,
}

/// 8.16.3.2 Segment Reference (ISO/IEC 14496-12)
#[allow(missing_docs)]
#[derive(Debug, Clone)]
pub struct SegmentReference {
    pub reference_type: bool,  // true = media, false = segment index
    pub referenced_size: u32,
    pub subsegment_duration: u32,
    pub starts_with_sap: bool,
    pub sap_type: u8,          // 0-3
    pub sap_delta_time: u32,
}

impl SegmentIndexBox {
    /// Creates a new SegmentIndexBox for a media segment
    pub fn new(reference_id: u32, timescale: u32, presentation_time: u64, first_offset: u64) -> Self {
        SegmentIndexBox {
            reference_id,
            timescale,
            earliest_presentation_time: presentation_time,
            first_offset,
            references: Vec::new(),
        }
    }

    /// Adds a reference to a subsegment
    pub fn add_reference(&mut self, reference: SegmentReference) {
        self.references.push(reference);
    }
}

impl Mp4Box for SegmentIndexBox {
    const BOX_TYPE: [u8; 4] = *b"sidx";

    fn box_version(&self) -> Option<u8> {
        Some(1)  // Version 1 supports 64-bit values
    }

    fn box_payload_size(&self) -> Result<u32> {
        // reference_id(4) + timescale(4) + earliest_presentation_time(8) + first_offset(8) +
        // reserved(2) + reference_count(2) + references * 12
        Ok(4 + 4 + 8 + 8 + 2 + 2 + (self.references.len() as u32 * 12))
    }

    fn write_box_payload<W: Write>(&self, mut writer: W) -> Result<()> {
        write_u32!(writer, self.reference_id);
        write_u32!(writer, self.timescale);
        write_u64!(writer, self.earliest_presentation_time);
        write_u64!(writer, self.first_offset);
        
        // Reserved (16 bits) + reference_count (16 bits)
        write_u16!(writer, 0);
        write_u16!(writer, self.references.len() as u16);

        for reference in &self.references {
            // reference_type (1 bit) + referenced_size (31 bits)
            let first_word = (reference.referenced_size & 0x7FFFFFFF) | 
                             ((reference.reference_type as u32) << 31);
            write_u32!(writer, first_word);
            
            write_u32!(writer, reference.subsegment_duration);
            
            // starts_with_sap (1 bit) + sap_type (3 bits) + sap_delta_time (28 bits)
            let third_word = (reference.sap_delta_time & 0x0FFFFFFF) | 
                             ((reference.starts_with_sap as u32) << 31) |
                             ((reference.sap_type as u32) << 28);
            write_u32!(writer, third_word);
        }
        
        Ok(())
    }
}


/// [ISO BMFF Byte Stream Format: 4. Media Segments][media_segment]
///
/// [media_segment]: https://w3c.github.io/media-source/isobmff-byte-stream-format.html#iso-media-segments
#[allow(missing_docs)]
#[derive(Debug, Default)]
pub struct MediaSegment {
    pub moof_box: MovieFragmentBox,
    pub mdat_boxes: Vec<MediaDataBox>,
        /// Segment Type Box (styp) - required for fMP4 segments
    pub styp_box: SegmentTypeBox,
    /// Segment Index Boxes (sidx) - one per track, optional but recommended
    pub sidx_boxes: Vec<SegmentIndexBox>,
}
impl MediaSegment {
    /// Creates a new MediaSegment with default styp box
    pub fn new() -> Self {
        MediaSegment {
            styp_box: SegmentTypeBox::new(),
            sidx_boxes: Vec::new(),
            moof_box: MovieFragmentBox::default(),
            mdat_boxes: Vec::new(),
        }
    }

    /// Adds a sidx box for a track
    pub fn add_sidx(&mut self, track_id: u32, timescale: u32, presentation_time: u64) {
        // first_offset - это смещение от начала этого бокса до первого медиа данных
        // Обычно это размер всех боксов до mdat
        let first_offset = 0; // Будет пересчитано при записи
        let mut sidx = SegmentIndexBox::new(track_id, timescale, presentation_time, first_offset);
        
        // Добавляем ссылку на первый субсегмент
        // В простейшем случае - один субсегмент = весь сегмент
        sidx.add_reference(SegmentReference {
            reference_type: false, // media reference
            referenced_size: 0,    // будет заполнено позже
            subsegment_duration: 0, // длительность в timescale units
            starts_with_sap: true,
            sap_type: 1,            // SAP Type 1 = sync sample
            sap_delta_time: 0,
        });
        
        self.sidx_boxes.push(sidx);
    }

    /// Updates sidx references with actual sizes and durations
    pub fn finalize(&mut self, total_mdat_size: u32, duration: u32) {
        // Compute once outside of mutable borrow iteration to avoid borrow conflict
        let offset_to_mdat = self.styp_box.box_size().unwrap_or(0)
            + self.sidx_boxes.iter().map(|b| b.box_size().unwrap_or(0)).sum::<u32>()
            + self.moof_box.box_size().unwrap_or(0);

        for sidx in &mut self.sidx_boxes {
            if let Some(first_ref) = sidx.references.first_mut() {
                first_ref.referenced_size = total_mdat_size;
                first_ref.subsegment_duration = duration;
            }

            // Обновляем first_offset - это размер от начала sidx до первого mdat
            sidx.first_offset = offset_to_mdat as u64;
        }
    }
}

impl WriteTo for MediaSegment {
    fn write_to<W: Write>(&self, mut writer: W) -> Result<()> {
        track_assert!(!self.mdat_boxes.is_empty(), ErrorKind::InvalidInput);
        
        // Сначала пишем styp
        write_box!(writer, self.styp_box);
        
        // Затем все sidx боксы
        write_boxes!(writer, &self.sidx_boxes);
        
        // Затем moof
        write_box!(writer, self.moof_box);
        
        // И наконец mdat
        write_boxes!(writer, &self.mdat_boxes);
        
        Ok(())
    }
}

/// 8.1.1 Media Data Box (ISO/IEC 14496-12).
#[allow(missing_docs)]
#[derive(Debug)]
pub struct MediaDataBox {
    pub data: Vec<u8>,
}
impl Mp4Box for MediaDataBox {
    const BOX_TYPE: [u8; 4] = *b"mdat";

    fn box_payload_size(&self) -> Result<u32> {
        Ok(self.data.len() as u32)
    }
    fn write_box_payload<W: Write>(&self, mut writer: W) -> Result<()> {
        write_all!(writer, &self.data);
        Ok(())
    }
}

/// 8.8.4 Movie Fragment Box (ISO/IEC 14496-12).
#[allow(missing_docs)]
#[derive(Debug, Default)]
pub struct MovieFragmentBox {
    pub mfhd_box: MovieFragmentHeaderBox,
    pub traf_boxes: Vec<TrackFragmentBox>,
}
impl Mp4Box for MovieFragmentBox {
    const BOX_TYPE: [u8; 4] = *b"moof";

    fn box_payload_size(&self) -> Result<u32> {
        let mut size = 0;
        size += box_size!(self.mfhd_box);
        size += boxes_size!(self.traf_boxes);
        Ok(size)
    }
    fn write_box_payload<W: Write>(&self, mut writer: W) -> Result<()> {
        track_assert!(!self.traf_boxes.is_empty(), ErrorKind::InvalidInput);
        write_box!(writer, self.mfhd_box);
        write_boxes!(writer, &self.traf_boxes);
        Ok(())
    }
}

/// 8.8.5 Movie Fragment Header Box (ISO/IEC 14496-12).
#[derive(Debug)]
pub struct MovieFragmentHeaderBox {
    /// The number associated with this fragment.
    pub sequence_number: u32,
}
impl Mp4Box for MovieFragmentHeaderBox {
    const BOX_TYPE: [u8; 4] = *b"mfhd";

    fn box_version(&self) -> Option<u8> {
        Some(0)
    }
    fn box_payload_size(&self) -> Result<u32> {
        Ok(4)
    }
    fn write_box_payload<W: Write>(&self, mut writer: W) -> Result<()> {
        write_u32!(writer, self.sequence_number);
        Ok(())
    }
}
impl Default for MovieFragmentHeaderBox {
    /// Return the default value of `MovieFragmentHeaderBox`.
    ///
    /// This is equivalent to `MovieFragmentHeaderBox { sequence_number: 1 }`.
    fn default() -> Self {
        MovieFragmentHeaderBox { sequence_number: 1 }
    }
}

/// 8.8.6 Track Fragment Box (ISO/IEC 14496-12).
#[allow(missing_docs)]
#[derive(Debug)]
pub struct TrackFragmentBox {
    pub tfhd_box: TrackFragmentHeaderBox,
    pub tfdt_box: TrackFragmentBaseMediaDecodeTimeBox,
    pub trun_box: TrackRunBox,
}
impl TrackFragmentBox {
    /// Makes a new `TrackFragmentBox` instance.
    pub fn new(is_video: bool) -> Self {
        let track_id = if is_video {
            VIDEO_TRACK_ID
        } else {
            AUDIO_TRACK_ID
        };
        TrackFragmentBox {
            tfhd_box: TrackFragmentHeaderBox::new(track_id),
            tfdt_box: TrackFragmentBaseMediaDecodeTimeBox,
            trun_box: TrackRunBox::default(),
        }
    }
}
impl Mp4Box for TrackFragmentBox {
    const BOX_TYPE: [u8; 4] = *b"traf";

    fn box_payload_size(&self) -> Result<u32> {
        let mut size = 0;
        size += box_size!(self.tfhd_box);
        size += box_size!(self.tfdt_box);
        size += box_size!(self.trun_box);
        Ok(size)
    }
    fn write_box_payload<W: Write>(&self, mut writer: W) -> Result<()> {
        write_box!(writer, self.tfhd_box);
        write_box!(writer, self.tfdt_box);
        write_box!(writer, self.trun_box);
        Ok(())
    }
}

/// 8.8.7 Track Fragment Header Box (ISO/IEC 14496-12).
#[allow(missing_docs)]
#[derive(Debug)]
pub struct TrackFragmentHeaderBox {
    track_id: u32,
    pub duration_is_empty: bool,
    pub default_base_is_moof: bool,
    pub base_data_offset: Option<u64>,
    pub sample_description_index: Option<u32>,
    pub default_sample_duration: Option<u32>,
    pub default_sample_size: Option<u32>,
    pub default_sample_flags: Option<SampleFlags>,
}
impl TrackFragmentHeaderBox {
    fn new(track_id: u32) -> Self {
        TrackFragmentHeaderBox {
            track_id,
            duration_is_empty: false,
            default_base_is_moof: true,
            base_data_offset: None,
            sample_description_index: None,
            default_sample_duration: None,
            default_sample_size: None,
            default_sample_flags: None,
        }
    }
}
impl Mp4Box for TrackFragmentHeaderBox {
    const BOX_TYPE: [u8; 4] = *b"tfhd";

    fn box_flags(&self) -> Option<u32> {
        let flags = self.base_data_offset.is_some() as u32
            | (self.sample_description_index.is_some() as u32 * 0x00_0002)
            | (self.default_sample_duration.is_some() as u32 * 0x00_0008)
            | (self.default_sample_size.is_some() as u32 * 0x00_0010)
            | (self.default_sample_flags.is_some() as u32 * 0x00_0020)
            | (self.duration_is_empty as u32 * 0x01_0000)
            | (self.default_base_is_moof as u32 * 0x02_0000);
        Some(flags)
    }
    fn box_payload_size(&self) -> Result<u32> {
        let size = track!(ByteCounter::calculate(|w| self.write_box_payload(w)))?;
        Ok(size as u32)
    }
    fn write_box_payload<W: Write>(&self, mut writer: W) -> Result<()> {
        write_u32!(writer, self.track_id);
        if let Some(x) = self.base_data_offset {
            write_u64!(writer, x);
        }
        if let Some(x) = self.sample_description_index {
            write_u32!(writer, x);
        }
        if let Some(x) = self.default_sample_duration {
            write_u32!(writer, x);
        }
        if let Some(x) = self.default_sample_size {
            write_u32!(writer, x);
        }
        if let Some(x) = self.default_sample_flags {
            write_u32!(writer, x.to_u32());
        }
        Ok(())
    }
}

/// 8.8.12 Track fragment decode time (ISO/IEC 14496-12).
#[derive(Debug)]
pub struct TrackFragmentBaseMediaDecodeTimeBox;
impl Mp4Box for TrackFragmentBaseMediaDecodeTimeBox {
    const BOX_TYPE: [u8; 4] = *b"tfdt";

    fn box_version(&self) -> Option<u8> {
        Some(0)
    }
    fn box_payload_size(&self) -> Result<u32> {
        Ok(4)
    }
    fn write_box_payload<W: Write>(&self, mut writer: W) -> Result<()> {
        write_u32!(writer, 0); // base_media_decode_time
        Ok(())
    }
}

/// 8.8.8 Track Fragment Run Box (ISO/IEC 14496-12).
#[allow(missing_docs)]
#[derive(Debug, Default)]
pub struct TrackRunBox {
    pub data_offset: Option<i32>,
    pub first_sample_flags: Option<SampleFlags>,
    pub samples: Vec<Sample>,
}
impl Mp4Box for TrackRunBox {
    const BOX_TYPE: [u8; 4] = *b"trun";

    fn box_version(&self) -> Option<u8> {
        Some(1)
    }
    fn box_flags(&self) -> Option<u32> {
        let sample = self
            .samples
            .first()
            .cloned()
            .unwrap_or_else(Sample::default);
        let flags = self.data_offset.is_some() as u32
            | (self.first_sample_flags.is_some() as u32 * 0x00_0004)
            | sample.to_box_flags();
        Some(flags)
    }
    fn box_payload_size(&self) -> Result<u32> {
        let size = track!(ByteCounter::calculate(|w| self.write_box_payload(w)))?;
        Ok(size as u32)
    }
    fn write_box_payload<W: Write>(&self, mut writer: W) -> Result<()> {
        write_u32!(writer, self.samples.len() as u32);
        if let Some(x) = self.data_offset {
            write_i32!(writer, x);
        }
        if let Some(x) = self.first_sample_flags {
            write_u32!(writer, x.to_u32());
        }

        let mut sample_flags = None;
        for sample in &self.samples {
            if sample_flags.is_none() {
                sample_flags = Some(sample.to_box_flags());
            }
            track_assert_eq!(
                Some(sample.to_box_flags()),
                sample_flags,
                ErrorKind::InvalidInput
            );

            if let Some(x) = sample.duration {
                write_u32!(writer, x);
            }
            if let Some(x) = sample.size {
                write_u32!(writer, x);
            }
            if let Some(x) = sample.flags {
                write_u32!(writer, x.to_u32());
            }
            if let Some(x) = sample.composition_time_offset {
                write_i32!(writer, x);
            }
        }
        Ok(())
    }
}

/// 8.8.8.2 A sample (ISO/IEC 14496-12).
#[allow(missing_docs)]
#[derive(Debug, Default, Clone, PartialEq, Eq, Hash)]
pub struct Sample {
    pub duration: Option<u32>,
    pub size: Option<u32>,
    pub flags: Option<SampleFlags>,
    pub composition_time_offset: Option<i32>,
}
impl Sample {
    fn to_box_flags(&self) -> u32 {
        (self.duration.is_some() as u32 * 0x00_0100)
            | (self.size.is_some() as u32 * 0x00_0200)
            | (self.flags.is_some() as u32 * 0x00_0400)
            | (self.composition_time_offset.is_some() as u32 * 0x00_0800)
    }
}

/// 8.8.8.1 Flags for a sample (ISO/IEC 14496-12).
#[allow(missing_docs)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SampleFlags {
    pub is_leading: u8,             // u2
    pub sample_depends_on: u8,      // u2
    pub sample_is_depdended_on: u8, // u2
    pub sample_has_redundancy: u8,  // u2
    pub sample_padding_value: u8,   // u3
    pub sample_is_non_sync_sample: bool,
    pub sample_degradation_priority: u16,
}
impl SampleFlags {
    fn to_u32(&self) -> u32 {
        (u32::from(self.is_leading) << 26)
            | (u32::from(self.sample_depends_on) << 24)
            | (u32::from(self.sample_is_depdended_on) << 22)
            | (u32::from(self.sample_has_redundancy) << 20)
            | (u32::from(self.sample_padding_value) << 17)
            | ((self.sample_is_non_sync_sample as u32) << 16)
            | u32::from(self.sample_degradation_priority)
    }
}
