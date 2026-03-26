use {
    super::{errors::MediaError, ts::Ts, utils},
    aws_sdk_s3::Client as S3Client,
    config::HlsConfig,
    bytes::BytesMut,
    streamhub::{define::{Segment}},
    std::{collections::VecDeque, fs, fs::File, io::Write},
};
use mpeg2ts::ts::TsPacketReader;
use mpeg2ts::pes::{PesPacketReader, ReadPesPacket};

use xmpegts::{
    define::{epsi_stream_type, MPEG_FLAG_IDR_FRAME},
    ts::TsMuxer,
};
pub struct M3u8 {
    version: u16,
    sequence_no: u32,
    ts_no: u64,
    /*What duration should media files be?
    A duration of 10 seconds of media per file seems to strike a reasonable balance for most broadcast content.
    http://devimages.apple.com/iphone/samples/bipbop/bipbopall.m3u8*/
    duration: i64,
    /*How many files should be listed in the index file during a continuous, ongoing session?
    The normal recommendation is 3, but the optimum number may be larger.*/
    live_ts_count: usize,

    pub segments: VecDeque<Segment>,

    m3u8_folder: String,
    live_m3u8_name: String,

    ts_handler: Ts,

    need_record: bool,
    vod_m3u8_content: String,
    vod_m3u8_name: String,
    prefix: Option<String>,
}

impl M3u8 {
    pub fn new(
        duration: i64,
        app_name: String,
        stream_name: String,
        hls_config: Option<HlsConfig>,
        s3_client: Option<S3Client>,
    ) -> Self {

        let path = hls_config
            .as_ref() 
            .and_then(|config| config.path.clone())
            .unwrap_or("./".to_string());

        let m3u8_folder = format!("{path}/{stream_name}");
        fs::create_dir_all(m3u8_folder.clone()).unwrap();
        let m3u8_folder_audio = format!("{path}/{stream_name}/audio");
        fs::create_dir_all(m3u8_folder_audio.clone()).unwrap();
        let live_m3u8_name = format!("{stream_name}.m3u8");

        let need_record = hls_config
            .as_ref() 
            .and_then(|config| Some(config.need_record))
            .unwrap_or(false);

        let live_ts_count = hls_config
            .as_ref() 
            .and_then(|config| config.live_ts_count)
            .unwrap_or(6); 

        let vod_m3u8_name = if need_record {
            format!("vod_{stream_name}.m3u8")
        } else {
            String::default()
        };

        let s3_config = hls_config.as_ref().and_then(|config| config.s3.clone());
        let s3_bucket = s3_config.as_ref().map(|c| c.bucket.clone());
        let s3_prefix = if let Some(prefix) = s3_config.as_ref().map(|c| c.prefix.clone()).unwrap() {
            format!("{}/{}/", prefix, stream_name)
        } else {
            format!("{}", stream_name)
        };

        let prefix = if let Some(prefix) = hls_config.and_then(|config| config.prefix) {
            Some(format!("{}/{}/", prefix, stream_name))
        } else {
            None
        };
        
        let mut m3u8 = Self {
            version: 3,
            sequence_no: utils::current_time(),
            ts_no: 0,
            duration,
            live_ts_count,
            segments: VecDeque::new(),
            m3u8_folder: m3u8_folder.clone(),
            live_m3u8_name,
            ts_handler: Ts::new(m3u8_folder, s3_client, s3_bucket, s3_prefix),
            // record,
            need_record,
            vod_m3u8_content: String::default(),
            vod_m3u8_name,
            prefix: prefix,
        };

        if need_record {
            m3u8.vod_m3u8_content = m3u8.generate_m3u8_header(true);
        }
        m3u8
    }

    fn is_idr_frame(payload: &[u8]) -> bool {
        // H.264 NAL unit types:
        // 1: Non-IDR slice
        // 5: IDR slice (keyframe)
        // 6: SEI
        // 7: SPS
        // 8: PPS
        // 9: Access unit delimiter
        
        let mut i = 0;
        while i < payload.len() {
            // Look for start code (0x000001 or 0x00000001)
            if i + 3 < payload.len() && payload[i] == 0x00 && payload[i + 1] == 0x00 {
                let start_code_len = if payload[i + 2] == 0x01 {
                    3
                } else if i + 4 < payload.len() && payload[i + 2] == 0x00 && payload[i + 3] == 0x01 {
                    4
                } else {
                    i += 1;
                    continue;
                };
                
                let nal_start = i + start_code_len;
                if nal_start < payload.len() {
                    let nal_header = payload[nal_start];
                    let nal_type = nal_header & 0x1F;
                    
                    // Check if this is an IDR NAL unit (type 5)
                    if nal_type == 5 {
                        return true;
                    }
                }
                i = nal_start;
            } else {
                i += 1;
            }
        }
        false
    }

    pub fn to_separate_ts<R: std::io::Read>(
        &mut self,
        ts_reader: R,
    ) -> std::result::Result<(BytesMut, BytesMut), MediaError> {

        let mut video_muxer = TsMuxer::new();
        let mut audio_muxer = TsMuxer::new();

        let video_pid = video_muxer
            .add_stream(epsi_stream_type::PSI_STREAM_H264, BytesMut::new())
            .map_err(MediaError::from)?;
        let audio_pid = audio_muxer
            .add_stream(epsi_stream_type::PSI_STREAM_AAC, BytesMut::new())
            .map_err(MediaError::from)?;

        let mut reader = PesPacketReader::new(TsPacketReader::new(ts_reader));
        while let Some(pes) = reader.read_pes_packet().map_err(MediaError::from)? {
            let pts = match pes.header.pts {
                Some(ts) => ts,
                None => continue,
            };
            let dts = pes.header.dts.unwrap_or(pts);

            let payload = BytesMut::from(&pes.data[..]);

            if pes.header.stream_id.is_video() {
                // Only set IDR flag for actual IDR frames
                let flags = if Self::is_idr_frame(&pes.data) {
                    MPEG_FLAG_IDR_FRAME
                } else {
                    0
                };
                video_muxer
                    .write(video_pid, pts.as_u64() as i64, dts.as_u64() as i64, flags, payload)
                    .map_err(MediaError::from)?;
            } else if pes.header.stream_id.is_audio() {
                audio_muxer
                    .write(audio_pid, pts.as_u64() as i64, dts.as_u64() as i64, 0, payload)
                    .map_err(MediaError::from)?;
            }
        }

        Ok((video_muxer.get_data(), audio_muxer.get_data()))
    }

    pub async fn add_segment(
        &mut self,
        duration: i64,
        discontinuity: bool,
        is_eof: bool,
        ts_data: BytesMut,
    ) -> Result<(), MediaError> {
        let segment_count: usize = self.segments.len();
        self.sequence_no = utils::current_time();
        self.ts_no += 1;

        if segment_count >= self.live_ts_count {
            let segment = self.segments.pop_front().unwrap();
            if !self.need_record {
                self.ts_handler.delete(segment.name.clone(), false).await;
                self.ts_handler.delete(segment.name, true).await;
            }
        }
        self.duration = std::cmp::max(duration, self.duration);
        let (video_ts, audio_ts) = self.to_separate_ts(&ts_data[..])?;

        let (ts_name, ts_path) = self.ts_handler.write(video_ts, self.sequence_no, false).await?;
        let (_ts_name, _ts_path) = self.ts_handler.write(audio_ts, self.sequence_no, true).await?;

        let ts_name_with_prefix = self.prefix
            .as_ref()
            .map(|prefix| format!("{}{}", prefix, ts_name))
            .unwrap_or(ts_name);
        let segment = Segment::new(duration, discontinuity, self.ts_no.clone(), ts_name_with_prefix, ts_path, is_eof);

        if self.need_record {
            self.update_vod_m3u8(&segment);
        }

        self.segments.push_back(segment.clone());

        Ok(())
    }

    pub async fn clear(&mut self) -> Result<(), MediaError> {
        if self.need_record {
            let vod_m3u8_path = format!("{}/{}", self.m3u8_folder, self.vod_m3u8_name);
            let mut file_handler = File::create(vod_m3u8_path).unwrap();
            self.vod_m3u8_content += "#EXT-X-ENDLIST\n";
            file_handler.write_all(self.vod_m3u8_content.as_bytes())?;
        } else {
            for segment in &self.segments {
                self.ts_handler.delete(segment.name.clone(), false).await;
                self.ts_handler.delete(segment.name.clone(), true).await;
            }
        }

        //clear live m3u8
        let live_m3u8_path = format!("{}/{}", self.m3u8_folder, self.live_m3u8_name);
        fs::remove_file(live_m3u8_path)?;

        Ok(())
    }

    pub fn generate_m3u8_header(&self, is_vod: bool) -> String {
        let mut m3u8_header = "#EXTM3U\n".to_string();
        m3u8_header += format!("#EXT-X-VERSION:{}\n", self.version).as_str();
        m3u8_header += format!("#EXT-X-TARGETDURATION:{}\n", (self.duration + 999) / 1000).as_str();

        if is_vod {
            m3u8_header += "#EXT-X-MEDIA-SEQUENCE:0\n";
            m3u8_header += "#EXT-X-PLAYLIST-TYPE:VOD\n";
            m3u8_header += "#EXT-X-ALLOW-CACHE:YES\n";
        } else {
            m3u8_header += format!("#EXT-X-MEDIA-SEQUENCE:{}\n", self.sequence_no).as_str();
        }

        m3u8_header
    }

    pub fn refresh_playlist(&mut self) -> Result<String, MediaError> {
        let mut m3u8_content = self.generate_m3u8_header(false);

        for segment in &self.segments {
            if segment.discontinuity {
                m3u8_content += "#EXT-X-DISCONTINUITY\n";
            }
            m3u8_content += format!(
                "#EXTINF:{:.3}\n{}\n",
                segment.duration as f64 / 1000.0,
                segment.name
            )
            .as_str();

            if segment.is_eof {
                m3u8_content += "#EXT-X-ENDLIST\n";
                break;
            }
        }

        let m3u8_path = format!("{}/{}", self.m3u8_folder, self.live_m3u8_name);

        let mut file_handler = File::create(m3u8_path).unwrap();
        file_handler.write_all(m3u8_content.as_bytes())?;

        Ok(m3u8_content)
    }

    pub fn update_vod_m3u8(&mut self, segment: &Segment) {
        if segment.discontinuity {
            self.vod_m3u8_content += "#EXT-X-DISCONTINUITY\n";
        }
        self.vod_m3u8_content += format!(
            "#EXTINF:{:.3}\n{}\n",
            segment.duration as f64 / 1000.0,
            segment.name
        )
        .as_str();
    }
}
